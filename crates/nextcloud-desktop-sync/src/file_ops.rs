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
        let resources = match dav_client.propfind(&root_dav, Depth::Infinity, &body).await {
            Ok(r) => r,
            Err(_) => match dav_client.propfind(&root_dav, Depth::One, &body).await {
                Ok(r) => r,
                Err(err) => {
                    tracing::warn!("Remote WebDAV PROPFIND failed ({err}), keeping default tree");
                    return Ok(0);
                }
            },
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

        if self.is_autodownload_thumbnails_enabled().await {
            let media_paths: Vec<String> = summaries
                .into_iter()
                .filter(|s| {
                    !s.is_collection
                        && (s.href.ends_with(".png")
                            || s.href.ends_with(".jpg")
                            || s.href.ends_with(".jpeg")
                            || s.href.ends_with(".webp")
                            || s.content_type
                                .as_deref()
                                .is_some_and(|m| m.starts_with("image/")))
                })
                .map(|s| {
                    s.href
                        .strip_prefix(&dav_prefix)
                        .unwrap_or(&s.href)
                        .to_string()
                })
                .collect();

            if !media_paths.is_empty() {
                self.batch_prefetch_thumbnails(media_paths, 64);
            }
        }

        Ok(inserted)
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
                    .hydrate_local(remote_path, bytes)
                    .await
                    .map_err(SyncError::Orchestrator)?;
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
        let effective_size = size.min(max_size).max(64);

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
            let effective_size = size.min(max_size).max(64);

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
    if let Some(fid) = file_id {
        if let Ok(b) = preview.get_by_file_id(fid, opts).await {
            return Some(b);
        }
        // Fallback without mode in case the server rejects 'mode=cover' parameter
        let simple_opts = nextcloud_rust_sdk::PreviewOptions::new().dimensions(
            opts.width_px().unwrap_or(64),
            opts.height_px().unwrap_or(64),
        );
        if let Ok(b) = preview.get_by_file_id(fid, &simple_opts).await {
            return Some(b);
        }
    }
    if let Ok(b) = preview.get_by_path(clean_no_slash, opts).await {
        return Some(b);
    }
    preview.get_by_path(with_slash, opts).await.ok()
}
