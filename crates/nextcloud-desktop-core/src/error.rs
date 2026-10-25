//! Error types for the core desktop application components.

use std::path::PathBuf;
use thiserror::Error;

/// Root error enum for the core library.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Failed to load application configuration file from disk.
    #[error("failed to load configuration from {path}: {source}")]
    ConfigLoad {
        /// Configuration file path that could not be read.
        path: PathBuf,
        /// Underlying I/O or decode error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to save application configuration file to disk.
    #[error("failed to save configuration to {path}: {source}")]
    ConfigSave {
        /// Configuration file path that could not be written.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Failed to serialize or deserialize configuration data.
    #[error("configuration parse error: {0}")]
    ConfigParse(#[from] serde_json::Error),

    /// Missing or invalid configuration directory.
    #[error("could not determine user configuration directory")]
    MissingConfigDir,

    /// Account with the requested ID was not found.
    #[error("account '{0}' was not found")]
    AccountNotFound(String),

    /// An invalid URL was supplied.
    #[error("invalid server URL: {0}")]
    InvalidUrl(String),

    /// Encrypted database storage error.
    #[error("encrypted database storage error: {0}")]
    Storage(#[from] nextcloud_rust_sdk::StorageError),

    /// Lock poisoned while accessing internal storage.
    #[error("internal storage lock was poisoned")]
    StorageLockPoisoned,
}
