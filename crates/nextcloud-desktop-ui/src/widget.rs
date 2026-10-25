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

    /// Returns fast in-memory cached visual theming metadata if available.
    fn cached_theming(&self) -> Option<nextcloud_rust_sdk::ThemeInfo>;

    /// Fast synchronous check returning a cached server logo path if already on disk.
    fn cached_theme_logo(&self) -> Option<PathBuf>;

    /// Fetches server theme logo or favicon image path if available.
    fn fetch_theme_logo(&self) -> impl Future<Output = Option<PathBuf>>;

    /// Returns the cache database size in bytes.
    fn cache_database_size(&self) -> impl Future<Output = u64>;

    /// Returns the total cached files size in bytes.
    fn cached_files_size(&self) -> impl Future<Output = u64>;

    /// Cleans thumbnail cache, returning the number of bytes freed.
    fn clean_thumbnails(&self) -> impl Future<Output = Result<u64, UiError>>;

    /// Cleans all caches, returning the number of bytes freed.
    fn clean_everything(&self) -> impl Future<Output = Result<u64, UiError>>;

    /// Cleans any cached thumbnails that exceed the user's max thumbnail size requirement, returning bytes freed.
    fn clean_oversized_thumbnails(&self) -> impl Future<Output = Result<u64, UiError>> {
        async { Ok(0) }
    }

    /// Returns the maximum allowed thumbnail dimension configured by the user.
    fn max_thumbnail_size(&self) -> impl Future<Output = u32> {
        async { 256 }
    }

    /// Sets the maximum allowed thumbnail dimension configured by the user.
    fn set_max_thumbnail_size(&self, _size: u32) -> impl Future<Output = Result<(), UiError>> {
        async { Ok(()) }
    }

    /// Returns the size in bytes of downloaded and hydrated virtual sync files.
    fn virtual_sync_downloaded_size(&self) -> impl Future<Output = u64> {
        async { 0 }
    }

    /// Clears downloaded virtual files for the virtual sync mount, returning bytes freed.
    fn clear_virtual_sync_cache(&self) -> impl Future<Output = Result<u64, UiError>> {
        async { Ok(0) }
    }

    /// Checks if auto-download of smallest thumbnails is enabled.
    fn is_autodownload_thumbnails_enabled(&self) -> impl Future<Output = bool>;

    /// Sets whether auto-download of smallest thumbnails is enabled.
    fn set_autodownload_thumbnails_enabled(
        &self,
        enabled: bool,
    ) -> impl Future<Output = Result<(), UiError>>;

    /// Searches files matching `query` across local VFS and server unified search.
    fn search_items(&self, query: &str)
    -> impl Future<Output = Result<Vec<FileViewItem>, UiError>>;

    /// Fast synchronous check returning a cached thumbnail path (preferred size or 64px placeholder) if already on disk.
    fn cached_thumbnail(&self, remote_path: &str, preferred_size: Option<u32>) -> Option<PathBuf>;

    /// Returns or fetches thumbnail path for a remote file at the given dimensions.
    fn get_thumbnail(&self, remote_path: &str, size: u32) -> impl Future<Output = Option<PathBuf>>;

    /// Batch prefetches thumbnails for multiple remote paths concurrently in background.
    fn prefetch_thumbnails(&self, paths: Vec<String>, size: u32);

    /// Returns the live snapshot of background thumbnail downloading queue and progress.
    fn thumbnail_queue_snapshot(&self) -> nextcloud_desktop_sync::ThumbnailQueueSnapshot;

    /// Subscribes to broadcast desktop engine events.
    fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<nextcloud_desktop_core::events::DesktopEvent>;

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

    /// Hydrates a virtual file on-demand, downloading content to local cache.
    fn hydrate_file(&self, remote_path: &str) -> impl Future<Output = Result<PathBuf, UiError>>;

    /// Dehydrates a file back to a cloud-only placeholder, freeing disk space.
    fn dehydrate_file(&self, remote_path: &str) -> impl Future<Output = Result<(), UiError>>;

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

    /// Retrieves an existing public share link for the given remote path if one exists.
    fn get_public_share(
        &self,
        _remote_path: &str,
    ) -> impl Future<Output = Result<Option<String>, UiError>> {
        async { Ok(None) }
    }

    /// Deletes any public share link for the given remote path.
    fn delete_public_share(
        &self,
        _remote_path: &str,
    ) -> impl Future<Output = Result<bool, UiError>> {
        async { Ok(false) }
    }

    /// Persists directory sorting preferences in the SDK.
    fn store_directory_sort(
        &self,
        _dir_path: &str,
        _criteria: nextcloud_rust_sdk::FileSortCriteria,
    ) -> impl Future<Output = ()> {
        async {}
    }

    /// Loads persisted directory sorting preferences from the SDK.
    fn load_directory_sort(
        &self,
        _dir_path: &str,
    ) -> impl Future<Output = Option<nextcloud_rust_sdk::FileSortCriteria>> {
        async { None }
    }

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

    /// Lists all configured sync mounts stored in the SDK database.
    fn sync_mounts(&self) -> impl Future<Output = Vec<nextcloud_rust_sdk::StoredSyncMount>> {
        async { Vec::new() }
    }

    /// Adds a new filesystem sync pairing in the SDK and orchestrator.
    fn add_sync_mount(
        &self,
        _remote_path: String,
        _local_target: PathBuf,
        _mode: String,
        _display_name: Option<String>,
    ) -> impl Future<Output = Result<String, UiError>> {
        async { Ok(String::new()) }
    }

    /// Removes a sync mount pairing by ID from both the SDK and orchestrator.
    fn remove_sync_mount(&self, _mount_id: &str) -> impl Future<Output = Result<(), UiError>> {
        async { Ok(()) }
    }

    /// Queries the disk storage usage report for a mount point from the SDK.
    fn mount_storage_usage(
        &self,
        _mount_id: &str,
    ) -> impl Future<Output = Result<nextcloud_rust_sdk::MountStorageUsage, UiError>> {
        async {
            Ok(nextcloud_rust_sdk::MountStorageUsage::new(
                nextcloud_rust_sdk::MountId::new(),
                0,
                0,
                0,
            ))
        }
    }

    /// Clears cached files for a mount point via the SDK, returning freed bytes.
    fn clear_mount_cache(
        &self,
        _mount_id: &str,
        _clear_hydrated: bool,
    ) -> impl Future<Output = Result<u64, UiError>> {
        async { Ok(0) }
    }

    /// Clears shared thumbnail previews across all mounts, returning freed bytes.
    fn clear_shared_thumbnails(&self) -> impl Future<Output = Result<u64, UiError>> {
        async { Ok(0) }
    }

    /// Queries real-time progress metrics for a mount point from the SDK.
    fn mount_progress(
        &self,
        _mount_id: &str,
    ) -> impl Future<Output = Result<nextcloud_rust_sdk::MountProgressSnapshot, UiError>> {
        async {
            Ok(nextcloud_rust_sdk::MountProgressSnapshot::new(
                nextcloud_rust_sdk::MountId::new(),
                nextcloud_rust_sdk::MountStatus::Idle,
            ))
        }
    }

    /// Returns the effective additional display config for file views.
    fn effective_display_config(
        &self,
    ) -> impl Future<Output = nextcloud_desktop_core::AdditionalItemsDisplayConfig> {
        async { nextcloud_desktop_core::AdditionalItemsDisplayConfig::default() }
    }

    /// Sets additional display configuration for a specific account.
    fn set_account_display_config(
        &self,
        _account_id: &AccountId,
        _display: nextcloud_desktop_core::AdditionalItemsDisplayConfig,
    ) -> impl Future<Output = Result<(), UiError>> {
        async { Ok(()) }
    }

    /// Sets global fallback display configuration.
    fn set_global_display_config(
        &self,
        _display: nextcloud_desktop_core::AdditionalItemsDisplayConfig,
    ) -> impl Future<Output = Result<(), UiError>> {
        async { Ok(()) }
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
