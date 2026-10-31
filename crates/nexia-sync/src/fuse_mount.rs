//! On-demand virtual filesystem mounting via FUSE.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use nexia_core::events::DesktopEvent;
use nexia_nextcloud_sdk::{
    AsyncHydrator, BackgroundSession, FuseDriver, VfsError, VirtualTree, mount_virtual_fs,
};
use tracing::{debug, error, info, warn};

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use crate::file_ops::make_dav_path;

/// On-demand file hydrator pulling payloads from the server over WebDAV for FUSE reads.
pub struct DesktopHydrator {
    coordinator: DesktopSyncCoordinator,
    runtime: Option<tokio::runtime::Handle>,
}

impl DesktopHydrator {
    /// Creates a new hydrator attached to `coordinator`.
    pub fn new(coordinator: DesktopSyncCoordinator) -> Self {
        Self {
            coordinator,
            runtime: tokio::runtime::Handle::try_current().ok(),
        }
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

    fn hydrate_range(
        &self,
        canonical_path: &str,
        offset: u64,
        size: u32,
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

            let end = offset.saturating_add(size as u64).saturating_sub(1);
            match dav_client.get_range(&dav_path, offset, end).await {
                Ok(bytes) => Ok(bytes),
                Err(e) => {
                    debug!(path = %path, offset, size, error = %e, "Range request failed, falling back to full hydration");
                    match dav_client.get(&dav_path).await {
                        Ok(full) => {
                            let start = (offset as usize).min(full.len());
                            let end = (start + size as usize).min(full.len());
                            Ok(full[start..end].to_vec())
                        }
                        Err(_) => Err(VfsError::NotFound(path)),
                    }
                }
            }
        })
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

    fn on_readdir(&self, paths: &[String]) {
        let coord = self.coordinator.clone();
        let paths = paths.to_vec();
        if let Some(ref rt) = self.runtime {
            rt.spawn(async move {
                coord.batch_prefetch_thumbnails(paths, 256);
            });
        }
    }

    fn on_directory_read(&self, path: &str) {
        let coord = self.coordinator.clone();
        let p = path.to_string();
        if let Some(ref rt) = self.runtime {
            rt.spawn(async move {
                let _ = coord.refresh_path(&p).await;
            });
        }
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

        let mut tree = if tree_file.exists() {
            std::fs::read_to_string(&tree_file)
                .ok()
                .and_then(|data| serde_json::from_str(&data).ok())
                .unwrap_or_else(|| VirtualTree::new("Nextcloud"))
        } else {
            VirtualTree::new("Nextcloud")
        };

        // Migrate a tree written by an older build: payloads were serialized inline as JSON number
        // arrays and one account's file reached 483 MB, which made start-up parse half a gigabyte.
        // Spilling the payloads into the cache and rewriting the file happens once.
        if tree.has_in_memory_payloads() {
            tracing::info!(
                path = %tree_file.display(),
                "compacting virtual tree saved by an older build"
            );
            let _ = self.persist_account_tree(&account_id_str, &mut tree);
        }

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

        // A live session at this path is already serving reads; remounting it would drop every
        // cached block and start the kernel over on a directory that is in use.
        if self.fuse_sessions.lock().await.contains_key(mount_id) && is_mountpoint(&mp) {
            debug!(
                mount_id,
                mountpoint = %mp.display(),
                "FUSE mount already active, skipping remount"
            );
            return Ok(());
        }

        // Mounting tears down whatever is live before mounting again, so two overlapping callers
        // (the start-up refresh and the account bootstrap both mount the same mount_id) would
        // unmount and mount twice and leave a second session stacked on a mounted path. The claim
        // is taken and released without holding the lock across an await.
        let claim = mp.to_string_lossy().to_string();
        {
            let mut claiming = self
                .fuse_mounting
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !claiming.insert(claim.clone()) {
                debug!(
                    mount_id,
                    mountpoint = %mp.display(),
                    "FUSE mount already in progress, skipping duplicate request"
                );
                return Ok(());
            }
        }

        let result = self
            .mount_claimed_fuse_folder(mount_id, remote_path, &mp)
            .await;

        self.fuse_mounting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&claim);
        result
    }

    /// Mounts `mp` for `mount_id`. The caller has already claimed `mp` in `fuse_mounting`.
    async fn mount_claimed_fuse_folder(
        &self,
        mount_id: &str,
        remote_path: &str,
        mp: &Path,
    ) -> Result<(), SyncError> {
        // Unmount any existing session for this mount_id first
        self.unmount_fuse_folder(mount_id).await;
        let _ = std::process::Command::new("fusermount3")
            .args(["-u", "-z", &mp.to_string_lossy()])
            .status();

        let shared_tree = self.get_or_create_fuse_tree().await;
        let hydrator = Arc::new(DesktopHydrator::new(self.clone()));
        let mut driver = FuseDriver::with_hydrator_and_root(shared_tree, hydrator, remote_path);
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            driver = driver.with_runtime(rt);
        }

        match mount_virtual_fs(driver, mp) {
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
                let coord = self.clone();
                tokio::spawn(async move {
                    coord.preload_cached_freedesktop_thumbnails().await;
                });
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
                    path: mp.to_path_buf(),
                    context: "mounting FUSE filesystem".to_string(),
                    source: e,
                })
            }
        }
    }

    /// Stops a FUSE session: unmounts the kernel side and joins the background thread.
    ///
    /// `BackgroundSession` has no `Drop` implementation that joins, so dropping it only detaches
    /// the `fuser-bg` thread: the filesystem stays mounted and the driver, its inode table and the
    /// open mountpoint leak for the rest of the process. The join blocks, so it runs on the
    /// blocking pool instead of stalling the runtime.
    async fn stop_fuse_session(session: BackgroundSession, mount_id: &str) {
        match tokio::task::spawn_blocking(move || session.umount_and_join()).await {
            Ok(Ok(())) => debug!(mount_id, "FUSE session unmounted and joined"),
            Ok(Err(err)) => {
                warn!(mount_id, error = %err, "FUSE session did not shut down cleanly")
            }
            Err(err) => warn!(mount_id, error = %err, "FUSE session join task failed"),
        }
    }

    /// Unmounts the running FUSE virtual filesystem for `mount_id` if active.
    pub async fn unmount_fuse_folder(&self, mount_id: &str) {
        let session = self.fuse_sessions.lock().await.remove(mount_id);
        if let Some(session) = session {
            debug!(mount_id, "Unmounting FUSE virtual folder session");
            Self::stop_fuse_session(session, mount_id).await;
        }
        for m in self.sync_mounts().await {
            if m.id == mount_id {
                let _ = std::process::Command::new("fusermount3")
                    .args(["-u", "-z", &m.local_target])
                    .status();
            }
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
        let sessions: Vec<(String, BackgroundSession)> =
            self.fuse_sessions.lock().await.drain().collect();
        for (id, session) in sessions {
            debug!(mount_id = %id, "Unmounting FUSE session");
            Self::stop_fuse_session(session, &id).await;
        }
        for m in self.sync_mounts().await {
            if m.mode == "Fuse" {
                let _ = std::process::Command::new("fusermount3")
                    .args(["-u", "-z", &m.local_target])
                    .status();
            }
        }
        if let Some(ref mp) = *self.fuse_mountpoint.read().await {
            let _ = std::process::Command::new("fusermount3")
                .args(["-u", "-z", &mp.to_string_lossy()])
                .status();
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
                .map(|h| h.join(format!("Nexia-{}", acc.name)))
                .unwrap_or_else(|| self.account_dir(&acc.id.0).join("mount"))
        } else {
            dirs::home_dir()
                .map(|h| h.join("Nexia"))
                .unwrap_or_else(|| self.cache_dir().join("mount"))
        }
    }
}

/// Returns `true` if `path` is currently a mountpoint according to `/proc/self/mountinfo`.
fn is_mountpoint(path: &Path) -> bool {
    let Ok(info) = std::fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    // Compare resolved paths: a mount created through a symlinked path (a home under /home that is
    // really /var/home) is recorded under its target, so a literal comparison would answer `false`
    // for the mount that was just created.
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let wanted = mountinfo_escape(&resolved.to_string_lossy());
    info.lines()
        .any(|line| line.split_whitespace().nth(4) == Some(wanted.as_str()))
}

/// Escapes a path the way `/proc/self/mountinfo` does, so it can be compared against its fields.
fn mountinfo_escape(path: &str) -> String {
    path.replace('\\', "\\134")
        .replace(' ', "\\040")
        .replace('\t', "\\011")
        .replace('\n', "\\012")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mountinfo_escape_matches_kernel_notation() {
        assert_eq!(
            mountinfo_escape("/home/lux/Pictures/Nextcloud"),
            "/home/lux/Pictures/Nextcloud"
        );
        assert_eq!(mountinfo_escape("/media/My Disk"), "/media/My\\040Disk");
        assert_eq!(mountinfo_escape("/a\\b"), "/a\\134b");
    }

    #[test]
    fn is_mountpoint_accepts_the_root_filesystem() {
        // `/` is a mountpoint on any Linux system, and a path that is definitely not mounted must
        // be rejected, so the check has to discriminate rather than always answer one way.
        assert!(is_mountpoint(Path::new("/")));
        assert!(!is_mountpoint(Path::new("/this/path/is/not/a/mountpoint")));
    }

    #[test]
    fn is_mountpoint_resolves_symlinked_paths() {
        let link =
            std::env::temp_dir().join(format!("nexia-mountpoint-link-{}", std::process::id()));
        let _ = std::fs::remove_file(&link);
        if std::os::unix::fs::symlink("/", &link).is_err() {
            return;
        }

        // The mount is recorded under `/`, so the check only answers `true` for the link once the
        // path is resolved before comparing.
        assert!(is_mountpoint(&link));

        let _ = std::fs::remove_file(&link);
    }
}
