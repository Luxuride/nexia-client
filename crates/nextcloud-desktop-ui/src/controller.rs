//! Controller managing application state and asynchronous backend operations.

use nextcloud_desktop_core::{Account, AccountId};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::UiError;
use crate::model::{FileSyncStatus, FileViewItem, FileViewMode, ZoomLevel};
use crate::widget::FileExplorerBackend;
use nextcloud_desktop_sync::{
    DesktopSyncCoordinator, MountDescriptor, MountId, MountState, SyncFileStatus,
};
use nextcloud_rust_sdk::NavigationHistory;

/// UI Controller bridging user actions to backend sync and VFS operations.
#[derive(Clone)]
pub struct DesktopUiController {
    coordinator: DesktopSyncCoordinator,
    navigation: Arc<RwLock<NavigationHistory>>,
    view_mode: Arc<RwLock<FileViewMode>>,
    zoom_level: Arc<RwLock<ZoomLevel>>,
}

impl DesktopUiController {
    /// Creates a new controller wrapping the given backend coordinator.
    pub fn new(coordinator: DesktopSyncCoordinator) -> Self {
        Self {
            coordinator,
            navigation: Arc::new(RwLock::new(NavigationHistory::new())),
            view_mode: Arc::new(RwLock::new(FileViewMode::Grid)),
            zoom_level: Arc::new(RwLock::new(ZoomLevel::Medium)),
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

    /// Returns the current view zoom level.
    pub async fn zoom_level(&self) -> ZoomLevel {
        *self.zoom_level.read().await
    }

    /// Sets the active view zoom level.
    pub async fn set_zoom_level(&self, zoom: ZoomLevel) {
        *self.zoom_level.write().await = zoom;
    }

    /// Searches files matching `query` across local VFS and server unified search.
    pub async fn search_items(&self, query: &str) -> Result<Vec<FileViewItem>, UiError> {
        let entries = self
            .coordinator
            .search(query)
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

                let size_formatted = e.display_size();

                FileViewItem {
                    path: e.path,
                    name: e.name,
                    is_directory: e.is_directory,
                    size_formatted,
                    size_bytes: e.size,
                    status,
                    mime_type: e.mime_type,
                    icon_category: e.icon_category,
                    modified_at: e.modified_at,
                    permissions: e.permissions,
                }
            })
            .collect();

        items.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(items)
    }

    /// Fetches or retrieves a cached thumbnail for `remote_path`.
    pub async fn get_thumbnail(&self, remote_path: &str, size: u32) -> Option<PathBuf> {
        self.coordinator
            .fetch_thumbnail(remote_path, size)
            .await
            .ok()
            .flatten()
    }

    /// Batch prefetches thumbnails in the background.
    pub fn prefetch_thumbnails(&self, paths: Vec<String>, size: u32) {
        self.coordinator.batch_prefetch_thumbnails(paths, size);
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
            .active_vfs()
            .await
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

                let size_formatted = e.display_size();

                FileViewItem {
                    path: e.path,
                    name: e.name,
                    is_directory: e.is_directory,
                    size_formatted,
                    size_bytes: e.size,
                    status,
                    mime_type: e.mime_type,
                    icon_category: e.icon_category,
                    modified_at: e.modified_at,
                    permissions: e.permissions,
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

    /// Creates a new virtual file in the current directory with content.
    pub async fn create_file(&self, file_name: &str, content: &[u8]) -> Result<String, UiError> {
        let trimmed = file_name.trim();
        if trimmed.is_empty() {
            return Err(UiError::Controller("File name cannot be empty".to_string()));
        }

        let cur = self.current_path().await;
        let full_path = if cur == "/" {
            format!("/{trimmed}")
        } else {
            format!("{}/{trimmed}", cur.trim_end_matches('/'))
        };

        self.coordinator
            .create_file(&full_path, content)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Renames a file or folder in the virtual drive.
    pub async fn rename_item(&self, remote_path: &str, new_name: &str) -> Result<String, UiError> {
        self.coordinator
            .rename_resource(remote_path, new_name)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Moves a file or folder in the virtual drive to a target directory.
    pub async fn move_item(
        &self,
        source_path: &str,
        destination_dir: &str,
    ) -> Result<String, UiError> {
        self.coordinator
            .move_resource(source_path, destination_dir)
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

    /// Returns all configured account profiles.
    pub async fn accounts(&self) -> Vec<Account> {
        self.coordinator.accounts().await
    }

    /// Returns the active account ID.
    pub async fn active_account_id(&self) -> Option<AccountId> {
        self.coordinator.active_account().await.map(|a| a.id)
    }

    /// Switches the active account profile to `account_id` and resets view navigation to root.
    pub async fn switch_account(&self, account_id: &AccountId) -> Result<(), UiError> {
        self.coordinator
            .switch_account(account_id)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))?;
        self.set_current_path("/").await;
        Ok(())
    }

    /// Adds a new account profile to the application.
    pub async fn add_account(&self, account: Account) -> Result<(), UiError> {
        self.coordinator
            .add_account(account)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Starts a browser-based OAuth login flow for the given server URL.
    pub async fn start_oauth_login(
        &self,
        server_url: &str,
    ) -> Result<(String, nextcloud_rust_sdk::Client), UiError> {
        self.coordinator
            .start_oauth_login(server_url)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Polls a pending OAuth login flow.
    pub async fn poll_oauth_login(
        &self,
        client: &nextcloud_rust_sdk::Client,
    ) -> Result<Option<Account>, UiError> {
        self.coordinator
            .poll_oauth_login(client)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Creates or retrieves a public share link for a virtual file or directory.
    pub async fn create_public_share(&self, remote_path: &str) -> Result<String, UiError> {
        self.coordinator
            .create_public_share(remote_path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Subscribes to application desktop events (tree updates, sync status, transfer progress).
    pub fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<nextcloud_desktop_core::events::DesktopEvent> {
        self.coordinator.event_bus().subscribe()
    }

    /// Synchronizes discovered system accounts on application startup.
    pub async fn ensure_discovered_accounts(&self) {
        self.coordinator.sync_discovered_accounts().await;
    }

    /// Lists all active mounts and their runtime states.
    pub async fn list_mounts(&self) -> Result<Vec<(MountDescriptor, MountState)>, UiError> {
        self.coordinator
            .list_mounts()
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Registers a new mount point connecting a remote directory to a local target.
    pub async fn add_mount(&self, descriptor: MountDescriptor) -> Result<MountId, UiError> {
        self.coordinator
            .add_mount(descriptor)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Removes an existing mount point.
    pub async fn remove_mount(&self, id: MountId) -> Result<(), UiError> {
        self.coordinator
            .remove_mount(id)
            .await
            .map(|_| ())
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Specifically pauses synchronization for a single mount point.
    pub async fn pause_mount(&self, id: MountId) -> Result<(), UiError> {
        self.coordinator
            .pause_mount(id)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Specifically resumes synchronization for a single mount point.
    pub async fn resume_mount(&self, id: MountId) -> Result<(), UiError> {
        self.coordinator
            .resume_mount(id)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Returns `true` if a specific mount point is currently paused.
    pub async fn is_mount_paused(&self, id: MountId) -> Result<bool, UiError> {
        self.coordinator
            .is_mount_paused(id)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Globally pauses synchronization across all mounts and background queues.
    pub async fn pause_all_sync(&self) -> Result<(), UiError> {
        self.coordinator
            .pause_all_sync()
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Globally resumes synchronization across all mounts and background queues.
    pub async fn resume_all_sync(&self) -> Result<(), UiError> {
        self.coordinator
            .resume_all_sync()
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Returns `true` if synchronization is globally paused.
    pub async fn is_sync_paused(&self) -> Result<bool, UiError> {
        self.coordinator
            .is_sync_paused()
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Resolves the file sync status and FreeDesktop icon name for a relative path in a mount.
    pub async fn file_status(
        &self,
        mount_id: MountId,
        path: &str,
    ) -> Result<SyncFileStatus, UiError> {
        self.coordinator
            .file_status(mount_id, path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Resolves the synchronization status for an arbitrary local path across all mounts.
    pub async fn status_for_local_path(
        &self,
        local_path: impl Into<PathBuf>,
    ) -> Result<Option<SyncFileStatus>, UiError> {
        self.coordinator
            .status_for_local_path(local_path)
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

    async fn zoom_level(&self) -> ZoomLevel {
        self.zoom_level().await
    }

    async fn set_zoom_level(&self, zoom: ZoomLevel) {
        self.set_zoom_level(zoom).await;
    }

    async fn search_items(&self, query: &str) -> Result<Vec<FileViewItem>, UiError> {
        self.search_items(query).await
    }

    async fn get_thumbnail(&self, remote_path: &str, size: u32) -> Option<PathBuf> {
        self.get_thumbnail(remote_path, size).await
    }

    fn prefetch_thumbnails(&self, paths: Vec<String>, size: u32) {
        self.prefetch_thumbnails(paths, size);
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

    async fn create_file(&self, file_name: &str, content: &[u8]) -> Result<String, UiError> {
        self.create_file(file_name, content).await
    }

    async fn rename_item(&self, remote_path: &str, new_name: &str) -> Result<String, UiError> {
        self.rename_item(remote_path, new_name).await
    }

    async fn move_item(&self, source_path: &str, destination_dir: &str) -> Result<String, UiError> {
        self.move_item(source_path, destination_dir).await
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
        self.coordinator.cache_dir().display().to_string()
    }

    async fn accounts(&self) -> Vec<Account> {
        self.accounts().await
    }

    async fn active_account_id(&self) -> Option<AccountId> {
        self.active_account_id().await
    }

    async fn switch_account(&self, account_id: &AccountId) -> Result<(), UiError> {
        self.switch_account(account_id).await
    }

    async fn add_account(&self, account: Account) -> Result<(), UiError> {
        self.add_account(account).await
    }

    async fn start_oauth_login(
        &self,
        server_url: &str,
    ) -> Result<(String, nextcloud_rust_sdk::Client), UiError> {
        self.start_oauth_login(server_url).await
    }

    async fn poll_oauth_login(
        &self,
        client: &nextcloud_rust_sdk::Client,
    ) -> Result<Option<Account>, UiError> {
        self.poll_oauth_login(client).await
    }

    async fn create_public_share(&self, remote_path: &str) -> Result<String, UiError> {
        self.create_public_share(remote_path).await
    }

    async fn pause_all_sync(&self) -> Result<(), UiError> {
        self.pause_all_sync().await
    }

    async fn resume_all_sync(&self) -> Result<(), UiError> {
        self.resume_all_sync().await
    }

    async fn is_sync_paused(&self) -> Result<bool, UiError> {
        self.is_sync_paused().await
    }

    async fn pause_mount(&self, mount_id: &str) -> Result<(), UiError> {
        self.pause_mount(MountId::from(mount_id.to_string())).await
    }

    async fn resume_mount(&self, mount_id: &str) -> Result<(), UiError> {
        self.resume_mount(MountId::from(mount_id.to_string())).await
    }

    async fn is_mount_paused(&self, mount_id: &str) -> Result<bool, UiError> {
        self.is_mount_paused(MountId::from(mount_id.to_string()))
            .await
    }
}
