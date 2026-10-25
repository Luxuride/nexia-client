//! Virtual synchronization engine managing the in-memory metadata tree and on-demand hydration.

use nextcloud_rust_sdk::{
    BasicAuth, DavClient, DavResource, DavResourceSummary, HydratedContent, HydrationState,
    NodeMetadata, ReqwestTransport, VirtualNode, VirtualTree, normalize_path,
};
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::SyncError;
use crate::vfs::entry::{VirtualEntrySummary, make_dav_path};
use crate::vfs::seed::{HierarchySeeder, StandardNextcloudSeeder};
use nextcloud_desktop_core::events::{DesktopEvent, EventBus};

/// Virtual file synchronization manager generic over its initial [`HierarchySeeder`].
#[derive(Clone)]
pub struct VirtualSyncEngine<S: HierarchySeeder = StandardNextcloudSeeder> {
    tree: Arc<RwLock<VirtualTree>>,
    cache_dir: PathBuf,
    event_bus: EventBus,
    vfs_handle: Option<nextcloud_rust_sdk::VfsHandle>,
    _seeder: PhantomData<S>,
}

impl<S: HierarchySeeder> VirtualSyncEngine<S> {
    /// Creates a new virtual sync engine storing cached files in `cache_dir`.
    pub fn new(cache_dir: impl Into<PathBuf>, event_bus: EventBus) -> Self {
        let cache_dir = cache_dir.into();
        let tree_file = cache_dir.join("tree.json");
        let initial_tree = if tree_file.exists() {
            fs::read_to_string(&tree_file)
                .ok()
                .and_then(|data| serde_json::from_str(&data).ok())
                .unwrap_or_else(|| {
                    let mut tree = VirtualTree::new("Nextcloud");
                    S::seed(&mut tree);
                    tree
                })
        } else {
            let mut tree = VirtualTree::new("Nextcloud");
            S::seed(&mut tree);
            tree
        };

        Self {
            tree: Arc::new(RwLock::new(initial_tree)),
            cache_dir,
            event_bus,
            vfs_handle: None,
            _seeder: PhantomData,
        }
    }

    /// Attaches an orchestrator VFS handle communicating with the background worker actor.
    #[must_use]
    pub fn with_vfs_handle(mut self, handle: nextcloud_rust_sdk::VfsHandle) -> Self {
        self.vfs_handle = Some(handle);
        self
    }

    fn persist_tree(&self, tree: &VirtualTree) {
        let tree_file = self.cache_dir.join("tree.json");
        if let Ok(json) = serde_json::to_string(tree) {
            let _ = fs::write(tree_file, json);
        }
    }

    /// Resets the virtual tree to the default Nextcloud folder hierarchy.
    pub async fn reset_default_hierarchy(&self) -> usize {
        let mut tree = self.tree.write().await;
        *tree = VirtualTree::new("Nextcloud");
        let count = S::seed(&mut tree);
        self.persist_tree(&tree);
        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        count
    }

    /// Access the local cache root directory for virtual files.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Clears and repopulates the virtual tree from a list of discovered WebDAV resources.
    pub async fn populate_from_resources(
        &self,
        base_dav_prefix: &str,
        resources: &[DavResource],
    ) -> Result<usize, SyncError> {
        let summaries: Vec<DavResourceSummary> = resources
            .iter()
            .map(|res| {
                let mut sum = DavResourceSummary::new(&res.href, res.is_collection);
                if let Some(len) = res.content_length {
                    sum = sum.with_content_length(len);
                }
                if let Some(ref etag) = res.etag {
                    sum = sum.with_etag(etag);
                }
                if let Some(ref mime) = res.content_type {
                    sum = sum.with_content_type(mime);
                }
                if let Some(has_preview) = res.has_preview {
                    sum = sum.with_has_preview(has_preview);
                }
                sum
            })
            .collect();

        let mut tree = self.tree.write().await;
        *tree = VirtualTree::new("Nextcloud");
        let inserted = tree.populate_from_resources(base_dav_prefix, &summaries);
        self.persist_tree(&tree);

        self.event_bus.emit(DesktopEvent::VfsTreeUpdated {
            node_count: inserted,
        });

        Ok(inserted)
    }

    /// Hydrates a virtual file on-demand by downloading it via WebDAV into the local cache.
    pub async fn hydrate_file(
        &self,
        user: &str,
        dav_client: &DavClient<'_, ReqwestTransport, BasicAuth>,
        remote_path: &str,
    ) -> Result<PathBuf, SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        {
            let tree = self.tree.read().await;
            if let Some(node) = tree.get(&clean_path) {
                if let Some(cached) = node.hydration.cached_path().filter(|p| p.exists()) {
                    return Ok(cached.to_path_buf());
                }
            } else {
                return Err(SyncError::NotFound(clean_path));
            }
        }

        let dav_path = make_dav_path(user, &clean_path)?;
        tracing::info!(path = %clean_path, "Virtual sync on-demand hydration starting");

        let bytes = dav_client.get(&dav_path).await?;
        let byte_len = bytes.len() as u64;
        let local_cached = self.write_cache_bytes(&clean_path, &bytes)?;

        {
            let mut tree = self.tree.write().await;
            let _ = tree.hydrate_path(&clean_path, local_cached.clone());
            self.persist_tree(&tree);
        }

        self.event_bus.emit(DesktopEvent::NodeHydrated {
            path: clean_path,
            size: byte_len,
        });

        Ok(local_cached)
    }

    /// Hydrates a placeholder node locally into the cache directory when operating without a live server.
    pub async fn hydrate_local_placeholder(&self, remote_path: &str) -> Result<PathBuf, SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));
        let expected_size = {
            let tree = self.tree.read().await;
            let node = tree
                .get(&clean_path)
                .ok_or_else(|| SyncError::NotFound(clean_path.clone()))?;
            if let Some(cached) = node.hydration.cached_path().filter(|p| p.exists()) {
                return Ok(cached.to_path_buf());
            }
            node.metadata.size
        };

        let ext = clean_path.rsplit('.').next().unwrap_or("");
        let placeholder_bytes = match ext.to_lowercase().as_str() {
            "pdf" => b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/MediaBox[0 0 612 792]/Parent 2 0 R/Resources<<>>>>endobj\nxref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000052 00000 n \n0000000101 00000 n \ntrailer<</Size 4/Root 1 0 R>>\nstartxref\n178\n%%EOF\n".to_vec(),
            "png" => b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15c4\x00\x00\x00\rIDATx\x9cc`\x00\x00\x00\x02\x00\x01H\xaf\xa4q\x00\x00\x00\x00IEND\xaeB`\x82".to_vec(),
            "jpg" | "jpeg" => b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x01\x00H\x00H\x00\x00\xff\xdb\x00C\x00\x08\x06\x06\x07\x06\x05\x08\x07\x07\x07\t\t\x08\n\x0c\x14\r\x0c\x0b\x0b\x0c\x19\x12\x13\x0f\x14\x1d\x1a\x1f\x1e\x1d\x1a\x1c\x1c $.' \",#\x1c\x1c(7),01444\x1f'9=82<.342\xff\xc0\x00\x0b\x08\x00\x01\x00\x01\x01\x01\x11\x00\xff\xc4\x00\x1f\x00\x00\x01\x05\x01\x01\x01\x01\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\xff\xda\x00\x08\x01\x01\x00\x00?\x00\xbf\x00\xff\xd9".to_vec(),
            "md" | "txt" => format!(
                "# {}\n\nThis file is synchronized with Nextcloud Desktop.\nPath: {}\nSize: {} bytes\n",
                clean_path.trim_start_matches('/'),
                clean_path,
                expected_size
            ).into_bytes(),
            _ => format!(
                "Nextcloud hydrated resource: {}\nSize: {} bytes\n",
                clean_path, expected_size
            ).into_bytes(),
        };

        let local_cached = self.write_cache_bytes(&clean_path, &placeholder_bytes)?;

        {
            let mut tree = self.tree.write().await;
            let _ = tree.hydrate_path(&clean_path, local_cached.clone());
            self.persist_tree(&tree);
        }

        self.event_bus.emit(DesktopEvent::NodeHydrated {
            path: clean_path,
            size: expected_size,
        });

        Ok(local_cached)
    }

    /// Dehydrates a file node back to a metadata placeholder, freeing local cache disk space.
    pub async fn dehydrate_file(&self, remote_path: &str) -> Result<(), SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));
        if let Some(ref handle) = self.vfs_handle {
            handle.dehydrate_local(&clean_path).await?;
            self.event_bus
                .emit(DesktopEvent::NodeDehydrated { path: clean_path });
            return Ok(());
        }

        let mut target_cached_path = None;

        {
            let mut tree = self.tree.write().await;
            if let Some(node) = tree.get(&clean_path)
                && let Some(cached) = node.hydration.cached_path()
            {
                target_cached_path = Some(cached.to_path_buf());
            }

            tree.dehydrate(&clean_path)?;
            self.persist_tree(&tree);
        }

        if let Some(path) = target_cached_path.filter(|p| p.exists()) {
            let _ = fs::remove_file(&path);
        }

        self.event_bus
            .emit(DesktopEvent::NodeDehydrated { path: clean_path });

        Ok(())
    }

    /// Lists direct children of a directory in the virtual tree, computing recursive directory sizes.
    pub async fn list_children(
        &self,
        dir_path: &str,
    ) -> Result<Vec<VirtualEntrySummary>, SyncError> {
        let clean_path = normalize_path(dir_path).unwrap_or_else(|_| "/".to_string());
        if let Some(ref handle) = self.vfs_handle {
            return Ok(handle.list_children(&clean_path).await?);
        }
        let tree = self.tree.read().await;
        Ok(tree.children_summaries(&clean_path))
    }

    /// Retrieves summary information for a single node path, computing recursive directory size.
    pub async fn get_entry(&self, path: &str) -> Result<Option<VirtualEntrySummary>, SyncError> {
        let clean_path = normalize_path(path).unwrap_or_else(|_| "/".to_string());
        if let Some(ref handle) = self.vfs_handle {
            return Ok(handle.entry(&clean_path).await?);
        }
        let tree = self.tree.read().await;
        Ok(tree.entry_summary(&clean_path))
    }

    /// Inserts a new virtual directory node at `remote_path`.
    pub async fn create_directory(&self, remote_path: &str) -> Result<(), SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));
        if clean_path == "/" {
            return Err(SyncError::InvalidPath(
                "cannot recreate root directory".to_string(),
            ));
        }

        if let Some(ref handle) = self.vfs_handle {
            handle.create_directory(&clean_path).await?;
            self.event_bus
                .emit(DesktopEvent::VfsTreeUpdated { node_count: 1 });
            return Ok(());
        }

        let mut tree = self.tree.write().await;
        tree.insert(VirtualNode::new_directory(NodeMetadata::directory(
            &clean_path,
        )));
        let count = tree.len();
        self.persist_tree(&tree);
        drop(tree);

        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        Ok(())
    }

    /// Inserts a new virtual file node at `remote_path` with the specified byte content.
    pub async fn create_file(
        &self,
        remote_path: &str,
        content: &[u8],
    ) -> Result<String, SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));
        if clean_path == "/" {
            return Err(SyncError::InvalidPath(
                "cannot create file at root directory".to_string(),
            ));
        }

        if let Some(ref handle) = self.vfs_handle {
            let _ = handle.hydrate_local(&clean_path, content.to_vec()).await?;
            self.event_bus
                .emit(DesktopEvent::VfsTreeUpdated { node_count: 1 });
            return Ok(clean_path);
        }

        let local_path = self.write_cache_bytes(&clean_path, content)?;
        let meta = NodeMetadata::file(&clean_path).with_size(content.len() as u64);

        let mut node = VirtualNode::new_file(meta);
        node.hydration = HydrationState::Hydrated(HydratedContent::Path(local_path));
        let count = {
            let mut tree = self.tree.write().await;
            tree.insert(node);
            self.persist_tree(&tree);
            tree.len()
        };

        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        Ok(clean_path)
    }

    /// Renames a file or folder in the virtual tree via SDK VirtualTree.
    pub async fn rename_entry(
        &self,
        old_remote_path: &str,
        new_name: &str,
    ) -> Result<String, SyncError> {
        if let Some(ref handle) = self.vfs_handle {
            handle.rename(old_remote_path, new_name).await?;
            self.event_bus
                .emit(DesktopEvent::VfsTreeUpdated { node_count: 1 });
            let parent =
                nextcloud_rust_sdk::parent_path(old_remote_path).unwrap_or_else(|| "/".to_string());
            let new_path = if parent == "/" {
                format!("/{}", new_name.trim_matches('/'))
            } else {
                format!(
                    "{}/{}",
                    parent.trim_end_matches('/'),
                    new_name.trim_matches('/')
                )
            };
            return Ok(new_path);
        }

        let (new_path, count) = {
            let mut tree = self.tree.write().await;
            let new_path = tree.rename(old_remote_path, new_name)?;
            self.persist_tree(&tree);
            (new_path, tree.len())
        };

        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        Ok(new_path)
    }

    /// Moves a file or directory into a target directory in the virtual tree via SDK VirtualTree.
    pub async fn move_entry(
        &self,
        source_path: &str,
        target_dir_path: &str,
    ) -> Result<String, SyncError> {
        let (new_path, count) = {
            let mut tree = self.tree.write().await;
            let new_path = tree.move_node(source_path, target_dir_path)?;
            self.persist_tree(&tree);
            (new_path, tree.len())
        };

        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        Ok(new_path)
    }

    /// Removes a file or directory subtree from the virtual tree and deletes any cached file.
    pub async fn remove_entry(&self, remote_path: &str) -> Result<(), SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));
        if clean_path == "/" {
            return Err(SyncError::InvalidPath(
                "cannot remove root directory".to_string(),
            ));
        }

        if let Some(ref handle) = self.vfs_handle {
            handle.remove(&clean_path).await?;
            self.event_bus
                .emit(DesktopEvent::VfsTreeUpdated { node_count: 1 });
            return Ok(());
        }

        let mut cached_to_delete = None;
        let count = {
            let mut tree = self.tree.write().await;
            if let Some(node) = tree.get(&clean_path)
                && let Some(cached) = node.hydration.cached_path()
            {
                cached_to_delete = Some(cached.to_path_buf());
            }
            if tree.remove(&clean_path).is_none() {
                return Err(SyncError::NotFound(clean_path));
            }
            self.persist_tree(&tree);
            tree.len()
        };

        if let Some(path) = cached_to_delete.filter(|p| p.exists()) {
            let _ = fs::remove_file(path);
        }

        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        Ok(())
    }

    fn write_cache_bytes(&self, clean_path: &str, bytes: &[u8]) -> Result<PathBuf, SyncError> {
        let safe_rel = clean_path.trim_start_matches('/');
        let local_cached = self.cache_dir.join(safe_rel);

        if let Some(parent) = local_cached.parent() {
            fs::create_dir_all(parent).map_err(|err| SyncError::LocalIo {
                path: parent.to_path_buf(),
                context: "creating VFS cache parent directory".to_string(),
                source: err,
            })?;
        }

        fs::write(&local_cached, bytes).map_err(|err| SyncError::LocalIo {
            path: local_cached.clone(),
            context: "writing hydrated VFS cache file".to_string(),
            source: err,
        })?;

        Ok(local_cached)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nextcloud_rust_sdk::ResourceIconCategory;

    #[tokio::test]
    async fn vfs_tree_operations_and_sdk_hydration() {
        let temp_cache = std::env::temp_dir().join("nc_desktop_test_vfs_cache");
        let _ = fs::remove_dir_all(&temp_cache);

        let bus = EventBus::new(16);
        let vfs: VirtualSyncEngine = VirtualSyncEngine::new(&temp_cache, bus);

        let seeded = vfs.reset_default_hierarchy().await;
        assert!(seeded > 0);

        let root_children = vfs.list_children("/").await.unwrap();
        assert!(
            root_children
                .iter()
                .any(|c| c.name == "Documents" && c.is_directory)
        );
        assert!(root_children.iter().any(|c| {
            c.name == "Nextcloud Manual.pdf" && c.icon_category == ResourceIconCategory::Document
        }));

        vfs.create_directory("/Documents/NewFolder").await.unwrap();
        let created_file = vfs
            .create_file("/Documents/test.txt", b"hello world")
            .await
            .expect("create file");
        assert_eq!(created_file, "/Documents/test.txt");

        let docs_children = vfs.list_children("/Documents").await.unwrap();
        assert!(
            docs_children
                .iter()
                .any(|c| c.name == "NewFolder" && c.is_directory)
        );
        assert!(
            docs_children
                .iter()
                .any(|c| c.name == "test.txt" && !c.is_directory)
        );

        let cached = vfs
            .hydrate_local_placeholder("/Notes.md")
            .await
            .expect("hydrate placeholder");
        assert!(cached.exists());

        let updated_root = vfs.list_children("/").await.unwrap();
        let notes = updated_root
            .iter()
            .find(|c| c.name == "Notes.md")
            .expect("Notes.md");
        assert!(notes.is_hydrated);

        vfs.dehydrate_file("/Notes.md")
            .await
            .expect("dehydrate placeholder");
        assert!(!cached.exists());

        let docs_summary = vfs.get_entry("/Documents").await.unwrap().unwrap();
        assert!(docs_summary.is_directory);
        assert!(docs_summary.size >= 11);
        assert!(docs_summary.directory_summary.is_some());

        // Rename resource
        let renamed = vfs
            .rename_entry("/Documents/NewFolder", "RenamedFolder")
            .await
            .expect("rename folder");
        assert_eq!(renamed, "/Documents/RenamedFolder");
        let docs_renamed = vfs.list_children("/Documents").await.unwrap();
        assert!(docs_renamed.iter().any(|c| c.name == "RenamedFolder"));

        // Move resource
        let moved = vfs
            .move_entry("/Documents/RenamedFolder", "/")
            .await
            .expect("move folder to root");
        assert_eq!(moved, "/RenamedFolder");
        let root_after_move = vfs.list_children("/").await.unwrap();
        assert!(root_after_move.iter().any(|c| c.name == "RenamedFolder"));

        // Delete resource
        vfs.remove_entry("/RenamedFolder")
            .await
            .expect("delete folder");
        let root_after_delete = vfs.list_children("/").await.unwrap();
        assert!(!root_after_delete.iter().any(|c| c.name == "RenamedFolder"));

        let _ = fs::remove_dir_all(&temp_cache);
    }
}
