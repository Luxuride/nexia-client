//! Core primitives, models, configuration, and event bus for Nexia Client.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod account;
pub mod autostart;
pub mod config;
pub mod error;
pub mod events;
pub mod sandbox;
pub mod storage;

pub use account::{Account, AccountCredentials, AccountId, AccountStatus, ServerConfig};
pub use autostart::{is_autostart_enabled, register_autostart, unregister_autostart};
pub use config::{
    AccountPreferences, AdditionalItemsDisplayConfig, AppConfig, FolderItemsDisplay,
    GlobalPreferences, SyncFolderConfig, SyncMode,
};
pub use error::CoreError;
pub use events::{DesktopEvent, EventBus, NotificationLevel, TransferKind};
pub use sandbox::{flatpak_app_id, is_flatpak};
pub use storage::DesktopStorage;
