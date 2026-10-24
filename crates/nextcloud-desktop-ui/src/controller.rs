//! Controller managing application state and asynchronous backend operations.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::UiError;
use crate::model::{FileSyncStatus, FileViewItem};
use nextcloud_desktop_sync::DesktopSyncCoordinator;

/// UI Controller bridging user actions to backend sync and VFS operations.
#[derive(Clone)]
pub struct DesktopUiController {
    coordinator: DesktopSyncCoordinator,
    current_directory: Arc<RwLock<String>>,
}

impl DesktopUiController {
    /// Creates a new controller wrapping the given backend coordinator.
    pub fn new(coordinator: DesktopSyncCoordinator) -> Self {
        Self {
            coordinator,
            current_directory: Arc::new(RwLock::new("/".to_string())),
        }
    }

    /// Access the underlying sync coordinator.
    pub fn coordinator(&self) -> &DesktopSyncCoordinator {
        &self.coordinator
    }

    /// Gets the current virtual directory path.
    pub async fn current_path(&self) -> String {
        self.current_directory.read().await.clone()
    }

    /// Navigates the file view into a child directory.
    pub async fn navigate_into(&self, folder_name: &str) {
        let mut cur = self.current_directory.write().await;
        if *cur == "/" {
            *cur = format!("/{}", folder_name.trim_matches('/'));
        } else {
            *cur = format!(
                "{}/{}",
                cur.trim_end_matches('/'),
                folder_name.trim_matches('/')
            );
        }
    }

    /// Navigates up to the parent directory.
    pub async fn navigate_up(&self) {
        let mut cur = self.current_directory.write().await;
        if *cur == "/" {
            return;
        }

        if let Some(pos) = cur.rfind('/') {
            if pos == 0 {
                *cur = "/".to_string();
            } else {
                *cur = cur[..pos].to_string();
            }
        } else {
            *cur = "/".to_string();
        }
    }

    /// Sets the current virtual directory directly.
    pub async fn set_current_path(&self, path: &str) {
        let clean = if path.is_empty() || path == "/" {
            "/".to_string()
        } else {
            format!("/{}", path.trim_matches('/'))
        };
        *self.current_directory.write().await = clean;
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
                    "--".to_string()
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

    /// Hydrates a virtual file on-demand.
    pub async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, UiError> {
        self.coordinator
            .hydrate_file(remote_path)
            .await
            .map_err(|err| UiError::Controller(err.to_string()))
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
