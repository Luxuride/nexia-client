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

/// Root application configuration persisted across sessions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// List of configured accounts.
    pub accounts: Vec<Account>,
    /// Configured sync folder pairings.
    pub sync_folders: Vec<SyncFolderConfig>,
    /// Global cache directory for virtual files and thumbnails.
    pub cache_dir: Option<PathBuf>,
    /// Interval in seconds for background sync cycles.
    pub sync_interval_secs: u64,
}

impl AppConfig {
    /// Returns the default configuration path in the user config directory.
    pub fn default_config_path() -> Result<PathBuf, CoreError> {
        let base = dirs::config_dir().ok_or(CoreError::MissingConfigDir)?;
        Ok(base.join("nextcloud-desktop-rs").join("config.json"))
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

        serde_json::from_str(&content).map_err(CoreError::ConfigParse)
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
}
