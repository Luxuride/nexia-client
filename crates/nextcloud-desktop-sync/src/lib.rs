//! Virtual synchronization engine, two-way folder synchronizer, and WebDAV file operations.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod engine;
pub mod error;
pub mod full_sync;
pub mod journal;
pub mod reconciler;
pub mod scanner;
pub mod vfs;

pub use engine::DesktopSyncCoordinator;
pub use error::SyncError;
pub use full_sync::FullSyncEngine;
pub use journal::{JournalEntry, SyncJournal};
pub use reconciler::{SyncAction, reconcile};
pub use scanner::{
    DiscoveredLocalFile, DiscoveredRemoteFile, scan_local_directory, scan_remote_hierarchy,
};
pub use vfs::{VirtualEntrySummary, VirtualSyncEngine, make_dav_path};

pub use nextcloud_rust_sdk::{
    FolderSyncRunner, MountDescriptor, MountId, MountMode, MountState, MountStatus, MountsHandle,
    NextcloudOrchestrator, OrchestratorConfig, OrchestratorError, OrchestratorEvent,
    OrchestratorHandle, SyncFileStatus, VfsHandle,
};
