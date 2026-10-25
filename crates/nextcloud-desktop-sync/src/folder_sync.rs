//! Two-way folder synchronization engine coordinating local and remote directory pairings.

use std::path::PathBuf;
use std::time::SystemTime;

use tracing::{error, info, warn};

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use crate::file_ops::make_dav_path;
use nextcloud_desktop_core::events::DesktopEvent;
use nextcloud_rust_sdk::{
    AuthProvider, DavClient, Depth, DiscoveredRemoteFile, JournalEntry, MountId,
    MountProgressSnapshot, MountStatus, PropfindBody, SyncAction, SyncJournal, Transport,
    reconcile, scan_local_directory,
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
        let dav_prefix = format!("/remote.php/dav/files/{user}");
        let mut collected = Vec::new();
        let mut dir_queue = vec![remote_root.to_string()];
        let mut visited = std::collections::HashSet::new();
        let body = PropfindBody::all();

        while let Some(current_dir) = dir_queue.pop() {
            let clean_current = if current_dir.is_empty() || current_dir == "/" {
                "/".to_string()
            } else {
                format!("/{}", current_dir.trim_matches('/'))
            };

            if !visited.insert(clean_current.clone()) {
                continue;
            }

            let dav_path = make_dav_path(user, &clean_current)?;
            let resources = match dav_client.propfind(&dav_path, Depth::One, &body).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(dir = %clean_current, error = %e, "WebDAV propfind failed during discovery");
                    continue;
                }
            };

            for res in resources {
                let decoded_href = percent_decode(&res.href);
                let path_after_prefix = if let Some(idx) = decoded_href.find(&dav_prefix) {
                    &decoded_href[idx + dav_prefix.len()..]
                } else if let Some(p) = decoded_href.strip_prefix(&dav_prefix) {
                    p
                } else {
                    continue;
                };

                let clean_path = format!("/{}", path_after_prefix.trim_matches('/'));

                // Skip the queried collection directory itself
                if clean_path == clean_current || (clean_path == "/" && clean_current == "/") {
                    continue;
                }

                // Calculate relative path within the mount root
                let clean_mount_root = if remote_root.is_empty() || remote_root == "/" {
                    "/".to_string()
                } else {
                    format!("/{}", remote_root.trim_matches('/'))
                };

                let relative_path = if clean_mount_root == "/" {
                    clean_path.clone()
                } else if let Some(rel) = clean_path.strip_prefix(&clean_mount_root) {
                    format!("/{}", rel.trim_matches('/'))
                } else {
                    continue;
                };

                let is_col = res.is_collection;
                let size = res.content_length.unwrap_or(0);
                let etag = res.etag.clone();

                collected.push(DiscoveredRemoteFile {
                    relative_path: relative_path.clone(),
                    href: res.href.clone(),
                    size,
                    etag,
                    is_directory: is_col,
                });

                if is_col && !visited.contains(&clean_path) {
                    dir_queue.push(clean_path);
                }
            }
        }

        Ok(collected)
    }

    /// Synchronizes a single sync mount by ID.
    pub async fn sync_mount(&self, mount_id: &str) -> Result<usize, SyncError> {
        if mount_id == "app-default-vfs" {
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
            Some(m) if m.enabled => m,
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

        let remote_files = Self::discover_remote(&dav_client, &user, &mount.remote_path).await?;
        let local_files = scan_local_directory(&local_target).map_err(SyncError::SdkSync)?;

        let cache_dir = self.coordinator.config.read().await.resolved_cache_dir();
        let mount_cache_dir = cache_dir.join("mounts").join(mount_id);
        if !mount_cache_dir.exists() {
            let _ = std::fs::create_dir_all(&mount_cache_dir);
        }
        let journal_path = mount_cache_dir.join("journal.json");
        let mut journal =
            SyncJournal::load_from_file(&journal_path).unwrap_or_else(|_| SyncJournal::new());

        let actions = reconcile(&local_files, &remote_files, &journal);
        let is_vfs = mount.mode == "VirtualVfs";
        let total_actions = actions.len();
        progress.total_items = total_actions;
        self.coordinator.set_mount_progress(progress.clone());
        self.coordinator
            .event_bus
            .emit(DesktopEvent::MountProgress(progress.clone()));
        self.coordinator.event_bus.emit(DesktopEvent::SyncStarted {
            pairing_id: mount_id.to_string(),
        });

        let mut downloaded = 0;
        let mut uploaded = 0;
        let mut conflicts = 0;

        // Partition directory creations first so parents exist on disk and remote
        let (dirs, file_actions): (Vec<_>, Vec<_>) = actions.into_iter().partition(|a| {
            matches!(
                a,
                SyncAction::CreateLocalDirectory { .. } | SyncAction::CreateRemoteDirectory { .. }
            )
        });

        for action in dirs.into_iter().chain(file_actions) {
            match action {
                SyncAction::CreateLocalDirectory { relative_path } => {
                    let dest = local_target.join(relative_path.trim_start_matches('/'));
                    if let Err(e) = std::fs::create_dir_all(&dest) {
                        error!(
                            path = %dest.display(),
                            error = %e,
                            "Failed to create local sync directory"
                        );
                    } else {
                        journal.record(JournalEntry::new(
                            &relative_path,
                            0,
                            SystemTime::now(),
                            true,
                        ));
                    }
                }
                SyncAction::Download {
                    relative_path,
                    size: _,
                    etag,
                } => {
                    let dest = local_target.join(relative_path.trim_start_matches('/'));
                    if let Some(parent) = dest.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let full_remote = if mount.remote_path == "/" {
                        relative_path.clone()
                    } else {
                        format!(
                            "{}/{}",
                            mount.remote_path.trim_end_matches('/'),
                            relative_path.trim_start_matches('/')
                        )
                    };
                    if is_vfs {
                        // In VirtualVfs mode, do not download bulk payload; create placeholder
                        if !dest.exists() {
                            let _ = std::fs::File::create(&dest);
                        }
                        let mtime = std::fs::metadata(&dest)
                            .and_then(|m| m.modified())
                            .unwrap_or(SystemTime::now());
                        let mut entry = JournalEntry::new(&relative_path, 0, mtime, false);
                        if let Some(e) = etag {
                            entry = entry.with_remote_etag(e);
                        }
                        journal.record(entry);
                        downloaded += 1;
                        progress.completed_items += 1;
                        self.coordinator.set_mount_progress(progress.clone());
                        self.coordinator
                            .event_bus
                            .emit(DesktopEvent::MountProgress(progress.clone()));
                        self.coordinator
                            .event_bus
                            .emit(DesktopEvent::FileTransferCompleted {
                                relative_path: relative_path.clone(),
                                kind: nextcloud_desktop_core::events::TransferKind::Hydration,
                            });

                        // Prefetch Nextcloud thumbnail and register with FreeDesktop system thumbnail cache
                        if is_thumbnail_candidate(&relative_path) {
                            let coord_for_thumb = self.coordinator.clone();
                            let full_rem_thumb = full_remote.clone();
                            let dest_thumb = dest.clone();
                            tokio::spawn(async move {
                                if let Ok(Some(thumb_path)) =
                                    coord_for_thumb.fetch_thumbnail(&full_rem_thumb, 256).await
                                    && let Ok(bytes) = tokio::fs::read(&thumb_path).await
                                {
                                    crate::freedesktop_thumbs::install_freedesktop_thumbnail(
                                        &dest_thumb,
                                        &bytes,
                                    );
                                }
                            });
                        }
                    } else {
                        if let Ok(dav_path) = make_dav_path(&user, &full_remote) {
                            match dav_client.get(&dav_path).await {
                                Ok(bytes) => {
                                    if let Err(e) = std::fs::write(&dest, &bytes) {
                                        error!(
                                            dest = %dest.display(),
                                            error = %e,
                                            "Failed to write downloaded sync file"
                                        );
                                    } else {
                                        let mtime = std::fs::metadata(&dest)
                                            .and_then(|m| m.modified())
                                            .unwrap_or(SystemTime::now());
                                        let mut entry = JournalEntry::new(
                                            &relative_path,
                                            bytes.len() as u64,
                                            mtime,
                                            false,
                                        );
                                        if let Some(e) = etag {
                                            entry = entry.with_remote_etag(e);
                                        }
                                        journal.record(entry);
                                        downloaded += 1;
                                        progress.completed_items += 1;
                                        progress.bytes_transferred += bytes.len() as u64;
                                        self.coordinator.set_mount_progress(progress.clone());
                                        self.coordinator
                                            .event_bus
                                            .emit(DesktopEvent::MountProgress(progress.clone()));
                                        self.coordinator
                                            .event_bus
                                            .emit(DesktopEvent::FileTransferCompleted {
                                                relative_path: relative_path.clone(),
                                                kind: nextcloud_desktop_core::events::TransferKind::Download,
                                            });
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        path = %full_remote,
                                        error = %e,
                                        "Failed to download remote file during sync"
                                    );
                                }
                            }
                        }
                    }
                }
                SyncAction::CreateRemoteDirectory { relative_path } => {
                    let full_remote = if mount.remote_path == "/" {
                        relative_path.clone()
                    } else {
                        format!(
                            "{}/{}",
                            mount.remote_path.trim_end_matches('/'),
                            relative_path.trim_start_matches('/')
                        )
                    };
                    if let Ok(dav_path) = make_dav_path(&user, &full_remote) {
                        let _ = dav_client.mkcol(&dav_path).await;
                        journal.record(JournalEntry::new(
                            &relative_path,
                            0,
                            SystemTime::now(),
                            true,
                        ));
                    }
                }
                SyncAction::Upload {
                    relative_path,
                    local_path,
                    size,
                } => {
                    let full_remote = if mount.remote_path == "/" {
                        relative_path.clone()
                    } else {
                        format!(
                            "{}/{}",
                            mount.remote_path.trim_end_matches('/'),
                            relative_path.trim_start_matches('/')
                        )
                    };
                    if let Ok(content) = std::fs::read(&local_path)
                        && let Ok(dav_path) = make_dav_path(&user, &full_remote)
                    {
                        match dav_client.put(&dav_path, content, None).await {
                            Ok(_) => {
                                let mtime = std::fs::metadata(&local_path)
                                    .and_then(|m| m.modified())
                                    .unwrap_or(SystemTime::now());
                                let entry = JournalEntry::new(&relative_path, size, mtime, false);
                                journal.record(entry);
                                uploaded += 1;
                                progress.completed_items += 1;
                                progress.bytes_transferred += size;
                                self.coordinator.set_mount_progress(progress.clone());
                                self.coordinator
                                    .event_bus
                                    .emit(DesktopEvent::MountProgress(progress.clone()));
                                self.coordinator.event_bus.emit(
                                    DesktopEvent::FileTransferCompleted {
                                        relative_path: relative_path.clone(),
                                        kind: nextcloud_desktop_core::events::TransferKind::Upload,
                                    },
                                );
                            }
                            Err(e) => {
                                error!(
                                    path = %full_remote,
                                    error = %e,
                                    "Failed to upload local file during sync"
                                );
                            }
                        }
                    }
                }
                SyncAction::DeleteLocal {
                    relative_path,
                    local_path,
                    is_directory,
                } => {
                    if is_directory {
                        let _ = std::fs::remove_dir_all(&local_path);
                    } else {
                        let _ = std::fs::remove_file(&local_path);
                    }
                    journal.remove(&relative_path);
                    progress.completed_items += 1;
                }
                SyncAction::DeleteRemote {
                    relative_path,
                    is_directory: _,
                } => {
                    let full_remote = if mount.remote_path == "/" {
                        relative_path.clone()
                    } else {
                        format!(
                            "{}/{}",
                            mount.remote_path.trim_end_matches('/'),
                            relative_path.trim_start_matches('/')
                        )
                    };
                    if let Ok(dav_path) = make_dav_path(&user, &full_remote) {
                        let _ = dav_client.delete(&dav_path).await;
                    }
                    journal.remove(&relative_path);
                    progress.completed_items += 1;
                }
                SyncAction::Conflict {
                    relative_path,
                    reason,
                } => {
                    warn!(
                        path = %relative_path,
                        reason = %reason,
                        "Sync conflict detected"
                    );
                    conflicts += 1;
                    progress.completed_items += 1;
                }
                SyncAction::InSync { relative_path: _ } => {}
            }
        }

        let _ = journal.save_to_file(&journal_path);

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
                uploaded,
                downloaded,
                conflicts,
            });

        info!(
            mount_id,
            downloaded, uploaded, conflicts, "Folder synchronization cycle completed successfully"
        );

        Ok(downloaded + uploaded)
    }

    /// Synchronizes all configured active folder mounts across the account.
    pub async fn sync_all_mounts(&self) {
        let mounts = self.coordinator.sync_mounts().await;
        for mount in mounts {
            if mount.id == "app-default-vfs" || !mount.enabled {
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

fn percent_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(h1), Some(h2)) = (h1, h2) {
                let hex_str = [h1, h2];
                if let Ok(hex) = std::str::from_utf8(&hex_str)
                    && let Ok(val) = u8::from_str_radix(hex, 16)
                {
                    bytes.push(val);
                    continue;
                }
            }
        }
        bytes.push(b);
    }
    String::from_utf8(bytes).unwrap_or_else(|_| s.to_string())
}

fn is_thumbnail_candidate(path: &str) -> bool {
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
    fn percent_decode_basic() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("plain_text"), "plain_text");
        assert_eq!(
            percent_decode("%2FDocuments%2Ffile.txt"),
            "/Documents/file.txt"
        );
    }

    #[test]
    fn path_prefix_stripping() {
        let dav_prefix = "/remote.php/dav/files/user";
        let href = "https://nextcloud.example.com/remote.php/dav/files/user/Documents/report.pdf";
        let idx = href.find(dav_prefix).expect("prefix found");
        let path_after = &href[idx + dav_prefix.len()..];
        assert_eq!(path_after, "/Documents/report.pdf");
    }
}
