//! Core traits for composable, generic GTK 4 / Libadwaita UI components and views.

use nextcloud_desktop_core::{Account, AccountId};
use std::future::Future;
use std::path::PathBuf;
#[cfg(feature = "gtk")]
use std::rc::Rc;

use crate::error::UiError;
use crate::model::{FileViewItem, FileViewMode, ZoomLevel};

/// Trait implemented by every self-contained UI component or page view.
pub trait ViewComponent {
    /// The underlying GTK/Libadwaita root widget type.
    type RootWidget;

    /// Returns a reference to the component's root widget.
    fn root_widget(&self) -> &Self::RootWidget;
}

/// Trait abstracting file explorer state and asynchronous actions for generic UI views.
pub trait FileExplorerBackend: Clone + 'static {
    /// Returns the current directory path in the virtual mount.
    fn current_path(&self) -> impl Future<Output = String>;

    /// Returns the current file layout mode (Grid or List).
    fn view_mode(&self) -> impl Future<Output = FileViewMode>;

    /// Updates the current file layout mode.
    fn set_view_mode(&self, mode: FileViewMode) -> impl Future<Output = ()>;

    /// Returns the current view zoom level.
    fn zoom_level(&self) -> impl Future<Output = ZoomLevel>;

    /// Updates the current view zoom level.
    fn set_zoom_level(&self, zoom: ZoomLevel) -> impl Future<Output = ()>;

    /// Returns the current file sorting criteria.
    fn sort_criteria(&self) -> impl Future<Output = nextcloud_rust_sdk::FileSortCriteria>;

    /// Updates the current file sorting criteria.
    fn set_sort_criteria(
        &self,
        criteria: nextcloud_rust_sdk::FileSortCriteria,
    ) -> impl Future<Output = ()>;

    /// Fetches server visual theming metadata.
    fn server_theming(&self) -> impl Future<Output = Option<nextcloud_rust_sdk::ThemeInfo>>;

    /// Fetches server theme logo or favicon image path if available.
    fn fetch_theme_logo(&self) -> impl Future<Output = Option<PathBuf>>;

    /// Searches files matching `query` across local VFS and server unified search.
    fn search_items(&self, query: &str)
    -> impl Future<Output = Result<Vec<FileViewItem>, UiError>>;

    /// Fast synchronous check returning a cached thumbnail path (preferred size or 64px placeholder) if already on disk.
    fn cached_thumbnail(&self, remote_path: &str, preferred_size: Option<u32>) -> Option<PathBuf>;

    /// Returns or fetches thumbnail path for a remote file at the given dimensions.
    fn get_thumbnail(&self, remote_path: &str, size: u32) -> impl Future<Output = Option<PathBuf>>;

    /// Batch prefetches thumbnails for multiple remote paths concurrently in background.
    fn prefetch_thumbnails(&self, paths: Vec<String>, size: u32);

    /// Returns `true` if backward history navigation is available.
    fn can_go_back(&self) -> impl Future<Output = bool>;

    /// Returns `true` if forward history navigation is available.
    fn can_go_forward(&self) -> impl Future<Output = bool>;

    /// Navigates backward in folder history.
    fn go_back(&self) -> impl Future<Output = bool>;

    /// Navigates forward in folder history.
    fn go_forward(&self) -> impl Future<Output = bool>;

    /// Navigates into a child directory by name.
    fn navigate_into(&self, folder_name: &str) -> impl Future<Output = ()>;

    /// Navigates directly to a target virtual path.
    fn set_current_path(&self, path: &str) -> impl Future<Output = ()>;

    /// Lists all items in the active directory.
    fn list_current_items(&self) -> impl Future<Output = Result<Vec<FileViewItem>, UiError>>;

    /// Creates a new folder in the active directory.
    fn create_folder(&self, folder_name: &str) -> impl Future<Output = Result<String, UiError>>;

    /// Creates a new file in the active directory with optional content bytes.
    fn create_file(
        &self,
        file_name: &str,
        content: &[u8],
    ) -> impl Future<Output = Result<String, UiError>>;

    /// Renames a file or folder at `remote_path` to `new_name`.
    fn rename_item(
        &self,
        remote_path: &str,
        new_name: &str,
    ) -> impl Future<Output = Result<String, UiError>>;

    /// Moves a file or folder at `source_path` into directory `destination_dir`.
    fn move_item(
        &self,
        source_path: &str,
        destination_dir: &str,
    ) -> impl Future<Output = Result<String, UiError>>;

    /// Deletes a file or folder at `remote_path`.
    fn delete_item(&self, remote_path: &str) -> impl Future<Output = Result<(), UiError>>;

    /// Opens a file, downloading content into local cache if not yet cached.
    fn open_file(&self, remote_path: &str) -> impl Future<Output = Result<PathBuf, UiError>>;

    /// Refreshes the virtual tree from the server or default hierarchy.
    fn refresh_virtual_tree(&self) -> impl Future<Output = Result<usize, UiError>>;

    /// Triggers two-way folder synchronization.
    fn trigger_sync(&self) -> impl Future<Output = Result<(), UiError>>;

    /// Returns the local hydration cache directory path string.
    fn cache_dir_display(&self) -> String;

    /// Returns all configured account profiles.
    fn accounts(&self) -> impl Future<Output = Vec<Account>>;

    /// Returns the active account ID.
    fn active_account_id(&self) -> impl Future<Output = Option<AccountId>>;

    /// Switches the active account profile to `account_id`.
    fn switch_account(&self, account_id: &AccountId) -> impl Future<Output = Result<(), UiError>>;

    /// Adds a new account profile to the application.
    fn add_account(&self, account: Account) -> impl Future<Output = Result<(), UiError>>;

    /// Starts a browser-based OAuth login flow for the given server URL.
    fn start_oauth_login(
        &self,
        server_url: &str,
    ) -> impl Future<Output = Result<(String, nextcloud_rust_sdk::Client), UiError>>;

    /// Polls a pending OAuth login flow.
    fn poll_oauth_login(
        &self,
        client: &nextcloud_rust_sdk::Client,
    ) -> impl Future<Output = Result<Option<Account>, UiError>>;

    /// Creates or retrieves a public share link for the given remote path.
    fn create_public_share(
        &self,
        remote_path: &str,
    ) -> impl Future<Output = Result<String, UiError>>;

    /// Globally pauses synchronization.
    fn pause_all_sync(&self) -> impl Future<Output = Result<(), UiError>> {
        async { Ok(()) }
    }

    /// Globally resumes synchronization.
    fn resume_all_sync(&self) -> impl Future<Output = Result<(), UiError>> {
        async { Ok(()) }
    }

    /// Returns `true` if synchronization is globally paused.
    fn is_sync_paused(&self) -> impl Future<Output = Result<bool, UiError>> {
        async { Ok(false) }
    }

    /// Pauses synchronization for a specific mount point.
    fn pause_mount(&self, _mount_id: &str) -> impl Future<Output = Result<(), UiError>> {
        async { Ok(()) }
    }

    /// Resumes synchronization for a specific mount point.
    fn resume_mount(&self, _mount_id: &str) -> impl Future<Output = Result<(), UiError>> {
        async { Ok(()) }
    }

    /// Returns `true` if a specific mount point is currently paused.
    fn is_mount_paused(&self, _mount_id: &str) -> impl Future<Output = Result<bool, UiError>> {
        async { Ok(false) }
    }
}

/// Trait implemented by file collection views (such as icon grid and columnar list views).
#[cfg(feature = "gtk")]
pub trait ItemCollectionView<B: FileExplorerBackend>: ViewComponent {
    /// Removes all rendered children from the collection view.
    fn clear(&self);

    /// Populates the collection view with the given directory items and active zoom level.
    fn populate(
        &self,
        items: &[FileViewItem],
        backend: &B,
        toast_overlay: &libadwaita::ToastOverlay,
        zoom: ZoomLevel,
        on_refresh: Rc<dyn Fn()>,
    );
}
