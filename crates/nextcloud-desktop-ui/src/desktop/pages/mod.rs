//! Top-level navigation sidebar and preferences pages.

pub mod activity;
pub mod add_sync_dialog;
pub mod mount_activity_dialog;
pub mod remote_browser_dialog;
pub mod settings;
pub mod sidebar;
pub mod sync_folders;
pub mod sync_mount_settings_dialog;

pub use activity::ActivityPage;
pub use add_sync_dialog::AddSyncFolderDialog;
pub use mount_activity_dialog::MountActivityDialog;
pub use remote_browser_dialog::RemoteDirectoryBrowserDialog;
pub use settings::SettingsPage;
pub use sidebar::NavigationSidebar;
pub use sync_folders::SyncFoldersPage;
pub use sync_mount_settings_dialog::SyncMountSettingsDialog;
