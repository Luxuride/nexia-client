//! Desktop synchronization coordinator unifying virtual and full sync engines.

use nextcloud_rust_sdk::{
    BasicAuth, DavClient, DavPath, Depth, PropfindBody, SecretString, ServerUrl, TransportBuilder,
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

    /// Returns a list of all configured accounts.
    pub async fn accounts(&self) -> Vec<Account> {
        self.config.read().await.accounts.clone()
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
        let _ = self.refresh_virtual_tree().await;
        Ok(())
    }

    /// Refreshes the virtual file tree for the first configured account, or resets the local virtual hierarchy if no account is configured.
    pub async fn refresh_virtual_tree(&self) -> Result<usize, SyncError> {
        let maybe_account = self.active_account().await;

        let Some(account) = maybe_account else {
            return Ok(self.vfs_engine.reset_default_hierarchy().await);
        };

        if account.is_system || account.credentials.username.is_empty() {
            return Ok(self.vfs_engine.reset_default_hierarchy().await);
        }

        let server_url = ServerUrl::parse(&account.server.server_url)
            .map_err(|err| SyncError::InvalidPath(err.to_string()))?;
        let transport = TransportBuilder::with_server(server_url).build()?;
        let secret = SecretString::new(account.credentials.secret());
        let auth = BasicAuth::new(&account.credentials.username, secret);
        let dav_client = DavClient::new(&transport, &auth);

        let root_dav = DavPath::root(&account.credentials.username)
            .map_err(|err| SyncError::InvalidPath(err.to_string()))?;

        let body = PropfindBody::all();
        let resources = dav_client
            .propfind(&root_dav, Depth::Infinity, &body)
            .await
            .map_err(SyncError::WebDav)?;

        let dav_prefix = format!("/remote.php/dav/files/{}", account.credentials.username);
        self.vfs_engine
            .populate_from_resources(&dav_prefix, &resources)
            .await
    }

    /// Performs on-demand hydration for a virtual file.
    pub async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, SyncError> {
        let maybe_account = {
            let conf = self.config.read().await;
            conf.accounts.first().cloned()
        };

        let Some(account) = maybe_account else {
            return self.vfs_engine.hydrate_local_placeholder(remote_path).await;
        };

        let server_url = ServerUrl::parse(&account.server.server_url)
            .map_err(|err| SyncError::InvalidPath(err.to_string()))?;
        let transport = TransportBuilder::with_server(server_url).build()?;
        let secret = SecretString::new(account.credentials.secret());
        let auth = BasicAuth::new(&account.credentials.username, secret);
        let dav_client = DavClient::new(&transport, &auth);

        self.vfs_engine
            .hydrate_file(&account.credentials.username, &dav_client, remote_path)
            .await
    }

    /// Dehydrates a file node, freeing local cache space.
    pub async fn dehydrate_file(&self, remote_path: &str) -> Result<(), SyncError> {
        self.vfs_engine.dehydrate_file(remote_path).await
    }

    /// Creates a new directory in the virtual tree and (if an account is configured) on the remote WebDAV server.
    pub async fn create_directory(&self, remote_path: &str) -> Result<(), SyncError> {
        let maybe_account = {
            let conf = self.config.read().await;
            conf.accounts.first().cloned()
        };

        if let Some(account) = maybe_account {
            let server_url = ServerUrl::parse(&account.server.server_url)
                .map_err(|err| SyncError::InvalidPath(err.to_string()))?;
            let transport = TransportBuilder::with_server(server_url).build()?;
            let secret = SecretString::new(account.credentials.secret());
            let auth = BasicAuth::new(&account.credentials.username, secret);
            let dav_client = DavClient::new(&transport, &auth);
            let dav_path = crate::vfs::make_dav_path(&account.credentials.username, remote_path)?;
            dav_client.mkcol(&dav_path).await?;
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
        let maybe_account = {
            let conf = self.config.read().await;
            conf.accounts.first().cloned()
        };

        if let Some(account) = maybe_account {
            let server_url = ServerUrl::parse(&account.server.server_url)
                .map_err(|err| SyncError::InvalidPath(err.to_string()))?;
            let transport = TransportBuilder::with_server(server_url).build()?;
            let secret = SecretString::new(account.credentials.secret());
            let auth = BasicAuth::new(&account.credentials.username, secret);
            let dav_client = DavClient::new(&transport, &auth);
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
        let maybe_account = {
            let conf = self.config.read().await;
            conf.accounts.first().cloned()
        };

        let clean_old = format!("/{}", old_remote_path.trim_matches('/'));
        let parent = nextcloud_rust_sdk::parent_path(&clean_old).unwrap_or_else(|| "/".to_string());
        let trimmed_name = new_name.trim().trim_matches('/');
        let clean_new = if parent == "/" {
            format!("/{trimmed_name}")
        } else {
            format!("{}/{}", parent.trim_end_matches('/'), trimmed_name)
        };

        if let Some(account) = maybe_account {
            let server_url = ServerUrl::parse(&account.server.server_url)
                .map_err(|err| SyncError::InvalidPath(err.to_string()))?;
            let transport = TransportBuilder::with_server(server_url).build()?;
            let secret = SecretString::new(account.credentials.secret());
            let auth = BasicAuth::new(&account.credentials.username, secret);
            let dav_client = DavClient::new(&transport, &auth);
            let src_dav = crate::vfs::make_dav_path(&account.credentials.username, &clean_old)?;
            let dst_dav = crate::vfs::make_dav_path(&account.credentials.username, &clean_new)?;
            dav_client.move_(&src_dav, &dst_dav, false).await?;
        }

        self.vfs_engine.rename_entry(&clean_old, new_name).await
    }

    /// Moves a file or directory into a target directory in the virtual tree and on the remote WebDAV server.
    pub async fn move_resource(
        &self,
        source_path: &str,
        destination_dir: &str,
    ) -> Result<String, SyncError> {
        let clean_src = format!("/{}", source_path.trim_matches('/'));
        let clean_dest_dir = format!("/{}", destination_dir.trim_matches('/'));
        let file_name = clean_src.rsplit('/').next().unwrap_or_default();
        if file_name.is_empty() {
            return Err(SyncError::InvalidPath(
                "invalid source item name".to_string(),
            ));
        }
        let clean_new = if clean_dest_dir == "/" {
            format!("/{file_name}")
        } else {
            format!("{}/{}", clean_dest_dir.trim_end_matches('/'), file_name)
        };

        let maybe_account = {
            let conf = self.config.read().await;
            conf.accounts.first().cloned()
        };

        if let Some(account) = maybe_account {
            let server_url = ServerUrl::parse(&account.server.server_url)
                .map_err(|err| SyncError::InvalidPath(err.to_string()))?;
            let transport = TransportBuilder::with_server(server_url).build()?;
            let secret = SecretString::new(account.credentials.secret());
            let auth = BasicAuth::new(&account.credentials.username, secret);
            let dav_client = DavClient::new(&transport, &auth);
            let src_dav = crate::vfs::make_dav_path(&account.credentials.username, &clean_src)?;
            let dst_dav = crate::vfs::make_dav_path(&account.credentials.username, &clean_new)?;
            dav_client.move_(&src_dav, &dst_dav, false).await?;
        }

        self.vfs_engine
            .move_entry(&clean_src, &clean_dest_dir)
            .await
    }

    /// Deletes a file or directory from the virtual tree and (if an account is configured) from the remote WebDAV server.
    pub async fn delete_resource(&self, remote_path: &str) -> Result<(), SyncError> {
        let maybe_account = {
            let conf = self.config.read().await;
            conf.accounts.first().cloned()
        };

        if let Some(account) = maybe_account {
            let server_url = ServerUrl::parse(&account.server.server_url)
                .map_err(|err| SyncError::InvalidPath(err.to_string()))?;
            let transport = TransportBuilder::with_server(server_url).build()?;
            let secret = SecretString::new(account.credentials.secret());
            let auth = BasicAuth::new(&account.credentials.username, secret);
            let dav_client = DavClient::new(&transport, &auth);
            let dav_path = crate::vfs::make_dav_path(&account.credentials.username, remote_path)?;
            dav_client.delete(&dav_path).await?;
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

            let server_url = ServerUrl::parse(&account.server.server_url)
                .map_err(|err| SyncError::InvalidPath(err.to_string()))?;
            let transport = TransportBuilder::with_server(server_url).build()?;
            let secret = SecretString::new(account.credentials.secret());
            let auth = BasicAuth::new(&account.credentials.username, secret);
            let dav_client = DavClient::new(&transport, &auth);

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
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].name, "System Account");
        assert!(accounts[0].is_system);

        let active = coordinator.active_account().await.expect("has active");
        assert_eq!(active.name, "System Account");

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
        assert_eq!(accounts2.len(), 2);

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
