//! Encrypted SQLite storage for Nexia Client settings and configuration.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nexia_nextcloud_sdk::{ConfigStore, KeyStore, KeychainKeyStore, StoreKey};

use crate::account::AccountId;
use crate::config::{AccountPreferences, AppConfig, GlobalPreferences};
use crate::error::CoreError;

/// Keychain application identifier owning the desktop storage key.
const KEYCHAIN_APPLICATION: &str = "eu.luxuride.nexia";

/// Keychain account label of the desktop storage key.
const KEYCHAIN_ACCOUNT: &str = "desktop-storage-key";

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

    /// Opens the default encrypted database in the user data directory.
    ///
    /// Key resolution never invents a key for a database that already exists. A key is only
    /// accepted once it has actually opened the database, because a wrong one would leave
    /// every stored credential unreachable while the application happily starts over.
    pub async fn open_default() -> Result<Self, CoreError> {
        let data_dir = dirs::data_dir().ok_or(CoreError::MissingDataDir)?;
        let db_dir = data_dir.join("nexia");
        let db_path = db_dir.join("desktop_config.db");
        let key_file = db_dir.join(".storage_key");

        let keystore = KeychainKeyStore::new(KEYCHAIN_APPLICATION, KEYCHAIN_ACCOUNT);

        if db_path.exists() {
            return Self::open_existing(db_path, &key_file, &keystore).await;
        }

        // No database yet, so a freshly minted key can orphan nothing: prefer the keychain,
        // and fall back to the key file when no keychain is available (headless launches).
        match keystore.load_or_create().await {
            Ok(key) => return Self::open(db_path, &key),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Platform keychain unavailable, using a local storage key file"
                );
            }
        }
        let key = match read_key_file(&key_file)? {
            Some(key) => key,
            None => store_key_file(&key_file, &StoreKey::generate()?)?,
        };
        Self::open(db_path, &key)
    }

    /// Opens an existing database with whichever known key actually unlocks it.
    async fn open_existing(
        db_path: PathBuf,
        key_file: &Path,
        keystore: &KeychainKeyStore,
    ) -> Result<Self, CoreError> {
        let keychain_key = match keystore.load().await {
            Ok(key) => key,
            Err(error) => {
                tracing::warn!(error = %error, "Could not read the storage key from the keychain");
                None
            }
        };
        let file_key = read_key_file(key_file)?;

        for (origin, key) in [("keychain", keychain_key), ("key file", file_key)]
            .into_iter()
            .filter_map(|(origin, key)| key.map(|key| (origin, key)))
        {
            match Self::open(&db_path, &key) {
                Ok(storage) => return Ok(storage),
                Err(error) => tracing::warn!(
                    origin,
                    error = %error,
                    "Storage key does not unlock the configuration database"
                ),
            }
        }

        Err(CoreError::StorageKeyUnusable { path: db_path })
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

/// Reads the storage key file, if it holds a complete key.
///
/// A zero-length file is what a crash between creating and writing the key leaves behind.
/// It cannot decrypt anything, so it is reported as absent rather than as a permanent,
/// unrecoverable error on every later start.
fn read_key_file(path: &Path) -> Result<Option<StoreKey>, CoreError> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(CoreError::ConfigLoad {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let hex = content.trim();
    if hex.is_empty() {
        return Ok(None);
    }
    StoreKey::from_hex(hex, KEYCHAIN_ACCOUNT)
        .map(Some)
        .map_err(|source| CoreError::StorageKey {
            path: path.to_path_buf(),
            source,
        })
}

/// Writes a freshly minted storage key to `path` with owner-only permissions.
///
/// The file is created exclusively, so a concurrent start cannot overwrite a key another
/// process is about to protect its database with; the loser adopts the winner's key.
fn store_key_file(path: &Path, key: &StoreKey) -> Result<StoreKey, CoreError> {
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
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            return read_key_file(path)?.ok_or(CoreError::ConfigSave {
                path: path.to_path_buf(),
                source,
            });
        }
        Err(source) => {
            return Err(CoreError::ConfigSave {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    file.write_all(key.to_hex().as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|source| {
            let _ = std::fs::remove_file(path);
            CoreError::ConfigSave {
                path: path.to_path_buf(),
                source,
            }
        })?;
    Ok(key.clone())
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
