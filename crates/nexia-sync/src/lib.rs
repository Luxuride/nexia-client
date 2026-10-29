//! Thin desktop synchronization adapter for Nexia Client, delegating entirely to
//! `nextcloud-rust-sdk`.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub(crate) mod account;
pub mod coordinator;
pub mod engine;
pub mod error;
pub(crate) mod file_ops;
pub mod folder_sync;
pub mod freedesktop_thumbs;
pub mod fuse_mount;
pub(crate) mod mounts;
pub mod watcher;

pub use coordinator::{DesktopSyncCoordinator, PriorityPermit, ThumbnailQueueSnapshot};
pub use error::SyncError;
pub use file_ops::make_dav_path;
pub use folder_sync::FolderSyncEngine;
pub use fuse_mount::DesktopHydrator;
pub use watcher::FileSystemWatcher;

/// Program name written to the FreeDesktop PNG `Software` chunk of every
/// thumbnail this application installs.
///
/// The Thumbnail Managing Standard defines that chunk as the name of the
/// generating program, so it doubles as the marker identifying cache entries
/// this application owns. Scoped accounting and cleanup of the shared system
/// thumbnail cache match on it.
pub const THUMBNAIL_SOFTWARE: &str = "Nexia Client";

pub use nextcloud_rust_sdk::{
    Client, DiscoveredLocalFile, DiscoveredRemoteFile, FolderSyncRunner, JournalEntry,
    MountDescriptor, MountId, MountMode, MountState, MountStatus, MountsHandle,
    NextcloudOrchestrator, OrchestratorConfig, OrchestratorError, OrchestratorEvent,
    OrchestratorHandle, SyncAction, SyncFileStatus, SyncJournal, VfsHandle, VirtualEntrySummary,
    reconcile, scan_local_directory,
};
