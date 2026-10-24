//! Typed errors for synchronization and WebDAV operations.

use std::path::PathBuf;
use thiserror::Error;

/// Root error type for sync and VFS operations.
#[derive(Debug, Error)]
pub enum SyncError {
    /// Error originating from WebDAV transport or SDK core.
    #[error("WebDAV operation error: {0}")]
    WebDav(#[from] nextcloud_sdk_core::Error),

    /// Error within the virtual file system tree or hydration tracking.
    #[error("VFS error: {0}")]
    Vfs(#[from] nextcloud_sdk_vfs::VfsError),

    /// Local filesystem I/O failure.
    #[error("local I/O failure at {path} while {context}: {source}")]
    LocalIo {
        /// Target path where I/O failed.
        path: PathBuf,
        /// Contextual description of the failed operation.
        context: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to read or parse synchronization journal.
    #[error("journal error: {0}")]
    Journal(String),

    /// Error encountered during synchronization reconciliation.
    #[error("reconciliation error: {0}")]
    Reconciliation(String),

    /// Requested resource was not found.
    #[error("resource not found: {0}")]
    NotFound(String),

    /// An invalid WebDAV path was provided.
    #[error("invalid DAV path: {0}")]
    InvalidPath(String),
}
