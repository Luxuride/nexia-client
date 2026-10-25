//! Thin Nextcloud desktop synchronization adapter delegating entirely to `nextcloud-rust-sdk`.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub(crate) mod account;
pub mod coordinator;
pub mod engine;
pub mod error;
pub(crate) mod file_ops;
pub(crate) mod mounts;

pub use coordinator::DesktopSyncCoordinator;
pub use error::SyncError;
pub use file_ops::make_dav_path;

pub use nextcloud_rust_sdk::{
    Client, DiscoveredLocalFile, DiscoveredRemoteFile, FolderSyncRunner, JournalEntry,
    MountDescriptor, MountId, MountMode, MountState, MountStatus, MountsHandle,
    NextcloudOrchestrator, OrchestratorConfig, OrchestratorError, OrchestratorEvent,
    OrchestratorHandle, SyncAction, SyncFileStatus, SyncJournal, VfsHandle, VirtualEntrySummary,
    reconcile, scan_local_directory,
};
