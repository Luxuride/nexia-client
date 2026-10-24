//! Virtual synchronization engine for metadata trees and on-demand file hydration.

use nextcloud_auth::BasicAuth;
use nextcloud_dav::{DavClient, DavPath, DavResource};
use nextcloud_sdk_vfs::{HydratedContent, HydrationState, NodeMetadata, VirtualNode, VirtualTree};
use nextcloud_transport::ReqwestTransport;
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
        Self {
            tree: Arc::new(RwLock::new(VirtualTree::new("Nextcloud"))),
            cache_dir,
            event_bus,
        }
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
            if let Some(node) = tree.get_mut(&clean_path) {
                node.hydration =
                    HydrationState::Hydrated(HydratedContent::Path(local_cached.clone()));
            }
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
                is_hydrated: node.hydration.is_hydrated(),
                cached_path: node.hydration.cached_path().map(|p| p.to_path_buf()),
            })
            .collect();

        Ok(summaries)
    }

    /// Retrieves summary information for a single node path.
    pub async fn get_entry(&self, path: &str) -> Result<Option<VirtualEntrySummary>, SyncError> {
        let clean_path = if path.is_empty() || path == "/" {
            "/".to_string()
        } else {
            format!("/{}", path.trim_matches('/'))
        };

        let tree = self.tree.read().await;
        let entry = tree.get(&clean_path).map(|node| VirtualEntrySummary {
            path: node.metadata.path.clone(),
            name: node.metadata.name.clone(),
            is_directory: node.metadata.is_directory,
            size: node.metadata.size,
            etag: node.metadata.etag.clone(),
            mime_type: node.metadata.mime_type.clone(),
            is_hydrated: node.hydration.is_hydrated(),
            cached_path: node.hydration.cached_path().map(|p| p.to_path_buf()),
        });

        Ok(entry)
    }
}
