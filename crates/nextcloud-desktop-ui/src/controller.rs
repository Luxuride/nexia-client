//! Controller managing application state and asynchronous backend operations.

use nextcloud_desktop_core::{Account, AccountId};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::UiError;
use crate::model::{
    FileSortCriteria, FileSyncStatus, FileViewItem, FileViewMode, ThemeInfo, ZoomLevel,
};
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
    sort_criteria: Arc<RwLock<FileSortCriteria>>,
    active_hydrations: Arc<RwLock<HashSet<String>>>,
}

fn format_folder_presentation(
    sum: &nextcloud_rust_sdk::DirectorySizeSummary,
    mod_at: Option<&str>,
    cfg: &nextcloud_desktop_core::AdditionalItemsDisplayConfig,
) -> String {
    let mut parts = Vec::new();

    let direct_count = sum.direct_items();
    let total_count = sum.total_items();

    if cfg.show_direct_item_count && (direct_count > 0 || sum.total_size == 0) {
        let label = if direct_count == 1 {
            "1 item".to_string()
        } else {
            format!("{direct_count} items")
        };
        parts.push(label);
    }

    if cfg.show_total_item_count && (total_count > 0 || sum.total_size == 0) {
        let label = if total_count == 1 {
            "1 total item".to_string()
        } else {
            format!("{total_count} total items")
        };
        parts.push(label);
    }

    if cfg.show_direct_folder_size {
        parts.push(nextcloud_rust_sdk::format_bytes(sum.direct_size));
    }

    if cfg.show_total_folder_size {
        parts.push(format!(
            "{} total",
            nextcloud_rust_sdk::format_bytes(sum.total_size)
        ));
    }

    if cfg.show_last_modified
        && let Some(m) = mod_at
    {
        parts.push(m.to_string());
    }

    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join("\n")
    }
}

fn format_file_presentation(
    size_bytes: u64,
    mod_at: Option<&str>,
    cfg: &nextcloud_desktop_core::AdditionalItemsDisplayConfig,
) -> String {
    let mut parts = Vec::new();
    if cfg.show_file_size {
        parts.push(nextcloud_rust_sdk::format_bytes(size_bytes));
    }
    if cfg.show_last_modified
        && let Some(m) = mod_at
    {
        parts.push(m.to_string());
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join("\n")
    }
}

impl DesktopUiController {
    /// Creates a new controller wrapping the given backend coordinator.
    pub fn new(coordinator: DesktopSyncCoordinator) -> Self {
        Self {
            coordinator,
            navigation: Arc::new(RwLock::new(NavigationHistory::new())),
            view_mode: Arc::new(RwLock::new(FileViewMode::Grid)),
            zoom_level: Arc::new(RwLock::new(ZoomLevel::Medium)),
            sort_criteria: Arc::new(RwLock::new(FileSortCriteria::default())),
            active_hydrations: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Access the underlying sync coordinator.
    pub fn coordinator(&self) -> &DesktopSyncCoordinator {
        &self.coordinator
    }

    /// Returns `true` if a virtual file is currently undergoing active hydration / download.
    pub async fn is_hydrating(&self, remote_path: &str) -> bool {
        let clean = format!("/{}", remote_path.trim_matches('/'));
        self.active_hydrations.read().await.contains(&clean)
    }

    /// Marks a remote path as actively hydrating.
    pub async fn mark_hydration_started(&self, remote_path: &str) {
        let clean = format!("/{}", remote_path.trim_matches('/'));
        self.active_hydrations.write().await.insert(clean);
    }

    /// Removes a remote path from active hydrations.
    pub async fn mark_hydration_finished(&self, remote_path: &str) {
        let clean = format!("/{}", remote_path.trim_matches('/'));
        self.active_hydrations.write().await.remove(&clean);
    }

    /// Returns the effective additional display configuration for the active account.
    pub async fn effective_display_config(
        &self,
    ) -> nextcloud_desktop_core::AdditionalItemsDisplayConfig {
        let cfg = self.coordinator.config().read().await;
        cfg.effective_display_config(cfg.active_account_id.as_ref())
    }

    /// Sets additional display configuration for an account and persists it.
    pub async fn set_account_display_config(
        &self,
        account_id: &AccountId,
        display: nextcloud_desktop_core::AdditionalItemsDisplayConfig,
    ) -> Result<(), UiError> {
        {
            let mut cfg = self.coordinator.config().write().await;
            let mut prefs = cfg.get_account_preferences(account_id);
            prefs.display = display;
            cfg.set_account_preferences(account_id, prefs);
            let _ = cfg.save_user_config(account_id);
        }
        self.coordinator.persist_config().await;
        self.coordinator
            .event_bus()
            .emit(nextcloud_desktop_core::events::DesktopEvent::DisplaySettingsChanged);
        Ok(())
    }

    /// Sets global default display configuration.
    pub async fn set_global_display_config(
        &self,
        display: nextcloud_desktop_core::AdditionalItemsDisplayConfig,
    ) -> Result<(), UiError> {
        {
            let mut cfg = self.coordinator.config().write().await;
            cfg.global.default_display = display;
        }
        self.coordinator.persist_config().await;
        self.coordinator
            .event_bus()
            .emit(nextcloud_desktop_core::events::DesktopEvent::DisplaySettingsChanged);
        Ok(())
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

    /// Sets the active view zoom level and persists it to the configuration database.
    pub async fn set_zoom_level(&self, zoom: ZoomLevel) {
        *self.zoom_level.write().await = zoom;
        if let Some(client) = self.coordinator.active_client().await {
            let _ = client.set_setting("view_zoom_level", zoom.as_str()).await;
        }
    }

    /// Loads persisted view zoom level from the SDK config store.
    pub async fn load_persisted_zoom(&self) {
        if let Some(client) = self.coordinator.active_client().await
            && let Ok(Some(val)) = client.get_setting("view_zoom_level").await
        {
            *self.zoom_level.write().await = ZoomLevel::parse(&val);
        }
    }

    /// Returns the current file sorting criteria.
    pub async fn sort_criteria(&self) -> FileSortCriteria {
        *self.sort_criteria.read().await
    }

    /// Sets the active file sorting criteria and persists it for the current directory in the SDK.
    pub async fn set_sort_criteria(&self, criteria: FileSortCriteria) {
        *self.sort_criteria.write().await = criteria;
        let cur = self.current_path().await;
        self.store_directory_sort(&cur, criteria).await;
    }

    /// Persists directory sorting preferences for `dir_path` in the SDK config store.
    pub async fn store_directory_sort(&self, dir_path: &str, criteria: FileSortCriteria) {
        let field_str = match criteria.field {
            nextcloud_rust_sdk::FileSortField::Name => "name",
            nextcloud_rust_sdk::FileSortField::Size => "size",
            nextcloud_rust_sdk::FileSortField::Modified => "modified",
            nextcloud_rust_sdk::FileSortField::Type => "type",
        };
        let order_str = match criteria.order {
            nextcloud_rust_sdk::FileSortOrder::Ascending => "asc",
            nextcloud_rust_sdk::FileSortOrder::Descending => "desc",
        };
        let _ = self
            .coordinator
            .store_directory_sort(dir_path, field_str, order_str)
            .await;
    }

    /// Loads directory sorting preferences for `dir_path` from the SDK config store.
    pub async fn load_directory_sort(&self, dir_path: &str) -> Option<FileSortCriteria> {
        if let Some((field_str, order_str)) = self.coordinator.load_directory_sort(dir_path).await {
            let field = match field_str.as_str() {
                "size" => nextcloud_rust_sdk::FileSortField::Size,
                "modified" => nextcloud_rust_sdk::FileSortField::Modified,
                "type" => nextcloud_rust_sdk::FileSortField::Type,
                _ => nextcloud_rust_sdk::FileSortField::Name,
            };
            let order = match order_str.as_str() {
                "desc" => nextcloud_rust_sdk::FileSortOrder::Descending,
                _ => nextcloud_rust_sdk::FileSortOrder::Ascending,
            };
            Some(FileSortCriteria::new(field, order, true))
        } else {
            None
        }
    }

    /// Applies persisted directory sort when navigating to `dir_path`.
    pub async fn apply_directory_sort(&self, dir_path: &str) {
        if let Some(criteria) = self.load_directory_sort(dir_path).await {
            *self.sort_criteria.write().await = criteria;
        }
    }

    /// Fetches server visual theming metadata.
    pub async fn server_theming(&self) -> Option<ThemeInfo> {
        self.coordinator.server_theming().await
    }

    /// Fetches and caches server theme logo, returning the cached image file path if successful.
    pub async fn fetch_theme_logo(&self) -> Option<PathBuf> {
        self.coordinator.fetch_theme_logo().await
    }

    /// Searches files matching `query` across local VFS and server unified search.
    pub async fn search_items(&self, query: &str) -> Result<Vec<FileViewItem>, UiError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let entries = self
            .coordinator
            .search(trimmed)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))?;

        let display_cfg = self.effective_display_config().await;
        let active_hydrations = self.active_hydrations.read().await;
        let mut items: Vec<FileViewItem> = entries
            .into_iter()
            .map(|e| {
                let clean_p = format!("/{}", e.path.trim_matches('/'));
                let is_syncing = active_hydrations.contains(&clean_p);
                let status = if is_syncing {
                    FileSyncStatus::Syncing
                } else if e.is_directory {
                    if let Some(ref dir_sum) = e.directory_summary {
                        if dir_sum.has_unhydrated && dir_sum.has_unsynced {
                            FileSyncStatus::UnhydratedAndUnsynced
                        } else if dir_sum.has_unhydrated {
                            FileSyncStatus::CloudPlaceholder
                        } else if dir_sum.has_unsynced {
                            FileSyncStatus::LocalOnly
                        } else if dir_sum.file_count > 0 {
                            FileSyncStatus::HydratedOffline
                        } else {
                            FileSyncStatus::CloudPlaceholder
                        }
                    } else {
                        FileSyncStatus::CloudPlaceholder
                    }
                } else if e.etag.is_none() {
                    FileSyncStatus::LocalOnly
                } else if e.is_hydrated {
                    FileSyncStatus::HydratedOffline
                } else {
                    FileSyncStatus::CloudPlaceholder
                };

                let size_formatted = if e.is_directory {
                    if let Some(ref sum) = e.directory_summary {
                        format_folder_presentation(sum, e.modified_at.as_deref(), &display_cfg)
                    } else {
                        "—".to_string()
                    }
                } else {
                    format_file_presentation(e.size, e.modified_at.as_deref(), &display_cfg)
                };

                FileViewItem {
                    path: e.path,
                    name: e.name,
                    is_directory: e.is_directory,
                    size_formatted,
                    size_bytes: e.size,
                    status,
                    mime_type: e.mime_type,
                    file_id: e.file_id,
                    icon_category: e.icon_category,
                    modified_at: e.modified_at,
                    permissions: e.permissions,
                    directory_summary: e.directory_summary,
                }
            })
            .collect();

        let criteria = self.sort_criteria().await;
        FileViewItem::sort_items(&mut items, &criteria);

        Ok(items)
    }

    /// Fast synchronous check returning a cached thumbnail path (preferred size or 64px placeholder) if already on disk.
    pub fn cached_thumbnail(
        &self,
        remote_path: &str,
        preferred_size: Option<u32>,
    ) -> Option<PathBuf> {
        self.coordinator
            .cached_thumbnail_path(remote_path, preferred_size)
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

    /// Returns the live snapshot of background thumbnail downloading queue and progress.
    pub fn thumbnail_queue_snapshot(&self) -> nextcloud_desktop_sync::ThumbnailQueueSnapshot {
        self.coordinator.thumbnail_queue_snapshot()
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
        let changed = self.navigation.write().await.go_back();
        if changed {
            let p = self.current_path().await;
            self.apply_directory_sort(&p).await;
        }
        changed
    }

    /// Navigates forward in the folder history stack.
    pub async fn go_forward(&self) -> bool {
        let changed = self.navigation.write().await.go_forward();
        if changed {
            let p = self.current_path().await;
            self.apply_directory_sort(&p).await;
        }
        changed
    }

    /// Navigates the file view into a child directory, recording history via `NavigationHistory`.
    pub async fn navigate_into(&self, folder_name: &str) {
        self.navigation.write().await.navigate_into(folder_name);
        let p = self.current_path().await;
        self.apply_directory_sort(&p).await;
    }

    /// Navigates up to the parent directory, recording history via `NavigationHistory`.
    pub async fn navigate_up(&self) {
        self.navigation.write().await.navigate_up();
        let p = self.current_path().await;
        self.apply_directory_sort(&p).await;
    }

    /// Sets the current virtual directory directly (e.g. from a breadcrumb click), recording history.
    pub async fn set_current_path(&self, path: &str) {
        self.navigation.write().await.navigate_to(path);
        let p = self.current_path().await;
        self.apply_directory_sort(&p).await;
    }

    /// Lists items in the current virtual directory for display in the file view.
    pub async fn list_current_items(&self) -> Result<Vec<FileViewItem>, UiError> {
        let path = self.current_path().await;
        let vfs = self.coordinator.active_vfs().await;
        let is_populated = vfs.is_directory_populated(&path).await.unwrap_or(false);
        let mut entries = vfs
            .list_children(&path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))?;

        if !is_populated || (entries.is_empty() && path != "/") {
            let _ = self.coordinator.refresh_path(&path).await;
            if let Ok(fresh) = vfs.list_children(&path).await {
                entries = fresh;
            }
        }

        let display_cfg = self.effective_display_config().await;
        let active_hydrations = self.active_hydrations.read().await;
        let mut items: Vec<FileViewItem> = entries
            .into_iter()
            .map(|e| {
                let clean_p = format!("/{}", e.path.trim_matches('/'));
                let is_syncing = active_hydrations.contains(&clean_p);
                let status = if is_syncing {
                    FileSyncStatus::Syncing
                } else if e.is_directory {
                    if let Some(ref dir_sum) = e.directory_summary {
                        if dir_sum.has_unhydrated && dir_sum.has_unsynced {
                            FileSyncStatus::UnhydratedAndUnsynced
                        } else if dir_sum.has_unhydrated {
                            FileSyncStatus::CloudPlaceholder
                        } else if dir_sum.has_unsynced {
                            FileSyncStatus::LocalOnly
                        } else if dir_sum.file_count > 0 {
                            FileSyncStatus::HydratedOffline
                        } else {
                            FileSyncStatus::CloudPlaceholder
                        }
                    } else {
                        FileSyncStatus::CloudPlaceholder
                    }
                } else if e.etag.is_none() {
                    FileSyncStatus::LocalOnly
                } else if e.is_hydrated {
                    FileSyncStatus::HydratedOffline
                } else {
                    FileSyncStatus::CloudPlaceholder
                };

                let size_formatted = if e.is_directory {
                    if let Some(ref sum) = e.directory_summary {
                        format_folder_presentation(sum, e.modified_at.as_deref(), &display_cfg)
                    } else {
                        "—".to_string()
                    }
                } else {
                    format_file_presentation(e.size, e.modified_at.as_deref(), &display_cfg)
                };

                FileViewItem {
                    path: e.path,
                    name: e.name,
                    is_directory: e.is_directory,
                    size_formatted,
                    size_bytes: e.size,
                    status,
                    mime_type: e.mime_type,
                    file_id: e.file_id,
                    icon_category: e.icon_category,
                    modified_at: e.modified_at,
                    permissions: e.permissions,
                    directory_summary: e.directory_summary,
                }
            })
            .collect();

        // Sort directories first, then alphabetically by name
        let criteria = self.sort_criteria().await;
        FileViewItem::sort_items(&mut items, &criteria);

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
        self.hydrate_file(remote_path).await
    }

    /// Hydrates a virtual file on-demand.
    pub async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, UiError> {
        let clean = format!("/{}", remote_path.trim_matches('/'));
        self.mark_hydration_started(&clean).await;
        let res = self
            .coordinator
            .hydrate_file(remote_path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()));
        self.mark_hydration_finished(&clean).await;
        res
    }

    /// Recursively hydrates all unhydrated files within a virtual directory path.
    pub async fn hydrate_directory(&self, remote_dir: &str) -> Result<usize, UiError> {
        self.coordinator
            .hydrate_directory(remote_dir)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Dehydrates a file back to a placeholder, freeing disk space.
    pub async fn dehydrate_file(&self, remote_path: &str) -> Result<(), UiError> {
        let clean = format!("/{}", remote_path.trim_matches('/'));
        self.mark_hydration_finished(&clean).await;
        self.coordinator
            .dehydrate_file(remote_path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Refreshes the virtual file tree from the remote Nextcloud server.
    pub async fn refresh_virtual_tree(&self) -> Result<usize, UiError> {
        let path = self.current_path().await;
        let _ = self.coordinator.refresh_path(&path).await;
        self.coordinator
            .refresh_virtual_tree()
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Triggers a synchronization cycle across all configured folder pairings.
    pub async fn trigger_sync(&self) -> Result<(), UiError> {
        self.coordinator.sync_all_mounts().await;
        Ok(())
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

    /// Signs out and removes a non-system account, optionally cleaning up synchronized files and pairings.
    pub async fn sign_out_account(
        &self,
        account_id: &AccountId,
        clean_syncs: bool,
    ) -> Result<(), UiError> {
        self.coordinator
            .sign_out_account(account_id, clean_syncs)
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

    /// Updates the synchronization mode for an existing sync mount.
    pub async fn update_sync_mount_mode(
        &self,
        mount_id: &str,
        new_mode: &str,
    ) -> Result<(), UiError> {
        self.coordinator
            .update_sync_mount_mode(mount_id, new_mode)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Queries remote Nextcloud via WebDAV to list child directories beneath `relative_path`.
    pub async fn list_remote_directory_folders(
        &self,
        relative_path: &str,
    ) -> Result<Vec<String>, UiError> {
        self.coordinator
            .list_remote_directory_folders(relative_path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Specifically pauses synchronization for a single mount point.
    pub async fn pause_mount(&self, id: MountId) -> Result<(), UiError> {
        self.coordinator
            .pause_mount(id)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Triggers an immediate folder synchronization cycle for a specific mount point.
    pub async fn sync_mount(&self, mount_id: &str) -> Result<(), UiError> {
        self.coordinator
            .sync_mount_folder(mount_id)
            .await
            .map(|_| ())
            .map_err(|err| UiError::Controller(err.to_string()))
    }

    /// Triggers an immediate folder synchronization cycle across all active mount points.
    pub async fn sync_all_mounts(&self) -> Result<(), UiError> {
        self.coordinator.sync_all_mounts().await;
        Ok(())
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

    /// Mounts the active Nextcloud account as an OS virtual drive via FUSE.
    pub async fn mount_fuse_vfs(
        &self,
        mountpoint: impl AsRef<std::path::Path>,
    ) -> Result<(), UiError> {
        self.coordinator
            .mount_fuse_vfs(mountpoint)
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    /// Unmounts the running FUSE virtual filesystem.
    pub async fn unmount_fuse_vfs(&self) {
        self.coordinator.unmount_fuse_vfs().await;
    }

    /// Returns `true` if the FUSE virtual filesystem is actively mounted.
    pub async fn is_fuse_mounted(&self) -> bool {
        self.coordinator.is_fuse_mounted().await
    }

    /// Returns the default FUSE mountpoint for the active account.
    pub async fn default_fuse_mountpoint(&self) -> std::path::PathBuf {
        self.coordinator.default_fuse_mountpoint().await
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

    async fn sort_criteria(&self) -> FileSortCriteria {
        self.sort_criteria().await
    }

    async fn set_sort_criteria(&self, criteria: FileSortCriteria) {
        self.set_sort_criteria(criteria).await;
    }

    async fn server_theming(&self) -> Option<ThemeInfo> {
        self.server_theming().await
    }

    fn cached_theming(&self) -> Option<ThemeInfo> {
        self.coordinator.cached_theme()
    }

    fn cached_theme_logo(&self) -> Option<PathBuf> {
        self.coordinator.cached_theme_logo()
    }

    async fn fetch_theme_logo(&self) -> Option<PathBuf> {
        self.fetch_theme_logo().await
    }

    async fn cache_database_size(&self) -> u64 {
        self.coordinator.cache_database_size().await
    }

    async fn cached_files_size(&self) -> u64 {
        self.coordinator.cached_files_size().await
    }

    async fn clean_thumbnails(&self) -> Result<u64, UiError> {
        self.coordinator
            .clean_thumbnails()
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn clean_everything(&self) -> Result<u64, UiError> {
        self.coordinator
            .clean_everything()
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn clean_oversized_thumbnails(&self) -> Result<u64, UiError> {
        self.coordinator
            .clean_oversized_thumbnails()
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn max_thumbnail_size(&self) -> u32 {
        self.coordinator.max_thumbnail_size().await
    }

    async fn set_max_thumbnail_size(&self, size: u32) -> Result<(), UiError> {
        self.coordinator
            .set_max_thumbnail_size(size)
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn virtual_sync_downloaded_size(&self) -> u64 {
        self.coordinator.virtual_sync_downloaded_size().await
    }

    async fn clear_virtual_sync_cache(&self) -> Result<u64, UiError> {
        self.coordinator
            .clear_virtual_sync_cache()
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn is_autodownload_thumbnails_enabled(&self) -> bool {
        self.coordinator.is_autodownload_thumbnails_enabled().await
    }

    async fn set_autodownload_thumbnails_enabled(&self, enabled: bool) -> Result<(), UiError> {
        self.coordinator
            .set_autodownload_thumbnails_enabled(enabled)
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn search_items(&self, query: &str) -> Result<Vec<FileViewItem>, UiError> {
        self.search_items(query).await
    }

    fn cached_thumbnail(&self, remote_path: &str, preferred_size: Option<u32>) -> Option<PathBuf> {
        self.cached_thumbnail(remote_path, preferred_size)
    }

    async fn get_thumbnail(&self, remote_path: &str, size: u32) -> Option<PathBuf> {
        self.get_thumbnail(remote_path, size).await
    }

    fn prefetch_thumbnails(&self, paths: Vec<String>, size: u32) {
        self.prefetch_thumbnails(paths, size);
    }

    fn thumbnail_queue_snapshot(&self) -> nextcloud_desktop_sync::ThumbnailQueueSnapshot {
        self.thumbnail_queue_snapshot()
    }

    fn subscribe_events(
        &self,
    ) -> tokio::sync::broadcast::Receiver<nextcloud_desktop_core::events::DesktopEvent> {
        self.subscribe_events()
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

    async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, UiError> {
        self.hydrate_file(remote_path).await
    }

    async fn hydrate_directory(&self, remote_dir: &str) -> Result<usize, UiError> {
        self.hydrate_directory(remote_dir).await
    }

    async fn dehydrate_file(&self, remote_path: &str) -> Result<(), UiError> {
        self.dehydrate_file(remote_path).await
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

    async fn sign_out_account(
        &self,
        account_id: &AccountId,
        clean_syncs: bool,
    ) -> Result<(), UiError> {
        self.sign_out_account(account_id, clean_syncs).await
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

    async fn get_public_share(&self, remote_path: &str) -> Result<Option<String>, UiError> {
        self.coordinator
            .get_public_share(remote_path)
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn delete_public_share(&self, remote_path: &str) -> Result<bool, UiError> {
        self.coordinator
            .delete_public_share(remote_path)
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn store_directory_sort(
        &self,
        dir_path: &str,
        criteria: nextcloud_rust_sdk::FileSortCriteria,
    ) {
        self.store_directory_sort(dir_path, criteria).await;
    }

    async fn load_directory_sort(
        &self,
        dir_path: &str,
    ) -> Option<nextcloud_rust_sdk::FileSortCriteria> {
        self.load_directory_sort(dir_path).await
    }

    async fn effective_display_config(
        &self,
    ) -> nextcloud_desktop_core::AdditionalItemsDisplayConfig {
        self.effective_display_config().await
    }

    async fn set_account_display_config(
        &self,
        account_id: &AccountId,
        display: nextcloud_desktop_core::AdditionalItemsDisplayConfig,
    ) -> Result<(), UiError> {
        self.set_account_display_config(account_id, display).await
    }

    async fn set_global_display_config(
        &self,
        display: nextcloud_desktop_core::AdditionalItemsDisplayConfig,
    ) -> Result<(), UiError> {
        self.set_global_display_config(display).await
    }

    async fn sync_mounts(&self) -> Vec<nextcloud_rust_sdk::StoredSyncMount> {
        self.coordinator.sync_mounts().await
    }

    async fn add_sync_mount(
        &self,
        remote_path: String,
        local_target: PathBuf,
        mode: String,
        display_name: Option<String>,
    ) -> Result<String, UiError> {
        self.coordinator
            .add_sync_mount(remote_path, local_target, &mode, display_name)
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn remove_sync_mount(&self, mount_id: &str) -> Result<(), UiError> {
        self.coordinator
            .remove_sync_mount(mount_id)
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn remove_sync_mount_with_cleanup(
        &self,
        mount_id: &str,
        clean_local_files: bool,
    ) -> Result<(), UiError> {
        self.coordinator
            .remove_sync_mount_with_cleanup(mount_id, clean_local_files)
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn sync_mount(&self, mount_id: &str) -> Result<(), UiError> {
        self.sync_mount(mount_id).await
    }

    async fn sync_all_mounts(&self) -> Result<(), UiError> {
        self.sync_all_mounts().await
    }

    async fn update_sync_mount_mode(&self, mount_id: &str, new_mode: &str) -> Result<(), UiError> {
        self.update_sync_mount_mode(mount_id, new_mode).await
    }

    async fn mount_fuse_vfs(&self, mountpoint: std::path::PathBuf) -> Result<(), UiError> {
        self.coordinator
            .mount_fuse_vfs(mountpoint)
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn unmount_fuse_vfs(&self) {
        self.coordinator.unmount_fuse_vfs().await;
    }

    async fn is_fuse_mounted(&self) -> bool {
        self.coordinator.is_fuse_mounted().await
    }

    async fn default_fuse_mountpoint(&self) -> std::path::PathBuf {
        self.coordinator.default_fuse_mountpoint().await
    }

    async fn list_remote_directory_folders(
        &self,
        relative_path: &str,
    ) -> Result<Vec<String>, UiError> {
        self.list_remote_directory_folders(relative_path).await
    }

    async fn mount_storage_usage(
        &self,
        mount_id: &str,
    ) -> Result<nextcloud_rust_sdk::MountStorageUsage, UiError> {
        self.coordinator
            .mount_storage_usage(nextcloud_rust_sdk::MountId::from(mount_id.to_string()))
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn clear_mount_cache(
        &self,
        mount_id: &str,
        clear_hydrated: bool,
    ) -> Result<u64, UiError> {
        self.coordinator
            .clear_mount_cache(
                nextcloud_rust_sdk::MountId::from(mount_id.to_string()),
                clear_hydrated,
            )
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn clear_shared_thumbnails(&self) -> Result<u64, UiError> {
        self.coordinator
            .clear_thumbnail_cache()
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }

    async fn mount_progress(
        &self,
        mount_id: &str,
    ) -> Result<nextcloud_rust_sdk::MountProgressSnapshot, UiError> {
        self.coordinator
            .mount_progress(nextcloud_rust_sdk::MountId::from(mount_id.to_string()))
            .await
            .map_err(|e| UiError::Controller(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nextcloud_desktop_core::config::AppConfig;
    use nextcloud_desktop_core::events::EventBus;

    #[tokio::test]
    async fn active_hydrations_tracking() {
        let config_lock = Arc::new(RwLock::new(AppConfig::default()));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));
        let controller = DesktopUiController::new(coordinator);

        assert!(!controller.is_hydrating("/Documents/test.pdf").await);
        controller
            .mark_hydration_started("Documents/test.pdf")
            .await;
        assert!(controller.is_hydrating("/Documents/test.pdf").await);
        assert!(controller.is_hydrating("Documents/test.pdf").await);

        controller
            .mark_hydration_finished("/Documents/test.pdf")
            .await;
        assert!(!controller.is_hydrating("/Documents/test.pdf").await);
    }

    #[test]
    fn format_folder_presentation_respects_direct_and_total_modes() {
        let sum =
            nextcloud_rust_sdk::DirectorySizeSummary::new(1048576, 50, 5).with_direct(4096, 3, 2);

        let mut cfg = nextcloud_desktop_core::AdditionalItemsDisplayConfig {
            show_direct_item_count: true,
            show_total_item_count: false,
            ..Default::default()
        };

        // Direct only (default)
        let direct = format_folder_presentation(&sum, None, &cfg);
        assert_eq!(direct, "5 items");

        // Total only
        cfg.show_direct_item_count = false;
        cfg.show_total_item_count = true;
        let total = format_folder_presentation(&sum, None, &cfg);
        assert_eq!(total, "55 total items");

        // Both
        cfg.show_direct_item_count = true;
        cfg.show_total_item_count = true;
        let both = format_folder_presentation(&sum, None, &cfg);
        assert_eq!(both, "5 items\n55 total items");

        // Hidden item count
        cfg.show_direct_item_count = false;
        cfg.show_total_item_count = false;
        let hidden = format_folder_presentation(&sum, None, &cfg);
        assert_eq!(hidden, "—");
    }
}
