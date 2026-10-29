//! Two-way folder synchronization engine coordinating local and remote directory pairings.

use std::path::PathBuf;

use tracing::info;

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
pub use crate::freedesktop_thumbs::path_to_file_uri;
use nexia_core::events::DesktopEvent;
use nextcloud_rust_sdk::{
    AuthProvider, DavClient, DiscoveredRemoteFile, FolderSyncOutcome, FolderSyncRunner, MountId,
    MountProgressSnapshot, MountStatus, Transport,
};

/// High-level engine executing synchronization cycles for local and remote folder pairings.
#[derive(Clone)]
pub struct FolderSyncEngine {
    coordinator: DesktopSyncCoordinator,
}

impl std::fmt::Debug for FolderSyncEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FolderSyncEngine").finish()
    }
}

impl FolderSyncEngine {
    /// Creates a new `FolderSyncEngine` bound to the given coordinator.
    #[must_use]
    pub fn new(coordinator: DesktopSyncCoordinator) -> Self {
        Self { coordinator }
    }

    /// Recursively discovers all items on the remote WebDAV instance under `remote_root`.
    pub async fn discover_remote<T: Transport, A: AuthProvider>(
        dav_client: &DavClient<'_, T, A>,
        user: &str,
        remote_root: &str,
    ) -> Result<Vec<DiscoveredRemoteFile>, SyncError> {
        FolderSyncRunner::discover_remote(dav_client, user, remote_root)
            .await
            .map_err(SyncError::Orchestrator)
    }

    /// Synchronizes a single sync mount by ID.
    pub async fn sync_mount(&self, mount_id: &str) -> Result<usize, SyncError> {
        let _priority = self.coordinator.priority_permit();
        if mount_id.starts_with("app-") {
            return Ok(0);
        }

        let m_id_typed = MountId::from(mount_id.to_string());
        if self
            .coordinator
            .is_mount_paused(m_id_typed.clone())
            .await
            .unwrap_or(false)
            || self.coordinator.is_sync_paused().await.unwrap_or(false)
        {
            info!(
                mount_id,
                "Folder synchronization skipped: mount or global sync is paused"
            );
            return Err(SyncError::OperationDisabled(
                "Synchronization is currently paused".into(),
            ));
        }

        let mounts = self.coordinator.sync_mounts().await;
        let mount = match mounts.into_iter().find(|m| m.id == mount_id) {
            Some(m) if m.enabled && m.mode != "Fuse" => m,
            _ => return Ok(0),
        };

        let Some((user, client, auth)) = self.coordinator.active_dav_context().await else {
            return Ok(0);
        };
        let dav_client = client.dav(&auth);

        let local_target = PathBuf::from(&mount.local_target);
        if !local_target.exists() {
            std::fs::create_dir_all(&local_target).map_err(|e| SyncError::LocalIo {
                path: local_target.clone(),
                context: "creating local sync root directory".to_string(),
                source: e,
            })?;
        }

        let mut progress = MountProgressSnapshot::new(m_id_typed.clone(), MountStatus::Syncing);
        self.coordinator
            .event_bus
            .emit(DesktopEvent::MountProgress(progress.clone()));

        info!(
            mount_id,
            remote = %mount.remote_path,
            local = %local_target.display(),
            "Starting folder synchronization cycle"
        );

        let journal_path = self
            .coordinator
            .mount_journal_path(&mount.account_id, mount_id);
        let is_vfs = mount.mode == "VirtualVfs";

        let runner = FolderSyncRunner::new(&local_target, &mount.remote_path, &journal_path);

        self.coordinator.event_bus.emit(DesktopEvent::SyncStarted {
            pairing_id: mount_id.to_string(),
        });

        let coord = self.coordinator.clone();
        let m_id_str = mount_id.to_string();
        let is_cancelled = move || {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(async { coord.is_mount_cancelled(&m_id_str).await })
            })
        };

        let coord_prog = self.coordinator.clone();
        let m_id_prog = m_id_typed.clone();
        let on_progress = move |completed: usize, total: usize, rel_path: &str| {
            let mut prog = MountProgressSnapshot::new(m_id_prog.clone(), MountStatus::Syncing);
            prog.total_items = total;
            prog.completed_items = completed;
            coord_prog.set_mount_progress(prog.clone());
            coord_prog.event_bus.emit(DesktopEvent::MountProgress(prog));
            coord_prog
                .event_bus
                .emit(DesktopEvent::FileTransferCompleted {
                    relative_path: rel_path.to_string(),
                    kind: nexia_core::events::TransferKind::Hydration,
                });
        };

        let outcome: FolderSyncOutcome = runner
            .execute_sync_cycle(&dav_client, &user, is_vfs, is_cancelled, on_progress)
            .await
            .map_err(SyncError::Orchestrator)?;

        if self.coordinator.is_mount_cancelled(mount_id).await {
            info!(
                mount_id,
                "Sync mount was cancelled; skipping thumbnail prefetching and progress"
            );
            return Ok(outcome.downloaded);
        }

        // Prefetch thumbnails for candidates if needed
        if let Ok(entries) =
            FolderSyncRunner::discover_remote(&dav_client, &user, &mount.remote_path).await
        {
            let candidates: Vec<(String, Option<u64>)> = entries
                .iter()
                .filter(|e| !e.is_directory && is_thumbnail_candidate(&e.relative_path))
                .map(|e| {
                    let path = if mount.remote_path == "/" {
                        e.relative_path.clone()
                    } else {
                        format!(
                            "{}/{}",
                            mount.remote_path.trim_end_matches('/'),
                            e.relative_path.trim_start_matches('/')
                        )
                    };
                    (path, e.file_id)
                })
                .collect();
            if !candidates.is_empty() {
                self.coordinator
                    .batch_prefetch_thumbnails_with_ids(candidates, 256);
            }
        }

        progress.status = MountStatus::Idle;
        progress.completed_items = progress.total_items;
        self.coordinator.set_mount_progress(progress.clone());
        self.coordinator
            .event_bus
            .emit(DesktopEvent::MountProgress(progress));
        self.coordinator
            .event_bus
            .emit(DesktopEvent::SyncCompleted {
                pairing_id: mount_id.to_string(),
                uploaded: outcome.uploaded,
                downloaded: outcome.downloaded,
                conflicts: outcome.conflicts,
            });

        info!(
            mount_id,
            downloaded = outcome.downloaded,
            uploaded = outcome.uploaded,
            conflicts = outcome.conflicts,
            "Folder synchronization cycle completed successfully"
        );

        Ok(outcome.downloaded + outcome.uploaded)
    }

    /// Synchronizes all configured active folder mounts across the account.
    pub async fn sync_all_mounts(&self) {
        let _priority = self.coordinator.priority_permit();
        let mounts = self.coordinator.sync_mounts().await;
        for mount in mounts {
            if mount.id.starts_with("app-") || !mount.enabled || mount.mode == "Fuse" {
                continue;
            }
            let m_id = MountId::from(mount.id.clone());
            if self
                .coordinator
                .is_mount_paused(m_id)
                .await
                .unwrap_or(false)
            {
                continue;
            }
            let _ = self.sync_mount(&mount.id).await;
        }
    }
}

pub(crate) fn extract_clean_dav_path(href: &str, base_dav_prefix: &str) -> String {
    let decoded = href;
    let prefix = base_dav_prefix.trim_end_matches('/');

    let path_after_prefix = if let Some(idx) = decoded.find(prefix) {
        &decoded[idx + prefix.len()..]
    } else if let Some(idx) = decoded.find("/remote.php/dav/files/") {
        let after_files = &decoded[idx + "/remote.php/dav/files/".len()..];
        if let Some(slash) = after_files.find('/') {
            &after_files[slash..]
        } else {
            ""
        }
    } else if let Some(idx) = decoded.find("/remote.php/webdav") {
        &decoded[idx + "/remote.php/webdav".len()..]
    } else if let Some(p) = decoded.strip_prefix(prefix) {
        p
    } else if let Some(idx) = decoded.find("://") {
        let after_scheme = &decoded[idx + 3..];
        if let Some(first_slash) = after_scheme.find('/') {
            &after_scheme[first_slash..]
        } else {
            decoded
        }
    } else {
        decoded
    };

    let trimmed = path_after_prefix.trim_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", trimmed)
    }
}

pub(crate) fn is_thumbnail_candidate(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".svg")
        || lower.ends_with(".bmp")
        || lower.ends_with(".tiff")
        || lower.ends_with(".mp4")
        || lower.ends_with(".mkv")
        || lower.ends_with(".webm")
        || lower.ends_with(".mov")
        || lower.ends_with(".avi")
        || lower.ends_with(".pdf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_candidate_matching() {
        assert!(is_thumbnail_candidate("picture.png"));
        assert!(is_thumbnail_candidate("video.mp4"));
        assert!(!is_thumbnail_candidate("text.txt"));
    }
}
