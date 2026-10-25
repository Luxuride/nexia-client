//! On-demand virtual filesystem mounting via FUSE.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use nextcloud_desktop_core::events::DesktopEvent;
use nextcloud_rust_sdk::{AsyncHydrator, FuseDriver, VfsError, VirtualTree, mount_virtual_fs};
use tracing::{debug, error, info};

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use crate::file_ops::make_dav_path;

/// On-demand file hydrator pulling payloads from Nextcloud WebDAV for FUSE reads.
pub struct DesktopHydrator {
    coordinator: DesktopSyncCoordinator,
}

impl DesktopHydrator {
    /// Creates a new hydrator attached to `coordinator`.
    pub fn new(coordinator: DesktopSyncCoordinator) -> Self {
        Self { coordinator }
    }
}

impl AsyncHydrator for DesktopHydrator {
    fn hydrate(
        &self,
        canonical_path: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, VfsError>> + Send + '_>> {
        let path = canonical_path.to_string();
        let coord = self.coordinator.clone();

        Box::pin(async move {
            let Some((user, client, auth)) = coord.active_dav_context().await else {
                return Err(VfsError::NotFound(path));
            };

            let dav_client = client.dav(&auth);
            let dav_path =
                make_dav_path(&user, &path).map_err(|e| VfsError::InvalidPath(e.to_string()))?;

            info!(path = %path, "Downloading remote file payload for virtual FUSE mount");
            match dav_client.get(&dav_path).await {
                Ok(bytes) => {
                    coord.event_bus().emit(DesktopEvent::NodeHydrated {
                        path: path.clone(),
                        size: bytes.len() as u64,
                    });
                    Ok(bytes)
                }
                Err(e) => {
                    error!(path = %path, error = %e, "Failed to hydrate file for virtual FUSE mount");
                    Err(VfsError::NotFound(path))
                }
            }
        })
    }

    fn on_readdir(&self, paths: &[String]) {
        let coord = self.coordinator.clone();
        let paths = paths.to_vec();
        tokio::spawn(async move {
            coord.batch_prefetch_thumbnails(paths, 256);
        });
    }

    fn on_directory_read(&self, path: &str) {
        let coord = self.coordinator.clone();
        let p = path.to_string();
        tokio::spawn(async move {
            let _ = coord.refresh_path(&p).await;
        });
    }
}

impl DesktopSyncCoordinator {
    /// Mounts the active Nextcloud account's virtual filesystem at `mountpoint`.
    pub async fn mount_fuse_vfs(&self, mountpoint: impl AsRef<Path>) -> Result<(), SyncError> {
        let mp = mountpoint.as_ref().to_path_buf();
        if !mp.exists() {
            std::fs::create_dir_all(&mp).map_err(|e| SyncError::LocalIo {
                path: mp.clone(),
                context: "creating FUSE mountpoint directory".to_string(),
                source: e,
            })?;
        }

        // Unmount any existing session first
        self.unmount_fuse_vfs().await;

        let active_account = self.active_account().await;
        let account_id_str = active_account
            .as_ref()
            .map(|a| a.id.0.clone())
            .unwrap_or_else(|| "default".to_string());

        let vfs_dir = self.account_vfs_dir(&account_id_str);
        let tree_file = vfs_dir.join("tree.json");

        let tree = if tree_file.exists() {
            std::fs::read_to_string(&tree_file)
                .ok()
                .and_then(|data| serde_json::from_str(&data).ok())
                .unwrap_or_else(|| VirtualTree::new("Nextcloud"))
        } else {
            VirtualTree::new("Nextcloud")
        };

        let shared_tree = Arc::new(RwLock::new(tree));
        *self.fuse_tree.write().await = Some(shared_tree.clone());

        let hydrator = Arc::new(DesktopHydrator::new(self.clone()));
        let driver = FuseDriver::with_hydrator(shared_tree, hydrator);

        match mount_virtual_fs(driver, &mp) {
            Ok(session) => {
                info!(mountpoint = %mp.display(), "Virtual FUSE filesystem mounted successfully");
                *self.fuse_session.lock().await = Some(session);
                *self.fuse_mountpoint.write().await = Some(mp);
                Ok(())
            }
            Err(e) => {
                error!(mountpoint = %mp.display(), error = %e, "Failed to mount FUSE virtual filesystem");
                Err(SyncError::LocalIo {
                    path: mp,
                    context: "mounting FUSE filesystem".to_string(),
                    source: e,
                })
            }
        }
    }

    /// Unmounts the running FUSE virtual filesystem if active.
    pub async fn unmount_fuse_vfs(&self) {
        let mut session_guard = self.fuse_session.lock().await;
        if let Some(session) = session_guard.take() {
            debug!("Unmounting FUSE virtual filesystem session");
            drop(session);
        }
        *self.fuse_tree.write().await = None;
        *self.fuse_mountpoint.write().await = None;
    }

    /// Returns `true` if the FUSE virtual filesystem is actively mounted.
    pub async fn is_fuse_mounted(&self) -> bool {
        self.fuse_session.lock().await.is_some()
    }

    /// Returns the active FUSE mountpoint path if mounted.
    pub async fn fuse_mountpoint(&self) -> Option<PathBuf> {
        self.fuse_mountpoint.read().await.clone()
    }

    /// Returns the default FUSE mountpoint for the active account.
    pub async fn default_fuse_mountpoint(&self) -> PathBuf {
        if let Some(acc) = self.active_account().await {
            dirs::home_dir()
                .map(|h| h.join(format!("Nextcloud-{}", acc.name)))
                .unwrap_or_else(|| self.account_dir(&acc.id.0).join("mount"))
        } else {
            dirs::home_dir()
                .map(|h| h.join("Nextcloud"))
                .unwrap_or_else(|| self.cache_dir().join("mount"))
        }
    }
}
