//! Top-level navigation sidebar and preferences pages.

pub mod activity;
pub mod settings;
pub mod sidebar;
pub mod sync_folders;

#[cfg(feature = "gtk")]
pub use activity::ActivityPage;
#[cfg(feature = "gtk")]
pub use settings::SettingsPage;
#[cfg(feature = "gtk")]
pub use sidebar::NavigationSidebar;
#[cfg(feature = "gtk")]
pub use sync_folders::SyncFoldersPage;
