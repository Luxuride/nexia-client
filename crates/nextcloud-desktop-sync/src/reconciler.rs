//! Bidirectional reconciliation logic between local files, remote files, and journal entries.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::journal::SyncJournal;
use crate::scanner::{DiscoveredLocalFile, DiscoveredRemoteFile};

/// Action determined by the reconciler to synchronize an individual file or directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncAction {
    /// Remote file is new or modified; download to local filesystem.
    Download {
        /// Canonical relative path.
        relative_path: String,
        /// Expected file size.
        size: u64,
        /// Remote ETag.
        etag: Option<String>,
    },
    /// Local file is new or modified; upload to Nextcloud WebDAV.
    Upload {
        /// Canonical relative path.
        relative_path: String,
        /// Full local path on host disk.
        local_path: PathBuf,
        /// Local file size.
        size: u64,
    },
    /// Remote directory needs to be created on local disk.
    CreateLocalDirectory {
        /// Canonical relative path.
        relative_path: String,
    },
    /// Local directory needs to be created on remote WebDAV.
    CreateRemoteDirectory {
        /// Canonical relative path.
        relative_path: String,
    },
    /// Item was deleted on Nextcloud; delete from local disk.
    DeleteLocal {
        /// Canonical relative path.
        relative_path: String,
        /// Full local path on host disk.
        local_path: PathBuf,
        /// Whether entry is a directory.
        is_directory: bool,
    },
    /// Item was deleted on local disk; delete from remote Nextcloud.
    DeleteRemote {
        /// Canonical relative path.
        relative_path: String,
        /// Whether entry is a directory.
        is_directory: bool,
    },
    /// Both sides were modified concurrently; flagged as a conflict.
    Conflict {
        /// Canonical relative path.
        relative_path: String,
        /// Description of the conflict reason.
        reason: String,
    },
    /// Item is unchanged and up to date on both ends.
    InSync {
        /// Canonical relative path.
        relative_path: String,
    },
}

impl SyncAction {
    /// Returns the canonical relative path for this action.
    pub fn relative_path(&self) -> &str {
        match self {
            Self::Download { relative_path, .. }
            | Self::Upload { relative_path, .. }
            | Self::CreateLocalDirectory { relative_path }
            | Self::CreateRemoteDirectory { relative_path }
            | Self::DeleteLocal { relative_path, .. }
            | Self::DeleteRemote { relative_path, .. }
            | Self::Conflict { relative_path, .. }
            | Self::InSync { relative_path } => relative_path,
        }
    }
}

/// Reconciles local files against remote files using the previous sync journal state.
pub fn reconcile(
    local_files: &[DiscoveredLocalFile],
    remote_files: &[DiscoveredRemoteFile],
    journal: &SyncJournal,
) -> Vec<SyncAction> {
    let mut actions = Vec::new();

    let local_map: HashMap<&str, &DiscoveredLocalFile> = local_files
        .iter()
        .map(|f| (f.relative_path.as_str(), f))
        .collect();

    let remote_map: HashMap<&str, &DiscoveredRemoteFile> = remote_files
        .iter()
        .map(|f| (f.relative_path.as_str(), f))
        .collect();

    let mut all_paths: HashSet<&str> = HashSet::new();
    all_paths.extend(local_map.keys());
    all_paths.extend(remote_map.keys());

    for path in all_paths {
        let local = local_map.get(path);
        let remote = remote_map.get(path);
        let journal_entry = journal.get(path);

        match (local, remote, journal_entry) {
            // Case 1: Present in both local and remote
            (Some(loc), Some(rem), Some(j)) => {
                let local_modified = loc.mtime > j.local_mtime || loc.size != j.local_size;
                let remote_modified = rem.etag != j.remote_etag;

                if local_modified && remote_modified {
                    actions.push(SyncAction::Conflict {
                        relative_path: (*path).to_string(),
                        reason:
                            "Both local file and remote Nextcloud file were modified concurrently"
                                .to_string(),
                    });
                } else if local_modified {
                    actions.push(SyncAction::Upload {
                        relative_path: (*path).to_string(),
                        local_path: loc.absolute_path.clone(),
                        size: loc.size,
                    });
                } else if remote_modified {
                    actions.push(SyncAction::Download {
                        relative_path: (*path).to_string(),
                        size: rem.size,
                        etag: rem.etag.clone(),
                    });
                } else {
                    actions.push(SyncAction::InSync {
                        relative_path: (*path).to_string(),
                    });
                }
            }

            // Case 2: Present in both, but not tracked in journal (initial sync)
            (Some(loc), Some(rem), None) => {
                if (loc.is_directory && rem.is_directory) || loc.size == rem.size {
                    actions.push(SyncAction::InSync {
                        relative_path: (*path).to_string(),
                    });
                } else {
                    // Conflict or remote-preferred
                    actions.push(SyncAction::Conflict {
                        relative_path: (*path).to_string(),
                        reason: "File exists on both sides with differing attributes without prior sync record"
                            .to_string(),
                    });
                }
            }

            // Case 3: Present locally, absent remotely
            (Some(loc), None, Some(_j)) => {
                // Previously synced, now missing remotely -> delete locally
                actions.push(SyncAction::DeleteLocal {
                    relative_path: (*path).to_string(),
                    local_path: loc.absolute_path.clone(),
                    is_directory: loc.is_directory,
                });
            }
            (Some(loc), None, None) => {
                // Newly created locally -> upload
                if loc.is_directory {
                    actions.push(SyncAction::CreateRemoteDirectory {
                        relative_path: (*path).to_string(),
                    });
                } else {
                    actions.push(SyncAction::Upload {
                        relative_path: (*path).to_string(),
                        local_path: loc.absolute_path.clone(),
                        size: loc.size,
                    });
                }
            }

            // Case 4: Absent locally, present remotely
            (None, Some(rem), Some(_j)) => {
                // Previously synced, now missing locally -> delete remotely
                actions.push(SyncAction::DeleteRemote {
                    relative_path: (*path).to_string(),
                    is_directory: rem.is_directory,
                });
            }
            (None, Some(rem), None) => {
                // Newly created remotely -> download
                if rem.is_directory {
                    actions.push(SyncAction::CreateLocalDirectory {
                        relative_path: (*path).to_string(),
                    });
                } else {
                    actions.push(SyncAction::Download {
                        relative_path: (*path).to_string(),
                        size: rem.size,
                        etag: rem.etag.clone(),
                    });
                }
            }

            (None, None, _) => {}
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn reconciliation_new_remote_file() {
        let local: Vec<DiscoveredLocalFile> = Vec::new();
        let remote = vec![DiscoveredRemoteFile {
            relative_path: "/notes.txt".to_string(),
            href: "/notes.txt".to_string(),
            size: 100,
            etag: Some("\"1\"".to_string()),
            is_directory: false,
        }];
        let journal = SyncJournal::new();

        let actions = reconcile(&local, &remote, &journal);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            SyncAction::Download {
                relative_path,
                size,
                etag,
            } => {
                assert_eq!(relative_path, "/notes.txt");
                assert_eq!(*size, 100);
                assert_eq!(etag.as_deref(), Some("\"1\""));
            }
            _ => panic!("expected Download action"),
        }
    }

    #[test]
    fn reconciliation_new_local_file() {
        let local = vec![DiscoveredLocalFile {
            relative_path: "/photo.png".to_string(),
            absolute_path: PathBuf::from("/tmp/photo.png"),
            size: 500,
            mtime: SystemTime::now(),
            is_directory: false,
        }];
        let remote: Vec<DiscoveredRemoteFile> = Vec::new();
        let journal = SyncJournal::new();

        let actions = reconcile(&local, &remote, &journal);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            SyncAction::Upload {
                relative_path,
                size,
                ..
            } => {
                assert_eq!(relative_path, "/photo.png");
                assert_eq!(*size, 500);
            }
            _ => panic!("expected Upload action"),
        }
    }
}
