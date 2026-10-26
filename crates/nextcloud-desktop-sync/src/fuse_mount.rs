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
            let _priority = coord.priority_permit();
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

    fn hydrate_directory(
        &self,
        canonical_path: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), VfsError>> + Send + '_>> {
        let path = canonical_path.to_string();
        let coord = self.coordinator.clone();
        Box::pin(async move {
            let _ = coord.refresh_path(&path).await;
            Ok(())
        })
    }
}

impl DesktopSyncCoordinator {
    /// Returns the shared in-memory virtual tree for FUSE, loading from disk cache if not already loaded.
    pub(crate) async fn get_or_create_fuse_tree(&self) -> Arc<RwLock<VirtualTree>> {
        let mut tree_guard = self.fuse_tree.write().await;
        if let Some(ref tree) = *tree_guard {
            return tree.clone();
        }

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
        *tree_guard = Some(shared_tree.clone());
        shared_tree
    }

    /// Mounts a specific sync mount as a FUSE virtual filesystem at `local_target`.
    pub async fn mount_fuse_folder(
        &self,
        mount_id: &str,
        remote_path: &str,
        local_target: &Path,
    ) -> Result<(), SyncError> {
        let mp = local_target.to_path_buf();
        if !mp.exists() {
            std::fs::create_dir_all(&mp).map_err(|e| SyncError::LocalIo {
                path: mp.clone(),
                context: "creating FUSE mountpoint directory".to_string(),
                source: e,
            })?;
        }

        // Unmount any existing session for this mount_id first
        self.unmount_fuse_folder(mount_id).await;

        let shared_tree = self.get_or_create_fuse_tree().await;
        let hydrator = Arc::new(DesktopHydrator::new(self.clone()));
        let mut driver = FuseDriver::with_hydrator_and_root(shared_tree, hydrator, remote_path);
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            driver = driver.with_runtime(rt);
        }

        match mount_virtual_fs(driver, &mp) {
            Ok(session) => {
                info!(
                    mount_id,
                    remote_path,
                    mountpoint = %mp.display(),
                    "FUSE virtual folder mounted successfully"
                );
                self.fuse_sessions
                    .lock()
                    .await
                    .insert(mount_id.to_string(), session);
                Ok(())
            }
            Err(e) => {
                error!(
                    mount_id,
                    remote_path,
                    mountpoint = %mp.display(),
                    error = %e,
                    "Failed to mount FUSE virtual folder"
                );
                Err(SyncError::LocalIo {
                    path: mp,
                    context: "mounting FUSE filesystem".to_string(),
                    source: e,
                })
            }
        }
    }

    /// Unmounts the running FUSE virtual filesystem for `mount_id` if active.
    pub async fn unmount_fuse_folder(&self, mount_id: &str) {
        let mut sessions = self.fuse_sessions.lock().await;
        if let Some(session) = sessions.remove(mount_id) {
            debug!(mount_id, "Unmounting FUSE virtual folder session");
            drop(session);
        }
    }

    /// Mounts the active Nextcloud account's virtual filesystem at `mountpoint`.
    pub async fn mount_fuse_vfs(&self, mountpoint: impl AsRef<Path>) -> Result<(), SyncError> {
        let mp = mountpoint.as_ref().to_path_buf();
        self.mount_fuse_folder("default", "/", &mp).await?;
        *self.fuse_mountpoint.write().await = Some(mp);
        Ok(())
    }

    /// Unmounts the default FUSE virtual filesystem if active.
    pub async fn unmount_fuse_vfs(&self) {
        self.unmount_fuse_folder("default").await;
        *self.fuse_mountpoint.write().await = None;
    }

    /// Unmounts all active FUSE virtual filesystem sessions.
    pub async fn unmount_all_fuse(&self) {
        let mut sessions = self.fuse_sessions.lock().await;
        for (id, session) in sessions.drain() {
            debug!(mount_id = %id, "Unmounting FUSE session");
            drop(session);
        }
        *self.fuse_tree.write().await = None;
        *self.fuse_mountpoint.write().await = None;
    }

    /// Returns `true` if any FUSE virtual filesystem is actively mounted.
    pub async fn is_fuse_mounted(&self) -> bool {
        !self.fuse_sessions.lock().await.is_empty()
    }

    /// Returns `true` if a specific FUSE virtual mount is active.
    pub async fn is_folder_fuse_mounted(&self, mount_id: &str) -> bool {
        self.fuse_sessions.lock().await.contains_key(mount_id)
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
