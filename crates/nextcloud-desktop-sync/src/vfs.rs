//! Virtual synchronization engine for metadata trees and on-demand file hydration.

use nextcloud_rust_sdk::{
    BasicAuth, DavClient, DavPath, DavResource, HydrationState, NodeMetadata, ReqwestTransport,
    ResourceIconCategory, VirtualNode, VirtualTree, normalize_path,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::SyncError;
use nextcloud_desktop_core::events::{DesktopEvent, EventBus};

/// Constructs a normalized [`DavPath`] from user name and relative path.
pub fn make_dav_path(user: &str, relative: &str) -> Result<DavPath, SyncError> {
    let clean = relative.trim_matches('/');
    if clean.is_empty() {
        DavPath::root(user).map_err(|err| SyncError::InvalidPath(err.to_string()))
    } else {
        DavPath::new(user, clean).map_err(|err| SyncError::InvalidPath(err.to_string()))
    }
}

/// Summary representation of a virtual file or directory entry for UI display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VirtualEntrySummary {
    /// Canonical relative path within the virtual mount (e.g. `/Photos/cat.jpg`).
    pub path: String,
    /// Display file or folder name.
    pub name: String,
    /// Whether this entry represents a directory.
    pub is_directory: bool,
    /// File size in bytes (as reported by metadata).
    pub size: u64,
    /// ETag identifier.
    pub etag: Option<String>,
    /// Content MIME type.
    pub mime_type: Option<String>,
    /// Semantic Freedesktop icon category determined by the SDK.
    pub icon_category: ResourceIconCategory,
    /// Whether the file is currently a metadata-only placeholder or hydrated.
    pub is_hydrated: bool,
    /// Local disk cache path if hydrated.
    pub cached_path: Option<PathBuf>,
}

/// Virtual file synchronization manager managing the metadata hierarchy and on-demand hydration.
#[derive(Clone)]
pub struct VirtualSyncEngine {
    tree: Arc<RwLock<VirtualTree>>,
    cache_dir: PathBuf,
    event_bus: EventBus,
}

impl VirtualSyncEngine {
    /// Creates a new virtual sync engine storing cached files in `cache_dir`.
    pub fn new(cache_dir: impl Into<PathBuf>, event_bus: EventBus) -> Self {
        let cache_dir = cache_dir.into();
        let mut initial_tree = VirtualTree::new("Nextcloud");
        seed_initial_nodes(&mut initial_tree);
        Self {
            tree: Arc::new(RwLock::new(initial_tree)),
            cache_dir,
            event_bus,
        }
    }

    /// Resets the virtual tree to the default Nextcloud folder hierarchy.
    pub async fn reset_default_hierarchy(&self) -> usize {
        let mut tree = self.tree.write().await;
        *tree = VirtualTree::new("Nextcloud");
        let count = seed_initial_nodes(&mut tree);
        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        count
    }

    /// Access the local cache root directory for virtual files.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Clears and repopulates the virtual tree from a list of discovered WebDAV resources.
    ///
    /// File nodes are initialized strictly as metadata-only placeholders
    /// without downloading file content upfront.
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

            let meta = if res.is_collection {
                let mut m = NodeMetadata::directory(&clean_path);
                if let Some(ref etag) = res.etag {
                    m = m.with_etag(etag.clone());
                }
                m
            } else {
                let mut m =
                    NodeMetadata::file(&clean_path).with_size(res.content_length.unwrap_or(0));
                if let Some(ref etag) = res.etag {
                    m = m.with_etag(etag.clone());
                }
                if let Some(ref mime) = res.content_type {
                    m = m.with_mime_type(mime.clone());
                }
                if let Some(has_prev) = res.has_preview {
                    m = m.with_has_preview(has_prev);
                }
                m
            };

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
    ///
    /// If the file is already hydrated and present on disk, returns the cached path immediately.
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

        let safe_rel = clean_path.trim_start_matches('/');
        let local_cached = self.cache_dir.join(safe_rel);

        if let Some(parent) = local_cached.parent() {
            fs::create_dir_all(parent).map_err(|err| SyncError::LocalIo {
                path: parent.to_path_buf(),
                context: "creating VFS cache parent directory".to_string(),
                source: err,
            })?;
        }

        fs::write(&local_cached, &bytes).map_err(|err| SyncError::LocalIo {
            path: local_cached.clone(),
            context: "writing hydrated VFS cache file".to_string(),
            source: err,
        })?;

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
        let clean_path = if dir_path.is_empty() || dir_path == "/" {
            "/".to_string()
        } else {
            format!("/{}", dir_path.trim_matches('/'))
        };

        let tree = self.tree.read().await;
        let children = tree.children(&clean_path);

        let summaries = children
            .into_iter()
            .map(|node| VirtualEntrySummary {
                path: node.metadata.path.clone(),
                name: node.metadata.name.clone(),
                is_directory: node.metadata.is_directory,
                size: node.metadata.size,
                etag: node.metadata.etag.clone(),
                mime_type: node.metadata.mime_type.clone(),
                icon_category: node.metadata.icon_category(),
                is_hydrated: node.hydration.is_hydrated(),
                cached_path: node.hydration.cached_path().map(|p| p.to_path_buf()),
            })
            .collect();

        Ok(summaries)
    }

    /// Retrieves summary information for a single node path.
    pub async fn get_entry(&self, path: &str) -> Result<Option<VirtualEntrySummary>, SyncError> {
        let clean_path = normalize_path(path).unwrap_or_else(|_| "/".to_string());

        let tree = self.tree.read().await;
        let entry = tree.get(&clean_path).map(|node| VirtualEntrySummary {
            path: node.metadata.path.clone(),
            name: node.metadata.name.clone(),
            is_directory: node.metadata.is_directory,
            size: node.metadata.size,
            etag: node.metadata.etag.clone(),
            mime_type: node.metadata.mime_type.clone(),
            icon_category: node.metadata.icon_category(),
            is_hydrated: node.hydration.is_hydrated(),
            cached_path: node.hydration.cached_path().map(|p| p.to_path_buf()),
        });

        Ok(entry)
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
        let meta = NodeMetadata::directory(&clean_path);
        tree.insert(VirtualNode::new_directory(meta));
        let count = tree.len();
        drop(tree);

        self.event_bus
            .emit(DesktopEvent::VfsTreeUpdated { node_count: count });
        Ok(())
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

        let safe_rel = clean_path.trim_start_matches('/');
        let local_cached = self.cache_dir.join(safe_rel);

        if let Some(parent) = local_cached.parent() {
            fs::create_dir_all(parent).map_err(|err| SyncError::LocalIo {
                path: parent.to_path_buf(),
                context: "creating VFS cache parent directory".to_string(),
                source: err,
            })?;
        }

        let placeholder_bytes = format!(
            "Nextcloud hydrated file: {}\nSize metadata: {} bytes\n",
            clean_path, expected_size
        )
        .into_bytes();

        fs::write(&local_cached, &placeholder_bytes).map_err(|err| SyncError::LocalIo {
            path: local_cached.clone(),
            context: "writing hydrated VFS cache file".to_string(),
            source: err,
        })?;

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
}

fn seed_initial_nodes(tree: &mut VirtualTree) -> usize {
    let dirs = [
        "/Documents",
        "/Documents/Projects",
        "/Photos",
        "/Music",
        "/Templates",
    ];
    for d in dirs {
        tree.insert(VirtualNode::new_directory(NodeMetadata::directory(d)));
    }

    let files: &[(&str, u64, &str)] = &[
        ("/Nextcloud Manual.pdf", 4_820_400, "application/pdf"),
        ("/Notes.md", 3_420, "text/markdown"),
        (
            "/Documents/Architecture Overview.pdf",
            1_945_600,
            "application/pdf",
        ),
        (
            "/Documents/Q4 Budget 2026.ods",
            86_016,
            "application/vnd.oasis.opendocument.spreadsheet",
        ),
        (
            "/Documents/Keynote Deck.odp",
            6_291_456,
            "application/vnd.oasis.opendocument.presentation",
        ),
        ("/Documents/Projects/Roadmap.md", 12_288, "text/markdown"),
        (
            "/Documents/Projects/Release Bundle.zip",
            18_450_944,
            "application/zip",
        ),
        ("/Photos/Mountain Sunrise.jpg", 3_670_016, "image/jpeg"),
        ("/Photos/Nextcloud Conference.png", 2_150_400, "image/png"),
        ("/Music/Focus Ambient.flac", 24_117_248, "audio/flac"),
        (
            "/Templates/Meeting Notes.odt",
            28_672,
            "application/vnd.oasis.opendocument.text",
        ),
    ];

    for &(path, size, mime) in files {
        let meta = NodeMetadata::file(path)
            .with_size(size)
            .with_mime_type(mime.to_string());
        tree.insert(VirtualNode::new_file(meta));
    }

    dirs.len() + files.len()
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
        let vfs = VirtualSyncEngine::new(&temp_cache, bus);

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

        // Create a folder
        vfs.create_directory("/Documents/NewFolder").await.unwrap();
        let docs_children = vfs.list_children("/Documents").await.unwrap();
        assert!(
            docs_children
                .iter()
                .any(|c| c.name == "NewFolder" && c.is_directory)
        );

        // Hydrate a file placeholder locally using VirtualTree::hydrate_path
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

        // Dehydrate back to placeholder
        vfs.dehydrate_file("/Notes.md")
            .await
            .expect("dehydrate placeholder");
        assert!(!cached.exists());

        // Delete resource
        vfs.remove_entry("/Documents/NewFolder")
            .await
            .expect("delete folder");
        let docs_after = vfs.list_children("/Documents").await.unwrap();
        assert!(!docs_after.iter().any(|c| c.name == "NewFolder"));

        let _ = fs::remove_dir_all(&temp_cache);
    }
}
