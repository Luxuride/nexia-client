//! Desktop synchronization coordinator unifying virtual and full sync engines with nextcloud-rust-sdk Client.

use nextcloud_rust_sdk::{
    BasicAuth, Client, DavPath, Depth, PropfindBody, SecretString, ServerUrl, StoreKey,
};
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::SyncError;
use crate::full_sync::FullSyncEngine;
use crate::vfs::VirtualSyncEngine;
use nextcloud_desktop_core::account::{Account, AccountId};
use nextcloud_desktop_core::config::{AppConfig, SyncMode};
use nextcloud_desktop_core::events::EventBus;

/// Master coordinator managing virtual tree synchronization and full folder synchronizers.
#[derive(Clone)]
pub struct DesktopSyncCoordinator {
    config: Arc<tokio::sync::RwLock<AppConfig>>,
    vfs_engine: VirtualSyncEngine,
    full_sync_engine: FullSyncEngine,
    event_bus: EventBus,
    sdk_client: Arc<tokio::sync::RwLock<Option<Client>>>,
}

impl DesktopSyncCoordinator {
    /// Creates a new synchronization coordinator.
    pub fn new(
        config: Arc<tokio::sync::RwLock<AppConfig>>,
        cache_dir: PathBuf,
        event_bus: EventBus,
    ) -> Self {
        let vfs_engine = VirtualSyncEngine::new(cache_dir, event_bus.clone());
        let full_sync_engine = FullSyncEngine::new(event_bus.clone());

        Self {
            config,
            vfs_engine,
            full_sync_engine,
            event_bus,
            sdk_client: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Access the virtual sync engine.
    pub fn vfs(&self) -> &VirtualSyncEngine {
        &self.vfs_engine
    }

    /// Access the full sync engine.
    pub fn full_sync(&self) -> &FullSyncEngine {
        &self.full_sync_engine
    }

    /// Access the shared event bus.
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Returns the active `nextcloud-rust-sdk` [`Client`] handle if configured.
    pub async fn client(&self) -> Result<Option<Client>, SyncError> {
        let account = match self.active_account().await {
            Some(acc) => acc,
            None => return Ok(None),
        };

        if account.is_system || account.credentials.username.is_empty() {
            return Ok(None);
        }

        {
            let current = self.sdk_client.read().await;
            if let Some(ref c) = *current
                && c.server_url().as_str() == account.server.server_url
                && c.current_user_id().as_deref() == Some(&account.credentials.username)
            {
                return Ok(Some(c.clone()));
            }
        }

        let server_url = ServerUrl::parse(&account.server.server_url)
            .map_err(|err| SyncError::InvalidPath(err.to_string()))?;

        let cache_dir = self.vfs_engine.cache_dir();
        let auth_db = cache_dir.join(format!("auth_{}.db", account.id));
        let cache_db = cache_dir.join(format!("cache_{}.db", account.id));
        let key = StoreKey::generate().map_err(|e| SyncError::InvalidPath(e.to_string()))?;

        let secret = SecretString::new(account.credentials.secret());
        let new_client = Client::builder()
            .server_url(server_url)
            .storage_paths(auth_db, cache_db)
            .store_key(key)
            .initial_credentials(&account.credentials.username, secret)
            .build()
            .await
            .map_err(|err| SyncError::InvalidPath(err.to_string()))?;

        let mut lock = self.sdk_client.write().await;
        *lock = Some(new_client.clone());
        Ok(Some(new_client))
    }

    /// Helper resolving active account, SDK client, and auth credentials.
    async fn active_dav_context(&self) -> Option<(Account, Client, BasicAuth)> {
        let account = self.active_account().await?;
        if account.is_system || account.credentials.password.is_empty() {
            return None;
        }
        let client = self.client().await.ok()??;
        let secret = SecretString::new(account.credentials.secret());
        let auth = BasicAuth::new(&account.credentials.username, secret);
        Some((account, client, auth))
    }

    /// Returns a list of all configured and discovered desktop system accounts.
    pub async fn accounts(&self) -> Vec<Account> {
        let mut accounts = self.config.read().await.accounts.clone();

        // Discover external desktop system accounts from GNOME Online Accounts, KDE, and Nextcloud configs
        let sso = nextcloud_rust_sdk::DesktopSso::new();
        for disc in sso.discover_accounts().await {
            let host = disc.server_url.as_str().trim_end_matches('/');
            let id = format!("sso-{}-{}", host, disc.username);
            let already_present = accounts.iter().any(|a| {
                a.id.0 == id
                    || (a.server.server_url == host && a.credentials.username == disc.username)
            });
            if !already_present {
                let label = disc.display_name.unwrap_or_else(|| {
                    let domain = host
                        .trim_start_matches("https://")
                        .trim_start_matches("http://");
                    format!("{} ({domain})", disc.username)
                });
                accounts.push(Account::system_sso_account(
                    id,
                    label,
                    nextcloud_desktop_core::account::ServerConfig::new(host),
                    nextcloud_desktop_core::account::AccountCredentials::new(&disc.username, ""),
                    None,
                ));
            }
        }

        accounts
    }

    /// Returns the currently active account if configured.
    pub async fn active_account(&self) -> Option<Account> {
        let conf = self.config.read().await;
        conf.active_account_id
            .as_ref()
            .and_then(|id| conf.get_account(id).cloned())
            .or_else(|| conf.accounts.first().cloned())
    }

    /// Switches the active account profile to `account_id` and resets the virtual drive.
    pub async fn switch_account(&self, account_id: &AccountId) -> Result<(), SyncError> {
        {
            let mut conf = self.config.write().await;
            conf.active_account_id = Some(account_id.clone());
        }
        {
            let mut lock = self.sdk_client.write().await;
            *lock = None;
        }
        let _ = self.refresh_virtual_tree().await;
        Ok(())
    }

    /// Adds a new account profile, ensuring the system account remains intact.
    pub async fn add_account(&self, account: Account) -> Result<(), SyncError> {
        {
            let mut conf = self.config.write().await;
            conf.upsert_account(account.clone());
            conf.active_account_id = Some(account.id);
        }
        {
            let mut lock = self.sdk_client.write().await;
            *lock = None;
        }
        let _ = self.refresh_virtual_tree().await;
        Ok(())
    }

    /// Refreshes the virtual file tree for the active configured account, or resets to local virtual hierarchy.
    pub async fn refresh_virtual_tree(&self) -> Result<usize, SyncError> {
        let Some((account, client, auth)) = self.active_dav_context().await else {
            return Ok(self.vfs_engine.reset_default_hierarchy().await);
        };

        let dav_client = client.dav(&auth);
        let root_dav = DavPath::root(&account.credentials.username)
            .map_err(|err| SyncError::InvalidPath(err.to_string()))?;

        let body = PropfindBody::all();
        let resources = match dav_client.propfind(&root_dav, Depth::Infinity, &body).await {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(
                    "Remote WebDAV PROPFIND failed ({err}), falling back to default tree"
                );
                return Ok(self.vfs_engine.reset_default_hierarchy().await);
            }
        };

        let dav_prefix = format!("/remote.php/dav/files/{}", account.credentials.username);
        self.vfs_engine
            .populate_from_resources(&dav_prefix, &resources)
            .await
    }

    /// Performs on-demand hydration for a virtual file, downloading via SDK or using local cache.
    pub async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, SyncError> {
        let Some((account, client, auth)) = self.active_dav_context().await else {
            return self.vfs_engine.hydrate_local_placeholder(remote_path).await;
        };

        let dav_client = client.dav(&auth);
        match self
            .vfs_engine
            .hydrate_file(&account.credentials.username, &dav_client, remote_path)
            .await
        {
            Ok(path) => Ok(path),
            Err(err) => {
                tracing::warn!("Remote hydration failed ({err}), using local placeholder content");
                self.vfs_engine.hydrate_local_placeholder(remote_path).await
            }
        }
    }

    /// Dehydrates a file node, freeing local cache space.
    pub async fn dehydrate_file(&self, remote_path: &str) -> Result<(), SyncError> {
        self.vfs_engine.dehydrate_file(remote_path).await
    }

    /// Creates a new directory in the virtual tree and (if an account is configured) on the remote WebDAV server.
    pub async fn create_directory(&self, remote_path: &str) -> Result<(), SyncError> {
        if let Some((account, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            let dav_path = crate::vfs::make_dav_path(&account.credentials.username, remote_path)?;
            let _ = dav_client.mkcol(&dav_path).await;
        }

        self.vfs_engine.create_directory(remote_path).await
    }

    /// Creates a new file in the virtual tree and uploads it if an account is configured.
    pub async fn create_file(
        &self,
        remote_path: &str,
        content: &[u8],
    ) -> Result<String, SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        if let Some((account, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            let dav_path = crate::vfs::make_dav_path(&account.credentials.username, &clean_path)?;
            let _ = dav_client.put(&dav_path, content.to_vec(), None).await;
        }

        self.vfs_engine.create_file(&clean_path, content).await
    }

    /// Renames a file or directory in the virtual tree and (if an account is configured) on the remote WebDAV server via MOVE.
    pub async fn rename_resource(
        &self,
        old_remote_path: &str,
        new_name: &str,
    ) -> Result<String, SyncError> {
        let clean_old = format!("/{}", old_remote_path.trim_matches('/'));
        let parent = nextcloud_rust_sdk::parent_path(&clean_old).unwrap_or_else(|| "/".to_string());
        let trimmed_name = new_name.trim().trim_matches('/');
        let clean_new = if parent == "/" {
            format!("/{trimmed_name}")
        } else {
            format!("{}/{}", parent.trim_end_matches('/'), trimmed_name)
        };

        if let Some((account, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            let src_dav = crate::vfs::make_dav_path(&account.credentials.username, &clean_old)?;
            let dst_dav = crate::vfs::make_dav_path(&account.credentials.username, &clean_new)?;
            let _ = dav_client.move_(&src_dav, &dst_dav, false).await;
        }

        self.vfs_engine.rename_entry(&clean_old, &clean_new).await?;
        Ok(clean_new)
    }

    /// Moves a file or directory into a target folder in the virtual tree and on the remote WebDAV server.
    pub async fn move_resource(
        &self,
        source_path: &str,
        destination_dir: &str,
    ) -> Result<String, SyncError> {
        let clean_src = format!("/{}", source_path.trim_matches('/'));
        let clean_dest_dir = format!("/{}", destination_dir.trim_matches('/'));

        let file_name = clean_src.rsplit('/').next().unwrap_or("file");
        let clean_new = if clean_dest_dir == "/" {
            format!("/{file_name}")
        } else {
            format!("{}/{}", clean_dest_dir.trim_end_matches('/'), file_name)
        };

        if let Some((account, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            let src_dav = crate::vfs::make_dav_path(&account.credentials.username, &clean_src)?;
            let dst_dav = crate::vfs::make_dav_path(&account.credentials.username, &clean_new)?;
            let _ = dav_client.move_(&src_dav, &dst_dav, false).await;
        }

        self.vfs_engine
            .move_entry(&clean_src, &clean_dest_dir)
            .await
    }

    /// Deletes a file or directory from the virtual tree and (if an account is configured) from the remote WebDAV server.
    pub async fn delete_resource(&self, remote_path: &str) -> Result<(), SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        if let Some((account, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            let dav_path = crate::vfs::make_dav_path(&account.credentials.username, &clean_path)?;
            let _ = dav_client.delete(&dav_path).await;
        }

        self.vfs_engine.remove_entry(remote_path).await
    }

    /// Runs synchronization on all enabled two-way sync folder pairs.
    pub async fn sync_all_pairs(&self) -> Result<(), SyncError> {
        let (pairs, accounts, cache_dir) = {
            let conf = self.config.read().await;
            (
                conf.sync_folders.clone(),
                conf.accounts.clone(),
                conf.cache_dir
                    .clone()
                    .unwrap_or_else(|| std::env::temp_dir().join("nextcloud-desktop")),
            )
        };

        for pair in pairs {
            if !pair.enabled || pair.mode != SyncMode::FullTwoWaySync {
                continue;
            }

            let Some(account) = accounts.iter().find(|acc| acc.id == pair.account_id) else {
                continue;
            };

            let client = match self.client().await {
                Ok(Some(c)) => c,
                _ => continue,
            };

            let secret = SecretString::new(account.credentials.secret());
            let auth = BasicAuth::new(&account.credentials.username, secret);
            let dav_client = client.dav(&auth);

            let dav_prefix = format!("/remote.php/dav/files/{}", account.credentials.username);
            let journal_path = cache_dir.join(format!("journal_{}.json", pair.id));

            let _ = self
                .full_sync_engine
                .run_sync(
                    &pair,
                    &account.credentials.username,
                    &dav_client,
                    &dav_prefix,
                    &journal_path,
                )
                .await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nextcloud_desktop_core::account::{AccountCredentials, ServerConfig};

    #[tokio::test]
    async fn account_management_and_system_account() {
        let mut config = AppConfig::default();
        config.ensure_system_account();
        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let temp_dir = std::env::temp_dir().join(format!("test_coord_{}", std::process::id()));
        let coordinator =
            DesktopSyncCoordinator::new(config_lock, temp_dir.clone(), EventBus::new(100));

        let accounts = coordinator.accounts().await;
        assert!(!accounts.is_empty());
        assert!(accounts[0].is_system);
        let initial_count = accounts.len();

        let active = coordinator.active_account().await.expect("has active");
        assert!(active.is_system);

        let new_acc = Account::new(
            "Alice Nextcloud",
            ServerConfig::new("https://cloud.alice.org"),
            AccountCredentials::new("alice", "token123"),
        );
        let alice_id = new_acc.id.clone();
        coordinator
            .add_account(new_acc)
            .await
            .expect("add succeeds");

        let accounts2 = coordinator.accounts().await;
        assert_eq!(accounts2.len(), initial_count + 1);

        let active2 = coordinator.active_account().await.expect("has active");
        assert_eq!(active2.id, alice_id);

        let sys_id = Account::system_account().id;
        coordinator
            .switch_account(&sys_id)
            .await
            .expect("switch succeeds");
        let active3 = coordinator.active_account().await.expect("has active");
        assert_eq!(active3.id, sys_id);

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
