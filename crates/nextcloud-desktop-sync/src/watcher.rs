//! Cross-platform filesystem watcher monitoring local folders for file changes and triggering cloud uploads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;

/// Cross-platform watcher monitoring local mount folders and dispatching sync cycles on file alterations.
pub struct FileSystemWatcher {
    watcher: Option<RecommendedWatcher>,
    watched_paths: Arc<Mutex<HashMap<PathBuf, String>>>,
    coordinator: DesktopSyncCoordinator,
}

impl std::fmt::Debug for FileSystemWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSystemWatcher").finish()
    }
}

impl FileSystemWatcher {
    /// Initializes a new filesystem watcher tied to the sync coordinator.
    pub fn new(coordinator: DesktopSyncCoordinator) -> Self {
        Self {
            watcher: None,
            watched_paths: Arc::new(Mutex::new(HashMap::new())),
            coordinator,
        }
    }

    /// Starts monitoring local directories for changes using an asynchronous debounced worker.
    pub fn start(&mut self) -> Result<(), SyncError> {
        let (tx, mut rx) = mpsc::channel::<(EventKind, PathBuf)>(100);
        let watched_map = self.watched_paths.clone();

        let event_handler = move |res: Result<Event, notify::Error>| match res {
            Ok(event) => {
                if matches!(event.kind, EventKind::Access(_)) {
                    for path in event.paths {
                        if path.is_file()
                            && let Ok(meta) = std::fs::metadata(&path)
                            && meta.len() == 0
                        {
                            let _ = tx.blocking_send((event.kind, path));
                        }
                    }
                } else if matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) {
                    for path in event.paths {
                        // Skip internal journal, temporary and hidden files
                        if path.components().any(|c| {
                            let s = c.as_os_str().to_string_lossy();
                            s.starts_with('.')
                                || s.ends_with(".part")
                                || s.ends_with(".crdownload")
                                || s.ends_with("journal.json")
                                || s.ends_with("tree.json")
                        }) {
                            continue;
                        }
                        let _ = tx.blocking_send((event.kind, path));
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Filesystem watcher error received");
            }
        };

        let mut watcher = RecommendedWatcher::new(event_handler, Config::default())
            .map_err(|e| SyncError::InvalidPath(e.to_string()))?;

        // Re-register any existing watched paths
        if let Ok(guard) = watched_map.lock() {
            for path in guard.keys() {
                if path.exists() {
                    let _ = watcher.watch(path, RecursiveMode::Recursive);
                }
            }
        }

        self.watcher = Some(watcher);

        // Spawn debouncing worker loop
        let coord = self.coordinator.clone();
        let map_for_worker = self.watched_paths.clone();
        tokio::spawn(async move {
            let mut pending_mounts: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            loop {
                // Collect events with a 1500ms debounce window
                tokio::select! {
                    Some((kind, changed_path)) = rx.recv() => {
                        // Check if 0-byte virtual placeholder file was accessed or opened: hydrate immediately!
                        if matches!(kind, EventKind::Access(_)) {
                            if changed_path.is_file()
                                && let Ok(meta) = std::fs::metadata(&changed_path)
                                && meta.len() == 0
                            {
                                let c = coord.clone();
                                let p = changed_path.clone();
                                tokio::spawn(async move {
                                    let _ = c.hydrate_local_file(&p).await;
                                });
                            }
                            // Access events never trigger upload sync cycles
                            continue;
                        }

                        if let Ok(guard) = map_for_worker.lock() {
                            for (root, mount_id) in guard.iter() {
                                if changed_path.starts_with(root) {
                                    pending_mounts.insert(mount_id.clone());
                                }
                            }
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(1500)), if !pending_mounts.is_empty() => {
                        let to_sync: Vec<String> = pending_mounts.drain().collect();
                        for m_id in to_sync {
                            if coord.is_mount_syncing(&m_id).await {
                                continue;
                            }
                            info!(mount_id = %m_id, "Local file change detected by watcher, triggering upload sync");
                            let _ = coord.sync_mount_folder(&m_id).await;
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Registers a local directory path to be monitored for a specific mount ID.
    pub fn watch_folder(&mut self, local_path: &Path, mount_id: &str) -> Result<(), SyncError> {
        let canonical = local_path.to_path_buf();
        if let Ok(mut guard) = self.watched_paths.lock() {
            guard.insert(canonical.clone(), mount_id.to_string());
        }

        if let Some(ref mut w) = self.watcher
            && canonical.exists()
        {
            w.watch(&canonical, RecursiveMode::Recursive)
                .map_err(|e| SyncError::InvalidPath(e.to_string()))?;
            debug!(path = %canonical.display(), mount_id, "Started filesystem watch on folder");
        }

        Ok(())
    }

    /// Unregisters a local directory path from the watcher.
    pub fn unwatch_folder(&mut self, local_path: &Path) {
        let canonical = local_path.to_path_buf();
        if let Ok(mut guard) = self.watched_paths.lock() {
            guard.remove(&canonical);
        }

        if let Some(ref mut w) = self.watcher {
            let _ = w.unwatch(&canonical);
        }
    }
}
