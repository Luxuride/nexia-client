//! Cross-platform filesystem watcher delegating to `nexia_nextcloud_sdk::FileSystemWatcher`.

use std::path::Path;
use std::sync::Arc;

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use nexia_nextcloud_sdk::FileSystemWatcher as SdkWatcher;

/// Cross-platform watcher monitoring local mount folders and dispatching sync cycles on file alterations.
pub struct FileSystemWatcher {
    inner: SdkWatcher,
}

impl std::fmt::Debug for FileSystemWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileSystemWatcher").finish()
    }
}

impl FileSystemWatcher {
    /// Initializes a new filesystem watcher tied to the sync coordinator.
    pub fn new(coordinator: DesktopSyncCoordinator) -> Self {
        let coord_s = coordinator.clone();
        let coord_h = coordinator.clone();

        let inner = SdkWatcher::new()
            .with_sync_handler(Arc::new(move |mount_id| {
                let c = coord_s.clone();
                tokio::spawn(async move {
                    if !c.is_mount_syncing(&mount_id).await {
                        tracing::info!(mount_id = %mount_id, "Local file change detected by watcher, triggering upload sync");
                        let _ = c.sync_mount_folder(&mount_id).await;
                    }
                });
            }))
            .with_hydration_handler(Arc::new(move |local_path| {
                let c = coord_h.clone();
                tokio::spawn(async move {
                    tracing::info!(path = %local_path.display(), "On-demand hydration triggered by external access");
                    let _ = c.hydrate_local_file(&local_path).await;
                });
            }));

        Self { inner }
    }

    /// Starts monitoring local directories for changes using an asynchronous debounced worker.
    pub fn start(&mut self) -> Result<(), SyncError> {
        self.inner.start().map_err(SyncError::Orchestrator)
    }

    /// Registers a local directory path to be monitored for a specific mount ID.
    pub fn watch_folder(&mut self, local_path: &Path, mount_id: &str) -> Result<(), SyncError> {
        self.inner
            .watch_folder(local_path, mount_id)
            .map_err(SyncError::Orchestrator)
    }

    /// Unregisters a local directory path from the watcher.
    pub fn unwatch_folder(&mut self, local_path: &Path) {
        self.inner.unwatch_folder(local_path);
    }
}
