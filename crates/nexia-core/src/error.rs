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
    #[error("failed to parse configuration at {path}: {source}")]
    ConfigParse {
        /// Configuration file the data came from (or was destined for).
        path: PathBuf,
        /// Underlying JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// Missing or invalid configuration directory.
    #[error("could not determine user configuration directory")]
    MissingConfigDir,

    /// Missing or invalid user data directory.
    #[error("could not determine user data directory")]
    MissingDataDir,

    /// The storage key file exists but does not hold a usable key.
    #[error("unreadable storage key at {path}: {source}")]
    StorageKey {
        /// Path of the key file that could not be decoded.
        path: PathBuf,
        /// Underlying storage error.
        #[source]
        source: nexia_nextcloud_sdk::StorageError,
    },

    /// An existing encrypted database cannot be opened with any available key.
    ///
    /// Minting a replacement key would orphan every credential stored in the database, so
    /// the loss is reported instead.
    #[error("no available key opens the configuration database at {path}")]
    StorageKeyUnusable {
        /// Path of the database that could not be opened.
        path: PathBuf,
    },

    /// Account with the requested ID was not found.
    #[error("account '{0}' was not found")]
    AccountNotFound(String),

    /// An invalid URL was supplied.
    #[error("invalid server URL: {0}")]
    InvalidUrl(String),

    /// Encrypted database storage error.
    #[error("encrypted database storage error: {0}")]
    Storage(#[from] nexia_nextcloud_sdk::StorageError),

    /// Lock poisoned while accessing internal storage.
    #[error("internal storage lock was poisoned")]
    StorageLockPoisoned,
}
