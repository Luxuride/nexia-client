//! App bar and system tray StatusNotifierItem integration for background operation.

#[cfg(all(feature = "tray", target_os = "linux"))]
use ksni::menu::{MenuItem, StandardItem};
#[cfg(all(feature = "tray", target_os = "linux"))]
use ksni::{Category, Icon, OfflineReason, Status, ToolTip, Tray, TrayMethods};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;

/// Actions initiated by the user through the app bar tray menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Show and raise the main application window.
    ShowWindow,
    /// Hide the main window to background.
    HideWindow,
    /// Trigger an immediate folder synchronization cycle.
    TriggerSync,
    /// Refresh the virtual file system tree from Nextcloud.
    RefreshVfs,
    /// Terminate and quit the application completely.
    Quit,
}

/// Whether the desktop currently shows an app bar indicator for this process.
///
/// The window may only be hidden when an indicator is reachable, otherwise the
/// application would keep running with no way to bring it back. The state is
/// optimistic until the tray service settles - a spawned item never reports
/// itself, while a missing watcher drives the flag to `false` through
/// [`Tray::watcher_offline`]. Because that settlement happens on a background
/// task, callers that must decide *before* it lands wait for
/// [`TrayAvailability::wait_until_settled`] instead of reading a half-decided
/// flag.
#[derive(Clone, Debug)]
pub struct TrayAvailability {
    registered: Arc<AtomicBool>,
    changes_tx: Arc<watch::Sender<Option<bool>>>,
}

impl Default for TrayAvailability {
    fn default() -> Self {
        let (changes_tx, _) = watch::channel(None);
        Self {
            registered: Arc::new(AtomicBool::new(true)),
            changes_tx: Arc::new(changes_tx),
        }
    }
}

impl TrayAvailability {
    /// A state that never becomes available and needs no waiting, for builds
    /// and sessions that have no tray service at all.
    pub fn unavailable() -> Self {
        let availability = Self::default();
        availability.set(false);
        availability
    }

    /// Reports whether a StatusNotifierItem is currently registered and reachable.
    pub fn is_registered(&self) -> bool {
        self.registered.load(Ordering::Relaxed)
    }

    /// Resolves once the tray service has finished its first attempt, so a
    /// caller that needs a definitive answer does not race the spawn task.
    pub async fn wait_until_settled(&self) {
        let mut changes = self.changes_tx.subscribe();
        while changes.borrow().is_none() {
            if changes.changed().await.is_err() {
                return;
            }
        }
    }

    /// Streams every settled state, so a caller that already hid the window can
    /// restore it when the indicator disappears.
    pub fn subscribe(&self) -> watch::Receiver<Option<bool>> {
        self.changes_tx.subscribe()
    }

    fn set(&self, registered: bool) {
        self.registered.store(registered, Ordering::Relaxed);
        self.changes_tx.send_replace(Some(registered));
    }
}

/// Freedesktop StatusNotifierItem representation for Nextcloud Desktop.
#[cfg(all(feature = "tray", target_os = "linux"))]
pub struct DesktopTrayItem {
    sender: UnboundedSender<TrayAction>,
    syncing: bool,
    status_text: String,
    availability: TrayAvailability,
}

#[cfg(all(feature = "tray", target_os = "linux"))]
impl DesktopTrayItem {
    /// Creates a new tray item with the given action dispatch sender.
    pub fn new(sender: UnboundedSender<TrayAction>) -> Self {
        Self {
            sender,
            syncing: false,
            status_text: "In Sync".to_string(),
            availability: TrayAvailability::default(),
        }
    }

    fn settle(&self, registered: bool) {
        self.availability.set(registered);
    }

    /// Returns the shared indicator availability state.
    pub fn availability(&self) -> TrayAvailability {
        self.availability.clone()
    }

    /// Updates the sync state indicator.
    pub fn set_syncing(&mut self, syncing: bool, status_text: impl Into<String>) {
        self.syncing = syncing;
        self.status_text = status_text.into();
    }
}

#[cfg(all(feature = "tray", target_os = "linux"))]
fn build_cloud_pixmap() -> Vec<Icon> {
    let size = 22usize;
    let mut data = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - 11.0;
            let dy = y as f32 - 11.0;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = (y * size + x) * 4;
            if dist <= 9.5 {
                // Nextcloud blue (#0082C9) with white inner ring
                let inner_ring = (dist - 5.0).abs() <= 1.6;
                if inner_ring {
                    data[idx] = 255;
                    data[idx + 1] = 255;
                    data[idx + 2] = 255;
                    data[idx + 3] = 255;
                } else {
                    data[idx] = 255;
                    data[idx + 1] = 0x00;
                    data[idx + 2] = 0x82;
                    data[idx + 3] = 0xC9;
                }
            }
        }
    }
    vec![Icon {
        width: size as i32,
        height: size as i32,
        data,
    }]
}

#[cfg(all(feature = "tray", target_os = "linux"))]
impl Tray for DesktopTrayItem {
    fn id(&self) -> String {
        "eu.luxuride.nextcloud-desktop".into()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn title(&self) -> String {
        "Nextcloud Desktop".into()
    }

    fn status(&self) -> Status {
        Status::Active
    }

    fn icon_name(&self) -> String {
        if self.syncing {
            "view-refresh-symbolic".into()
        } else {
            "folder-remote-symbolic".into()
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        build_cloud_pixmap()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Nextcloud Desktop".into(),
            description: self.status_text.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.sender.send(TrayAction::ShowWindow);
    }

    fn watcher_online(&self) {
        self.settle(true);
        tracing::info!("StatusNotifierWatcher became available; app bar indicator is live");
    }

    fn watcher_offline(&self, reason: OfflineReason) -> bool {
        self.settle(false);
        tracing::warn!(
            reason = ?reason,
            "StatusNotifierWatcher is unavailable; no app bar indicator is shown"
        );
        true
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let tx_open = self.sender.clone();
        let tx_hide = self.sender.clone();
        let tx_sync = self.sender.clone();
        let tx_vfs = self.sender.clone();
        let tx_quit = self.sender.clone();

        vec![
            StandardItem {
                label: "Open Nextcloud".into(),
                icon_name: "window-new-symbolic".into(),
                enabled: true,
                visible: true,
                activate: Box::new(move |_| {
                    let _ = tx_open.send(TrayAction::ShowWindow);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Hide to Background".into(),
                icon_name: "window-minimize-symbolic".into(),
                enabled: true,
                visible: true,
                activate: Box::new(move |_| {
                    let _ = tx_hide.send(TrayAction::HideWindow);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Sync All Folders Now".into(),
                icon_name: "view-refresh-symbolic".into(),
                enabled: true,
                visible: true,
                activate: Box::new(move |_| {
                    let _ = tx_sync.send(TrayAction::TriggerSync);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Refresh Virtual Drive".into(),
                icon_name: "view-refresh-symbolic".into(),
                enabled: true,
                visible: true,
                activate: Box::new(move |_| {
                    let _ = tx_vfs.send(TrayAction::RefreshVfs);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit Nextcloud".into(),
                icon_name: "application-exit-symbolic".into(),
                enabled: true,
                visible: true,
                activate: Box::new(move |_| {
                    let _ = tx_quit.send(TrayAction::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Spawns the system tray item in the background using Tokio.
///
/// A Flatpak sandbox denies `RequestName` for the `org.kde.StatusNotifierItem-<pid>-<n>`
/// name ksni generates, so the item is registered under its connection's unique
/// name instead. Either way `org.kde.StatusNotifierWatcher` must be reachable on
/// the session bus.
#[cfg(all(feature = "tray", target_os = "linux"))]
pub fn spawn_app_bar_tray(sender: UnboundedSender<TrayAction>) -> TrayAvailability {
    let tray = DesktopTrayItem::new(sender);
    let availability = tray.availability();
    let availability_for_task = availability.clone();
    let sandboxed = nextcloud_desktop_core::is_flatpak();

    tokio::spawn(async move {
        let builder = tray.assume_sni_available(true).disable_dbus_name(sandboxed);
        match builder.spawn().await {
            Ok(_handle) => {
                let registered = availability_for_task.is_registered();
                availability_for_task.set(registered);
                if registered {
                    tracing::info!(
                        sandboxed,
                        "Registered StatusNotifierItem in desktop app bar (eu.luxuride.nextcloud-desktop)"
                    );
                } else {
                    tracing::warn!(
                        sandboxed,
                        "StatusNotifierItem is running but no app bar host accepted it"
                    );
                }
                std::future::pending::<()>().await;
            }
            Err(err) => {
                availability_for_task.set(false);
                tracing::warn!(
                    error = %err,
                    sandboxed,
                    "StatusNotifierItem service could not register on D-Bus session bus"
                );
            }
        }
    });

    availability
}

/// Stub spawn function when tray feature or Linux is not enabled.
#[cfg(not(all(feature = "tray", target_os = "linux")))]
pub fn spawn_app_bar_tray(_sender: UnboundedSender<TrayAction>) -> TrayAvailability {
    tracing::debug!("Tray feature not compiled; skipping app bar status notifier initialization");
    TrayAvailability::unavailable()
}
