//! Mount points and synchronization pause/resume coordination.

use nexia_nextcloud_sdk::{
    MountDescriptor, MountId, MountState, MountStatus, MountStorageUsage, OrchestratorError,
    SyncFileStatus,
};
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
    ) -> Result<nexia_nextcloud_sdk::MountStorageUsage, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle
            && let Ok(usage) = h.mounts().storage_usage(id.clone()).await
        {
            return Ok(usage);
        }

        // Direct fallback: scan local folder pairing if orchestrator mount not yet loaded
        let mounts = self.sync_mounts().await;
        let found = mounts.into_iter().find(|m| m.id == id.as_str());
        let (hydrated_count, local_bytes) = match found {
            Some(ref m) => {
                let p = PathBuf::from(&m.local_target);
                let usage = crate::cache_usage::scan_dir_usage(&p);
                (usage.files, usage.bytes)
            }
            None => (0, 0),
        };

        let mut shared_thumbnail_bytes =
            crate::cache_usage::scan_dir_usage(&self.active_thumbnails_dir().await).bytes;
        shared_thumbnail_bytes += crate::freedesktop_thumbs::app_thumbnail_bytes();
        Ok(MountStorageUsage::new(
            id,
            local_bytes,
            hydrated_count,
            shared_thumbnail_bytes,
        ))
    }

    /// Clears cached files for a mount point, returning the number of bytes freed.
    pub async fn clear_mount_cache(
        &self,
        id: MountId,
        clear_hydrated: bool,
    ) -> Result<u64, SyncError> {
        // 1. Try orchestrator mount first if active
        let orch_result = {
            let handle = self.orchestrator_handle.read().await;
            if let Some(ref h) = *handle {
                h.mounts()
                    .clear_cache(id.clone(), clear_hydrated)
                    .await
                    .ok()
            } else {
                None
            }
        };
        if let Some(freed) = orch_result {
            return Ok(freed);
        }

        // 2. Direct fallback for FUSE and folder-sync mounts:
        let mounts = self.sync_mounts().await;
        let mount_opt = mounts.into_iter().find(|m| m.id == id.as_str());

        let mut total_freed = 0u64;

        // Dehydrate shared in-memory FUSE tree
        let shared_tree = self.get_or_create_fuse_tree().await;
        let dehydrated_any = {
            let mut tree = match shared_tree.write() {
                Ok(g) => g,
                Err(_) => return Ok(0),
            };
            let mut paths_to_dehydrate = Vec::new();
            for p in tree.paths() {
                if let Some(node) = tree.get(p)
                    && node.is_hydrated()
                {
                    // Only bytes backed by a cache file are actually freed from disk. Counting
                    // `node.size()` here would add the server-side size of a node that has no
                    // local file, so clearing the cache would report freeing far more than it did.
                    if let nexia_nextcloud_sdk::HydrationState::Hydrated(
                        nexia_nextcloud_sdk::HydratedContent::Path(cached),
                    ) = &node.hydration
                        && let Ok(meta) = std::fs::metadata(cached)
                    {
                        total_freed += meta.len();
                    }
                    paths_to_dehydrate.push(p.to_string());
                }
            }
            for p in paths_to_dehydrate {
                let _ = tree.dehydrate(&p);
            }
            total_freed > 0
        };

        if dehydrated_any {
            // Persist the dehydrated tree, then drop the payload files it used to reference.
            // Without this the cache directory keeps growing: dehydrating a node clears the
            // reference but leaves the spilled file behind.
            if let Some(account) = self.active_account().await {
                let account_id = account.id.0.clone();
                let snapshot = shared_tree.read().map(|guard| guard.clone()).ok();
                if let Some(snapshot) = snapshot {
                    let _ = self
                        .persist_account_tree_async(&account_id, &snapshot)
                        .await;
                }
                let content_dir = self.account_content_dir(&account_id);
                if content_dir.exists() {
                    let _ = tokio::fs::remove_dir_all(&content_dir).await;
                }
            }
        }

        // Clear local target directory files if clear_hydrated requested and local target is an internal VFS cache
        if let Some(ref m) = mount_opt {
            let local_p = PathBuf::from(&m.local_target);
            if Self::is_internal_vfs(&m.id) && local_p.exists() {
                total_freed += crate::cache_usage::scan_dir_usage(&local_p).bytes;
                let _ = tokio::fs::remove_dir_all(&local_p).await;
                let _ = tokio::fs::create_dir_all(&local_p).await;
            }
        }

        Ok(total_freed)
    }

    /// Clears shared thumbnail cache, returning the number of bytes freed.
    pub async fn clear_thumbnail_cache(&self) -> Result<u64, SyncError> {
        let orch_freed = {
            let handle = self.orchestrator_handle.read().await;
            if let Some(ref h) = *handle {
                h.mounts().clear_thumbnail_cache().await.unwrap_or(0)
            } else {
                0
            }
        };
        let coord_freed = self.clean_thumbnails().await.unwrap_or(0);
        Ok(orch_freed + coord_freed)
    }

    /// Retrieves live progress metrics for a mount point from the SDK.
    pub async fn mount_progress(
        &self,
        id: MountId,
    ) -> Result<nexia_nextcloud_sdk::MountProgressSnapshot, SyncError> {
        if let Ok(map) = self.mount_progress_cache.read()
            && let Some(snap) = map.get(id.as_str())
        {
            return Ok(snap.clone());
        }

        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle
            && let Ok(snap) = h.mounts().progress(id.clone()).await
        {
            return Ok(snap);
        }

        let is_paused = self.is_mount_paused(id.clone()).await.unwrap_or(false);
        let status = if is_paused {
            MountStatus::Paused
        } else {
            MountStatus::Idle
        };
        Ok(nexia_nextcloud_sdk::MountProgressSnapshot::new(id, status))
    }

    /// Returns `true` if a folder synchronization cycle is actively in progress for `id`.
    pub async fn is_mount_syncing(&self, id: &str) -> bool {
        if let Ok(map) = self.mount_progress_cache.read()
            && let Some(snap) = map.get(id)
        {
            return matches!(snap.status, MountStatus::Syncing);
        }
        false
    }
}
