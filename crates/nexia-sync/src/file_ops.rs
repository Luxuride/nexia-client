//! High-level virtual file mutations and two-way folder synchronization.

use nexia_nextcloud_sdk::{DavPath, DavProperty, Depth, FolderSyncRunner, MountId, PropfindBody};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::time::Instant;

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use nexia_core::config::SyncMode;
use nexia_core::events::DesktopEvent;

/// How long a directory listing must be quiet before its thumbnail pass starts.
///
/// A file manager lists a folder several times while opening it. Waiting out the burst means the
/// pass runs once, after the file manager has stopped asking, instead of competing with it.
const THUMBNAIL_PASS_DEBOUNCE: Duration = Duration::from_millis(700);

/// Upper bound on how long a listing may wait for its pass.
///
/// Scrolling through a large folder re-lists it continuously, which would otherwise defer the pass
/// for the whole session and leave the folder without thumbnails.
const THUMBNAIL_PASS_MAX_DELAY: Duration = Duration::from_secs(4);

/// A queued thumbnail pass: the listing's remote paths with any known file IDs, and the size.
type ThumbnailPassJob = (Vec<(String, Option<u64>)>, u32);

/// The newest thumbnail pass still waiting to run, plus the runner's bookkeeping.
#[derive(Default)]
struct ThumbnailPassQueue {
    /// The newest listing: later listings replace it, since the user has moved on.
    pending: Option<ThumbnailPassJob>,
    /// When the current burst of listings started, used for the cap.
    first_request: Option<Instant>,
    /// When the newest listing arrived, used for the debounce.
    last_request: Option<Instant>,
    /// Whether a runner task already exists; only one may drain the queue at a time.
    running: bool,
}

static THUMBNAIL_PASS_QUEUE: OnceLock<tokio::sync::Mutex<ThumbnailPassQueue>> = OnceLock::new();

fn thumbnail_pass_queue() -> &'static tokio::sync::Mutex<ThumbnailPassQueue> {
    THUMBNAIL_PASS_QUEUE.get_or_init(|| tokio::sync::Mutex::new(ThumbnailPassQueue::default()))
}

/// Records `job` as the pass to run, replacing any earlier one still waiting.
///
/// Returns whether the caller has to start the runner: exactly one caller is told to, and it then
/// stays responsible for the queue until the queue runs dry.
async fn enqueue_thumbnail_pass(
    queue: &tokio::sync::Mutex<ThumbnailPassQueue>,
    job: ThumbnailPassJob,
) -> bool {
    let mut q = queue.lock().await;
    let now = Instant::now();
    q.pending = Some(job);
    q.first_request.get_or_insert(now);
    q.last_request = Some(now);
    let start = !q.running;
    q.running = true;
    start
}

/// Waits for the listing burst to settle, then hands the newest listing to `run`, until the queue
/// runs dry.
///
/// Kept separate from the queue so the timing rules can be exercised without a coordinator: the
/// caller supplies what running a pass means. `run` is awaited, so a slow pass makes later listings
/// wait in the queue rather than pile up beside it.
async fn drain_thumbnail_passes<F, Fut>(queue: &tokio::sync::Mutex<ThumbnailPassQueue>, run: F)
where
    F: Fn(ThumbnailPassJob) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    loop {
        let (has_work, sleep_for) = {
            let q = queue.lock().await;
            let last = q.last_request.unwrap_or_else(Instant::now);
            let first = q.first_request.unwrap_or_else(Instant::now);
            let until_cap = THUMBNAIL_PASS_MAX_DELAY.saturating_sub(first.elapsed());
            (
                q.pending.is_some(),
                THUMBNAIL_PASS_DEBOUNCE
                    .saturating_sub(last.elapsed())
                    .min(until_cap),
            )
        };

        if !has_work {
            let mut q = queue.lock().await;
            // Re-checked under the lock: a listing that arrived while the pass was running must not
            // be stranded by this runner standing down.
            if q.pending.is_none() {
                q.running = false;
                q.first_request = None;
                q.last_request = None;
                return;
            }
            continue;
        }

        if !sleep_for.is_zero() {
            tokio::time::sleep(sleep_for).await;
            continue;
        }

        let job = {
            let mut q = queue.lock().await;
            let job = q.pending.take();
            q.first_request = None;
            q.last_request = None;
            job
        };
        let Some(job) = job else {
            continue;
        };
        run(job).await;
    }
}

/// Constructs a normalized [`DavPath`] from user name and relative path.
pub fn make_dav_path(user: &str, relative: &str) -> Result<DavPath, SyncError> {
    nexia_nextcloud_sdk::make_dav_path(user, relative).map_err(SyncError::Orchestrator)
}

/// What a thumbnail install pass did, so its cost can be observed instead of guessed.
///
/// A folder that has already been thumbnailed should report `installed == 0`: every file is
/// skipped without decoding the source image. A non-zero `installed` on a folder that has been
/// opened before is the signal that something is invalidating the cache entries.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PreviewInstallSummary {
    /// Files that had at least one mount target rewritten.
    pub installed: usize,
    /// Files left untouched because every mount target was already current.
    pub skipped: usize,
}

/// Resolves `remote_path` relative to a mount's remote root, on path-segment boundaries.
///
/// A plain prefix test would let a mount rooted at `/photos` claim `/photos2/x`, writing a file
/// that belongs to a sibling folder into the wrong local tree. The result is empty for the mount
/// root itself and `None` when the path is outside the mount.
fn mount_relative_path(remote_path: &str, mount_remote: &str) -> Option<String> {
    let mount_remote = mount_remote.trim_matches('/');
    let remote_path = remote_path.trim_matches('/');
    if mount_remote.is_empty() {
        return Some(remote_path.to_string());
    }
    let rest = remote_path.strip_prefix(mount_remote)?;
    if rest.is_empty() {
        return Some(String::new());
    }
    rest.strip_prefix('/').map(|rest| rest.to_string())
}

/// Converts a PROPFIND result into the summary shape the SDK virtual tree consumes.
///
/// Every discovery path (full refresh, single directory refresh, per-account refresh) feeds the
/// same tree, so the field mapping lives here once instead of being repeated at each call site.
fn dav_summaries(
    resources: &[nexia_nextcloud_sdk::DavResource],
) -> Vec<nexia_nextcloud_sdk::DavResourceSummary> {
    resources
        .iter()
        .map(|res| {
            let mut sum =
                nexia_nextcloud_sdk::DavResourceSummary::new(&res.href, res.is_collection);
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
        .collect()
}

impl DesktopSyncCoordinator {
    /// Refreshes the virtual file tree for the active configured account, or resets to local virtual hierarchy.
    pub async fn refresh_virtual_tree(&self) -> Result<usize, SyncError> {
        let _priority = self.priority_permit();
        let vfs = self.active_vfs().await?;

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
        let summaries = dav_summaries(&resources);

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

        self.sync_fuse_tree_from_snapshot(
            vfs.snapshot().await.ok(),
            summaries.as_slice(),
            &dav_prefix,
        )
        .await;

        self.event_bus
            .emit(nexia_core::events::DesktopEvent::VfsTreeUpdated {
                node_count: inserted,
            });

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
        let vfs = self.active_vfs().await?;

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
        let summaries = dav_summaries(&resources);

        let inserted = vfs
            .populate_from_resources(&dav_prefix, summaries.clone())
            .await
            .unwrap_or(0);

        self.sync_fuse_tree_from_snapshot(
            vfs.snapshot().await.ok(),
            summaries.as_slice(),
            &dav_prefix,
        )
        .await;

        self.event_bus
            .emit(nexia_core::events::DesktopEvent::VfsTreeUpdated {
                node_count: inserted,
            });

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

    /// Merges the actor's VFS snapshot into the shared in-memory tree a live FUSE mount reads
    /// from, then persists it as `tree.json`.
    ///
    /// Assigning the snapshot wholesale (`*guard = snapshot`) used to drop every file the FUSE
    /// driver had hydrated since the snapshot was taken, so images that had just been downloaded
    /// reverted to placeholders and the next read failed. This now adopts the live hydration state
    /// for unchanged files and only merges the new resources in.
    async fn sync_fuse_tree_from_snapshot(
        &self,
        snapshot: Option<nexia_nextcloud_sdk::VirtualTree>,
        summaries: &[nexia_nextcloud_sdk::DavResourceSummary],
        dav_prefix: &str,
    ) {
        let Some(mut snapshot) = snapshot else {
            return;
        };

        if let Some(ref ft_lock) = *self.fuse_tree.read().await
            && let Ok(mut guard) = ft_lock.write()
        {
            // Keep hydrated payloads that only the running mount knows about.
            snapshot.adopt_hydration_from(&guard);
            guard.merge_from_resources(dav_prefix, summaries);
        }

        if let Some(acc) = self.active_account().await {
            // Persist the live tree, not the snapshot: the mount may have hydrated files while the
            // refresh was running, and those payloads must be spilled into the account cache.
            if let Some(ref ft_lock) = *self.fuse_tree.read().await
                && let Ok(mut guard) = ft_lock.write()
            {
                let _ = self.persist_account_tree(&acc.id.0, &mut guard);
                return;
            }
            let _ = self.persist_account_tree(&acc.id.0, &mut snapshot);
        }
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
            let decoded = crate::folder_sync::decode_percent_str(relative);
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

    /// Starts a thumbnail fetch without waiting for it.
    ///
    /// `fetch_thumbnail` waits for every priority permit to drain before it downloads, and the
    /// caller here still holds its own permit: awaiting it inline parks the task forever, leaking
    /// that permit and with it the gate for the whole session.
    fn spawn_thumbnail_fetch(&self, remote_path: String) {
        let this = self.clone();
        tokio::spawn(async move {
            let _ = this.fetch_thumbnail(&remote_path, 256).await;
        });
    }

    /// Reports a hydration that ended without content, so the view clears its in-flight mark.
    fn emit_hydration_failed(&self, path: &str, reason: &str) {
        self.event_bus.emit(DesktopEvent::NodeHydrationFailed {
            path: path.to_string(),
            reason: reason.to_string(),
        });
    }

    /// Performs on-demand hydration for a virtual file, downloading via SDK or using local cache.
    pub async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, SyncError> {
        let _priority = self.priority_permit();
        let _download_permit = self.download_semaphore.acquire().await.ok();
        let clean = format!("/{}", remote_path.trim_matches('/'));
        let vfs = self.active_vfs().await?;

        let Some((user, client, auth)) = self.active_dav_context().await else {
            return Err(SyncError::NotFound("No active account session".to_string()));
        };

        let dav_client = client.dav(&auth);
        let clean_sub = remote_path.trim_matches('/');
        let dav_path = make_dav_path(&user, clean_sub)?;

        // Announced after the checks: a request that fails here leaves no transfer behind, and
        // the view keeps the path in its in-flight set until a matching completion arrives.
        self.event_bus.emit(DesktopEvent::NodeHydrationStarted {
            path: clean.clone(),
        });
        let bytes = match dav_client.get(&dav_path).await {
            Ok(bytes) => bytes,
            Err(err) => {
                self.emit_hydration_failed(&clean, &err.to_string());
                return Err(SyncError::WebDav(err));
            }
        };
        let byte_len = bytes.len() as u64;
        let local_path = match vfs.hydrate_local(remote_path, bytes.clone()).await {
            Ok(path) => path,
            Err(err) => {
                self.emit_hydration_failed(&clean, &err.to_string());
                return Err(SyncError::Orchestrator(err));
            }
        };

        // If this file matches a local folder sync mount, hydrate the local file directly
        for mount in self.sync_mounts().await {
            if Self::is_internal_vfs(&mount.id) || !mount.enabled {
                continue;
            }
            if let Some(rel_path) = mount_relative_path(clean_sub, &mount.remote_path) {
                let rel_clean = rel_path.trim_start_matches('/');
                let target_file = PathBuf::from(&mount.local_target).join(rel_clean);
                if let Some(parent) = target_file.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if mount.mode != "Fuse" && std::fs::write(&target_file, &bytes).is_ok() {
                    let journal_path = self.mount_journal_path(&mount.account_id, &mount.id);
                    let mut journal =
                        nexia_nextcloud_sdk::SyncJournal::load_from_file(&journal_path)
                            .unwrap_or_else(|_| nexia_nextcloud_sdk::SyncJournal::new());
                    let mtime = std::fs::metadata(&target_file)
                        .and_then(|m| m.modified())
                        .unwrap_or_else(|_| std::time::SystemTime::now());
                    let canonical_rel = format!("/{}", rel_clean.trim_matches('/'));
                    journal.record(nexia_nextcloud_sdk::JournalEntry::new(
                        &canonical_rel,
                        byte_len,
                        mtime,
                        false,
                    ));
                    let _ = journal.save_to_file(&journal_path);
                    nexia_nextcloud_sdk::remove_freedesktop_fail_marker(&target_file);
                    self.spawn_thumbnail_fetch(clean.clone());
                    self.event_bus.emit(DesktopEvent::FileTransferCompleted {
                        relative_path: canonical_rel,
                        kind: nexia_core::events::TransferKind::Hydration,
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
                        nexia_nextcloud_sdk::SyncJournal::load_from_file(&journal_path)
                            .unwrap_or_else(|_| nexia_nextcloud_sdk::SyncJournal::new());
                    let mtime = std::fs::metadata(&canonical)
                        .and_then(|m| m.modified())
                        .unwrap_or_else(|_| std::time::SystemTime::now());
                    let canonical_rel = format!("/{}", rel_str.trim_matches('/'));
                    journal.record(nexia_nextcloud_sdk::JournalEntry::new(
                        &canonical_rel,
                        byte_len,
                        mtime,
                        false,
                    ));
                    let _ = journal.save_to_file(&journal_path);
                    nexia_nextcloud_sdk::remove_freedesktop_fail_marker(&canonical);
                    self.spawn_thumbnail_fetch(full_remote.clone());

                    self.event_bus.emit(DesktopEvent::FileTransferCompleted {
                        relative_path: canonical_rel,
                        kind: nexia_core::events::TransferKind::Hydration,
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
        let vfs = self.active_vfs().await?;
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
        let vfs = self.active_vfs().await?;
        vfs.dehydrate_local(remote_path)
            .await
            .map_err(SyncError::Orchestrator)?;
        // Emitted only after the transition happened: announcing a still-hydrated file would make
        // the file manager show a placeholder that still occupies its full payload.
        self.event_bus
            .emit(DesktopEvent::NodeDehydrated { path: clean });
        Ok(())
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

        let vfs = self.active_vfs().await?;
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

        let vfs = self.active_vfs().await?;
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
        let parent =
            nexia_nextcloud_sdk::parent_path(&clean_old).unwrap_or_else(|| "/".to_string());
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

        let vfs = self.active_vfs().await?;
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

        let vfs = self.active_vfs().await?;
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

        let vfs = self.active_vfs().await?;
        vfs.remove(&clean_path)
            .await
            .map_err(SyncError::Orchestrator)
    }

    /// Creates a public share link for a virtual file or directory via OCS share API.
    pub async fn create_public_share(&self, remote_path: &str) -> Result<String, SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        if let Some((_, client, auth)) = self.active_dav_context().await {
            let share_client = client.shares(&auth);
            let share_creation = nexia_nextcloud_sdk::CreateShare::public_link(&clean_path);

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
                "No active account configured".to_string(),
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
    ///
    /// The payloads live where the SDK spills them, under the per-account `vfs` and `content`
    /// directories; the tree file itself is metadata and is counted with them.
    pub async fn virtual_sync_downloaded_size(&self) -> u64 {
        let Some(account) = self.active_account().await else {
            return 0;
        };
        let account_id = account.id.0.clone();
        crate::cache_usage::scan_dirs_usage([
            &self.account_vfs_dir(&account_id),
            &self.account_content_dir(&account_id),
        ])
        .bytes
    }

    /// Releases the downloaded payloads of the active account's mounts, returning bytes freed.
    ///
    /// Every mount is cleared through [`Self::clear_mount_cache`] instead of deleting files here:
    /// the hydration states live in the SDK's tree and mount handles, so removing payloads behind
    /// its back would leave nodes claiming local content that no longer exists.
    pub async fn clear_virtual_sync_cache(&self) -> Result<u64, SyncError> {
        let before = self.virtual_sync_downloaded_size().await;
        let Some(account) = self.active_account().await else {
            return Ok(0);
        };
        let account_id = account.id.0.clone();

        for mount in self
            .sync_mounts()
            .await
            .into_iter()
            .filter(|mount| mount.account_id == account_id)
        {
            self.clear_mount_cache(MountId::from_string(mount.id), true)
                .await?;
        }

        Ok(before)
    }

    /// Cleans and deletes cached thumbnails across the app and FreeDesktop system cache, returning bytes freed.
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

        // Also clean our FreeDesktop thumbnails in ~/.cache/thumbnails/{normal, large, x-large, xx-large}.
        // Fail markers are not cleaned here: the SDK writes them without a `Software` chunk
        // (`install_freedesktop_fail_marker` passes `None`), so this application has no way to tell
        // its own markers from another thumbnailer's in the shared `fail` directory, and deleting
        // all of them would break other applications' caching.
        freed += crate::freedesktop_thumbs::remove_app_thumbnails();

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
    ) -> Result<Vec<nexia_nextcloud_sdk::VirtualEntrySummary>, SyncError> {
        let trimmed = term.trim().to_string();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let this = self.clone();
        tokio::task::spawn(async move {
            let vfs = this.active_vfs().await?;
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
                            let summary = nexia_nextcloud_sdk::VirtualEntrySummary {
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
                                    nexia_nextcloud_sdk::ResourceIconCategory::Folder
                                } else {
                                    nexia_nextcloud_sdk::ResourceIconCategory::Document
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
                // Only the path identifies a node: a remote hit named like some other local
                // file is a different file and dropping it hides real search results.
                if !matches.iter().any(|m| m.path == remote_entry.path) {
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
        let hash = nexia_nextcloud_sdk::md5_hex(clean_path.as_bytes());

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
        account: &nexia_core::account::Account,
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
        let summaries = dav_summaries(&resources);

        // 1. Update the account's persistent VFS tree on disk
        let vfs_dir = self.account_vfs_dir(&account.id.0);
        let tree_file = vfs_dir.join("tree.json");
        let mut tree = if tree_file.exists() {
            std::fs::read_to_string(&tree_file)
                .ok()
                .and_then(|data| serde_json::from_str(&data).ok())
                .unwrap_or_else(|| nexia_nextcloud_sdk::VirtualTree::new("Nextcloud"))
        } else {
            nexia_nextcloud_sdk::VirtualTree::new("Nextcloud")
        };

        let count = tree.populate_from_resources(&dav_prefix, &summaries);
        let _ = self.persist_account_tree(&account.id.0, &mut tree);

        // 2. If this is the active account, also sync to orchestrator and FUSE in-memory tree:
        let is_active = self
            .active_account()
            .await
            .map(|a| a.id.0 == account.id.0)
            .unwrap_or(false);
        if is_active {
            let vfs = self.active_vfs().await?;
            let _ = vfs
                .populate_from_resources(&dav_prefix, summaries.clone())
                .await;

            // Merge into the live tree instead of assigning the on-disk copy: a running FUSE mount
            // keeps its hydration state in that tree, and replacing it wholesale turns the files it
            // just downloaded back into placeholders. This also persists the merged tree.
            self.sync_fuse_tree_from_snapshot(Some(tree), &summaries, &dav_prefix)
                .await;

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
        let _ = self
            .install_freedesktop_previews_batch(&[(remote_path.to_string(), png_bytes.to_vec())])
            .await;
    }

    /// Installs FreeDesktop system thumbnails for a whole directory's worth of remote files,
    /// resolving the mount roots once instead of once per file.
    ///
    /// A directory listing hands over several hundred paths at a time. Resolving the FUSE
    /// mountpoint, the account VFS directory, the sync mounts and the remote mtime per file turns
    /// that into thousands of awaits, and every thumbnail is decoded and re-encoded from the source
    /// image; together that is seconds of work competing with the file manager populating the same
    /// folder, which is felt as stutter while it opens.
    ///
    /// Two things keep it cheap: the roots are looked up once per batch, and a target whose
    /// thumbnails are already present is skipped before any decoding happens.
    pub async fn install_freedesktop_previews_batch(
        &self,
        items: &[(String, Vec<u8>)],
    ) -> PreviewInstallSummary {
        let mut summary = PreviewInstallSummary::default();
        if items.is_empty() {
            return summary;
        }

        let fuse_mp = self.fuse_mountpoint().await;
        let account_vfs_dir = match self.active_account().await {
            Some(account) => Some(self.account_vfs_dir(&account.id.0)),
            None => None,
        };
        let folder_mounts: Vec<(String, PathBuf)> = self
            .sync_mounts()
            .await
            .into_iter()
            .filter(|mount| mount.enabled && !Self::is_internal_vfs(&mount.id))
            .map(|mount| {
                (
                    mount
                        .remote_path
                        .trim_end_matches('/')
                        .trim_start_matches('/')
                        .to_string(),
                    PathBuf::from(&mount.local_target),
                )
            })
            .collect();

        let now_sec = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        for (remote_path, png_bytes) in items {
            let clean_path = remote_path.trim_matches('/');
            let mut targets: Vec<PathBuf> = Vec::new();
            if let Some(ref fuse_mp) = fuse_mp {
                targets.push(fuse_mp.join(clean_path));
            }
            if let Some(ref dir) = account_vfs_dir {
                targets.push(dir.join(clean_path));
            }
            for (mount_remote, mount_local) in &folder_mounts {
                if let Some(rel) = mount_relative_path(clean_path, mount_remote) {
                    targets.push(mount_local.join(rel.trim_start_matches('/')));
                }
            }

            // Decoded lazily: reinstalling thumbnails that are already current is the common case
            // for a folder that has been opened before, and decoding every source image to throw
            // the result away is what made reopening such a folder expensive.
            let mut png: Option<Vec<u8>> = None;
            let mut touched = false;
            let mut examined = 0usize;
            for target in targets {
                if !target.exists() {
                    continue;
                }
                examined += 1;
                if crate::freedesktop_thumbs::freedesktop_thumbnail_is_current(&target) {
                    continue;
                }
                let png = png
                    .get_or_insert_with(|| crate::freedesktop_thumbs::ensure_png_bytes(png_bytes));
                // The standard keys a thumbnail on the file's own modification time, so the value
                // that makes it valid is the one on disk rather than the remote date.
                let target_mtime = target
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(now_sec);
                crate::freedesktop_thumbs::install_freedesktop_thumbnail_with_mtime(
                    &target,
                    png,
                    target_mtime,
                    crate::THUMBNAIL_SOFTWARE,
                );
                touched = true;
            }

            if touched {
                summary.installed += 1;
            } else if examined > 0 {
                summary.skipped += 1;
            }
        }

        summary
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
        let hash = nexia_nextcloud_sdk::md5_hex(clean_path.as_bytes());
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
            self.event_bus
                .emit(nexia_core::events::DesktopEvent::ThumbnailProgress {
                    completed: stats.completed,
                    total: stats.total,
                    in_progress: stats.in_progress,
                });
        }

        self.event_bus
            .emit(nexia_core::events::DesktopEvent::ThumbnailDownloadStarted {
                path: remote_path.to_string(),
            });

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
            self.event_bus
                .emit(nexia_core::events::DesktopEvent::ThumbnailReady {
                    path: remote_path.to_string(),
                    cached_path: thumb_path.clone(),
                });
            return Ok(Some(thumb_path));
        }

        // Look up file_id: prefer known_file_id from remote discovery, fallback to active VFS
        let file_id = if known_file_id.is_some() {
            known_file_id
        } else {
            let vfs = self.active_vfs().await?;
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

            let opts = nexia_nextcloud_sdk::PreviewOptions::new()
                .dimensions(effective_size, effective_size)
                .mode(nexia_nextcloud_sdk::PreviewMode::Cover);

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
                    self.event_bus
                        .emit(nexia_core::events::DesktopEvent::ThumbnailProgress {
                            completed: stats.completed,
                            total: stats.total,
                            in_progress: stats.in_progress,
                        });
                }
                self.event_bus
                    .emit(nexia_core::events::DesktopEvent::ThumbnailReady {
                        path: remote_path.to_string(),
                        cached_path: thumb_path.clone(),
                    });
                return Ok(Some(thumb_path));
            }
        }

        // Last resort: the server exposes no preview and the file carries no embedded EXIF
        // thumbnail. Without one here the file manager reads the whole original to make its own,
        // because the FreeDesktop standard has no "do not read this file" answer for a missing
        // thumbnail. Reading the original once, bounded by size, keeps that cost out of the folder
        // view and the result is cached for every later listing.
        if let Some(png_bytes) = self
            .thumbnail_from_original(remote_path, effective_size)
            .await
            && tokio::fs::write(&thumb_path, &png_bytes).await.is_ok()
        {
            self.install_freedesktop_previews_for_remote_path(remote_path, &png_bytes)
                .await;
            if let Ok(mut stats) = self.thumbnail_stats.write() {
                stats.in_progress = stats.in_progress.saturating_sub(1);
                stats.completed += 1;
                stats.active_items.retain(|p| p != remote_path);
                self.event_bus
                    .emit(nexia_core::events::DesktopEvent::ThumbnailProgress {
                        completed: stats.completed,
                        total: stats.total,
                        in_progress: stats.in_progress,
                    });
            }
            self.event_bus
                .emit(nexia_core::events::DesktopEvent::ThumbnailReady {
                    path: remote_path.to_string(),
                    cached_path: thumb_path.clone(),
                });
            return Ok(Some(thumb_path));
        }

        if let Ok(mut stats) = self.thumbnail_stats.write() {
            stats.in_progress = stats.in_progress.saturating_sub(1);
            stats.failed += 1;
            stats.active_items.retain(|p| p != remote_path);
            self.event_bus
                .emit(nexia_core::events::DesktopEvent::ThumbnailProgress {
                    completed: stats.completed,
                    total: stats.total,
                    in_progress: stats.in_progress,
                });
        }

        self.event_bus
            .emit(nexia_core::events::DesktopEvent::ThumbnailFailed {
                path: remote_path.to_string(),
            });

        Ok(None)
    }

    /// Largest original the client downloads purely to build a thumbnail.
    const LOCAL_THUMBNAIL_MAX_BYTES: u64 = 12 * 1024 * 1024;

    /// Builds a thumbnail from the original when every server-side route has failed.
    ///
    /// This hydrates the file, which the orchestrator persists, so the download is paid once and
    /// later listings install the cached result instead of repeating it.
    async fn thumbnail_from_original(&self, remote_path: &str, size: u32) -> Option<Vec<u8>> {
        let clean = remote_path.trim_matches('/');
        let vfs = self.active_vfs().await.ok()?;
        let entry = vfs.entry(clean).await.ok().flatten()?;
        if entry.is_directory || entry.size > Self::LOCAL_THUMBNAIL_MAX_BYTES {
            return None;
        }
        if !entry
            .mime_type
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        {
            return None;
        }

        tracing::debug!(
            path = clean,
            bytes = entry.size,
            "server preview unavailable, building a thumbnail from the original"
        );
        // Boxed because `hydrate_file` fetches a thumbnail for the files it writes, which comes
        // back here when the server has no preview: the indirection keeps the future finite.
        let local_path = Box::pin(self.hydrate_file(clean)).await.ok()?;
        let bytes = tokio::fs::read(&local_path).await.ok()?;
        downscale_image_to_png(&bytes, size)
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
        self.queue_thumbnail_pass(items, size);
    }

    /// Queues a deferred thumbnail pass for a directory listing.
    ///
    /// Listings are coalesced rather than served one by one: the newest listing replaces any
    /// earlier one still waiting (the user has moved on), a pass starts only after the folder has
    /// been quiet for [`THUMBNAIL_PASS_DEBOUNCE`], and a folder that keeps being listed is still
    /// served once [`THUMBNAIL_PASS_MAX_DELAY`] has elapsed. Only one runner drains the queue at a
    /// time, so the decoding work cannot stack up behind a scrolling user.
    fn queue_thumbnail_pass(&self, items: Vec<(String, Option<u64>)>, size: u32) {
        let coord = self.clone();
        tokio::spawn(async move {
            let queue = thumbnail_pass_queue();
            if !enqueue_thumbnail_pass(queue, (items, size)).await {
                return;
            }
            drain_thumbnail_passes(queue, |(items, size)| {
                let coord = coord.clone();
                async move { coord.run_thumbnail_pass(items, size).await }
            })
            .await;
        });
    }

    /// Installs already-cached thumbnails for `items` in one pass, then fetches the missing ones.
    async fn run_thumbnail_pass(&self, items: Vec<(String, Option<u64>)>, size: u32) {
        let max_size = self.max_thumbnail_size().await;
        // The maximum comes from a persisted setting, so it may sit below the 64px floor that
        // `clamp` requires; taking the floor as the lower bound of the range keeps this total.
        let effective_size = size.clamp(64, max_size.max(64));

        let mut missing_items: Vec<(String, Option<u64>)> = Vec::new();
        let mut cached_installs: Vec<(String, Vec<u8>)> = Vec::new();
        for (path, fid) in items {
            if let Some(cached_file) = self.cached_thumbnail_path(&path, Some(effective_size))
                && let Ok(bytes) = tokio::fs::read(&cached_file).await
            {
                cached_installs.push((path, bytes));
                continue;
            }
            missing_items.push((path, fid));
        }

        // Install the already-cached thumbnails in one pass: the mount roots are resolved once and
        // up-to-date entries are skipped before any decoding, instead of paying both costs per file
        // while the file manager is still populating the folder.
        let install_started = Instant::now();
        let summary = self
            .install_freedesktop_previews_batch(&cached_installs)
            .await;
        if !cached_installs.is_empty() {
            tracing::debug!(
                cached = cached_installs.len(),
                installed = summary.installed,
                skipped = summary.skipped,
                missing = missing_items.len(),
                elapsed_ms = install_started.elapsed().as_millis() as u64,
                "Thumbnail pass installed the cached previews"
            );
        }

        if missing_items.is_empty() {
            return;
        }

        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(4));
        let mut tasks = Vec::with_capacity(missing_items.len());
        for (path, fid) in missing_items {
            let c = self.clone();
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
    }

    /// Pre-populates FreeDesktop system thumbnails for all files in the virtual tree that already have local cached previews.
    pub async fn preload_cached_freedesktop_thumbnails(&self) {
        let tree = self.get_or_create_fuse_tree().await;
        let entries: Vec<String> = {
            let t = match tree.read() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            t.paths()
                .filter(|p| {
                    t.get(p)
                        .map(|n| {
                            !n.is_directory()
                                && (n.metadata.has_preview
                                    || crate::folder_sync::is_thumbnail_candidate(n.name()))
                        })
                        .unwrap_or(false)
                })
                .map(|p| p.to_string())
                .collect()
        };
        if !entries.is_empty() {
            self.batch_prefetch_thumbnails(entries, 256);
        }
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
        let Ok(vfs) = self.active_vfs().await else {
            return;
        };
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
    T: nexia_nextcloud_sdk::Transport,
    A: nexia_nextcloud_sdk::AuthProvider,
>(
    preview: &nexia_nextcloud_sdk::PreviewClient<'_, T, A>,
    dav: &nexia_nextcloud_sdk::DavClient<'_, T, A>,
    user: &str,
    file_id: Option<u64>,
    clean_no_slash: &str,
    opts: &nexia_nextcloud_sdk::PreviewOptions,
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
    let simple_opts = nexia_nextcloud_sdk::PreviewOptions::new().dimensions(
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
        && let Some(thumb) = nexia_nextcloud_sdk::extract_embedded_jpeg_thumbnail(&header)
    {
        return Some(thumb.to_vec());
    }

    None
}

/// Decodes an image and re-encodes it as a PNG no larger than `size` on either side.
///
/// Returns `None` for anything the enabled decoders cannot read, so an undecodable original is
/// handled exactly like an absent one.
fn downscale_image_to_png(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let side = size.max(64);
    let thumbnail = decoded.thumbnail(side, side);
    let mut png = Vec::new();
    thumbnail
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(crate::freedesktop_thumbs::ensure_png_bytes(&png))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_relative_path_respects_segment_boundaries() {
        assert_eq!(
            mount_relative_path("photos/2024/a.jpg", "/photos").as_deref(),
            Some("2024/a.jpg")
        );
        assert_eq!(mount_relative_path("photos", "photos").as_deref(), Some(""));
        assert_eq!(
            mount_relative_path("/anything", "/").as_deref(),
            Some("anything")
        );
        // A sibling folder sharing the mount name as a prefix is not inside the mount.
        assert_eq!(mount_relative_path("photos2/a.jpg", "/photos"), None);
        assert_eq!(mount_relative_path("other/a.jpg", "/photos"), None);
    }

    /// Builds a one-file listing, which is all the scheduler looks at.
    fn listing(name: &str) -> ThumbnailPassJob {
        (vec![(name.to_string(), None)], 256)
    }

    /// A queue of its own per test: the production queue is a process-wide static.
    fn fresh_queue() -> &'static tokio::sync::Mutex<ThumbnailPassQueue> {
        Box::leak(Box::new(tokio::sync::Mutex::new(
            ThumbnailPassQueue::default(),
        )))
    }

    /// Mirrors `queue_thumbnail_pass`: whoever is told to start the runner does so, and reports each
    /// listing the scheduler actually runs.
    fn spawn_runner(
        queue: &'static tokio::sync::Mutex<ThumbnailPassQueue>,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        tokio::spawn(drain_thumbnail_passes(queue, move |(items, _size)| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(items[0].0.clone());
            }
        }));
    }

    /// A file manager lists a folder several times while opening it. Running one pass per listing
    /// put hundreds of thumbnail decodes and cache writes on the machine while that same file
    /// manager was still resolving the folder, and the newest listing is the only one the user is
    /// still looking at.
    #[tokio::test(start_paused = true)]
    async fn a_burst_of_listings_runs_only_the_newest_pass() {
        let queue = fresh_queue();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        assert!(enqueue_thumbnail_pass(queue, listing("first")).await);
        spawn_runner(queue, tx.clone());
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            !enqueue_thumbnail_pass(queue, listing("second")).await,
            "the running scheduler already owns the queue"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!enqueue_thumbnail_pass(queue, listing("third")).await);

        assert_eq!(
            rx.recv().await.as_deref(),
            Some("third"),
            "only the newest listing of the burst is worth running"
        );
        assert!(
            rx.try_recv().is_err(),
            "the replaced listings must not run as well"
        );
    }

    /// A folder that keeps being listed — the user scrolling — must not defer its pass forever.
    #[tokio::test(start_paused = true)]
    async fn a_folder_listed_continuously_still_gets_a_pass() {
        let queue = fresh_queue();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        for round in 0..40 {
            let start = enqueue_thumbnail_pass(queue, listing(&format!("listing-{round}"))).await;
            if start {
                spawn_runner(queue, tx.clone());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        let first = rx
            .recv()
            .await
            .expect("the delay cap must admit a pass instead of deferring it indefinitely");
        assert!(first.starts_with("listing-"));
    }

    /// Once the queue is empty the scheduler stands down, so the next listing starts a fresh one
    /// with its own delay window rather than inheriting a stale deadline.
    #[tokio::test(start_paused = true)]
    async fn the_scheduler_stands_down_and_a_later_listing_gets_its_own_pass() {
        let queue = fresh_queue();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        assert!(enqueue_thumbnail_pass(queue, listing("early")).await);
        spawn_runner(queue, tx.clone());
        assert_eq!(rx.recv().await.as_deref(), Some("early"));

        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(!queue.lock().await.running, "the scheduler must stand down");

        assert!(enqueue_thumbnail_pass(queue, listing("late")).await);
        spawn_runner(queue, tx.clone());
        assert_eq!(rx.recv().await.as_deref(), Some("late"));
    }

    /// The last-resort thumbnail has to fit the size the caller asked for, otherwise the file
    /// manager scales it down again every time the folder is opened.
    #[test]
    fn a_decoded_original_is_shrunk_to_the_requested_size() {
        let source = image::RgbImage::new(200, 120);
        let mut encoded = Vec::new();
        image::DynamicImage::ImageRgb8(source)
            .write_to(
                &mut std::io::Cursor::new(&mut encoded),
                image::ImageFormat::Png,
            )
            .expect("the fixture must encode as PNG");

        let thumbnail = downscale_image_to_png(&encoded, 64).expect("a PNG must decode");

        assert_eq!(&thumbnail[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = image::load_from_memory(&thumbnail).expect("the thumbnail must decode");
        assert!(decoded.width() <= 64 && decoded.height() <= 64);
    }

    /// An original the enabled decoders cannot read must be reported as absent rather than
    /// installed as a corrupt thumbnail, which the file manager would cache and never retry.
    #[test]
    fn an_undecodable_original_yields_no_thumbnail() {
        assert!(downscale_image_to_png(b"not an image", 64).is_none());
    }

    /// GIF and WebP are the formats a server most often declines to preview, so a decoder has to be
    /// compiled in here too; without one the fallback silently fails and the file manager renders
    /// the whole original instead.
    #[test]
    fn a_gif_original_is_also_thumbnailed() {
        // The smallest valid GIF: 1x1 pixel, single frame.
        const GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

        let thumbnail = downscale_image_to_png(GIF, 64).expect("a GIF must decode");

        assert_eq!(&thumbnail[..8], b"\x89PNG\r\n\x1a\n");
    }
}
