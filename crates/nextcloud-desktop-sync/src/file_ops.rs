//! High-level virtual file mutations and two-way folder synchronization.

use nextcloud_rust_sdk::{DavPath, DavProperty, Depth, FolderSyncRunner, PropfindBody};
use std::path::PathBuf;

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use nextcloud_desktop_core::config::SyncMode;
use nextcloud_desktop_core::events::DesktopEvent;

/// Constructs a normalized [`DavPath`] from user name and relative path.
pub fn make_dav_path(user: &str, relative: &str) -> Result<DavPath, SyncError> {
    let clean = relative.trim_matches('/');
    if clean.is_empty() {
        DavPath::root(user).map_err(|err| SyncError::InvalidPath(err.to_string()))
    } else {
        DavPath::new(user, clean).map_err(|err| SyncError::InvalidPath(err.to_string()))
    }
}

impl DesktopSyncCoordinator {
    /// Refreshes the virtual file tree for the active configured account, or resets to local virtual hierarchy.
    pub async fn refresh_virtual_tree(&self) -> Result<usize, SyncError> {
        let vfs = self.active_vfs().await;

        let Some((user, client, auth)) = self.active_dav_context().await else {
            return Ok(0);
        };

        let dav_client = client.dav(&auth);
        let root_dav =
            DavPath::root(&user).map_err(|err| SyncError::InvalidPath(err.to_string()))?;

        let body = PropfindBody::all();
        let resources = match dav_client.propfind(&root_dav, Depth::One, &body).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!("Remote WebDAV PROPFIND failed ({err}), keeping default tree");
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
        let inserted = vfs
            .populate_from_resources(&dav_prefix, summaries.clone())
            .await
            .unwrap_or(0);

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
        let clean = format!("/{}", remote_path.trim_matches('/'));
        self.event_bus.emit(DesktopEvent::NodeHydrationStarted {
            path: clean.clone(),
        });
        let vfs = self.active_vfs().await;

        if let Some((user, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            let clean_sub = remote_path.trim_matches('/');
            let dav_path = DavPath::new(&user, clean_sub)
                .map_err(|e| SyncError::InvalidPath(e.to_string()))?;
            if let Ok(bytes) = dav_client.get(&dav_path).await {
                let byte_len = bytes.len() as u64;
                let local_path = vfs
                    .hydrate_local(remote_path, bytes.clone())
                    .await
                    .map_err(SyncError::Orchestrator)?;

                // If this file matches a local folder sync mount, hydrate the local file directly
                for mount in self.sync_mounts().await {
                    if mount.id == "app-default-vfs" || !mount.enabled {
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
                        if std::fs::write(&target_file, &bytes).is_ok() {
                            let journal_dir = PathBuf::from(&mount.local_target).join(".nextcloud");
                            let _ = std::fs::create_dir_all(&journal_dir);
                            let journal_path = journal_dir.join("journal.json");
                            let mut journal =
                                nextcloud_rust_sdk::SyncJournal::load_from_file(&journal_path)
                                    .unwrap_or_else(|_| nextcloud_rust_sdk::SyncJournal::new());
                            let mtime = std::fs::metadata(&target_file)
                                .and_then(|m| m.modified())
                                .unwrap_or_else(|_| std::time::SystemTime::now());
                            journal.record(nextcloud_rust_sdk::JournalEntry::new(
                                rel_clean, byte_len, mtime, false,
                            ));
                            let _ = journal.save_to_file(&journal_path);
                            self.event_bus.emit(DesktopEvent::FileTransferCompleted {
                                relative_path: rel_clean.to_string(),
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
                return Ok(local_path);
            }
        }

        let placeholder = format!("Offline placeholder content for {remote_path}\n");
        let byte_len = placeholder.len() as u64;
        let local_path = vfs
            .hydrate_local(remote_path, placeholder.into_bytes())
            .await
            .map_err(SyncError::Orchestrator)?;
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
        let mounts = self.sync_mounts().await;
        let canonical = local_path
            .canonicalize()
            .unwrap_or_else(|_| local_path.to_path_buf());

        for mount in mounts {
            if mount.id == "app-default-vfs" || !mount.enabled {
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

                    let journal_dir = target_canonical.join(".nextcloud");
                    let _ = std::fs::create_dir_all(&journal_dir);
                    let journal_path = journal_dir.join("journal.json");
                    let mut journal =
                        nextcloud_rust_sdk::SyncJournal::load_from_file(&journal_path)
                            .unwrap_or_else(|_| nextcloud_rust_sdk::SyncJournal::new());
                    let mtime = std::fs::metadata(&canonical)
                        .and_then(|m| m.modified())
                        .unwrap_or_else(|_| std::time::SystemTime::now());
                    journal.record(nextcloud_rust_sdk::JournalEntry::new(
                        &rel_str, byte_len, mtime, false,
                    ));
                    let _ = journal.save_to_file(&journal_path);

                    self.event_bus.emit(DesktopEvent::FileTransferCompleted {
                        relative_path: rel_str,
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
        let thumbs_dir = self.cache_dir().join("thumbnails");
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
        let thumbs_dir = self.cache_dir().join("thumbnails");
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
        let cache_dir = self.cache_dir();
        let thumbs_dir = cache_dir.join("thumbnails");
        let clean_path = remote_path.trim_matches('/');
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&clean_path, &mut hasher);
        let path_hash = std::hash::Hasher::finish(&hasher);

        if let Some(s) = preferred_size {
            let p = thumbs_dir.join(format!("{:016x}_{s}.png", path_hash));
            if p.exists() {
                return Some(p);
            }
        }

        let low_p = thumbs_dir.join(format!("{:016x}_64.png", path_hash));
        if low_p.exists() {
            return Some(low_p);
        }

        None
    }

    /// Fetches or retrieves a cached thumbnail for `remote_path`.
    pub async fn fetch_thumbnail(
        &self,
        remote_path: &str,
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

        let cache_dir = self.cache_dir();
        let thumbs_dir = cache_dir.join("thumbnails");
        let _ = tokio::fs::create_dir_all(&thumbs_dir).await;

        let clean_path = remote_path.trim_matches('/');
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&clean_path, &mut hasher);
        let path_hash = std::hash::Hasher::finish(&hasher);
        let thumb_filename = format!("{:016x}_{effective_size}.png", path_hash);
        let thumb_path = thumbs_dir.join(thumb_filename);

        if thumb_path.exists() {
            return Ok(Some(thumb_path));
        }

        // Fast check: is low-res fallback already cached?
        let low_filename = format!("{:016x}_64.png", path_hash);
        let low_path = thumbs_dir.join(low_filename);

        // Update coordinator thumbnail stats
        if let Ok(mut stats) = self.thumbnail_stats.write() {
            stats.total += 1;
            stats.in_progress += 1;
            if !stats.active_items.contains(&remote_path.to_string()) {
                stats.active_items.push(remote_path.to_string());
            }
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

        let _permit = self.preview_semaphore.acquire().await.ok();

        if thumb_path.exists() {
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

        // Look up file_id from active VFS, or query WebDAV PROPFIND if not present in tree
        let vfs = self.active_vfs().await;
        let mut file_id = vfs
            .entry(remote_path)
            .await
            .ok()
            .flatten()
            .and_then(|e| e.file_id);

        if file_id.is_none()
            && let Some((user, client, auth)) = self.active_dav_context().await
        {
            let dav = client.dav(&auth);
            if let Ok(dav_path) = make_dav_path(&user, remote_path) {
                let body = PropfindBody::new([DavProperty::FileId]);
                if let Ok(res) = dav.propfind(&dav_path, Depth::Zero, &body).await
                    && let Some(first) = res.first()
                    && let Some(ref fid) = first.file_id
                    && let Ok(parsed) = fid.parse::<u64>()
                {
                    file_id = Some(parsed);
                }
            }
        }

        if let Some((_, client, auth)) = self.active_dav_context().await {
            let preview = client.preview(&auth);

            let clean_no_slash = clean_path.trim_start_matches('/');
            let with_slash = format!("/{clean_no_slash}");

            // Progressive loading: if target size > 64 and 64px is not cached yet,
            // fetch smallest 64px thumbnail first and emit ThumbnailReady immediately!
            if effective_size > 64 && !low_path.exists() {
                let low_opts = nextcloud_rust_sdk::PreviewOptions::new()
                    .dimensions(64, 64)
                    .mode(nextcloud_rust_sdk::PreviewMode::Cover);

                if let Some(bytes) =
                    fetch_preview_bytes(&preview, file_id, clean_no_slash, &with_slash, &low_opts)
                        .await
                    && tokio::fs::write(&low_path, &bytes).await.is_ok()
                {
                    self.event_bus.emit(
                        nextcloud_desktop_core::events::DesktopEvent::ThumbnailReady {
                            path: remote_path.to_string(),
                            cached_path: low_path.clone(),
                        },
                    );
                }
            }

            let opts = nextcloud_rust_sdk::PreviewOptions::new()
                .dimensions(effective_size, effective_size)
                .mode(nextcloud_rust_sdk::PreviewMode::Cover);

            if let Some(bytes) =
                fetch_preview_bytes(&preview, file_id, clean_no_slash, &with_slash, &opts).await
                && tokio::fs::write(&thumb_path, &bytes).await.is_ok()
            {
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

            // Fallback to 64x64 if higher-res preview returned server error
            if effective_size != 64 {
                if low_path.exists() {
                    if let Ok(mut stats) = self.thumbnail_stats.write() {
                        stats.in_progress = stats.in_progress.saturating_sub(1);
                        stats.completed += 1;
                        stats.active_items.retain(|p| p != remote_path);
                    }
                    self.event_bus.emit(
                        nextcloud_desktop_core::events::DesktopEvent::ThumbnailReady {
                            path: remote_path.to_string(),
                            cached_path: low_path.clone(),
                        },
                    );
                    return Ok(Some(low_path));
                }

                let low_opts = nextcloud_rust_sdk::PreviewOptions::new()
                    .dimensions(64, 64)
                    .mode(nextcloud_rust_sdk::PreviewMode::Cover);

                if let Some(bytes) =
                    fetch_preview_bytes(&preview, file_id, clean_no_slash, &with_slash, &low_opts)
                        .await
                    && tokio::fs::write(&low_path, &bytes).await.is_ok()
                {
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
                            cached_path: low_path.clone(),
                        },
                    );
                    return Ok(Some(low_path));
                }
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

        if low_path.exists() {
            return Ok(Some(low_path));
        }

        Ok(None)
    }

    /// Batch prefetches thumbnails for multiple remote paths progressively from smallest up to target size.
    pub fn batch_prefetch_thumbnails(&self, paths: Vec<String>, size: u32) {
        if paths.is_empty() {
            return;
        }

        let coord = self.clone();
        tokio::spawn(async move {
            let max_size = coord.max_thumbnail_size().await;
            // Snap to standard Nextcloud thumbnail square sizes: 64, 128, 256, 512, 1024
            let standard_size = match size {
                s if s <= 64 => 64,
                s if s <= 128 => 128,
                s if s <= 256 => 256,
                s if s <= 512 => 512,
                _ => 1024,
            };
            let effective_size = standard_size.min(max_size).max(64);

            // Progressive size tiers: start with smallest (64px), then intermediate, up to target_size
            let mut tiers = vec![64u32];
            if effective_size > 128 {
                tiers.push(128);
            }
            if effective_size > 64 && !tiers.contains(&effective_size) {
                tiers.push(effective_size);
            }

            for size_tier in tiers {
                let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(6));
                let mut tasks = Vec::with_capacity(paths.len());
                for path in &paths {
                    let c = coord.clone();
                    let p = path.clone();
                    let sem = semaphore.clone();
                    tasks.push(tokio::spawn(async move {
                        let _permit = match sem.acquire().await {
                            Ok(p) => p,
                            Err(_) => return,
                        };
                        let _ = c.fetch_thumbnail(&p, size_tier).await;
                    }));
                }
                for task in tasks {
                    let _ = task.await;
                }
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
    file_id: Option<u64>,
    clean_no_slash: &str,
    with_slash: &str,
    opts: &nextcloud_rust_sdk::PreviewOptions,
) -> Option<Vec<u8>> {
    let simple_opts = nextcloud_rust_sdk::PreviewOptions::new().dimensions(
        opts.width_px().unwrap_or(64),
        opts.height_px().unwrap_or(64),
    );

    if let Some(fid) = file_id {
        if let Ok(b) = preview.get_by_file_id(fid, opts).await {
            return Some(b);
        }
        // Fallback without mode in case the server rejects 'mode=cover' parameter
        if let Ok(b) = preview.get_by_file_id(fid, &simple_opts).await {
            return Some(b);
        }
    }
    if let Ok(b) = preview.get_by_path(clean_no_slash, opts).await {
        return Some(b);
    }
    if let Ok(b) = preview.get_by_path(with_slash, opts).await {
        return Some(b);
    }
    if let Ok(b) = preview.get_by_path(clean_no_slash, &simple_opts).await {
        return Some(b);
    }
    preview.get_by_path(with_slash, &simple_opts).await.ok()
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
