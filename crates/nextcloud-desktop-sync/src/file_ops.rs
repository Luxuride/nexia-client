//! High-level virtual file mutations and two-way folder synchronization.

use nextcloud_rust_sdk::{DavPath, DavProperty, Depth, FolderSyncRunner, PropfindBody};
use std::path::PathBuf;

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use nextcloud_desktop_core::config::SyncMode;
use nextcloud_desktop_core::events::DesktopEvent;

/// Constructs a normalized [`DavPath`] from user name and relative path.
pub fn make_dav_path(user: &str, relative: &str) -> Result<DavPath, SyncError> {
    nextcloud_rust_sdk::make_dav_path(user, relative).map_err(SyncError::Orchestrator)
}

impl DesktopSyncCoordinator {
    /// Refreshes the virtual file tree for the active configured account, or resets to local virtual hierarchy.
    pub async fn refresh_virtual_tree(&self) -> Result<usize, SyncError> {
        let _priority = self.priority_permit();
        let vfs = self.active_vfs().await;

        let Some((user, client, auth)) = self.active_dav_context().await else {
            tracing::warn!("Cannot refresh virtual tree: no active DAV context");
            return Ok(0);
        };

        let dav_client = client.dav(&auth);
        let root_dav =
            DavPath::root(&user).map_err(|err| SyncError::InvalidPath(err.to_string()))?;

        tracing::info!(user = %user, "Querying WebDAV root PROPFIND for virtual tree");
        let body = PropfindBody::all();
        let resources = match dav_client.propfind(&root_dav, Depth::One, &body).await {
            Ok(r) => {
                tracing::info!(count = r.len(), "WebDAV PROPFIND returned root resources");
                r
            }
            Err(err) => {
                tracing::warn!(error = %err, "Remote WebDAV PROPFIND failed, keeping default tree");
                return Ok(0);
            }
        };

        let dav_prefix = format!("/remote.php/dav/files/{user}");
        let summaries: Vec<nextcloud_rust_sdk::DavResourceSummary> = resources
            .iter()
            .map(|res| {
                let mut sum =
                    nextcloud_rust_sdk::DavResourceSummary::new(&res.href, res.is_collection);
                if let Some(len) = res.content_length {
                    sum = sum.with_content_length(len);
                }
                if let Some(ref etag) = res.etag {
                    sum = sum.with_etag(etag);
                }
                if let Some(ref lm) = res.last_modified {
                    sum = sum.with_last_modified(lm);
                }
                if let Some(ref mime) = res.content_type {
                    sum = sum.with_content_type(mime);
                }
                if let Some(ref fid) = res.file_id
                    && let Ok(id) = fid.parse::<u64>()
                {
                    sum = sum.with_file_id(id);
                }
                if let Some(prev) = res.has_preview {
                    sum = sum.with_has_preview(prev);
                }
                sum
            })
            .collect();

        // Repopulate directly inside the orchestrator actor thread
        let inserted = match vfs
            .populate_from_resources(&dav_prefix, summaries.clone())
            .await
        {
            Ok(count) => {
                tracing::info!(count, "VFS tree populated with resources");
                count
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to populate VFS tree from resources");
                0
            }
        };

        // Sync to running FUSE in-memory tree and persist tree.json
        if let Ok(tree_snap) = vfs.snapshot().await {
            if let Some(ref ft_lock) = *self.fuse_tree.read().await
                && let Ok(mut guard) = ft_lock.write()
            {
                *guard = tree_snap.clone();
            }
            if let Some(acc) = self.active_account().await {
                let tree_file = self.account_vfs_dir(&acc.id.0).join("tree.json");
                if let Ok(json) = serde_json::to_string_pretty(&tree_snap) {
                    let _ = std::fs::write(&tree_file, json);
                }
            }
        }

        self.event_bus.emit(
            nextcloud_desktop_core::events::DesktopEvent::VfsTreeUpdated {
                node_count: inserted,
            },
        );

        self.trigger_background_tree_preload();

        if self.is_autodownload_thumbnails_enabled().await {
            let media_paths: Vec<String> = summaries
                .into_iter()
                .filter(|s| {
                    !s.is_collection
                        && s.has_preview != Some(false)
                        && !s.href.ends_with(".svg")
                        && (s.href.ends_with(".png")
                            || s.href.ends_with(".jpg")
                            || s.href.ends_with(".jpeg")
                            || s.href.ends_with(".webp")
                            || s.content_type
                                .as_deref()
                                .is_some_and(|m| m.starts_with("image/")))
                })
                .map(|s| crate::folder_sync::extract_clean_dav_path(&s.href, &dav_prefix))
                .collect();

            if !media_paths.is_empty() {
                self.batch_prefetch_thumbnails(media_paths, 64);
            }
        }

        Ok(inserted)
    }

    /// Refreshes the virtual file entries for a specific relative directory path.
    pub async fn refresh_path(&self, relative_path: &str) -> Result<usize, SyncError> {
        let _priority = self.priority_permit();
        let vfs = self.active_vfs().await;

        let Some((user, client, auth)) = self.active_dav_context().await else {
            return Ok(0);
        };

        let dav_client = client.dav(&auth);
        let dav_path = make_dav_path(&user, relative_path)?;

        let body = PropfindBody::all();
        let resources = match dav_client.propfind(&dav_path, Depth::One, &body).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(path = %relative_path, error = %err, "Remote WebDAV PROPFIND for path failed");
                return Ok(0);
            }
        };

        let dav_prefix = format!("/remote.php/dav/files/{user}");
        let summaries: Vec<nextcloud_rust_sdk::DavResourceSummary> = resources
            .iter()
            .map(|res| {
                let mut sum =
                    nextcloud_rust_sdk::DavResourceSummary::new(&res.href, res.is_collection);
                if let Some(len) = res.content_length {
                    sum = sum.with_content_length(len);
                }
                if let Some(ref etag) = res.etag {
                    sum = sum.with_etag(etag);
                }
                if let Some(ref lm) = res.last_modified {
                    sum = sum.with_last_modified(lm);
                }
                if let Some(ref mime) = res.content_type {
                    sum = sum.with_content_type(mime);
                }
                if let Some(ref fid) = res.file_id
                    && let Ok(id) = fid.parse::<u64>()
                {
                    sum = sum.with_file_id(id);
                }
                if let Some(prev) = res.has_preview {
                    sum = sum.with_has_preview(prev);
                }
                sum
            })
            .collect();

        let inserted = vfs
            .populate_from_resources(&dav_prefix, summaries.clone())
            .await
            .unwrap_or(0);

        // Sync to running FUSE in-memory tree and persist tree.json
        if let Ok(tree_snap) = vfs.snapshot().await {
            if let Some(ref ft_lock) = *self.fuse_tree.read().await
                && let Ok(mut guard) = ft_lock.write()
            {
                *guard = tree_snap.clone();
            }
            if let Some(acc) = self.active_account().await {
                let tree_file = self.account_vfs_dir(&acc.id.0).join("tree.json");
                if let Ok(json) = serde_json::to_string_pretty(&tree_snap) {
                    let _ = std::fs::write(&tree_file, json);
                }
            }
        }

        self.event_bus.emit(
            nextcloud_desktop_core::events::DesktopEvent::VfsTreeUpdated {
                node_count: inserted,
            },
        );

        self.trigger_background_tree_preload();

        if self.is_autodownload_thumbnails_enabled().await {
            let media_paths: Vec<String> = summaries
                .into_iter()
                .filter(|s| {
                    !s.is_collection
                        && s.has_preview != Some(false)
                        && !s.href.ends_with(".svg")
                        && (s.href.ends_with(".png")
                            || s.href.ends_with(".jpg")
                            || s.href.ends_with(".jpeg")
                            || s.href.ends_with(".webp")
                            || s.content_type
                                .as_deref()
                                .is_some_and(|m| m.starts_with("image/")))
                })
                .map(|s| crate::folder_sync::extract_clean_dav_path(&s.href, &dav_prefix))
                .collect();

            if !media_paths.is_empty() {
                self.batch_prefetch_thumbnails(media_paths, 64);
            }
        }

        Ok(inserted)
    }

    /// Queries remote Nextcloud via WebDAV to list child directories beneath `relative_path`.
    pub async fn list_remote_directory_folders(
        &self,
        relative_path: &str,
    ) -> Result<Vec<String>, SyncError> {
        let Some((user, client, auth)) = self.active_dav_context().await else {
            return Ok(Vec::new());
        };

        let dav_client = client.dav(&auth);
        let dav_path = make_dav_path(&user, relative_path)?;

        let body = PropfindBody::all();
        let resources = match dav_client.propfind(&dav_path, Depth::One, &body).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(path = %relative_path, error = %err, "Failed to list remote folders");
                return Ok(Vec::new());
            }
        };

        let dav_prefix = format!("/remote.php/dav/files/{user}");
        let clean_prefix = dav_prefix.trim_end_matches('/');

        let mut folders = Vec::new();
        for res in resources {
            if !res.is_collection {
                continue;
            }
            let href = res.href.trim_end_matches('/');
            let relative = if let Some(idx) = href.find(clean_prefix) {
                &href[idx + clean_prefix.len()..]
            } else {
                href
            };
            let decoded = decode_percent_str(relative);
            let clean = format!("/{}", decoded.trim_matches('/'));
            let requested_clean = format!("/{}", relative_path.trim_matches('/'));
            if clean != requested_clean && clean != "/" {
                folders.push(clean);
            }
        }
        folders.sort();
        folders.dedup();
        Ok(folders)
    }

    /// Performs on-demand hydration for a virtual file, downloading via SDK or using local cache.
    pub async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, SyncError> {
        let _priority = self.priority_permit();
        let _download_permit = self.download_semaphore.acquire().await.ok();
        let clean = format!("/{}", remote_path.trim_matches('/'));
        self.event_bus.emit(DesktopEvent::NodeHydrationStarted {
            path: clean.clone(),
        });
        let vfs = self.active_vfs().await;

        let Some((user, client, auth)) = self.active_dav_context().await else {
            return Err(SyncError::NotFound("No active account session".to_string()));
        };

        let dav_client = client.dav(&auth);
        let clean_sub = remote_path.trim_matches('/');
        let dav_path = make_dav_path(&user, clean_sub)?;

        let bytes = dav_client.get(&dav_path).await.map_err(SyncError::WebDav)?;
        let byte_len = bytes.len() as u64;
        let local_path = vfs
            .hydrate_local(remote_path, bytes.clone())
            .await
            .map_err(SyncError::Orchestrator)?;

        // If this file matches a local folder sync mount, hydrate the local file directly
        for mount in self.sync_mounts().await {
            if Self::is_internal_vfs(&mount.id) || !mount.enabled {
                continue;
            }
            let m_remote = mount.remote_path.trim_end_matches('/');
            let rel = if m_remote.is_empty() || m_remote == "/" {
                Some(clean_sub)
            } else {
                clean_sub.strip_prefix(m_remote.trim_start_matches('/'))
            };
            if let Some(rel_path) = rel {
                let rel_clean = rel_path.trim_start_matches('/');
                let target_file = PathBuf::from(&mount.local_target).join(rel_clean);
                if let Some(parent) = target_file.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if mount.mode != "Fuse" && std::fs::write(&target_file, &bytes).is_ok() {
                    let journal_path = self.mount_journal_path(&mount.account_id, &mount.id);
                    let mut journal =
                        nextcloud_rust_sdk::SyncJournal::load_from_file(&journal_path)
                            .unwrap_or_else(|_| nextcloud_rust_sdk::SyncJournal::new());
                    let mtime = std::fs::metadata(&target_file)
                        .and_then(|m| m.modified())
                        .unwrap_or_else(|_| std::time::SystemTime::now());
                    let canonical_rel = format!("/{}", rel_clean.trim_matches('/'));
                    journal.record(nextcloud_rust_sdk::JournalEntry::new(
                        &canonical_rel,
                        byte_len,
                        mtime,
                        false,
                    ));
                    let _ = journal.save_to_file(&journal_path);
                    nextcloud_rust_sdk::remove_freedesktop_fail_marker(&target_file);
                    let _ = self.fetch_thumbnail(&clean, 256).await;
                    self.event_bus.emit(DesktopEvent::FileTransferCompleted {
                        relative_path: canonical_rel,
                        kind: nextcloud_desktop_core::events::TransferKind::Hydration,
                    });
                    self.event_bus.emit(DesktopEvent::NodeHydrated {
                        path: clean,
                        size: byte_len,
                    });
                    return Ok(target_file);
                }
            }
        }

        self.event_bus.emit(DesktopEvent::NodeHydrated {
            path: clean,
            size: byte_len,
        });
        Ok(local_path)
    }

    /// Hydrates a local 0-byte placeholder in a sync folder by downloading its content from Nextcloud.
    pub async fn hydrate_local_file(
        &self,
        local_path: &std::path::Path,
    ) -> Result<PathBuf, SyncError> {
        let _priority = self.priority_permit();
        let _download_permit = self.download_semaphore.acquire().await.ok();
        let mounts = self.sync_mounts().await;
        let canonical = local_path
            .canonicalize()
            .unwrap_or_else(|_| local_path.to_path_buf());

        for mount in mounts {
            if Self::is_internal_vfs(&mount.id) || !mount.enabled {
                continue;
            }
            let target_root = PathBuf::from(&mount.local_target);
            let target_canonical = target_root
                .canonicalize()
                .unwrap_or_else(|_| target_root.clone());

            if let Ok(rel) = canonical.strip_prefix(&target_canonical) {
                let rel_str = rel.to_string_lossy().to_string();
                let full_remote = if mount.remote_path == "/" {
                    rel_str.clone()
                } else {
                    format!(
                        "{}/{}",
                        mount.remote_path.trim_end_matches('/'),
                        rel_str.trim_start_matches('/')
                    )
                };

                if let Some((user, client, auth)) = self.active_dav_context().await {
                    let dav = client.dav(&auth);
                    let dav_path = make_dav_path(&user, &full_remote)?;
                    let bytes = dav
                        .get(&dav_path)
                        .await
                        .map_err(|e| SyncError::InvalidPath(e.to_string()))?;
                    let byte_len = bytes.len() as u64;

                    std::fs::write(&canonical, &bytes)
                        .map_err(|e| SyncError::InvalidPath(e.to_string()))?;

                    let journal_path = self.mount_journal_path(&mount.account_id, &mount.id);
                    let mut journal =
                        nextcloud_rust_sdk::SyncJournal::load_from_file(&journal_path)
                            .unwrap_or_else(|_| nextcloud_rust_sdk::SyncJournal::new());
                    let mtime = std::fs::metadata(&canonical)
                        .and_then(|m| m.modified())
                        .unwrap_or_else(|_| std::time::SystemTime::now());
                    let canonical_rel = format!("/{}", rel_str.trim_matches('/'));
                    journal.record(nextcloud_rust_sdk::JournalEntry::new(
                        &canonical_rel,
                        byte_len,
                        mtime,
                        false,
                    ));
                    let _ = journal.save_to_file(&journal_path);
                    nextcloud_rust_sdk::remove_freedesktop_fail_marker(&canonical);
                    let _ = self.fetch_thumbnail(&full_remote, 256).await;

                    self.event_bus.emit(DesktopEvent::FileTransferCompleted {
                        relative_path: canonical_rel,
                        kind: nextcloud_desktop_core::events::TransferKind::Hydration,
                    });
                    self.event_bus.emit(DesktopEvent::NodeHydrated {
                        path: full_remote,
                        size: byte_len,
                    });

                    return Ok(canonical);
                }
            }
        }

        Err(SyncError::NotFound(format!(
            "No active sync mount found for {}",
            local_path.display()
        )))
    }

    /// Recursively hydrates all unhydrated files within a virtual directory path.
    pub async fn hydrate_directory(&self, remote_dir: &str) -> Result<usize, SyncError> {
        let vfs = self.active_vfs().await;
        let mut queue = vec![remote_dir.to_string()];
        let mut count = 0;

        while let Some(current) = queue.pop() {
            if let Ok(children) = vfs.list_children(&current).await {
                for child in children {
                    if child.is_directory {
                        queue.push(child.path);
                    } else if self.hydrate_file(&child.path).await.is_ok() {
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }

    /// Dehydrates a file node, freeing local cache space.
    pub async fn dehydrate_file(&self, remote_path: &str) -> Result<(), SyncError> {
        let clean = format!("/{}", remote_path.trim_matches('/'));
        let vfs = self.active_vfs().await;
        let res = vfs
            .dehydrate_local(remote_path)
            .await
            .map_err(SyncError::Orchestrator);
        self.event_bus
            .emit(DesktopEvent::NodeDehydrated { path: clean });
        res
    }

    /// Creates a new directory in the virtual tree and on the remote WebDAV server if online.
    pub async fn create_directory(&self, remote_path: &str) -> Result<(), SyncError> {
        if self.is_sync_paused().await? {
            return Err(SyncError::OperationDisabled(
                "Uploads and modifications are disabled while synchronization is paused"
                    .to_string(),
            ));
        }

        if let Some((user, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            if let Ok(dav_path) = make_dav_path(&user, remote_path) {
                let _ = dav_client.mkcol(&dav_path).await;
            }
        }

        let vfs = self.active_vfs().await;
        vfs.create_directory(remote_path)
            .await
            .map_err(SyncError::Orchestrator)
    }

    /// Creates a new file in the virtual tree and uploads it if online.
    pub async fn create_file(
        &self,
        remote_path: &str,
        content: &[u8],
    ) -> Result<String, SyncError> {
        if self.is_sync_paused().await? {
            return Err(SyncError::OperationDisabled(
                "Uploads are disabled while synchronization is paused".to_string(),
            ));
        }

        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        if let Some((user, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            if let Ok(dav_path) = make_dav_path(&user, &clean_path) {
                let _ = dav_client.put(&dav_path, content.to_vec(), None).await;
            }
        }

        let vfs = self.active_vfs().await;
        vfs.hydrate_local(&clean_path, content.to_vec())
            .await
            .map_err(SyncError::Orchestrator)?;
        Ok(clean_path)
    }

    /// Renames a file or directory in the virtual tree and on the remote WebDAV server.
    pub async fn rename_resource(
        &self,
        old_remote_path: &str,
        new_name: &str,
    ) -> Result<String, SyncError> {
        let clean_old = format!("/{}", old_remote_path.trim_matches('/'));
        let parent = nextcloud_rust_sdk::parent_path(&clean_old).unwrap_or_else(|| "/".to_string());
        let trimmed_name = new_name.trim().trim_matches('/');
        let clean_new = if parent == "/" {
            format!("/{trimmed_name}")
        } else {
            format!("{}/{}", parent.trim_end_matches('/'), trimmed_name)
        };

        if let Some((user, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            if let (Ok(src_dav), Ok(dst_dav)) = (
                make_dav_path(&user, &clean_old),
                make_dav_path(&user, &clean_new),
            ) {
                let _ = dav_client.move_(&src_dav, &dst_dav, false).await;
            }
        }

        let vfs = self.active_vfs().await;
        vfs.rename(&clean_old, trimmed_name)
            .await
            .map_err(SyncError::Orchestrator)?;
        Ok(clean_new)
    }

    /// Moves a file or directory into a target folder in the virtual tree and on the remote WebDAV server.
    pub async fn move_resource(
        &self,
        source_path: &str,
        destination_dir: &str,
    ) -> Result<String, SyncError> {
        let clean_src = format!("/{}", source_path.trim_matches('/'));
        let clean_dest_dir = format!("/{}", destination_dir.trim_matches('/'));

        let file_name = clean_src.rsplit('/').next().unwrap_or("file");
        let clean_new = if clean_dest_dir == "/" {
            format!("/{file_name}")
        } else {
            format!("{}/{}", clean_dest_dir.trim_end_matches('/'), file_name)
        };

        if let Some((user, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            if let (Ok(src_dav), Ok(dst_dav)) = (
                make_dav_path(&user, &clean_src),
                make_dav_path(&user, &clean_new),
            ) {
                let _ = dav_client.move_(&src_dav, &dst_dav, false).await;
            }
        }

        let vfs = self.active_vfs().await;
        vfs.rename(&clean_src, file_name)
            .await
            .map_err(SyncError::Orchestrator)?;
        Ok(clean_new)
    }

    /// Deletes a file or directory from the virtual tree and remote WebDAV server.
    pub async fn delete_resource(&self, remote_path: &str) -> Result<(), SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        if let Some((user, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            if let Ok(dav_path) = make_dav_path(&user, &clean_path) {
                let _ = dav_client.delete(&dav_path).await;
            }
        }

        let vfs = self.active_vfs().await;
        vfs.remove(&clean_path)
            .await
            .map_err(SyncError::Orchestrator)
    }

    /// Creates a public share link for a virtual file or directory via OCS share API.
    pub async fn create_public_share(&self, remote_path: &str) -> Result<String, SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        if let Some((_, client, auth)) = self.active_dav_context().await {
            let share_client = client.shares(&auth);
            let share_creation = nextcloud_rust_sdk::CreateShare::public_link(&clean_path);

            if let Ok(share) = share_client.create(&share_creation).await
                && let Some(url) = share.url()
            {
                return Ok(url.to_string());
            }

            let base = client.server_url().as_str().trim_end_matches('/');
            let encoded_path = clean_path.trim_start_matches('/');
            Ok(format!("{base}/index.php/apps/files/?dir=/{encoded_path}"))
        } else {
            let server_url = self
                .active_account()
                .await
                .map(|a| a.server.server_url)
                .unwrap_or_else(|| "https://cloud.nextcloud.com".to_string());
            let base = server_url.trim_end_matches('/');
            Ok(format!(
                "{base}/index.php/apps/files/{}",
                clean_path.trim_start_matches('/')
            ))
        }
    }

    /// Retrieves an existing public share link for a virtual file or directory, if one exists.
    pub async fn get_public_share(&self, remote_path: &str) -> Result<Option<String>, SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        if let Some((_, client, auth)) = self.active_dav_context().await {
            let share_client = client.shares(&auth);
            match share_client.get_public_link(&clean_path).await {
                Ok(Some(share)) => Ok(share.url().map(String::from)),
                Ok(None) => Ok(None),
                Err(err) => {
                    tracing::warn!(path = %clean_path, error = %err, "Failed to retrieve public share link");
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Deletes any public share link associated with a virtual file or directory.
    pub async fn delete_public_share(&self, remote_path: &str) -> Result<bool, SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        if let Some((_, client, auth)) = self.active_dav_context().await {
            let share_client = client.shares(&auth);
            share_client
                .delete_public_link(&clean_path)
                .await
                .map_err(SyncError::WebDav)
        } else {
            Err(SyncError::NotFound(
                "No active Nextcloud account configured".to_string(),
            ))
        }
    }

    /// Persists directory sorting preferences for a specific directory in the permanent SDK config store.
    pub async fn store_directory_sort(
        &self,
        dir_path: &str,
        sort_field: &str,
        sort_order: &str,
    ) -> Result<(), SyncError> {
        if let Some(ref storage) = self.storage {
            let key = format!("sort:{dir_path}");
            let val = format!("{sort_field}:{sort_order}");
            let _ = storage.set_setting(&key, &val);
        }
        if let Some(client) = self.active_client().await {
            client
                .store_directory_sort(dir_path, sort_field, sort_order)
                .await
                .map_err(SyncError::Client)?;
        }
        Ok(())
    }

    /// Loads persisted directory sorting preferences for a specific directory from the permanent SDK config store.
    pub async fn load_directory_sort(&self, dir_path: &str) -> Option<(String, String)> {
        let key = format!("sort:{dir_path}");
        if let Some(ref storage) = self.storage
            && let Ok(Some(val)) = storage.get_setting(&key)
            && let Some((field, order)) = val.split_once(':')
        {
            return Some((field.to_string(), order.to_string()));
        }
        if let Some(client) = self.active_client().await
            && let Ok(Some(pair)) = client.load_directory_sort(dir_path).await
        {
            return Some(pair);
        }
        None
    }

    /// Returns the size in bytes of downloaded and hydrated virtual sync files.
    pub async fn virtual_sync_downloaded_size(&self) -> u64 {
        let hydrated_dir = self.cache_dir().join("hydrated");
        fn dir_size(path: &std::path::Path) -> u64 {
            let mut total = 0;
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        total += dir_size(&p);
                    } else if let Ok(meta) = entry.metadata() {
                        total += meta.len();
                    }
                }
            }
            total
        }
        dir_size(&hydrated_dir)
    }

    /// Clears downloaded virtual files for the virtual sync mount, returning bytes freed.
    pub async fn clear_virtual_sync_cache(&self) -> Result<u64, SyncError> {
        let before_size = self.virtual_sync_downloaded_size().await;
        let hydrated_dir = self.cache_dir().join("hydrated");
        if hydrated_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&hydrated_dir).await;
            let _ = tokio::fs::create_dir_all(&hydrated_dir).await;
        }
        Ok(before_size)
    }

    /// Cleans and deletes cached thumbnails, returning bytes freed.
    pub async fn clean_thumbnails(&self) -> Result<u64, SyncError> {
        let thumbs_dir = self.active_thumbnails_dir().await;
        let mut freed = 0;
        if let Ok(entries) = std::fs::read_dir(&thumbs_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    freed += meta.len();
                }
            }
        }
        if thumbs_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&thumbs_dir).await;
            let _ = tokio::fs::create_dir_all(&thumbs_dir).await;
        }
        Ok(freed)
    }

    /// Cleans all caches: thumbnails, temporary hydrated files, and cache database, returning total bytes freed.
    pub async fn clean_everything(&self) -> Result<u64, SyncError> {
        let thumbs_freed = self.clean_thumbnails().await.unwrap_or(0);
        let vfs_freed = self.clear_virtual_sync_cache().await.unwrap_or(0);
        if let Some(client) = self.active_client().await {
            let _ = client.clear_cache().await;
        }
        Ok(thumbs_freed + vfs_freed)
    }

    /// Removes cached thumbnails that exceed the configured maximum thumbnail size, returning bytes freed.
    pub async fn clean_oversized_thumbnails(&self) -> Result<u64, SyncError> {
        let max_size = self.max_thumbnail_size().await;
        let thumbs_dir = self.active_thumbnails_dir().await;
        let mut freed = 0;
        if let Ok(entries) = std::fs::read_dir(&thumbs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
                if let Some(rest) = filename.strip_suffix(".png")
                    && let Some((_, size_str)) = rest.rsplit_once('_')
                    && let Ok(size) = size_str.parse::<u32>()
                    && size > max_size
                {
                    if let Ok(meta) = entry.metadata() {
                        freed += meta.len();
                    }
                    let _ = tokio::fs::remove_file(&path).await;
                }
            }
        }
        Ok(freed)
    }

    /// Returns the maximum allowed thumbnail dimension configured by the user.
    pub async fn max_thumbnail_size(&self) -> u32 {
        if let Some(client) = self.active_client().await
            && let Ok(Some(val)) = client.get_setting("max_thumbnail_size").await
        {
            val.parse::<u32>().unwrap_or(256)
        } else {
            256
        }
    }

    /// Sets the maximum allowed thumbnail dimension configured by the user.
    pub async fn set_max_thumbnail_size(&self, size: u32) -> Result<(), SyncError> {
        if let Some(client) = self.active_client().await {
            let _ = client
                .set_setting("max_thumbnail_size", size.to_string())
                .await;
        }
        Ok(())
    }

    /// Runs synchronization on all enabled two-way sync folder pairs via SDK `FolderSyncRunner`.
    pub async fn sync_all_pairs(&self) -> Result<(), SyncError> {
        let (pairs, cache_dir) = {
            let conf = self.config.read().await;
            (conf.sync_folders.clone(), conf.resolved_cache_dir())
        };

        for pair in pairs {
            if !pair.enabled || pair.mode != SyncMode::FullTwoWaySync {
                continue;
            }

            let journal_path = cache_dir.join(format!("journal_{}.json", pair.id));
            let runner = FolderSyncRunner::new(&pair.local_path, &pair.remote_path, journal_path);
            let _ = runner.run_local_reconciliation();
        }

        Ok(())
    }

    /// Recursively searches the virtual tree and server unified search for matching files/directories.
    pub async fn search(
        &self,
        term: &str,
    ) -> Result<Vec<nextcloud_rust_sdk::VirtualEntrySummary>, SyncError> {
        let trimmed = term.trim().to_string();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let this = self.clone();
        tokio::task::spawn(async move {
            let vfs = this.active_vfs().await;
            let term_lower = trimmed.to_lowercase();

            let local_future = async {
                let mut matches = Vec::new();
                let mut queue = vec!["/".to_string()];
                while let Some(current_dir) = queue.pop() {
                    if let Ok(children) = vfs.list_children(&current_dir).await {
                        for child in children {
                            if child.name.to_lowercase().contains(&term_lower)
                                || child.path.to_lowercase().contains(&term_lower)
                            {
                                matches.push(child.clone());
                            }
                            if child.is_directory {
                                queue.push(child.path);
                            }
                        }
                    }
                }
                matches
            };

            let remote_future = async {
                let mut remote_matches = Vec::new();
                if let Some((_, client, _)) = this.active_dav_context().await
                    && let Ok(ocs) = client.ocs()
                {
                    let search_client = client.search(&ocs);
                    // Impose 3s timeout on remote Unified Search so network latency never freezes search
                    let search_res = tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        search_client.search("files", &trimmed),
                    )
                    .await;

                    if let Ok(Ok(result_set)) = search_res {
                        for entry in result_set.entries {
                            let candidate_path = if let Some(sub) = entry.subline {
                                if sub.starts_with('/') {
                                    sub
                                } else {
                                    format!("/{sub}")
                                }
                            } else {
                                format!("/{}", entry.title)
                            };

                            let is_dir = entry
                                .icon
                                .as_deref()
                                .is_some_and(|ic| ic.contains("folder"));
                            let summary = nextcloud_rust_sdk::VirtualEntrySummary {
                                path: candidate_path,
                                name: entry.title,
                                is_directory: is_dir,
                                size: entry
                                    .attributes
                                    .get("size")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0),
                                etag: None,
                                modified_at: None,
                                permissions: None,
                                mime_type: None,
                                file_id: entry.attributes.get("fileid").and_then(|v| v.as_u64()),
                                icon_category: if is_dir {
                                    nextcloud_rust_sdk::ResourceIconCategory::Folder
                                } else {
                                    nextcloud_rust_sdk::ResourceIconCategory::Document
                                },
                                is_hydrated: false,
                                cached_path: None,
                                directory_summary: None,
                            };
                            remote_matches.push(summary);
                        }
                    }
                }
                remote_matches
            };

            // Run local VFS tree traversal and remote Unified Search concurrently
            let (mut matches, remote_entries) = tokio::join!(local_future, remote_future);

            for remote_entry in remote_entries {
                if !matches
                    .iter()
                    .any(|m| m.path == remote_entry.path || m.name == remote_entry.name)
                {
                    matches.push(remote_entry);
                }
            }

            Ok(matches)
        })
        .await?
    }

    /// Fast synchronous check returning a cached thumbnail path (preferred size or 64px placeholder) if already on disk.
    pub fn cached_thumbnail_path(
        &self,
        remote_path: &str,
        preferred_size: Option<u32>,
    ) -> Option<PathBuf> {
        let thumbs_dir = self.active_thumbnails_dir_sync();
        let fallback_thumbs_dir = self.cache_dir().join("thumbnails");
        let clean_path = remote_path.trim_matches('/');
        let hash = nextcloud_rust_sdk::md5_hex(clean_path.as_bytes());

        if let Some(s) = preferred_size {
            let p = thumbs_dir.join(format!("{hash}_{s}.png"));
            if p.exists() {
                return Some(p);
            }
            let fb_p = fallback_thumbs_dir.join(format!("{hash}_{s}.png"));
            if fb_p.exists() {
                return Some(fb_p);
            }
        }

        let low_p = thumbs_dir.join(format!("{hash}_64.png"));
        if low_p.exists() {
            return Some(low_p);
        }
        let fb_low_p = fallback_thumbs_dir.join(format!("{hash}_64.png"));
        if fb_low_p.exists() {
            return Some(fb_low_p);
        }

        None
    }

    /// Polls remote WebDAV root ETags across all configured accounts to detect server-side moves and file changes.
    pub async fn check_remote_changes_all_accounts(&self) {
        let accounts = self.accounts().await;
        if accounts.is_empty() {
            return;
        }

        for account in accounts {
            let Some((user, client, auth)) = self.dav_context_for_account(&account).await else {
                continue;
            };

            let dav_client = client.dav(&auth);
            let Ok(root_dav) = DavPath::root(&user) else {
                continue;
            };

            // Lightweight PROPFIND Depth: 0 asking ONLY for d:getetag
            let body = PropfindBody::new([DavProperty::Etag]);
            let Ok(res) = dav_client.propfind(&root_dav, Depth::Zero, &body).await else {
                continue;
            };

            let Some(first) = res.first() else {
                continue;
            };

            let Some(ref etag) = first.etag else {
                continue;
            };

            let changed = {
                let mut etags = self.last_root_etags.write().await;
                match etags.get(&account.id.0) {
                    Some(prev) if prev == etag => false,
                    _ => {
                        etags.insert(account.id.0.clone(), etag.clone());
                        true
                    }
                }
            };

            if changed {
                tracing::info!(
                    account = %account.id.0,
                    user = %user,
                    etag = %etag,
                    "Remote server changes detected via root ETag poll; refreshing VFS tree"
                );

                // Refresh this account's virtual tree, pruning moved/deleted nodes
                let _ = self.refresh_account_virtual_tree(&account).await;

                // Synchronize any two-way folder sync pairings belonging to this account
                self.sync_account_mounts(&account.id.0).await;
            }
        }
    }

    /// Refreshes the virtual file tree for a specific account, pruning moved/deleted items and syncing FUSE.
    pub async fn refresh_account_virtual_tree(
        &self,
        account: &nextcloud_desktop_core::account::Account,
    ) -> Result<usize, SyncError> {
        let Some((user, client, auth)) = self.dav_context_for_account(account).await else {
            return Ok(0);
        };

        let dav_client = client.dav(&auth);
        let root_dav =
            DavPath::root(&user).map_err(|err| SyncError::InvalidPath(err.to_string()))?;

        let body = PropfindBody::all();
        let resources = match dav_client.propfind(&root_dav, Depth::One, &body).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(
                    account = %account.id.0,
                    error = %err,
                    "Failed to query WebDAV root during remote change refresh"
                );
                return Ok(0);
            }
        };

        let dav_prefix = format!("/remote.php/dav/files/{user}");
        let summaries: Vec<nextcloud_rust_sdk::DavResourceSummary> = resources
            .iter()
            .map(|res| {
                let mut sum =
                    nextcloud_rust_sdk::DavResourceSummary::new(&res.href, res.is_collection);
                if let Some(len) = res.content_length {
                    sum = sum.with_content_length(len);
                }
                if let Some(ref etag) = res.etag {
                    sum = sum.with_etag(etag);
                }
                if let Some(ref lm) = res.last_modified {
                    sum = sum.with_last_modified(lm);
                }
                if let Some(ref mime) = res.content_type {
                    sum = sum.with_content_type(mime);
                }
                if let Some(ref fid) = res.file_id
                    && let Ok(id) = fid.parse::<u64>()
                {
                    sum = sum.with_file_id(id);
                }
                if let Some(prev) = res.has_preview {
                    sum = sum.with_has_preview(prev);
                }
                sum
            })
            .collect();

        // 1. Update the account's persistent VFS tree on disk
        let vfs_dir = self.account_vfs_dir(&account.id.0);
        let tree_file = vfs_dir.join("tree.json");
        let mut tree = if tree_file.exists() {
            std::fs::read_to_string(&tree_file)
                .ok()
                .and_then(|data| serde_json::from_str(&data).ok())
                .unwrap_or_else(|| nextcloud_rust_sdk::VirtualTree::new("Nextcloud"))
        } else {
            nextcloud_rust_sdk::VirtualTree::new("Nextcloud")
        };

        let count = tree.populate_from_resources(&dav_prefix, &summaries);
        if let Ok(json) = serde_json::to_string_pretty(&tree) {
            let _ = std::fs::write(&tree_file, json);
        }

        // 2. If this is the active account, also sync to orchestrator and FUSE in-memory tree:
        let is_active = self
            .active_account()
            .await
            .map(|a| a.id.0 == account.id.0)
            .unwrap_or(false);
        if is_active {
            let vfs = self.active_vfs().await;
            let _ = vfs
                .populate_from_resources(&dav_prefix, summaries.clone())
                .await;

            if let Some(ref ft_lock) = *self.fuse_tree.read().await
                && let Ok(mut guard) = ft_lock.write()
            {
                *guard = tree.clone();
            }

            self.event_bus
                .emit(DesktopEvent::VfsTreeUpdated { node_count: count });

            self.trigger_background_tree_preload();
        }

        Ok(count)
    }

    /// Installs FreeDesktop system thumbnails for a remote file across all active mounts (FUSE, internal VFS, folder pairs).
    pub async fn install_freedesktop_previews_for_remote_path(
        &self,
        remote_path: &str,
        png_bytes: &[u8],
    ) {
        let clean_path = remote_path.trim_matches('/');
        let png = crate::freedesktop_thumbs::ensure_png_bytes(png_bytes);

        // 1. Resolve mtime from active VFS or fallback to now
        let vfs = self.active_vfs().await;
        let mtime_sec = vfs
            .entry(remote_path)
            .await
            .ok()
            .flatten()
            .and_then(|e| e.modified_at)
            .and_then(|s| nextcloud_rust_sdk::parse_date_to_unix_secs(&s))
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            });

        // 2. FUSE mountpoint (if mounted)
        if let Some(fuse_mp) = self.fuse_mountpoint().await {
            let target = fuse_mp.join(clean_path);
            let target_mtime = target
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or_else(|_| std::time::SystemTime::now())
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(mtime_sec);
            crate::freedesktop_thumbs::install_freedesktop_thumbnail_with_mtime(
                &target,
                &png,
                target_mtime,
            );
        }

        // 3. Account internal VFS directory
        if let Some(account) = self.active_account().await {
            let vfs_target = self.account_vfs_dir(&account.id.0).join(clean_path);
            if vfs_target.exists() {
                let vfs_mtime = vfs_target
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or_else(|_| std::time::SystemTime::now())
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(mtime_sec);
                crate::freedesktop_thumbs::install_freedesktop_thumbnail_with_mtime(
                    &vfs_target,
                    &png,
                    vfs_mtime,
                );
            }
        }

        // 4. Any user sync folders matching remote_path
        for mount in self.sync_mounts().await {
            if Self::is_internal_vfs(&mount.id) || !mount.enabled {
                continue;
            }
            let m_remote = mount.remote_path.trim_end_matches('/');
            let rel = if m_remote.is_empty() || m_remote == "/" {
                Some(clean_path)
            } else {
                clean_path.strip_prefix(m_remote.trim_start_matches('/'))
            };
            if let Some(rel_path) = rel {
                let rel_clean = rel_path.trim_start_matches('/');
                let local_target = PathBuf::from(&mount.local_target).join(rel_clean);
                if local_target.exists() {
                    let local_mtime = local_target
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or_else(|_| std::time::SystemTime::now())
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(mtime_sec);
                    crate::freedesktop_thumbs::install_freedesktop_thumbnail_with_mtime(
                        &local_target,
                        &png,
                        local_mtime,
                    );
                }
            }
        }
    }

    /// Fetches or retrieves a cached thumbnail for `remote_path`.
    pub async fn fetch_thumbnail(
        &self,
        remote_path: &str,
        size: u32,
    ) -> Result<Option<PathBuf>, SyncError> {
        self.fetch_thumbnail_with_id(remote_path, None, size).await
    }

    /// Fetches or retrieves a cached thumbnail for `remote_path` with an optionally known Nextcloud file ID.
    pub async fn fetch_thumbnail_with_id(
        &self,
        remote_path: &str,
        known_file_id: Option<u64>,
        size: u32,
    ) -> Result<Option<PathBuf>, SyncError> {
        let max_size = self.max_thumbnail_size().await;
        // Snap to standard Nextcloud thumbnail square sizes: 64, 128, 256, 512, 1024
        let standard_size = match size {
            s if s <= 64 => 64,
            s if s <= 128 => 128,
            s if s <= 256 => 256,
            s if s <= 512 => 512,
            _ => 1024,
        };
        let effective_size = standard_size.min(max_size).max(64);

        let thumbs_dir = self.active_thumbnails_dir().await;
        let _ = tokio::fs::create_dir_all(&thumbs_dir).await;

        let clean_path = remote_path.trim_matches('/');
        let hash = nextcloud_rust_sdk::md5_hex(clean_path.as_bytes());
        let thumb_filename = format!("{hash}_{effective_size}.png");
        let thumb_path = thumbs_dir.join(thumb_filename);

        if thumb_path.exists() {
            if let Ok(bytes) = tokio::fs::read(&thumb_path).await {
                self.install_freedesktop_previews_for_remote_path(remote_path, &bytes)
                    .await;
            }
            return Ok(Some(thumb_path));
        }

        // Deduplicate in-flight requests: if this remote path is already being fetched, return immediately
        if let Ok(mut stats) = self.thumbnail_stats.write() {
            if stats.active_items.contains(&remote_path.to_string()) {
                return Ok(None);
            }
            stats.active_items.push(remote_path.to_string());
            stats.total += 1;
            stats.in_progress += 1;
            self.event_bus.emit(
                nextcloud_desktop_core::events::DesktopEvent::ThumbnailProgress {
                    completed: stats.completed,
                    total: stats.total,
                    in_progress: stats.in_progress,
                },
            );
        }

        self.event_bus.emit(
            nextcloud_desktop_core::events::DesktopEvent::ThumbnailDownloadStarted {
                path: remote_path.to_string(),
            },
        );

        self.wait_for_priority_ops().await;
        let _permit = self.preview_semaphore.acquire().await.ok();

        if thumb_path.exists() {
            if let Ok(bytes) = tokio::fs::read(&thumb_path).await {
                self.install_freedesktop_previews_for_remote_path(remote_path, &bytes)
                    .await;
            }
            if let Ok(mut stats) = self.thumbnail_stats.write() {
                stats.in_progress = stats.in_progress.saturating_sub(1);
                stats.completed += 1;
                stats.active_items.retain(|p| p != remote_path);
            }
            self.event_bus.emit(
                nextcloud_desktop_core::events::DesktopEvent::ThumbnailReady {
                    path: remote_path.to_string(),
                    cached_path: thumb_path.clone(),
                },
            );
            return Ok(Some(thumb_path));
        }

        // Look up file_id: prefer known_file_id from remote discovery, fallback to active VFS
        let file_id = if known_file_id.is_some() {
            known_file_id
        } else {
            let vfs = self.active_vfs().await;
            vfs.entry(remote_path)
                .await
                .ok()
                .flatten()
                .and_then(|e| e.file_id)
        };

        if let Some((user, client, auth)) = self.active_dav_context().await {
            let preview = client.preview(&auth);
            let dav = client.dav(&auth);
            let clean_no_slash = clean_path.trim_start_matches('/');

            let opts = nextcloud_rust_sdk::PreviewOptions::new()
                .dimensions(effective_size, effective_size)
                .mode(nextcloud_rust_sdk::PreviewMode::Cover);

            if let Some(bytes) =
                fetch_preview_bytes(&preview, &dav, &user, file_id, clean_no_slash, &opts).await
            {
                let png_bytes = crate::freedesktop_thumbs::ensure_png_bytes(&bytes);
                if tokio::fs::write(&thumb_path, &png_bytes).await.is_ok() {
                    self.install_freedesktop_previews_for_remote_path(remote_path, &png_bytes)
                        .await;
                }
                if let Ok(mut stats) = self.thumbnail_stats.write() {
                    stats.in_progress = stats.in_progress.saturating_sub(1);
                    stats.completed += 1;
                    stats.active_items.retain(|p| p != remote_path);
                    self.event_bus.emit(
                        nextcloud_desktop_core::events::DesktopEvent::ThumbnailProgress {
                            completed: stats.completed,
                            total: stats.total,
                            in_progress: stats.in_progress,
                        },
                    );
                }
                self.event_bus.emit(
                    nextcloud_desktop_core::events::DesktopEvent::ThumbnailReady {
                        path: remote_path.to_string(),
                        cached_path: thumb_path.clone(),
                    },
                );
                return Ok(Some(thumb_path));
            }
        }

        if let Ok(mut stats) = self.thumbnail_stats.write() {
            stats.in_progress = stats.in_progress.saturating_sub(1);
            stats.failed += 1;
            stats.active_items.retain(|p| p != remote_path);
            self.event_bus.emit(
                nextcloud_desktop_core::events::DesktopEvent::ThumbnailProgress {
                    completed: stats.completed,
                    total: stats.total,
                    in_progress: stats.in_progress,
                },
            );
        }

        self.event_bus.emit(
            nextcloud_desktop_core::events::DesktopEvent::ThumbnailFailed {
                path: remote_path.to_string(),
            },
        );

        Ok(None)
    }

    /// Batch prefetches thumbnails for multiple remote paths in the background.
    pub fn batch_prefetch_thumbnails(&self, paths: Vec<String>, size: u32) {
        self.batch_prefetch_thumbnails_with_ids(
            paths.into_iter().map(|p| (p, None)).collect(),
            size,
        );
    }

    /// Batch prefetches thumbnails for multiple remote paths with known file IDs in the background.
    pub fn batch_prefetch_thumbnails_with_ids(&self, items: Vec<(String, Option<u64>)>, size: u32) {
        if items.is_empty() {
            return;
        }

        let coord = self.clone();
        tokio::spawn(async move {
            let max_size = coord.max_thumbnail_size().await;
            let effective_size = size.clamp(64, max_size);

            let thumbs_dir = coord.active_thumbnails_dir().await;
            let mut missing_items: Vec<(String, Option<u64>)> = Vec::new();
            for (path, fid) in items {
                let clean = path.trim_matches('/');
                let hash = nextcloud_rust_sdk::md5_hex(clean.as_bytes());
                let thumb_path = thumbs_dir.join(format!("{hash}_{effective_size}.png"));
                if thumb_path.exists() {
                    if let Ok(bytes) = tokio::fs::read(&thumb_path).await {
                        coord
                            .install_freedesktop_previews_for_remote_path(&path, &bytes)
                            .await;
                    }
                } else {
                    missing_items.push((path, fid));
                }
            }

            if missing_items.is_empty() {
                return;
            }

            let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
            let mut tasks = Vec::with_capacity(missing_items.len());
            for (path, fid) in missing_items {
                let c = coord.clone();
                let sem = semaphore.clone();
                tasks.push(tokio::spawn(async move {
                    c.wait_for_priority_ops().await;
                    let _permit = match sem.acquire().await {
                        Ok(p) => p,
                        Err(_) => return,
                    };
                    c.wait_for_priority_ops().await;
                    let _ = c.fetch_thumbnail_with_id(&path, fid, effective_size).await;
                }));
            }
            for task in tasks {
                let _ = task.await;
            }
        });
    }

    /// Triggers background crawling and preloading of unpopulated virtual directory trees.
    pub fn trigger_background_tree_preload(&self) {
        if self
            .is_preloading_tree
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return;
        }

        let this = self.clone();
        tokio::spawn(async move {
            this.run_background_tree_preload().await;
            this.is_preloading_tree
                .store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    async fn run_background_tree_preload(&self) {
        let vfs = self.active_vfs().await;
        while let Ok(unpopulated) = vfs.unpopulated_directories().await {
            if unpopulated.is_empty() {
                break;
            }
            let mut any_fetched = false;
            for dir in unpopulated {
                if let Ok(true) = vfs.is_directory_populated(&dir).await {
                    continue;
                }
                if let Ok(true) = self.is_sync_paused().await {
                    return;
                }
                tracing::debug!(path = %dir, "Background preloading virtual directory");
                let _ = self.refresh_path(&dir).await;
                any_fetched = true;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            if !any_fetched {
                break;
            }
        }
    }
}

async fn fetch_preview_bytes<
    T: nextcloud_rust_sdk::Transport,
    A: nextcloud_rust_sdk::AuthProvider,
>(
    preview: &nextcloud_rust_sdk::PreviewClient<'_, T, A>,
    dav: &nextcloud_rust_sdk::DavClient<'_, T, A>,
    user: &str,
    file_id: Option<u64>,
    clean_no_slash: &str,
    opts: &nextcloud_rust_sdk::PreviewOptions,
) -> Option<Vec<u8>> {
    // 1. Primary: fetch by numeric file ID if known (fastest and most reliable in Nextcloud)
    if let Some(fid) = file_id
        && let Ok(bytes) = preview.get_by_file_id(fid, opts).await
    {
        return Some(bytes);
    }
    // 2. Secondary: fetch by path
    if let Ok(bytes) = preview.get_by_path(clean_no_slash, opts).await {
        return Some(bytes);
    }
    // 3. Fallback without mode (Nextcloud rejects mode=cover for HEIC, video, and PDF formats)
    let simple_opts = nextcloud_rust_sdk::PreviewOptions::new().dimensions(
        opts.width_px().unwrap_or(64),
        opts.height_px().unwrap_or(64),
    );
    if let Some(fid) = file_id
        && let Ok(bytes) = preview.get_by_file_id(fid, &simple_opts).await
    {
        return Some(bytes);
    }
    if let Ok(bytes) = preview.get_by_path(clean_no_slash, &simple_opts).await {
        return Some(bytes);
    }

    // 4. Fallback for large camera photos: extract embedded EXIF thumbnail via Range request
    let lower = clean_no_slash.to_ascii_lowercase();
    if (lower.ends_with(".jpg") || lower.ends_with(".jpeg"))
        && let Ok(dav_path) = make_dav_path(user, clean_no_slash)
        && let Ok(header) = dav.get_range(&dav_path, 0, 131071).await
        && let Some(thumb) = nextcloud_rust_sdk::extract_embedded_jpeg_thumbnail(&header)
    {
        return Some(thumb.to_vec());
    }

    None
}

fn decode_percent_str(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                let hex_slice = [c1, c2];
                if let Ok(s) = std::str::from_utf8(&hex_slice)
                    && let Ok(val) = u8::from_str_radix(s, 16)
                {
                    bytes.push(val);
                    continue;
                }
                bytes.push(b'%');
                bytes.push(c1);
                bytes.push(c2);
            } else {
                bytes.push(b'%');
            }
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8_lossy(&bytes).to_string()
}
