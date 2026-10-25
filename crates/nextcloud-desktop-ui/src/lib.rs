//! GTK 4 desktop user interface views and controllers for Nextcloud Desktop.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod controller;
pub mod error;
pub mod model;
pub mod tray;
pub mod widget;

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

pub use controller::DesktopUiController;
pub use error::UiError;
#[cfg(feature = "gtk")]
pub use gtk_app::launch_gtk_application;
#[cfg(feature = "gtk")]
pub use gtk_app::run_headless;
pub use model::{
    FileSyncStatus, FileViewItem, FileViewMode, NavigationPage, PathSegment, TransferItem,
};
pub use tray::{TrayAction, spawn_app_bar_tray};
pub use widget::{FileExplorerBackend, ItemCollectionView, ViewComponent};
