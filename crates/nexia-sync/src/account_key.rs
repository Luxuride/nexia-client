//! Per-account encryption keys for the SDK's account databases.
//!
//! Every account owns a set of SQLCipher databases (`auth.db`, `cache.db`, `config.db`). Their key
//! is *stored* — in the platform keychain, with a `0600` file next to the databases as the fallback
//! when no keyring is available — rather than derived from the account password. A derived key is
//! only as strong as the password, is identical for every account that shares a password, and
//! cannot be rotated without re-encrypting the databases.
//!
//! Accounts created before this scheme have databases but no stored key. Those fall back to the
//! legacy password-derived key once, and it is then stored like any other: minting a random key
//! instead would make the existing databases undecryptable.

use std::path::Path;

use nexia_nextcloud_sdk::{KeyStore, KeychainKeyStore, StoreKey};

use crate::error::SyncError;

/// Keychain application id, matching the one the configuration database uses.
const KEYCHAIN_APPLICATION: &str = "eu.luxuride.nexia";

/// Fallback key file, beside the account's databases.
const KEY_FILE_NAME: &str = ".store_key";

/// Whether this build uses the platform keychain for account keys.
///
/// The in-crate test suite must not touch a real keyring — reading one can prompt the user and
/// writing one leaves state behind — so tests exercise the key file path only.
const USE_KEYCHAIN: bool = !cfg!(test);

/// Database files whose presence means the account was opened by an older build.
const ACCOUNT_DATABASES: [&str; 3] = ["auth.db", "cache.db", "config.db"];

/// Returns the database key for `account_id`, minting and storing one when the account is new.
///
/// `account_secret` is only read for a legacy account, whose databases were keyed from it.
pub(crate) async fn account_db_key(
    account_dir: &Path,
    account_id: &str,
    account_secret: &str,
) -> Result<StoreKey, SyncError> {
    let key_file = account_dir.join(KEY_FILE_NAME);

    if let Some(key) = load_stored_key(account_id, &key_file).await? {
        return Ok(key);
    }

    let key = if has_databases(account_dir) {
        tracing::debug!(
            account = account_id,
            "adopting the legacy password-derived key of an existing account"
        );
        legacy_derived_key(account_id, account_secret)?
    } else {
        StoreKey::generate().map_err(|source| SyncError::StoreKey {
            account: account_id.to_string(),
            source,
        })?
    };

    store_key(account_id, &key_file, &key).await?;
    Ok(key)
}

/// Returns whether any of the account's databases already exists.
fn has_databases(account_dir: &Path) -> bool {
    ACCOUNT_DATABASES
        .iter()
        .any(|name| account_dir.join(name).exists())
}

/// Reads the key from the keychain, then from the fallback file.
async fn load_stored_key(account_id: &str, key_file: &Path) -> Result<Option<StoreKey>, SyncError> {
    if USE_KEYCHAIN {
        let keystore = KeychainKeyStore::new(KEYCHAIN_APPLICATION, keychain_account(account_id));
        match keystore.load().await {
            Ok(Some(key)) => return Ok(Some(key)),
            Ok(None) => {}
            Err(source) => {
                tracing::debug!(
                    account = account_id,
                    error = %source,
                    "no keychain entry for the account database key; falling back to the key file"
                );
            }
        }
    }

    read_key_file(key_file)
}

/// Stores the key in the keychain and in the fallback file.
///
/// A keyring failure is not fatal: the file is the fallback that keeps the account openable, and
/// the next start retries the keychain.
async fn store_key(account_id: &str, key_file: &Path, key: &StoreKey) -> Result<(), SyncError> {
    if USE_KEYCHAIN {
        let keystore = KeychainKeyStore::new(KEYCHAIN_APPLICATION, keychain_account(account_id));
        if let Err(source) = keystore.store(key.clone()).await {
            tracing::warn!(
                account = account_id,
                error = %source,
                "could not store the account database key in the keychain"
            );
        }
    }

    write_key_file(key_file, key)
}

/// Keychain account label for one account's key.
fn keychain_account(account_id: &str) -> String {
    format!("account-key-{account_id}")
}

/// Reads a key file, treating a missing or blank file as "no key".
fn read_key_file(path: &Path) -> Result<Option<StoreKey>, SyncError> {
    let hex = match std::fs::read_to_string(path) {
        Ok(hex) => hex,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(SyncError::LocalIo {
                path: path.to_path_buf(),
                context: "reading the account database key file".to_string(),
                source,
            });
        }
    };

    let hex = hex.trim();
    if hex.is_empty() {
        return Ok(None);
    }

    StoreKey::from_hex(hex, path.display().to_string())
        .map(Some)
        .map_err(|source| SyncError::StoreKey {
            account: path.display().to_string(),
            source,
        })
}

/// Writes `key` to `key_file` with owner-only permissions.
///
/// The file is created exclusively; if it already appeared, its content wins so two processes
/// opening the same account do not end up with different keys for one database.
fn write_key_file(path: &Path, key: &StoreKey) -> Result<(), SyncError> {
    let hex = key.to_hex();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| SyncError::LocalIo {
            path: parent.to_path_buf(),
            context: "creating the account cache directory".to_string(),
            source,
        })?;
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    match options.open(path) {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(hex.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|source| SyncError::LocalIo {
                    path: path.to_path_buf(),
                    context: "writing the account database key file".to_string(),
                    source,
                })
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            if read_key_file(path)?.is_none() {
                tracing::warn!(
                    path = %path.display(),
                    "account key file exists but holds no usable key"
                );
            }
            Ok(())
        }
        Err(source) => Err(SyncError::LocalIo {
            path: path.to_path_buf(),
            context: "creating the account database key file".to_string(),
            source,
        }),
    }
}

/// Derives the key an older build used for `account_id`.
///
/// Kept only to adopt existing databases: it is a byte fold of `account id:secret`, not a KDF, so
/// it offers none of the properties a real derivation would (uniqueness per account, resistance to
/// brute force, rotatability).
fn legacy_derived_key(account_id: &str, account_secret: &str) -> Result<StoreKey, SyncError> {
    let mut key_bytes = [0u8; 32];
    let seed = format!("{account_id}:{account_secret}");
    for (i, byte) in seed.as_bytes().iter().cycle().take(32).enumerate() {
        key_bytes[i] = byte ^ (i as u8).wrapping_mul(31);
    }

    StoreKey::from_bytes(key_bytes).map_err(|source| SyncError::StoreKey {
        account: account_id.to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_account_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nexia-account-key-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create account dir");
        dir
    }

    #[tokio::test]
    async fn new_account_gets_a_stored_key_that_is_reused() {
        let dir = temp_account_dir("new");

        let first = account_db_key(&dir, "acc-1", "secret")
            .await
            .expect("mint key");
        let second = account_db_key(&dir, "acc-1", "secret")
            .await
            .expect("load key");

        assert_eq!(first.to_hex(), second.to_hex());
        assert!(dir.join(KEY_FILE_NAME).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn existing_databases_adopt_the_legacy_derived_key() {
        let dir = temp_account_dir("legacy");
        std::fs::write(dir.join("config.db"), b"ciphertext").expect("seed database");

        let key = account_db_key(&dir, "acc-legacy", "secret")
            .await
            .expect("adopt legacy key");
        let expected = legacy_derived_key("acc-legacy", "secret").expect("derive");

        assert_eq!(key.to_hex(), expected.to_hex());
        // The adopted key is stored, so the password is not needed on the next start.
        let reloaded = account_db_key(&dir, "acc-legacy", "")
            .await
            .expect("load adopted key");
        assert_eq!(reloaded.to_hex(), expected.to_hex());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_blank_key_file_counts_as_absent() {
        let dir = temp_account_dir("blank");
        let key_file = dir.join(KEY_FILE_NAME);
        std::fs::write(&key_file, "   \n").expect("write blank file");

        assert!(read_key_file(&key_file).expect("read").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
