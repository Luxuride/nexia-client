//! GTK 4 desktop user interface views and controllers for Nextcloud Desktop.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod controller;
pub mod error;
pub mod gtk_app;
pub mod model;
pub mod tray;
pub mod views;

pub use controller::DesktopUiController;
pub use error::UiError;
#[cfg(feature = "gtk")]
pub use gtk_app::launch_gtk_application;
pub use gtk_app::run_headless;
pub use model::{FileSyncStatus, FileViewItem, NavigationPage, TransferItem};
pub use tray::{TrayAction, spawn_app_bar_tray};
