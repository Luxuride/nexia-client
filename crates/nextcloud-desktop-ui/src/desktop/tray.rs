//! App bar and system tray StatusNotifierItem integration for background operation.

#[cfg(all(feature = "tray", target_os = "linux"))]
use ksni::menu::{MenuItem, StandardItem};
#[cfg(all(feature = "tray", target_os = "linux"))]
use ksni::{Category, Icon, Status, ToolTip, Tray, TrayMethods};
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
#[cfg(all(feature = "tray", target_os = "linux"))]
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

#[cfg(feature = "tray")]
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
            "emblem-synchronizing-symbolic".into()
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
                icon_name: "emblem-synchronizing-symbolic".into(),
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
#[cfg(all(feature = "tray", target_os = "linux"))]
pub fn spawn_app_bar_tray(sender: UnboundedSender<TrayAction>) {
    tokio::spawn(async move {
        let tray = DesktopTrayItem::new(sender);
        match tray.spawn().await {
            Ok(_handle) => {
                tracing::info!(
                    "Registered StatusNotifierItem in desktop app bar (eu.luxuride.nextcloud-desktop)"
                );
                std::future::pending::<()>().await;
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "StatusNotifierItem service could not register on D-Bus session bus"
                );
            }
        }
    });
}

/// Stub spawn function when tray feature or Linux is not enabled.
#[cfg(not(all(feature = "tray", target_os = "linux")))]
pub fn spawn_app_bar_tray(_sender: UnboundedSender<TrayAction>) {
    tracing::debug!("Tray feature not compiled; skipping app bar status notifier initialization");
}
