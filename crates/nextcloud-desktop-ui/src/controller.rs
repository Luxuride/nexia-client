//! Controller managing application state and asynchronous backend operations.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::UiError;
use crate::model::{FileSyncStatus, FileViewItem, FileViewMode};
use crate::widget::FileExplorerBackend;
use nextcloud_desktop_sync::DesktopSyncCoordinator;
use nextcloud_rust_sdk::NavigationHistory;

/// UI Controller bridging user actions to backend sync and VFS operations.
#[derive(Clone)]
pub struct DesktopUiController {
    coordinator: DesktopSyncCoordinator,
    navigation: Arc<RwLock<NavigationHistory>>,
    view_mode: Arc<RwLock<FileViewMode>>,
}

impl DesktopUiController {
    /// Creates a new controller wrapping the given backend coordinator.
    pub fn new(coordinator: DesktopSyncCoordinator) -> Self {
        Self {
            coordinator,
            navigation: Arc::new(RwLock::new(NavigationHistory::new())),
            view_mode: Arc::new(RwLock::new(FileViewMode::Grid)),
        }
    }

    /// Access the underlying sync coordinator.
    pub fn coordinator(&self) -> &DesktopSyncCoordinator {
        &self.coordinator
    }

    /// Gets the current virtual directory path.
    pub async fn current_path(&self) -> String {
        self.navigation.read().await.current_path().to_string()
    }

    /// Gets the active Nautilus file view mode (Grid or List).
    pub async fn view_mode(&self) -> FileViewMode {
        *self.view_mode.read().await
    }

    /// Sets the active Nautilus file view mode (Grid or List).
    pub async fn set_view_mode(&self, mode: FileViewMode) {
        *self.view_mode.write().await = mode;
    }

    /// Returns `true` if there is a previous directory in the navigation history.
    pub async fn can_go_back(&self) -> bool {
        self.navigation.read().await.can_go_back()
    }

    /// Returns `true` if there is a forward directory in the navigation history.
    pub async fn can_go_forward(&self) -> bool {
        self.navigation.read().await.can_go_forward()
    }

    /// Navigates backward in the folder history stack.
    pub async fn go_back(&self) -> bool {
        self.navigation.write().await.go_back()
    }

    /// Navigates forward in the folder history stack.
    pub async fn go_forward(&self) -> bool {
        self.navigation.write().await.go_forward()
    }

    /// Navigates the file view into a child directory, recording history via `NavigationHistory`.
    pub async fn navigate_into(&self, folder_name: &str) {
        self.navigation.write().await.navigate_into(folder_name);
    }

    /// Navigates up to the parent directory, recording history via `NavigationHistory`.
    pub async fn navigate_up(&self) {
        self.navigation.write().await.navigate_up();
    }

    /// Sets the current virtual directory directly (e.g. from a breadcrumb click), recording history.
    pub async fn set_current_path(&self, path: &str) {
        self.navigation.write().await.navigate_to(path);
    }

    /// Lists items in the current virtual directory for display in the file view.
    pub async fn list_current_items(&self) -> Result<Vec<FileViewItem>, UiError> {
        let path = self.current_path().await;
        let entries = self
            .coordinator
            .vfs()
            .list_children(&path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))?;

        let mut items: Vec<FileViewItem> = entries
            .into_iter()
            .map(|e| {
                let status = if e.is_directory {
                    FileSyncStatus::CloudPlaceholder
                } else if e.is_hydrated {
                    FileSyncStatus::HydratedOffline
                } else {
                    FileSyncStatus::CloudPlaceholder
                };

                let size_formatted = if e.is_directory {
                    "Folder".to_string()
                } else {
                    FileViewItem::format_bytes(e.size)
                };

                FileViewItem {
                    path: e.path,
                    name: e.name,
                    is_directory: e.is_directory,
                    size_formatted,
                    size_bytes: e.size,
                    status,
                    mime_type: e.mime_type,
                    icon_category: e.icon_category,
                }
            })
            .collect();

        // Sort directories first, then alphabetically by name
        items.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(items)
    }

    /// Creates a new folder inside the current virtual directory.
    pub async fn create_folder(&self, folder_name: &str) -> Result<String, UiError> {
        let trimmed = folder_name.trim().trim_matches('/');
        if trimmed.is_empty() {
            return Err(UiError::Controller(
                "Folder name cannot be empty".to_string(),
            ));
        }

        let cur = self.current_path().await;
        let full_path = if cur == "/" {
            format!("/{trimmed}")
        } else {
            format!("{}/{trimmed}", cur.trim_end_matches('/'))
        };

        self.coordinator
            .create_directory(&full_path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))?;

        Ok(full_path)
    }

    /// Renames a file or folder in the virtual drive.
    pub async fn rename_item(&self, remote_path: &str, new_name: &str) -> Result<String, UiError> {
        self.coordinator
            .rename_resource(remote_path, new_name)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Deletes a file or folder from the virtual drive (and remote server if connected).
    pub async fn delete_item(&self, remote_path: &str) -> Result<(), UiError> {
        self.coordinator
            .delete_resource(remote_path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Opens a virtual file by hydrating it to local cache.
    pub async fn open_file(&self, remote_path: &str) -> Result<PathBuf, UiError> {
        self.coordinator
            .hydrate_file(remote_path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Hydrates a virtual file on-demand.
    pub async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, UiError> {
        self.open_file(remote_path).await
    }

    /// Dehydrates a file back to a placeholder, freeing disk space.
    pub async fn dehydrate_file(&self, remote_path: &str) -> Result<(), UiError> {
        self.coordinator
            .dehydrate_file(remote_path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Refreshes the virtual file tree from the remote Nextcloud server.
    pub async fn refresh_virtual_tree(&self) -> Result<usize, UiError> {
        self.coordinator
            .refresh_virtual_tree()
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Triggers a synchronization cycle across all configured folder pairings.
    pub async fn trigger_sync(&self) -> Result<(), UiError> {
        self.coordinator
            .sync_all_pairs()
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }
}

impl FileExplorerBackend for DesktopUiController {
    async fn current_path(&self) -> String {
        self.current_path().await
    }

    async fn view_mode(&self) -> FileViewMode {
        self.view_mode().await
    }

    async fn set_view_mode(&self, mode: FileViewMode) {
        self.set_view_mode(mode).await;
    }

    async fn can_go_back(&self) -> bool {
        self.can_go_back().await
    }

    async fn can_go_forward(&self) -> bool {
        self.can_go_forward().await
    }

    async fn go_back(&self) -> bool {
        self.go_back().await
    }

    async fn go_forward(&self) -> bool {
        self.go_forward().await
    }

    async fn navigate_into(&self, folder_name: &str) {
        self.navigate_into(folder_name).await;
    }

    async fn set_current_path(&self, path: &str) {
        self.set_current_path(path).await;
    }

    async fn list_current_items(&self) -> Result<Vec<FileViewItem>, UiError> {
        self.list_current_items().await
    }

    async fn create_folder(&self, folder_name: &str) -> Result<String, UiError> {
        self.create_folder(folder_name).await
    }

    async fn rename_item(&self, remote_path: &str, new_name: &str) -> Result<String, UiError> {
        self.rename_item(remote_path, new_name).await
    }

    async fn delete_item(&self, remote_path: &str) -> Result<(), UiError> {
        self.delete_item(remote_path).await
    }

    async fn open_file(&self, remote_path: &str) -> Result<PathBuf, UiError> {
        self.open_file(remote_path).await
    }

    async fn refresh_virtual_tree(&self) -> Result<usize, UiError> {
        self.refresh_virtual_tree().await
    }

    async fn trigger_sync(&self) -> Result<(), UiError> {
        self.trigger_sync().await
    }

    fn cache_dir_display(&self) -> String {
        self.coordinator.vfs().cache_dir().display().to_string()
    }
}
