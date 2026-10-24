//! Core primitives, models, configuration, and event bus for Nextcloud Desktop.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod account;
pub mod config;
pub mod error;
pub mod events;

pub use account::{Account, AccountCredentials, AccountId, AccountStatus, ServerConfig};
pub use config::{AppConfig, SyncFolderConfig, SyncMode};
pub use error::CoreError;
pub use events::{DesktopEvent, EventBus, NotificationLevel, TransferKind, TransferProgress};
