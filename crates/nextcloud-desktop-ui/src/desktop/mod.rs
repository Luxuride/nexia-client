//! GTK 4 and Libadwaita desktop user interface views and application components.
//!
//! The desktop application serves as the **source of truth** for all Nextcloud UI
//! functionality, including file browsing, synchronization management, multi-account
//! switching, branding theming, and system tray integration.

#[cfg(feature = "gtk")]
pub mod account;
#[cfg(feature = "gtk")]
pub mod files;
#[cfg(feature = "gtk")]
pub mod gtk_app;
#[cfg(feature = "gtk")]
pub mod header;
#[cfg(feature = "gtk")]
pub mod pages;
#[cfg(feature = "tray")]
pub mod tray;

#[cfg(feature = "gtk")]
pub use gtk_app::{launch_gtk_application, run_headless, suppress_gtk_logs};
#[cfg(feature = "tray")]
pub use tray::{TrayAction, spawn_app_bar_tray};
