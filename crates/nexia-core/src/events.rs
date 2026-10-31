//! Asynchronous event bus and notification model for desktop updates.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Severity of an application notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationLevel {
    /// Informational message.
    Info,
    /// Warning message regarding non-fatal issues.
    Warning,
    /// Error notification requiring user attention.
    Error,
}

/// Category of an active file transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    /// Upload from local storage to remote Nextcloud.
    Upload,
    /// Download from remote Nextcloud to local storage or VFS cache.
    Download,
    /// On-demand file hydration.
    Hydration,
}

/// High-level events emitted by the desktop engines.
#[derive(Clone, Debug)]
pub enum DesktopEvent {
    /// Synchronization cycle started.
    SyncStarted {
        /// Associated sync folder pairing ID.
        pairing_id: String,
    },
    /// File completed transfer.
    FileTransferCompleted {
        /// Remote path of the transferred file.
        relative_path: String,
        /// Kind of transfer.
        kind: TransferKind,
    },
    /// A virtual file node has started on-demand hydration and download.
    NodeHydrationStarted {
        /// Remote path of the node being hydrated.
        path: String,
    },
    /// A hydration attempt ended without content.
    ///
    /// Every path that announced [`DesktopEvent::NodeHydrationStarted`] ends with either this or
    /// [`DesktopEvent::NodeHydrated`]: the view keeps the path in its in-flight set until a
    /// terminal event arrives, so a failure without one leaves a spinner that never stops.
    NodeHydrationFailed {
        /// Remote path of the node whose hydration failed.
        path: String,
        /// Reason the content was not fetched, for the activity log.
        reason: String,
    },
    /// A virtual file node was hydrated with content.
    NodeHydrated {
        /// Remote path of the hydrated node.
        path: String,
        /// Number of bytes loaded into cache.
        size: u64,
    },
    /// A virtual file node was dehydrated back to placeholder status.
    NodeDehydrated {
        /// Remote path of the dehydrated node.
        path: String,
    },
    /// A thumbnail preview download started.
    ThumbnailDownloadStarted {
        /// Remote path of the file.
        path: String,
    },
    /// A thumbnail preview was downloaded and cached on disk.
    ThumbnailReady {
        /// Remote path of the file.
        path: String,
        /// Local cached thumbnail path.
        cached_path: PathBuf,
    },
    /// A thumbnail preview download failed or was unavailable.
    ThumbnailFailed {
        /// Remote path of the file.
        path: String,
    },
    /// Thumbnail queue progress update.
    ThumbnailProgress {
        /// Number of completed thumbnails.
        completed: usize,
        /// Total number of queued thumbnails in this session.
        total: usize,
        /// Current number of thumbnails in progress.
        in_progress: usize,
    },
    /// File explorer or display presentation settings were changed.
    DisplaySettingsChanged,
    /// Virtual file tree was refreshed from remote WebDAV.
    VfsTreeUpdated {
        /// Total number of nodes currently indexed in the tree.
        node_count: usize,
    },
    /// Synchronization cycle finished.
    SyncCompleted {
        /// Associated sync folder pairing ID.
        pairing_id: String,
        /// Number of files uploaded.
        uploaded: usize,
        /// Number of files downloaded.
        downloaded: usize,
        /// Number of conflicts encountered.
        conflicts: usize,
    },
    /// Live synchronization progress update for a mount point.
    MountProgress(nexia_nextcloud_sdk::MountProgressSnapshot),
    /// Active account was switched to another configured profile.
    AccountSwitched {
        /// Identifier of the newly activated account.
        account_id: String,
    },
    /// An account profile was signed out and removed.
    AccountSignedOut {
        /// Identifier of the signed out account.
        account_id: String,
    },
    /// A configured sync folder mount pairing was added or registered.
    SyncMountAdded {
        /// Identifier of the added sync mount.
        mount_id: String,
    },
    /// A configured sync folder mount pairing was updated.
    SyncMountUpdated {
        /// Identifier of the updated sync mount.
        mount_id: String,
    },
    /// A configured sync folder mount pairing was removed.
    SyncMountRemoved {
        /// Identifier of the removed sync mount.
        mount_id: String,
    },
    /// An application or sync error occurred.
    Notification {
        /// Message content.
        message: String,
        /// Severity level.
        level: NotificationLevel,
    },
}

/// Returns the next event from `rx`, skipping the gap a lagging subscriber left behind.
///
/// `Err(Lagged)` is not the end of the stream: the receiver stays usable and the events after
/// the gap still arrive. Treating it as the end is what silently stops a UI subscriber for the
/// rest of the session, so `None` here means exactly one thing — the bus is gone.
pub async fn next_event(
    rx: &mut tokio::sync::broadcast::Receiver<DesktopEvent>,
) -> Option<DesktopEvent> {
    loop {
        match rx.recv().await {
            Ok(event) => return Some(event),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(
                    skipped,
                    "Event subscriber lagged behind; resuming subscription"
                );
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
        }
    }
}

/// Broadcast event bus for distributing events across workers and UI.
#[derive(Clone, Debug)]
pub struct EventBus {
    sender: Arc<broadcast::Sender<DesktopEvent>>,
}

impl EventBus {
    /// Creates a new event bus with the given buffer capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender: Arc::new(sender),
        }
    }

    /// Emits an event to all active subscribers.
    pub fn emit(&self, event: DesktopEvent) {
        let _ = self.sender.send(event);
    }

    /// Subscribes a new receiver to the event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<DesktopEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn event_bus_broadcast() {
        let bus = EventBus::new(16);
        let mut sub1 = bus.subscribe();
        let mut sub2 = bus.subscribe();

        bus.emit(DesktopEvent::NodeHydrated {
            path: "/test.txt".to_string(),
            size: 1024,
        });

        let ev1 = sub1.recv().await.expect("received event on sub1");
        let ev2 = sub2.recv().await.expect("received event on sub2");

        match (ev1, ev2) {
            (
                DesktopEvent::NodeHydrated { path: p1, size: s1 },
                DesktopEvent::NodeHydrated { path: p2, size: s2 },
            ) => {
                assert_eq!(p1, "/test.txt");
                assert_eq!(s1, 1024);
                assert_eq!(p2, "/test.txt");
                assert_eq!(s2, 1024);
            }
            _ => panic!("unexpected event variant received"),
        }
    }
}
