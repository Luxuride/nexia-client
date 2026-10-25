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
}
