//! Encrypted SQLite storage for Nexia Client settings and configuration.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nexia_nextcloud_sdk::{ConfigStore, KeyStore, KeychainKeyStore, StoreKey};

use crate::account::AccountId;
use crate::config::{AccountPreferences, AppConfig, GlobalPreferences};
use crate::error::CoreError;

/// Encrypted storage engine for desktop settings, preferences, and state.
#[derive(Clone)]
pub struct DesktopStorage {
    store: Arc<Mutex<ConfigStore>>,
    db_path: PathBuf,
}

impl DesktopStorage {
    /// Opens the encrypted configuration database at `path` using `key`.
    pub fn open(path: impl Into<PathBuf>, key: &StoreKey) -> Result<Self, CoreError> {
        let db_path = path.into();
        let store = ConfigStore::open(&db_path, key)?;
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            db_path,
        })
    }

    /// Resolves the storage key using the platform keychain, falling back to a local key file.
    ///
    /// The key file is created with mode `0600` because it decrypts the database that
    /// holds the configured credentials. An existing but unreadable key is an error:
    /// generating a replacement would orphan the database it was protecting.
    pub async fn resolve_key(key_file: &Path) -> Result<StoreKey, CoreError> {
        let keystore = KeychainKeyStore::new("eu.luxuride.nexia", "desktop-storage-key");
        if let Ok(key) = keystore.load_or_create().await {
            return Ok(key);
        }

        if key_file.exists() {
            let content =
                std::fs::read_to_string(key_file).map_err(|source| CoreError::ConfigLoad {
                    path: key_file.to_path_buf(),
                    source,
                })?;
            return StoreKey::from_hex(content.trim(), "desktop-storage-key").map_err(|source| {
                CoreError::StorageKey {
                    path: key_file.to_path_buf(),
                    source,
                }
            });
        }

        let key = StoreKey::generate()?;
        write_key_file(key_file, &key.to_hex())?;
        Ok(key)
    }

    /// Opens the default encrypted database in the user data directory.
    pub async fn open_default() -> Result<Self, CoreError> {
        let data_dir = dirs::data_dir().ok_or(CoreError::MissingDataDir)?;
        let db_dir = data_dir.join("nexia");
        let db_path = db_dir.join("desktop_config.db");
        let key_file = db_dir.join(".storage_key");

        let key = Self::resolve_key(&key_file).await?;
        Self::open(db_path, &key)
    }

    /// Returns the filesystem path to this encrypted database.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Persists an arbitrary key-value setting.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let store = self
            .store
            .lock()
            .map_err(|_| CoreError::StorageLockPoisoned)?;
        store.set_setting(key, value)?;
        Ok(())
    }

    /// Retrieves an arbitrary key-value setting.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, CoreError> {
        let store = self
            .store
            .lock()
            .map_err(|_| CoreError::StorageLockPoisoned)?;
        Ok(store.get_setting(key)?)
    }

    /// Saves the complete application configuration into the encrypted database.
    pub fn save_app_config(&self, config: &AppConfig) -> Result<(), CoreError> {
        let json = encode(config, &self.db_path)?;
        self.set_setting("app_config", &json)?;
        Ok(())
    }

    /// Loads the application configuration from the encrypted database if present.
    pub fn load_app_config(&self) -> Result<Option<AppConfig>, CoreError> {
        if let Some(json) = self.get_setting("app_config")? {
            let cfg: AppConfig = decode(&json, &self.db_path)?;
            Ok(Some(cfg))
        } else {
            Ok(None)
        }
    }

    /// Saves account-specific preferences into the encrypted database.
    pub fn save_account_preferences(
        &self,
        account_id: &AccountId,
        prefs: &AccountPreferences,
    ) -> Result<(), CoreError> {
        let json = encode(prefs, &self.db_path)?;
        self.set_setting(&format!("account_preferences:{}", account_id.0), &json)?;
        Ok(())
    }

    /// Loads account-specific preferences from the encrypted database if present.
    pub fn load_account_preferences(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<AccountPreferences>, CoreError> {
        if let Some(json) = self.get_setting(&format!("account_preferences:{}", account_id.0))? {
            let prefs: AccountPreferences = decode(&json, &self.db_path)?;
            Ok(Some(prefs))
        } else {
            Ok(None)
        }
    }

    /// Saves global application preferences into the encrypted database.
    pub fn save_global_preferences(&self, prefs: &GlobalPreferences) -> Result<(), CoreError> {
        let json = encode(prefs, &self.db_path)?;
        self.set_setting("global_preferences", &json)?;
        Ok(())
    }

    /// Loads global application preferences from the encrypted database if present.
    pub fn load_global_preferences(&self) -> Result<Option<GlobalPreferences>, CoreError> {
        if let Some(json) = self.get_setting("global_preferences")? {
            let prefs: GlobalPreferences = decode(&json, &self.db_path)?;
            Ok(Some(prefs))
        } else {
            Ok(None)
        }
    }
}

/// Serializes a value into the database, naming the database in the error.
fn encode<T: serde::Serialize>(value: &T, db_path: &Path) -> Result<String, CoreError> {
    serde_json::to_string(value).map_err(|source| CoreError::ConfigParse {
        path: db_path.to_path_buf(),
        source,
    })
}

/// Decodes a value read from the database, naming the database in the error.
fn decode<T: serde::de::DeserializeOwned>(json: &str, db_path: &Path) -> Result<T, CoreError> {
    serde_json::from_str(json).map_err(|source| CoreError::ConfigParse {
        path: db_path.to_path_buf(),
        source,
    })
}

/// Writes the storage key to `path` with owner-only permissions, never overwriting.
fn write_key_file(path: &Path, hex: &str) -> Result<(), CoreError> {
    use std::io::Write as _;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| CoreError::ConfigSave {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| CoreError::ConfigSave {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(hex.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|source| CoreError::ConfigSave {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_encrypted_desktop_storage() {
        let db_path =
            std::env::temp_dir().join(format!("test_desktop_{}.db", uuid::Uuid::new_v4()));
        let key = StoreKey::generate().unwrap();

        let storage = DesktopStorage::open(&db_path, &key).expect("opens cleanly");

        let mut config = AppConfig {
            sync_interval_secs: 120,
            ..Default::default()
        };
        config.global.default_display.show_last_modified = true;

        storage.save_app_config(&config).expect("saves config");
        let loaded = storage.load_app_config().expect("loads config").unwrap();
        assert_eq!(loaded.sync_interval_secs, 120);
        assert!(loaded.global.default_display.show_last_modified);

        let acc_id = AccountId("acc-123".to_string());
        let mut prefs = AccountPreferences::default();
        prefs.display.show_total_item_count = true;
        storage
            .save_account_preferences(&acc_id, &prefs)
            .expect("saves account prefs");

        let loaded_prefs = storage
            .load_account_preferences(&acc_id)
            .expect("loads account prefs")
            .unwrap();
        assert!(loaded_prefs.display.show_total_item_count);

        let _ = std::fs::remove_file(&db_path);
    }
}
