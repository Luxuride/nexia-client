//! Configuration management and persistence for the desktop application.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::account::{Account, AccountId};
use crate::error::CoreError;

/// Synchronization mode for a folder pairing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    /// Virtual synchronization: hierarchy and metadata downloaded as placeholders;
    /// file contents are hydrated on-demand without consuming upfront disk space.
    VirtualSync,
    /// Full bidirectional synchronization: files are downloaded to local storage
    /// and local changes are synced back to Nextcloud.
    FullTwoWaySync,
}

/// A configured folder pairing between local storage and Nextcloud.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncFolderConfig {
    /// Unique pairing ID.
    pub id: String,
    /// Associated account ID.
    pub account_id: AccountId,
    /// Absolute local folder path on host disk.
    pub local_path: PathBuf,
    /// Remote relative folder path on Nextcloud (e.g. `/`, `/Documents`).
    pub remote_path: String,
    /// Selected synchronization mode.
    pub mode: SyncMode,
    /// Whether automatic periodic sync is enabled for this pair.
    pub enabled: bool,
}

impl SyncFolderConfig {
    /// Creates a new sync folder pairing.
    pub fn new(
        account_id: AccountId,
        local_path: impl Into<PathBuf>,
        remote_path: impl Into<String>,
        mode: SyncMode,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            account_id,
            local_path: local_path.into(),
            remote_path: remote_path.into(),
            mode,
            enabled: true,
        }
    }
}

/// Folder item count presentation mode for file tiles and rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderItemsDisplay {
    /// Show direct item count only (e.g. "3 items"). Default.
    #[default]
    DirectOnly,
    /// Show recursive total item count only (e.g. "124 items").
    TotalOnly,
    /// Show both direct items and recursive total (e.g. "3 items (124 total)").
    Both,
}

/// Metadata presentation options configuring what additional items are always shown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdditionalItemsDisplayConfig {
    /// How folder item counts are presented in file explorer tiles and rows.
    pub folder_items: FolderItemsDisplay,
    /// Whether to display permissions in properties dialog and detailed views.
    pub show_permissions: bool,
    /// Whether to display MIME type information.
    pub show_mime_type: bool,
    /// Whether to display direct vs total folder size breakdown in properties.
    pub show_size_breakdown: bool,
    /// Whether to display file storage usage (size on disk vs remote size).
    pub show_disk_usage: bool,
    /// Maximum allowed thumbnail size in pixels (e.g. 64, 128, 256, 512). Default is 256.
    pub max_thumbnail_size: u32,
    /// Whether to auto-download thumbnails when browsing.
    pub autodownload_thumbnails: bool,
}

impl Default for AdditionalItemsDisplayConfig {
    fn default() -> Self {
        Self {
            folder_items: FolderItemsDisplay::DirectOnly,
            show_permissions: true,
            show_mime_type: true,
            show_size_breakdown: true,
            show_disk_usage: true,
            max_thumbnail_size: 256,
            autodownload_thumbnails: true,
        }
    }
}

/// Per-account configuration preferences stored in desktop app config database.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPreferences {
    /// Additional display items configuration for this account.
    pub display: AdditionalItemsDisplayConfig,
    /// Default view mode for this account (Grid or List).
    pub default_view_mode: Option<String>,
    /// Default zoom level for this account (Small, Medium, Large, ExtraLarge).
    pub default_zoom_level: Option<String>,
}

/// Global desktop application configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalPreferences {
    /// Start application minimized to background app bar / system tray.
    pub start_minimized: bool,
    /// Autostart application on system desktop login.
    pub autostart: bool,
    /// Global fallback display configuration when no account-specific override is set.
    pub default_display: AdditionalItemsDisplayConfig,
}

/// Root application configuration persisted across sessions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// List of configured accounts.
    pub accounts: Vec<Account>,
    /// Currently active account identifier.
    pub active_account_id: Option<AccountId>,
    /// Configured sync folder pairings.
    pub sync_folders: Vec<SyncFolderConfig>,
    /// Global cache directory for virtual files and thumbnails.
    pub cache_dir: Option<PathBuf>,
    /// Interval in seconds for background sync cycles.
    pub sync_interval_secs: u64,
    /// Global application-level preferences.
    #[serde(default)]
    pub global: GlobalPreferences,
    /// Per-account configuration preferences indexed by account ID string.
    #[serde(default)]
    pub account_preferences: std::collections::HashMap<String, AccountPreferences>,
}

impl AppConfig {
    /// Returns the default configuration path in the user config directory.
    pub fn default_config_path() -> Result<PathBuf, CoreError> {
        let base = dirs::config_dir().ok_or(CoreError::MissingConfigDir)?;
        Ok(base.join("nextcloud-desktop-rs").join("config.json"))
    }

    /// Returns the default cache directory path in the user cache directory.
    pub fn default_cache_dir() -> PathBuf {
        dirs::cache_dir()
            .map(|c| c.join("nextcloud-desktop"))
            .unwrap_or_else(|| std::env::temp_dir().join("nextcloud-desktop"))
    }

    /// Resolves the configured cache directory, falling back to [`Self::default_cache_dir`].
    pub fn resolved_cache_dir(&self) -> PathBuf {
        self.cache_dir
            .clone()
            .unwrap_or_else(Self::default_cache_dir)
    }

    /// Loads configuration from the specified file path, or creates a default if absent.
    pub fn load_from_path(path: &Path) -> Result<Self, CoreError> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path).map_err(|source| CoreError::ConfigLoad {
            path: path.to_path_buf(),
            source,
        })?;

        let cfg: Self = serde_json::from_str(&content).map_err(CoreError::ConfigParse)?;
        Ok(cfg)
    }

    /// Synchronizes discovered desktop system accounts with persistent configuration.
    pub fn sync_system_accounts(&mut self, system_accounts: Vec<Account>) {
        // Clean up any legacy placeholder dummy system accounts
        self.accounts.retain(|acc| acc.id.0 != "system-account");

        // Remove obsolete system accounts that are no longer discovered
        self.accounts
            .retain(|acc| !acc.is_system || system_accounts.iter().any(|sys| sys.id == acc.id));

        for sys in system_accounts {
            if let Some(pos) = self.accounts.iter().position(|a| a.id == sys.id) {
                self.accounts[pos] = sys;
            } else {
                self.accounts.push(sys);
            }
        }

        let needs_fallback = self.active_account_id.is_none()
            || !self
                .accounts
                .iter()
                .any(|a| Some(&a.id) == self.active_account_id.as_ref());
        if needs_fallback {
            self.active_account_id = self.accounts.first().map(|a| a.id.clone());
        }
    }

    /// Saves configuration to the specified file path, creating parent directories if needed.
    pub fn save_to_path(&self, path: &Path) -> Result<(), CoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| CoreError::ConfigSave {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let data = serde_json::to_string_pretty(self).map_err(CoreError::ConfigParse)?;
        fs::write(path, data).map_err(|source| CoreError::ConfigSave {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(())
    }

    /// Finds an account by its ID.
    pub fn get_account(&self, id: &AccountId) -> Option<&Account> {
        self.accounts.iter().find(|acc| &acc.id == id)
    }

    /// Adds or updates an account profile.
    pub fn upsert_account(&mut self, account: Account) {
        if let Some(pos) = self.accounts.iter().position(|acc| acc.id == account.id) {
            self.accounts[pos] = account;
        } else {
            self.accounts.push(account);
        }
    }

    /// Removes an account by ID.
    pub fn remove_account(&mut self, id: &AccountId) {
        self.accounts.retain(|acc| &acc.id != id);
        self.account_preferences.remove(&id.0);
        if self.active_account_id.as_ref() == Some(id) {
            self.active_account_id = self.accounts.first().map(|a| a.id.clone());
        }
    }

    /// Gets configuration preferences for a specific account, falling back to default.
    pub fn get_account_preferences(&self, id: &AccountId) -> AccountPreferences {
        self.account_preferences
            .get(&id.0)
            .cloned()
            .unwrap_or_else(|| AccountPreferences {
                display: self.global.default_display.clone(),
                default_view_mode: None,
                default_zoom_level: None,
            })
    }

    /// Sets or updates configuration preferences for a specific account.
    pub fn set_account_preferences(&mut self, id: &AccountId, prefs: AccountPreferences) {
        self.account_preferences.insert(id.0.clone(), prefs);
    }

    /// Resolves the effective additional items display config for an account, falling back to global preferences.
    pub fn effective_display_config(
        &self,
        account_id: Option<&AccountId>,
    ) -> AdditionalItemsDisplayConfig {
        if let Some(id) = account_id
            && let Some(prefs) = self.account_preferences.get(&id.0)
        {
            prefs.display.clone()
        } else {
            self.global.default_display.clone()
        }
    }

    /// Returns the directory used for per-account / per-user configuration files.
    pub fn user_config_dir() -> Result<PathBuf, CoreError> {
        let base = dirs::config_dir().ok_or(CoreError::MissingConfigDir)?;
        Ok(base.join("nextcloud-desktop-rs").join("accounts"))
    }

    /// Persists an account's configuration into a dedicated per-user database/file.
    pub fn save_user_config(&self, id: &AccountId) -> Result<(), CoreError> {
        let dir = Self::user_config_dir()?;
        fs::create_dir_all(&dir).map_err(|source| CoreError::ConfigSave {
            path: dir.clone(),
            source,
        })?;
        let prefs = self.get_account_preferences(id);
        let path = dir.join(format!("{}.json", id.0));
        let data = serde_json::to_string_pretty(&prefs).map_err(CoreError::ConfigParse)?;
        fs::write(&path, data).map_err(|source| CoreError::ConfigSave { path, source })?;
        Ok(())
    }

    /// Loads an account's configuration from its dedicated per-user database/file if present.
    pub fn load_user_config(id: &AccountId) -> Result<Option<AccountPreferences>, CoreError> {
        let path = Self::user_config_dir()?.join(format!("{}.json", id.0));
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path).map_err(|source| CoreError::ConfigLoad {
            path: path.clone(),
            source,
        })?;
        let prefs: AccountPreferences =
            serde_json::from_str(&data).map_err(CoreError::ConfigParse)?;
        Ok(Some(prefs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{AccountCredentials, ServerConfig};

    #[test]
    fn config_roundtrip() {
        let dir = std::env::temp_dir().join(format!("nc_test_config_{}", Uuid::new_v4()));
        let config_file = dir.join("config.json");

        let mut config = AppConfig::default();
        let acc = Account::new(
            "Test Account",
            ServerConfig::new("https://cloud.test.org"),
            AccountCredentials::new("user", "pass"),
        );
        let acc_id = acc.id.clone();
        config.upsert_account(acc);
        config.sync_folders.push(SyncFolderConfig::new(
            acc_id.clone(),
            "/home/user/Nextcloud",
            "/",
            SyncMode::VirtualSync,
        ));

        config.save_to_path(&config_file).expect("save succeeded");
        let loaded = AppConfig::load_from_path(&config_file).expect("load succeeded");

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].name, "Test Account");
        assert_eq!(loaded.sync_folders.len(), 1);
        assert_eq!(loaded.sync_folders[0].mode, SyncMode::VirtualSync);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn account_preferences_and_display_config_roundtrip() {
        let mut config = AppConfig::default();
        let acc_id = AccountId("acc-123".to_string());

        let default_disp = config.effective_display_config(Some(&acc_id));
        assert_eq!(default_disp.folder_items, FolderItemsDisplay::DirectOnly);
        assert!(default_disp.show_size_breakdown);
        assert!(default_disp.show_disk_usage);

        let mut custom_prefs = AccountPreferences::default();
        custom_prefs.display.folder_items = FolderItemsDisplay::Both;
        custom_prefs.display.max_thumbnail_size = 128;
        config.set_account_preferences(&acc_id, custom_prefs);

        let acc_disp = config.effective_display_config(Some(&acc_id));
        assert_eq!(acc_disp.folder_items, FolderItemsDisplay::Both);
        assert_eq!(acc_disp.max_thumbnail_size, 128);

        // Fallback for unconfigured account uses global
        let other_id = AccountId("other-456".to_string());
        let fallback_disp = config.effective_display_config(Some(&other_id));
        assert_eq!(fallback_disp.folder_items, FolderItemsDisplay::DirectOnly);
    }
}
