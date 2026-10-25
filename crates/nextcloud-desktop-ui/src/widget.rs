//! Core traits for composable, generic GTK 4 / Libadwaita UI components and views.

use nextcloud_desktop_core::{Account, AccountId};
use std::future::Future;
use std::path::PathBuf;
#[cfg(feature = "gtk")]
use std::rc::Rc;

use crate::error::UiError;
use crate::model::{FileViewItem, FileViewMode};

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

    /// Creates or retrieves a public share link for the given remote path.
    fn create_public_share(
        &self,
        remote_path: &str,
    ) -> impl Future<Output = Result<String, UiError>>;
}

/// Trait implemented by file collection views (such as icon grid and columnar list views).
#[cfg(feature = "gtk")]
pub trait ItemCollectionView<B: FileExplorerBackend>: ViewComponent {
    /// Removes all rendered children from the collection view.
    fn clear(&self);

    /// Populates the collection view with the given directory items.
    fn populate(
        &self,
        items: &[FileViewItem],
        backend: &B,
        toast_overlay: &libadwaita::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    );
}
