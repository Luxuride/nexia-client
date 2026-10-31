//! Typed errors for synchronization and WebDAV operations.

use std::path::PathBuf;
use thiserror::Error;

/// Root error type for sync and VFS operations.
#[derive(Debug, Error)]
pub enum SyncError {
    /// Error originating from WebDAV transport or SDK core.
    #[error("WebDAV operation error: {0}")]
    WebDav(#[from] nexia_nextcloud_sdk::Error),

    /// Error originating from high-level client operations.
    #[error("SDK client error: {0}")]
    Client(#[from] nexia_nextcloud_sdk::ClientError),

    /// Error building HTTP transport.
    #[error("transport build error: {0}")]
    TransportBuild(#[from] nexia_nextcloud_sdk::TransportBuildError),

    /// Error within the virtual file system tree or hydration tracking.
    #[error("VFS error: {0}")]
    Vfs(#[from] nexia_nextcloud_sdk::VfsError),

    /// Error originating from nexia-nextcloud-sdk sync engine.
    #[error("SDK sync error: {0}")]
    SdkSync(#[from] nexia_nextcloud_sdk::SyncError),

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

    /// The per-account store key could not be derived.
    #[error("deriving the store key for account {account}: {source}")]
    StoreKey {
        /// Account whose key was being derived.
        account: String,
        /// Underlying storage error.
        #[source]
        source: nexia_nextcloud_sdk::StorageError,
    },

    /// An isolated SDK client could not be built for an account.
    #[error("building the client for account {account}: {source}")]
    ClientBuild {
        /// Account the client was being built for.
        account: String,
        /// Underlying client error.
        #[source]
        source: nexia_nextcloud_sdk::ClientError,
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

    /// Error originating from background orchestrator actor.
    #[error("orchestrator error: {0}")]
    Orchestrator(#[from] nexia_nextcloud_sdk::OrchestratorError),

    /// Synchronization is paused or operation is disabled.
    #[error("operation disabled: {0}")]
    OperationDisabled(String),

    /// Background asynchronous task join failure.
    #[error("background task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}
