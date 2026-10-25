//! Top-level navigation sidebar and preferences pages.

pub mod activity;
pub mod settings;
pub mod sidebar;
pub mod sync_folders;

pub use activity::ActivityPage;
pub use settings::SettingsPage;
pub use sidebar::NavigationSidebar;
pub use sync_folders::SyncFoldersPage;
