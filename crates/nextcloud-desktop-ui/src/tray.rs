//! App bar and system tray StatusNotifierItem integration for background operation.

#[cfg(feature = "tray")]
use ksni::menu::{MenuItem, StandardItem};
#[cfg(feature = "tray")]
use ksni::{Category, Status, ToolTip, Tray, TrayMethods};
use tokio::sync::mpsc::UnboundedSender;

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

/// Freedesktop StatusNotifierItem representation for Nextcloud Desktop.
#[cfg(feature = "tray")]
pub struct DesktopTrayItem {
    sender: UnboundedSender<TrayAction>,
    syncing: bool,
    status_text: String,
}

#[cfg(feature = "tray")]
impl DesktopTrayItem {
    /// Creates a new tray item with the given action dispatch sender.
    pub fn new(sender: UnboundedSender<TrayAction>) -> Self {
        Self {
            sender,
            syncing: false,
            status_text: "In Sync".to_string(),
        }
    }

    /// Updates the sync state indicator.
    pub fn set_syncing(&mut self, syncing: bool, status_text: impl Into<String>) {
        self.syncing = syncing;
        self.status_text = status_text.into();
    }
}

#[cfg(feature = "tray")]
impl Tray for DesktopTrayItem {
    fn id(&self) -> String {
        "com.nextcloud.desktop.rs".into()
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
            "cloud-symbolic".into()
        }
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

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let tx_open = self.sender.clone();
        let tx_sync = self.sender.clone();
        let tx_vfs = self.sender.clone();
        let tx_quit = self.sender.clone();

        vec![
            StandardItem {
                label: "Open Nextcloud".into(),
                enabled: true,
                visible: true,
                activate: Box::new(move |_| {
                    let _ = tx_open.send(TrayAction::ShowWindow);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Sync All Folders Now".into(),
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
#[cfg(feature = "tray")]
pub fn spawn_app_bar_tray(sender: UnboundedSender<TrayAction>) {
    tokio::spawn(async move {
        let tray = DesktopTrayItem::new(sender);
        if let Err(err) = tray.spawn().await {
            tracing::warn!(error = %err, "StatusNotifierItem service could not register on D-Bus session bus");
        }
    });
}

/// Stub spawn function when tray feature is not enabled.
#[cfg(not(feature = "tray"))]
pub fn spawn_app_bar_tray(_sender: UnboundedSender<TrayAction>) {
    tracing::debug!("Tray feature not compiled; skipping app bar status notifier initialization");
}
