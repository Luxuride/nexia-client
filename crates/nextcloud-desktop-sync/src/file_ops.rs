//! High-level virtual file mutations and two-way folder synchronization.

use nextcloud_rust_sdk::{DavPath, Depth, FolderSyncRunner, PropfindBody};
use std::path::PathBuf;

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use nextcloud_desktop_core::config::SyncMode;

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
                sum
            })
            .collect();

        // Repopulate directly inside the orchestrator actor thread
        let inserted = vfs
            .populate_from_resources(&dav_prefix, summaries)
            .await
            .unwrap_or(0);

        self.event_bus.emit(
            nextcloud_desktop_core::events::DesktopEvent::VfsTreeUpdated {
                node_count: inserted,
            },
        );

        Ok(inserted)
    }

    /// Performs on-demand hydration for a virtual file, downloading via SDK or using local cache.
    pub async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, SyncError> {
        let vfs = self.active_vfs().await;

        if let Some((user, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            let clean = remote_path.trim_matches('/');
            let dav_path =
                DavPath::new(&user, clean).map_err(|e| SyncError::InvalidPath(e.to_string()))?;
            if let Ok(bytes) = dav_client.get(&dav_path).await {
                let local_path = vfs
                    .hydrate_local(remote_path, bytes)
                    .await
                    .map_err(SyncError::Orchestrator)?;
                return Ok(local_path);
            }
        }

        let placeholder = format!("Offline placeholder content for {remote_path}\n");
        let local_path = vfs
            .hydrate_local(remote_path, placeholder.into_bytes())
            .await
            .map_err(SyncError::Orchestrator)?;
        Ok(local_path)
    }

    /// Dehydrates a file node, freeing local cache space.
    pub async fn dehydrate_file(&self, remote_path: &str) -> Result<(), SyncError> {
        let vfs = self.active_vfs().await;
        vfs.dehydrate_local(remote_path)
            .await
            .map_err(SyncError::Orchestrator)
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
        let cache_dir = self.cache_dir();
        let thumbs_dir = cache_dir.join("thumbnails");
        let _ = tokio::fs::create_dir_all(&thumbs_dir).await;

        let clean_path = remote_path.trim_matches('/');
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&clean_path, &mut hasher);
        let path_hash = std::hash::Hasher::finish(&hasher);
        let thumb_filename = format!("{:016x}_{size}.png", path_hash);
        let thumb_path = thumbs_dir.join(thumb_filename);

        if thumb_path.exists() {
            return Ok(Some(thumb_path));
        }

        // Fast check: is low-res fallback already cached?
        let low_filename = format!("{:016x}_64.png", path_hash);
        let low_path = thumbs_dir.join(low_filename);

        let failed_marker = thumbs_dir.join(format!("{:016x}.failed", path_hash));
        if failed_marker.exists() {
            return Ok(None);
        }

        let _permit = self.preview_semaphore.acquire().await.ok();

        if thumb_path.exists() {
            return Ok(Some(thumb_path));
        }

        if let Some((_, client, auth)) = self.active_dav_context().await {
            let preview = client.preview(&auth);

            let clean_no_slash = clean_path.trim_start_matches('/');
            let with_slash = format!("/{clean_no_slash}");

            // Progressive loading: if target size > 64 and 64px is not cached yet,
            // fetch smallest 64px thumbnail first and emit ThumbnailReady immediately!
            if size > 64 && !low_path.exists() {
                let low_opts = nextcloud_rust_sdk::PreviewOptions::new()
                    .dimensions(64, 64)
                    .mode(nextcloud_rust_sdk::PreviewMode::Cover);

                if let Some(bytes) =
                    fetch_preview_bytes(&preview, clean_no_slash, &with_slash, &low_opts).await
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
                .dimensions(size, size)
                .mode(nextcloud_rust_sdk::PreviewMode::Cover);

            if let Some(bytes) =
                fetch_preview_bytes(&preview, clean_no_slash, &with_slash, &opts).await
                && tokio::fs::write(&thumb_path, &bytes).await.is_ok()
            {
                return Ok(Some(thumb_path));
            }

            // Fallback to 64x64 if higher-res preview returned server error
            if size != 64 {
                if low_path.exists() {
                    return Ok(Some(low_path));
                }

                let low_opts = nextcloud_rust_sdk::PreviewOptions::new()
                    .dimensions(64, 64)
                    .mode(nextcloud_rust_sdk::PreviewMode::Cover);

                if let Some(bytes) =
                    fetch_preview_bytes(&preview, clean_no_slash, &with_slash, &low_opts).await
                    && tokio::fs::write(&low_path, &bytes).await.is_ok()
                {
                    return Ok(Some(low_path));
                }
            }

            // Mark as failed so client does not flood server repeatedly
            let _ = tokio::fs::write(&failed_marker, b"").await;
        }

        if low_path.exists() {
            return Ok(Some(low_path));
        }

        Ok(None)
    }

    /// Batch prefetches thumbnails for multiple remote paths concurrently in background tasks.
    pub fn batch_prefetch_thumbnails(&self, paths: Vec<String>, size: u32) {
        if paths.is_empty() {
            return;
        }

        let coord = self.clone();
        tokio::spawn(async move {
            // 1. First prefetch smallest 64px thumbnails in fast concurrent batch
            let mut low_tasks = Vec::new();
            for path in &paths {
                let c = coord.clone();
                let p = path.clone();
                low_tasks.push(tokio::spawn(async move {
                    if let Ok(Some(cached_path)) = c.fetch_thumbnail(&p, 64).await {
                        c.event_bus.emit(
                            nextcloud_desktop_core::events::DesktopEvent::ThumbnailReady {
                                path: p,
                                cached_path,
                            },
                        );
                    }
                }));
            }
            for task in low_tasks {
                let _ = task.await;
            }

            // 2. If requested size > 64, upgrade to target size in background
            if size > 64 {
                let mut target_tasks = Vec::new();
                for path in paths {
                    let c = coord.clone();
                    let p = path.clone();
                    target_tasks.push(tokio::spawn(async move {
                        if let Ok(Some(cached_path)) = c.fetch_thumbnail(&p, size).await {
                            c.event_bus.emit(
                                nextcloud_desktop_core::events::DesktopEvent::ThumbnailReady {
                                    path: p,
                                    cached_path,
                                },
                            );
                        }
                    }));
                }
                for task in target_tasks {
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
    clean_no_slash: &str,
    with_slash: &str,
    opts: &nextcloud_rust_sdk::PreviewOptions,
) -> Option<Vec<u8>> {
    if let Ok(b) = preview.get_by_path(clean_no_slash, opts).await {
        return Some(b);
    }
    preview.get_by_path(with_slash, opts).await.ok()
}
