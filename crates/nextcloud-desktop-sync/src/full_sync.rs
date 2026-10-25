//! Full bidirectional synchronization engine for local folder pairings.

use nextcloud_rust_sdk::{BasicAuth, DavClient, ReqwestTransport};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crate::error::SyncError;
use crate::journal::{JournalEntry, SyncJournal};
use crate::reconciler::{SyncAction, reconcile};
use crate::scanner::{scan_local_directory, scan_remote_hierarchy};
use crate::vfs::make_dav_path;
use nextcloud_desktop_core::config::SyncFolderConfig;
use nextcloud_desktop_core::events::{DesktopEvent, EventBus, NotificationLevel, TransferKind};

/// Execution engine for running bidirectional synchronization cycles.
#[derive(Clone)]
pub struct FullSyncEngine {
    event_bus: EventBus,
}

impl FullSyncEngine {
    /// Creates a new `FullSyncEngine` with the given event bus.
    pub fn new(event_bus: EventBus) -> Self {
        Self { event_bus }
    }

    /// Executes a full synchronization cycle for a configured folder pairing.
    pub async fn run_sync(
        &self,
        folder_config: &SyncFolderConfig,
        user: &str,
        dav_client: &DavClient<'_, ReqwestTransport, BasicAuth>,
        dav_prefix: &str,
        journal_path: &Path,
    ) -> Result<usize, SyncError> {
        let pairing_id = folder_config.id.clone();
        self.event_bus.emit(DesktopEvent::SyncStarted {
            pairing_id: pairing_id.clone(),
        });

        if !folder_config.local_path.exists() {
            fs::create_dir_all(&folder_config.local_path).map_err(|err| SyncError::LocalIo {
                path: folder_config.local_path.clone(),
                context: "creating local sync root directory".to_string(),
                source: err,
            })?;
        }

        // 1. Scan local filesystem via SDK scanner
        let local_files = scan_local_directory(&folder_config.local_path)?;

        // 2. Scan remote WebDAV hierarchy
        let remote_files =
            scan_remote_hierarchy(dav_client, user, &folder_config.remote_path, dav_prefix).await?;

        // 3. Load sync journal via SDK journal
        let mut journal = SyncJournal::load_from_file(journal_path)?;

        // 4. Reconcile differences via SDK 3-way reconciler
        let actions = reconcile(&local_files, &remote_files, &journal);

        let mut uploaded = 0;
        let mut downloaded = 0;
        let mut conflicts = 0;

        for action in &actions {
            match action {
                SyncAction::CreateLocalDirectory { relative_path } => {
                    let safe = relative_path.trim_start_matches('/');
                    let local_dir = folder_config.local_path.join(safe);
                    fs::create_dir_all(&local_dir).map_err(|err| SyncError::LocalIo {
                        path: local_dir,
                        context: "creating reconciled local directory".to_string(),
                        source: err,
                    })?;
                    journal.record(JournalEntry {
                        relative_path: relative_path.clone(),
                        remote_etag: None,
                        local_size: 0,
                        local_mtime: SystemTime::now(),
                        is_directory: true,
                    });
                }
                SyncAction::CreateRemoteDirectory { relative_path } => {
                    let safe = relative_path.trim_start_matches('/');
                    let remote_full = format!(
                        "{}/{}",
                        folder_config.remote_path.trim_end_matches('/'),
                        safe
                    );
                    let dav_path = make_dav_path(user, &remote_full)?;
                    let _ = dav_client.mkcol(&dav_path).await;
                    journal.record(JournalEntry {
                        relative_path: relative_path.clone(),
                        remote_etag: None,
                        local_size: 0,
                        local_mtime: SystemTime::now(),
                        is_directory: true,
                    });
                }
                SyncAction::Download {
                    relative_path,
                    size,
                    etag,
                } => {
                    let safe = relative_path.trim_start_matches('/');
                    let local_dest = folder_config.local_path.join(safe);
                    let remote_full = format!(
                        "{}/{}",
                        folder_config.remote_path.trim_end_matches('/'),
                        safe
                    );
                    let dav_path = make_dav_path(user, &remote_full)?;
                    let bytes = dav_client.get(&dav_path).await?;
                    if let Some(parent) = local_dest.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    fs::write(&local_dest, &bytes).map_err(|err| SyncError::LocalIo {
                        path: local_dest.clone(),
                        context: "writing downloaded content to disk".to_string(),
                        source: err,
                    })?;

                    self.event_bus.emit(DesktopEvent::FileTransferCompleted {
                        relative_path: relative_path.clone(),
                        kind: TransferKind::Download,
                    });
                    downloaded += 1;

                    let mtime = fs::metadata(&local_dest)
                        .and_then(|m| m.modified())
                        .unwrap_or(SystemTime::now());

                    journal.record(JournalEntry {
                        relative_path: relative_path.clone(),
                        remote_etag: etag.clone(),
                        local_size: *size,
                        local_mtime: mtime,
                        is_directory: false,
                    });
                }
                SyncAction::Upload {
                    relative_path,
                    local_path,
                    size,
                } => {
                    let safe = relative_path.trim_start_matches('/');
                    let remote_full = format!(
                        "{}/{}",
                        folder_config.remote_path.trim_end_matches('/'),
                        safe
                    );
                    let bytes = fs::read(local_path).map_err(|err| SyncError::LocalIo {
                        path: local_path.to_path_buf(),
                        context: "reading local file for upload".to_string(),
                        source: err,
                    })?;
                    let dav_path = make_dav_path(user, &remote_full)?;
                    dav_client.put(&dav_path, bytes, None).await?;

                    self.event_bus.emit(DesktopEvent::FileTransferCompleted {
                        relative_path: relative_path.clone(),
                        kind: TransferKind::Upload,
                    });
                    uploaded += 1;

                    let mtime = fs::metadata(local_path)
                        .and_then(|m| m.modified())
                        .unwrap_or(SystemTime::now());

                    journal.record(JournalEntry {
                        relative_path: relative_path.clone(),
                        remote_etag: None,
                        local_size: *size,
                        local_mtime: mtime,
                        is_directory: false,
                    });
                }
                SyncAction::DeleteLocal {
                    relative_path,
                    local_path,
                    is_directory,
                } => {
                    if *is_directory {
                        let _ = fs::remove_dir_all(local_path);
                    } else {
                        let _ = fs::remove_file(local_path);
                    }
                    journal.remove(relative_path);
                }
                SyncAction::DeleteRemote {
                    relative_path,
                    is_directory: _,
                } => {
                    let safe = relative_path.trim_start_matches('/');
                    let remote_full = format!(
                        "{}/{}",
                        folder_config.remote_path.trim_end_matches('/'),
                        safe
                    );
                    let dav_path = make_dav_path(user, &remote_full)?;
                    let _ = dav_client.delete(&dav_path).await;
                    journal.remove(relative_path);
                }
                SyncAction::Conflict {
                    relative_path,
                    reason,
                } => {
                    conflicts += 1;
                    self.event_bus.emit(DesktopEvent::Notification {
                        message: format!("Conflict at {relative_path}: {reason}"),
                        level: NotificationLevel::Warning,
                    });
                }
                SyncAction::InSync { .. } => {}
            }
        }

        // 5. Persist updated journal to disk
        journal.save_to_file(journal_path)?;

        let total_processed = uploaded + downloaded;
        self.event_bus.emit(DesktopEvent::SyncCompleted {
            pairing_id,
            uploaded,
            downloaded,
            conflicts,
        });

        Ok(total_processed)
    }
}
