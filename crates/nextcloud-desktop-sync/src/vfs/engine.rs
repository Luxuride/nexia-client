//! Virtual synchronization engine managing the in-memory metadata tree and on-demand hydration.

use nextcloud_rust_sdk::{
    BasicAuth, DavClient, DavResource, HydratedContent, HydrationState, NodeMetadata,
    ReqwestTransport, VirtualNode, VirtualTree, normalize_path, parent_path,
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
    _seeder: PhantomData<S>,
}

impl<S: HierarchySeeder> VirtualSyncEngine<S> {
    /// Creates a new virtual sync engine storing cached files in `cache_dir`.
    pub fn new(cache_dir: impl Into<PathBuf>, event_bus: EventBus) -> Self {
        let cache_dir = cache_dir.into();
        let mut initial_tree = VirtualTree::new("Nextcloud");
        S::seed(&mut initial_tree);
        Self {
            tree: Arc::new(RwLock::new(initial_tree)),
            cache_dir,
            event_bus,
            _seeder: PhantomData,
        }
    }

    /// Resets the virtual tree to the default Nextcloud folder hierarchy.
    pub async fn reset_default_hierarchy(&self) -> usize {
        let mut tree = self.tree.write().await;
        *tree = VirtualTree::new("Nextcloud");
        let count = S::seed(&mut tree);
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
        let mut tree = self.tree.write().await;
        *tree = VirtualTree::new("Nextcloud");

        let mut inserted = 0;
        let prefix = base_dav_prefix.trim_end_matches('/');

        for res in resources {
            let relative = if res.href.starts_with(prefix) {
                &res.href[prefix.len()..]
            } else {
                &res.href
            };

            let clean_path = if relative.is_empty() || relative == "/" {
                "/".to_string()
            } else {
                format!("/{}", relative.trim_matches('/'))
            };

            if clean_path == "/" {
                continue;
            }

            let meta = build_node_metadata(&clean_path, res);
            let node = if res.is_collection {
                VirtualNode::new_directory(meta)
            } else {
                VirtualNode::new_file(meta)
            };

            tree.insert(node);
            inserted += 1;
        }

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

        let placeholder_bytes = format!(
            "Nextcloud hydrated file: {}\nSize metadata: {} bytes\n",
            clean_path, expected_size
        )
        .into_bytes();

        let local_cached = self.write_cache_bytes(&clean_path, &placeholder_bytes)?;

        {
            let mut tree = self.tree.write().await;
            let _ = tree.hydrate_path(&clean_path, local_cached.clone());
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
        let mut target_cached_path = None;

        {
            let mut tree = self.tree.write().await;
            let node = tree
                .get_mut(&clean_path)
                .ok_or_else(|| SyncError::NotFound(clean_path.clone()))?;

            if let Some(cached) = node.hydration.cached_path() {
                target_cached_path = Some(cached.to_path_buf());
            }

            node.hydration = HydrationState::Placeholder;
        }

        if let Some(path) = target_cached_path.filter(|p| p.exists()) {
            let _ = fs::remove_file(&path);
        }

        self.event_bus
            .emit(DesktopEvent::NodeDehydrated { path: clean_path });

        Ok(())
    }

    /// Lists direct children of a directory in the virtual tree.
    pub async fn list_children(
        &self,
        dir_path: &str,
    ) -> Result<Vec<VirtualEntrySummary>, SyncError> {
        let clean_path = normalize_path(dir_path).unwrap_or_else(|_| "/".to_string());
        let tree = self.tree.read().await;
        Ok(tree
            .children(&clean_path)
            .into_iter()
            .map(VirtualEntrySummary::from)
            .collect())
    }

    /// Retrieves summary information for a single node path.
    pub async fn get_entry(&self, path: &str) -> Result<Option<VirtualEntrySummary>, SyncError> {
        let clean_path = normalize_path(path).unwrap_or_else(|_| "/".to_string());
        let tree = self.tree.read().await;
        Ok(tree.get(&clean_path).map(VirtualEntrySummary::from))
    }

    /// Inserts a new virtual directory node at `remote_path`.
    pub async fn create_directory(&self, remote_path: &str) -> Result<(), SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));
        if clean_path == "/" {
            return Err(SyncError::InvalidPath(
                "cannot recreate root directory".to_string(),
            ));
        }

        let mut tree = self.tree.write().await;
        tree.insert(VirtualNode::new_directory(NodeMetadata::directory(
            &clean_path,
        )));
        let count = tree.len();
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

        let local_path = self.write_cache_bytes(&clean_path, content)?;
        let meta = NodeMetadata::file(&clean_path).with_size(content.len() as u64);

        let mut node = VirtualNode::new_file(meta);
        node.hydration = HydrationState::Hydrated(HydratedContent::Path(local_path));
        let count = {
            let mut tree = self.tree.write().await;
            tree.insert(node);
            tree.len()
        };

        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        Ok(clean_path)
    }

    /// Renames a file or folder in the virtual tree, updating local cache paths if hydrated.
    pub async fn rename_entry(
        &self,
        old_remote_path: &str,
        new_name: &str,
    ) -> Result<String, SyncError> {
        let clean_old = format!("/{}", old_remote_path.trim_matches('/'));
        if clean_old == "/" {
            return Err(SyncError::InvalidPath(
                "cannot rename root directory".to_string(),
            ));
        }

        let trimmed_new = new_name.trim().trim_matches('/');
        if trimmed_new.is_empty() || trimmed_new.contains('/') {
            return Err(SyncError::InvalidPath(format!(
                "invalid new name '{new_name}'"
            )));
        }

        let parent = parent_path(&clean_old).unwrap_or_else(|| "/".to_string());
        let clean_new = if parent == "/" {
            format!("/{trimmed_new}")
        } else {
            format!("{parent}/{trimmed_new}")
        };

        let mut old_cached = None;
        let mut new_cached = None;
        let count = {
            let mut tree = self.tree.write().await;
            let Some(mut node) = tree.remove(&clean_old) else {
                return Err(SyncError::NotFound(clean_old));
            };

            if let Some(cached) = node.hydration.cached_path() {
                old_cached = Some(cached.to_path_buf());
                let rel_new = clean_new.trim_start_matches('/');
                let target = self.cache_dir.join(rel_new);
                new_cached = Some(target.clone());
                node.hydration = HydrationState::Hydrated(HydratedContent::Path(target));
            }

            node.metadata.path = clean_new.clone();
            node.metadata.name = trimmed_new.to_string();
            tree.insert(node);
            tree.len()
        };

        if let (Some(from), Some(to)) = (old_cached, new_cached)
            && from.exists()
        {
            if let Some(to_parent) = to.parent() {
                let _ = fs::create_dir_all(to_parent);
            }
            let _ = fs::rename(from, to);
        }

        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        Ok(clean_new)
    }

    /// Moves a file or directory in the virtual tree into `target_dir_path`.
    pub async fn move_entry(
        &self,
        source_path: &str,
        target_dir_path: &str,
    ) -> Result<String, SyncError> {
        let clean_src = format!("/{}", source_path.trim_matches('/'));
        if clean_src == "/" {
            return Err(SyncError::InvalidPath(
                "cannot move root directory".to_string(),
            ));
        }

        let clean_target_dir = format!("/{}", target_dir_path.trim_matches('/'));
        let file_name = clean_src.rsplit('/').next().unwrap_or_default();
        if file_name.is_empty() {
            return Err(SyncError::InvalidPath(
                "invalid source item name".to_string(),
            ));
        }

        let clean_dest = if clean_target_dir == "/" {
            format!("/{file_name}")
        } else {
            format!("{clean_target_dir}/{file_name}")
        };

        if clean_src == clean_dest {
            return Ok(clean_dest);
        }

        let mut old_cached = None;
        let mut new_cached = None;
        let count = {
            let mut tree = self.tree.write().await;
            let Some(mut node) = tree.remove(&clean_src) else {
                return Err(SyncError::NotFound(clean_src));
            };

            if let Some(cached) = node.hydration.cached_path() {
                old_cached = Some(cached.to_path_buf());
                let rel_dest = clean_dest.trim_start_matches('/');
                let target = self.cache_dir.join(rel_dest);
                new_cached = Some(target.clone());
                node.hydration = HydrationState::Hydrated(HydratedContent::Path(target));
            }

            node.metadata.path = clean_dest.clone();
            node.metadata.name = file_name.to_string();
            tree.insert(node);
            tree.len()
        };

        if let (Some(from), Some(to)) = (old_cached, new_cached)
            && from.exists()
        {
            if let Some(to_parent) = to.parent() {
                let _ = fs::create_dir_all(to_parent);
            }
            let _ = fs::rename(from, to);
        }

        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        Ok(clean_dest)
    }

    /// Removes a file or directory subtree from the virtual tree and deletes any cached file.
    pub async fn remove_entry(&self, remote_path: &str) -> Result<(), SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));
        if clean_path == "/" {
            return Err(SyncError::InvalidPath(
                "cannot remove root directory".to_string(),
            ));
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

fn build_node_metadata(clean_path: &str, res: &DavResource) -> NodeMetadata {
    if res.is_collection {
        let mut meta = NodeMetadata::directory(clean_path);
        if let Some(ref etag) = res.etag {
            meta = meta.with_etag(etag.clone());
        }
        meta
    } else {
        let mut meta = NodeMetadata::file(clean_path).with_size(res.content_length.unwrap_or(0));
        if let Some(ref etag) = res.etag {
            meta = meta.with_etag(etag.clone());
        }
        if let Some(ref mime) = res.content_type {
            meta = meta.with_mime_type(mime.clone());
        }
        if let Some(has_prev) = res.has_preview {
            meta = meta.with_has_preview(has_prev);
        }
        meta
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
