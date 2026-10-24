//! GTK 4 desktop user interface views and controllers for Nextcloud Desktop.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod controller;
pub mod error;
pub mod files;
pub mod gtk_app;
pub mod header;
pub mod model;
pub mod pages;
pub mod tray;
pub mod widget;

pub use controller::DesktopUiController;
pub use error::UiError;
#[cfg(feature = "gtk")]
pub use gtk_app::launch_gtk_application;
pub use gtk_app::run_headless;
pub use model::{
    FileSyncStatus, FileViewItem, FileViewMode, NavigationPage, PathSegment, TransferItem,
};
pub use tray::{TrayAction, spawn_app_bar_tray};
pub use widget::{FileExplorerBackend, ItemCollectionView, ViewComponent};
