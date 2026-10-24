//! Remote Nextcloud WebDAV file operations.

use nextcloud_rust_sdk::{BasicAuth, DavClient, ReqwestTransport};
use std::fs;
use std::path::Path;

use crate::error::SyncError;
use crate::vfs::make_dav_path;
use nextcloud_desktop_core::events::{DesktopEvent, EventBus, TransferKind};

/// High-level file operation helpers wrapping WebDAV primitives with events and tracing.
pub struct FileOperations<'a> {
    client: &'a DavClient<'a, ReqwestTransport, BasicAuth>,
    user: &'a str,
    event_bus: EventBus,
}

impl<'a> FileOperations<'a> {
    /// Creates a new `FileOperations` bound to a `DavClient`, username, and `EventBus`.
    pub fn new(
        client: &'a DavClient<'a, ReqwestTransport, BasicAuth>,
        user: &'a str,
        event_bus: EventBus,
    ) -> Self {
        Self {
            client,
            user,
            event_bus,
        }
    }

    /// Uploads a local file from disk to remote WebDAV storage.
    pub async fn upload_file(
        &self,
        local_path: &Path,
        remote_path: &str,
        content_type: Option<&str>,
    ) -> Result<(), SyncError> {
        let bytes = fs::read(local_path).map_err(|err| SyncError::LocalIo {
            path: local_path.to_path_buf(),
            context: "reading local file for upload".to_string(),
            source: err,
        })?;

        let dav_path = make_dav_path(self.user, remote_path)?;

        tracing::info!(
            remote_path,
            bytes = bytes.len(),
            "Uploading file to Nextcloud"
        );
        self.client.put(&dav_path, bytes, content_type).await?;

        self.event_bus.emit(DesktopEvent::FileTransferCompleted {
            relative_path: remote_path.to_string(),
            kind: TransferKind::Upload,
        });

        Ok(())
    }

    /// Downloads a remote file directly to a designated local path on disk.
    pub async fn download_file(
        &self,
        remote_path: &str,
        destination_path: &Path,
    ) -> Result<u64, SyncError> {
        let dav_path = make_dav_path(self.user, remote_path)?;

        tracing::info!(remote_path, destination = %destination_path.display(), "Downloading file from Nextcloud");
        let bytes = self.client.get(&dav_path).await?;
        let len = bytes.len() as u64;

        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent).map_err(|err| SyncError::LocalIo {
                path: parent.to_path_buf(),
                context: "creating destination directory for download".to_string(),
                source: err,
            })?;
        }

        fs::write(destination_path, &bytes).map_err(|err| SyncError::LocalIo {
            path: destination_path.to_path_buf(),
            context: "writing downloaded content to disk".to_string(),
            source: err,
        })?;

        self.event_bus.emit(DesktopEvent::FileTransferCompleted {
            relative_path: remote_path.to_string(),
            kind: TransferKind::Download,
        });

        Ok(len)
    }

    /// Creates a new directory on the remote Nextcloud server via `MKCOL`.
    pub async fn create_directory(&self, remote_path: &str) -> Result<(), SyncError> {
        let dav_path = make_dav_path(self.user, remote_path)?;

        tracing::info!(remote_path, "Creating remote collection MKCOL");
        self.client.mkcol(&dav_path).await?;
        Ok(())
    }

    /// Deletes a file or directory collection on the remote Nextcloud server via `DELETE`.
    pub async fn delete_resource(&self, remote_path: &str) -> Result<(), SyncError> {
        let dav_path = make_dav_path(self.user, remote_path)?;

        tracing::info!(remote_path, "Deleting remote resource DELETE");
        self.client.delete(&dav_path).await?;
        Ok(())
    }

    /// Moves or renames a remote resource on the Nextcloud server via `MOVE`.
    pub async fn move_resource(
        &self,
        source_path: &str,
        destination_path: &str,
        overwrite: bool,
    ) -> Result<(), SyncError> {
        let src = make_dav_path(self.user, source_path)?;
        let dst = make_dav_path(self.user, destination_path)?;

        tracing::info!(
            source_path,
            destination_path,
            overwrite,
            "Moving remote resource MOVE"
        );
        self.client.move_(&src, &dst, overwrite).await?;
        Ok(())
    }

    /// Copies a remote resource on the Nextcloud server via `COPY`.
    pub async fn copy_resource(
        &self,
        source_path: &str,
        destination_path: &str,
        overwrite: bool,
    ) -> Result<(), SyncError> {
        let src = make_dav_path(self.user, source_path)?;
        let dst = make_dav_path(self.user, destination_path)?;

        tracing::info!(
            source_path,
            destination_path,
            overwrite,
            "Copying remote resource COPY"
        );
        self.client.copy(&src, &dst, overwrite).await?;
        Ok(())
    }
}
