//! Mount points and synchronization pause/resume coordination.

use nextcloud_rust_sdk::{MountDescriptor, MountId, MountState, OrchestratorError, SyncFileStatus};
use std::path::PathBuf;

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;

impl DesktopSyncCoordinator {
    /// Adds a new mount point to the orchestrator.
    pub async fn add_mount(&self, descriptor: MountDescriptor) -> Result<MountId, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .add_mount(descriptor)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Err(SyncError::Orchestrator(OrchestratorError::WorkerShutdown(
                "orchestrator not initialized".to_string(),
            )))
        }
    }

    /// Removes an existing mount point.
    pub async fn remove_mount(&self, id: MountId) -> Result<MountDescriptor, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .remove_mount(id)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Err(SyncError::Orchestrator(OrchestratorError::WorkerShutdown(
                "orchestrator not initialized".to_string(),
            )))
        }
    }

    /// Lists all active mounts and their runtime states.
    pub async fn list_mounts(&self) -> Result<Vec<(MountDescriptor, MountState)>, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .list_mounts()
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(Vec::new())
        }
    }

    /// Queries the file sync status and FreeDesktop icon name for a relative path in a mount.
    pub async fn file_status(
        &self,
        mount_id: MountId,
        path: &str,
    ) -> Result<SyncFileStatus, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .file_status(mount_id, path)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(SyncFileStatus::OnlineOnly)
        }
    }

    /// Resolves the synchronization status for an arbitrary local path across all mounts.
    pub async fn status_for_local_path(
        &self,
        local_path: impl Into<PathBuf>,
    ) -> Result<Option<SyncFileStatus>, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .status_for_local_path(local_path)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(None)
        }
    }

    /// Specifically pauses synchronization for a single mount point.
    pub async fn pause_mount(&self, id: MountId) -> Result<(), SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .pause_mount(id)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(())
        }
    }

    /// Specifically resumes synchronization for a single mount point.
    pub async fn resume_mount(&self, id: MountId) -> Result<(), SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .resume_mount(id.clone())
                .await
                .map_err(SyncError::Orchestrator)?;
        }
        let coord = self.clone();
        let id_str = id.to_string();
        tokio::spawn(async move {
            let _ = coord.sync_mount_folder(&id_str).await;
        });
        Ok(())
    }

    /// Queries whether a specific mount point is currently paused.
    pub async fn is_mount_paused(&self, id: MountId) -> Result<bool, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .is_paused(id)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(false)
        }
    }

    /// Globally pauses synchronization across all mounts and background queues.
    pub async fn pause_all_sync(&self) -> Result<(), SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.pause_all_sync().await.map_err(SyncError::Orchestrator)
        } else {
            Ok(())
        }
    }

    /// Globally resumes synchronization across all mounts and background queues.
    pub async fn resume_all_sync(&self) -> Result<(), SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.resume_all_sync().await.map_err(SyncError::Orchestrator)
        } else {
            Ok(())
        }
    }

    /// Queries whether synchronization is globally paused.
    pub async fn is_sync_paused(&self) -> Result<bool, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.is_sync_paused().await.map_err(SyncError::Orchestrator)
        } else {
            Ok(false)
        }
    }

    /// Queries the disk storage usage report for a mount point from the SDK orchestrator.
    pub async fn mount_storage_usage(
        &self,
        id: MountId,
    ) -> Result<nextcloud_rust_sdk::MountStorageUsage, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .storage_usage(id)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Err(SyncError::NotFound(
                "Orchestrator not initialized".to_string(),
            ))
        }
    }

    /// Clears cached files for a mount point via the SDK, returning the number of bytes freed.
    pub async fn clear_mount_cache(
        &self,
        id: MountId,
        clear_hydrated: bool,
    ) -> Result<u64, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .clear_cache(id, clear_hydrated)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Err(SyncError::NotFound(
                "Orchestrator not initialized".to_string(),
            ))
        }
    }

    /// Clears shared thumbnail cache via the SDK orchestrator, returning the number of bytes freed.
    pub async fn clear_thumbnail_cache(&self) -> Result<u64, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .clear_thumbnail_cache()
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Err(SyncError::NotFound(
                "Orchestrator not initialized".to_string(),
            ))
        }
    }

    /// Retrieves live progress metrics for a mount point from the SDK.
    pub async fn mount_progress(
        &self,
        id: MountId,
    ) -> Result<nextcloud_rust_sdk::MountProgressSnapshot, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .progress(id)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Err(SyncError::NotFound(
                "Orchestrator not initialized".to_string(),
            ))
        }
    }
}
