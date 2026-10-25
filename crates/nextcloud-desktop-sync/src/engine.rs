//! Desktop synchronization coordinator unifying virtual and full sync engines with nextcloud-rust-sdk Client.

use nextcloud_rust_sdk::{
    BasicAuth, Client, DavPath, Depth, MountDescriptor, MountId, MountMode, MountState,
    MountsHandle, NextcloudOrchestrator, OrchestratorConfig, OrchestratorError, OrchestratorHandle,
    PropfindBody, SecretString, ServerUrl, StoreKey, SyncFileStatus,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::SyncError;
use crate::full_sync::FullSyncEngine;
use crate::vfs::VirtualSyncEngine;
use nextcloud_desktop_core::account::{Account, AccountId, ServerConfig};
use nextcloud_desktop_core::config::{AppConfig, SyncMode};
use nextcloud_desktop_core::events::EventBus;

/// Master coordinator managing virtual tree synchronization and full folder synchronizers.
#[derive(Clone)]
pub struct DesktopSyncCoordinator {
    config: Arc<tokio::sync::RwLock<AppConfig>>,
    config_path: Option<PathBuf>,
    event_bus: EventBus,
    sdk_clients: Arc<tokio::sync::RwLock<HashMap<AccountId, Client>>>,
    orchestrator: Arc<std::sync::Mutex<Option<NextcloudOrchestrator>>>,
    orchestrator_handle: Arc<tokio::sync::RwLock<Option<OrchestratorHandle>>>,
    default_mount_id: Arc<tokio::sync::RwLock<Option<MountId>>>,
}

impl DesktopSyncCoordinator {
    /// Creates a new synchronization coordinator.
    pub fn new(config: Arc<tokio::sync::RwLock<AppConfig>>, event_bus: EventBus) -> Self {
        let cache_dir = if let Ok(conf) = config.try_read() {
            conf.resolved_cache_dir()
        } else {
            AppConfig::default_cache_dir()
        };

        let orch_config = OrchestratorConfig::new(cache_dir);
        let orch = NextcloudOrchestrator::start(orch_config).ok();
        let handle = orch.as_ref().map(|o| o.handle());

        Self {
            config,
            config_path: None,
            event_bus,
            sdk_clients: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            orchestrator: Arc::new(std::sync::Mutex::new(orch)),
            orchestrator_handle: Arc::new(tokio::sync::RwLock::new(handle)),
            default_mount_id: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Attaches a configuration file path for persistent state storage.
    #[must_use]
    pub fn with_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    /// Access the base cache directory path.
    pub fn cache_dir(&self) -> PathBuf {
        if let Ok(conf) = self.config.try_read() {
            return conf.resolved_cache_dir();
        }
        AppConfig::default_cache_dir()
    }

    /// Returns `true` if the background orchestrator actor thread is active.
    pub fn is_orchestrator_running(&self) -> bool {
        self.orchestrator
            .lock()
            .map(|o| o.is_some())
            .unwrap_or(false)
    }

    /// Access the virtual sync engine for the active account.
    pub async fn active_vfs(&self) -> VirtualSyncEngine {
        let active_id = self
            .active_account()
            .await
            .map(|a| a.id)
            .unwrap_or_else(|| AccountId("default".to_string()));

        let account_vfs_dir = self
            .cache_dir()
            .join("accounts")
            .join(&active_id.0)
            .join("vfs");
        let _ = std::fs::create_dir_all(&account_vfs_dir);
        let vfs = VirtualSyncEngine::new(account_vfs_dir.clone(), self.event_bus.clone());

        if let Some(ref handle) = *self.orchestrator_handle.read().await {
            let mut mount_lock = self.default_mount_id.write().await;
            let mount_id = if let Some(ref id) = *mount_lock {
                id.clone()
            } else {
                let desc =
                    MountDescriptor::new(&active_id.0, "/", account_vfs_dir, MountMode::VirtualVfs);
                if let Ok(id) = handle.mounts().add_mount(desc).await {
                    *mount_lock = Some(id.clone());
                    id
                } else {
                    MountId::new()
                }
            };
            return vfs.with_vfs_handle(handle.vfs(mount_id));
        }

        vfs
    }

    /// Access the mounts management interface.
    pub async fn mounts(&self) -> Option<MountsHandle> {
        self.orchestrator_handle
            .read()
            .await
            .as_ref()
            .map(|h| h.mounts())
    }

    /// Adds a new mount point to the orchestrator.
    pub async fn add_mount(&self, descriptor: MountDescriptor) -> Result<MountId, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .add_mount(descriptor)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Err(SyncError::Orchestrator(OrchestratorError::WorkerShutdown(
                "orchestrator not initialized".to_string(),
            )))
        }
    }

    /// Removes an existing mount point.
    pub async fn remove_mount(&self, id: MountId) -> Result<MountDescriptor, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .remove_mount(id)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Err(SyncError::Orchestrator(OrchestratorError::WorkerShutdown(
                "orchestrator not initialized".to_string(),
            )))
        }
    }

    /// Lists all active mounts and their runtime states.
    pub async fn list_mounts(&self) -> Result<Vec<(MountDescriptor, MountState)>, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .list_mounts()
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(Vec::new())
        }
    }

    /// Queries the file sync status and FreeDesktop icon name for a relative path in a mount.
    pub async fn file_status(
        &self,
        mount_id: MountId,
        path: &str,
    ) -> Result<SyncFileStatus, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .file_status(mount_id, path)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(SyncFileStatus::OnlineOnly)
        }
    }

    /// Resolves the synchronization status for an arbitrary local path across all mounts.
    pub async fn status_for_local_path(
        &self,
        local_path: impl Into<PathBuf>,
    ) -> Result<Option<SyncFileStatus>, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .status_for_local_path(local_path)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(None)
        }
    }

    /// Specifically pauses synchronization for a single mount point.
    pub async fn pause_mount(&self, id: MountId) -> Result<(), SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .pause_mount(id)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(())
        }
    }

    /// Specifically resumes synchronization for a single mount point.
    pub async fn resume_mount(&self, id: MountId) -> Result<(), SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .resume_mount(id)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(())
        }
    }

    /// Queries whether a specific mount point is currently paused.
    pub async fn is_mount_paused(&self, id: MountId) -> Result<bool, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.mounts()
                .is_paused(id)
                .await
                .map_err(SyncError::Orchestrator)
        } else {
            Ok(false)
        }
    }

    /// Globally pauses synchronization across all mounts and background queues.
    pub async fn pause_all_sync(&self) -> Result<(), SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.pause_all_sync().await.map_err(SyncError::Orchestrator)
        } else {
            Ok(())
        }
    }

    /// Globally resumes synchronization across all mounts and background queues.
    pub async fn resume_all_sync(&self) -> Result<(), SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.resume_all_sync().await.map_err(SyncError::Orchestrator)
        } else {
            Ok(())
        }
    }

    /// Queries whether synchronization is globally paused.
    pub async fn is_sync_paused(&self) -> Result<bool, SyncError> {
        let handle = self.orchestrator_handle.read().await;
        if let Some(ref h) = *handle {
            h.is_sync_paused().await.map_err(SyncError::Orchestrator)
        } else {
            Ok(false)
        }
    }

    /// Access the full sync engine.
    pub fn full_sync(&self) -> FullSyncEngine {
        FullSyncEngine::new(self.event_bus.clone())
    }

    /// Access the shared event bus.
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Returns an SDK [`Client`] instance configured with per-account storage and persistent encryption key.
    pub async fn client_for_account(&self, account: &Account) -> Result<Option<Client>, SyncError> {
        if account.is_system || account.credentials.username.is_empty() {
            return Ok(None);
        }

        {
            let clients = self.sdk_clients.read().await;
            if let Some(client) = clients.get(&account.id) {
                return Ok(Some(client.clone()));
            }
        }

        let cache_dir = self.cache_dir();
        let account_dir = cache_dir.join("accounts").join(&account.id.0);
        let _ = std::fs::create_dir_all(&account_dir);

        let auth_db = account_dir.join("auth.db");
        let cache_db = account_dir.join("cache.db");
        let key_file = account_dir.join("store.key");

        let key = if key_file.exists() {
            let hex = std::fs::read_to_string(&key_file)
                .map_err(|e| SyncError::InvalidPath(e.to_string()))?;
            StoreKey::from_hex(hex.trim(), &account.id.0)
                .map_err(|e| SyncError::InvalidPath(e.to_string()))?
        } else {
            let generated =
                StoreKey::generate().map_err(|e| SyncError::InvalidPath(e.to_string()))?;
            let _ = std::fs::write(&key_file, generated.to_hex());
            generated
        };

        let server_url = ServerUrl::parse(&account.server.server_url)
            .map_err(|err| SyncError::InvalidPath(err.to_string()))?;

        let secret = SecretString::new(account.credentials.secret());
        let new_client = Client::builder()
            .server_url(server_url)
            .storage_paths(auth_db, cache_db)
            .store_key(key)
            .initial_credentials(&account.credentials.username, secret)
            .build()
            .await
            .map_err(|err| SyncError::InvalidPath(err.to_string()))?;

        let mut clients = self.sdk_clients.write().await;
        clients.insert(account.id.clone(), new_client.clone());
        Ok(Some(new_client))
    }

    /// Returns the active `nextcloud-rust-sdk` [`Client`] handle if configured.
    pub async fn client(&self) -> Result<Option<Client>, SyncError> {
        let account = match self.active_account().await {
            Some(acc) => acc,
            None => return Ok(None),
        };
        self.client_for_account(&account).await
    }

    /// Helper resolving active account, SDK client, and auth credentials.
    async fn active_dav_context(&self) -> Option<(Account, Client, BasicAuth)> {
        let account = self.active_account().await?;
        if account.is_system || account.credentials.password.is_empty() {
            return None;
        }
        let client = self.client_for_account(&account).await.ok()??;
        let secret = SecretString::new(account.credentials.secret());
        let auth = BasicAuth::new(&account.credentials.username, secret);
        Some((account, client, auth))
    }

    /// Discovers external desktop system accounts (SSO), resolves credentials, and synchronizes configuration.
    pub async fn sync_discovered_accounts(&self) {
        let sso = nextcloud_rust_sdk::DesktopSso::new();
        let keyring = nextcloud_rust_sdk::PlatformKeyring::new();
        let discovered = sso.discover_accounts().await;

        let (_, _, host_avatar) = Account::detect_host_user_info();

        let mut system_accounts = Vec::new();
        for disc in discovered {
            let clean_server = ServerConfig::new(disc.server_url.as_str()).server_url;
            let id = format!("sso-{clean_server}-{}", disc.username);

            let label = disc.display_name.clone().unwrap_or_else(|| {
                let domain = clean_server
                    .trim_start_matches("https://")
                    .trim_start_matches("http://");
                format!("{} ({domain})", disc.username)
            });

            let password = sso
                .resolve_credentials(&disc, &keyring)
                .await
                .ok()
                .flatten()
                .map(|p| p.into_inner())
                .unwrap_or_default();

            system_accounts.push(Account::system_sso_account(
                id,
                label,
                ServerConfig::new(&clean_server),
                nextcloud_desktop_core::account::AccountCredentials::new(&disc.username, password),
                host_avatar.clone(),
            ));
        }

        {
            let mut conf = self.config.write().await;
            conf.sync_system_accounts(system_accounts);
            if let Some(ref path) = self.config_path {
                let _ = conf.save_to_path(path);
            }
        }
    }

    /// Returns a list of all configured desktop accounts.
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
            if !conf.accounts.iter().any(|a| &a.id == account_id) {
                return Err(SyncError::NotFound(format!(
                    "Account {} not found",
                    account_id.0
                )));
            }
            conf.active_account_id = Some(account_id.clone());
            if let Some(ref path) = self.config_path {
                let _ = conf.save_to_path(path);
            }
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
            if let Some(ref path) = self.config_path {
                let _ = conf.save_to_path(path);
            }
        }

        let _ = self.refresh_virtual_tree().await;
        Ok(())
    }

    /// Refreshes the virtual file tree for the active configured account, or resets to local virtual hierarchy.
    pub async fn refresh_virtual_tree(&self) -> Result<usize, SyncError> {
        let vfs = self.active_vfs().await;

        let Some((account, client, auth)) = self.active_dav_context().await else {
            return Ok(vfs.reset_default_hierarchy().await);
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
                return Ok(vfs.reset_default_hierarchy().await);
            }
        };

        let dav_prefix = format!("/remote.php/dav/files/{}", account.credentials.username);
        vfs.populate_from_resources(&dav_prefix, &resources).await
    }

    /// Performs on-demand hydration for a virtual file, downloading via SDK or using local cache.
    pub async fn hydrate_file(&self, remote_path: &str) -> Result<PathBuf, SyncError> {
        let vfs = self.active_vfs().await;

        let Some((account, client, auth)) = self.active_dav_context().await else {
            return vfs.hydrate_local_placeholder(remote_path).await;
        };

        let dav_client = client.dav(&auth);
        match vfs
            .hydrate_file(&account.credentials.username, &dav_client, remote_path)
            .await
        {
            Ok(path) => Ok(path),
            Err(err) => {
                tracing::warn!("Remote hydration failed ({err}), using local placeholder content");
                vfs.hydrate_local_placeholder(remote_path).await
            }
        }
    }

    /// Dehydrates a file node, freeing local cache space.
    pub async fn dehydrate_file(&self, remote_path: &str) -> Result<(), SyncError> {
        self.active_vfs().await.dehydrate_file(remote_path).await
    }

    /// Creates a new directory in the virtual tree and (if an account is configured) on the remote WebDAV server.
    pub async fn create_directory(&self, remote_path: &str) -> Result<(), SyncError> {
        if let Some((account, client, auth)) = self.active_dav_context().await {
            let dav_client = client.dav(&auth);
            let dav_path = crate::vfs::make_dav_path(&account.credentials.username, remote_path)?;
            let _ = dav_client.mkcol(&dav_path).await;
        }

        self.active_vfs().await.create_directory(remote_path).await
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

        self.active_vfs()
            .await
            .create_file(&clean_path, content)
            .await
    }

    /// Renames a file or directory in the virtual tree and on the remote WebDAV server via MOVE.
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

        self.active_vfs()
            .await
            .rename_entry(&clean_old, &clean_new)
            .await?;
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

        self.active_vfs()
            .await
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

        self.active_vfs().await.remove_entry(remote_path).await
    }

    /// Creates or retrieves a public link share for a remote path using Nextcloud OCS Sharing API.
    pub async fn create_public_share(&self, remote_path: &str) -> Result<String, SyncError> {
        let clean_path = format!("/{}", remote_path.trim_matches('/'));

        if let Some((account, client, auth)) = self.active_dav_context().await {
            let shares_client = client.shares(&auth);
            match shares_client.get_or_create_public_link(&clean_path).await {
                Ok(share) => {
                    if let Some(url) = share
                        .public_url(&account.server.server_url)
                        .or_else(|| share.url().map(|s| s.to_string()))
                    {
                        return Ok(url);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed to create public share via OCS API ({err}), falling back to direct server link"
                    );
                }
            }

            let base = account.server.server_url.trim_end_matches('/');
            let encoded_path = clean_path.trim_start_matches('/');
            Ok(format!("{base}/index.php/apps/files/?dir=/{encoded_path}"))
        } else {
            let server_url = self
                .active_account()
                .await
                .map(|a| a.server.server_url)
                .unwrap_or_else(|| "https://cloud.nextcloud.com".to_string());
            let base = server_url.trim_end_matches('/');
            Ok(format!(
                "{base}/index.php/apps/files/{}",
                clean_path.trim_start_matches('/')
            ))
        }
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

            let client = match self.client_for_account(account).await {
                Ok(Some(c)) => c,
                _ => continue,
            };

            let secret = SecretString::new(account.credentials.secret());
            let auth = BasicAuth::new(&account.credentials.username, secret);
            let dav_client = client.dav(&auth);

            let dav_prefix = format!("/remote.php/dav/files/{}", account.credentials.username);
            let journal_path = cache_dir.join(format!("journal_{}.json", pair.id));

            let sync_engine = FullSyncEngine::new(self.event_bus.clone());
            let _ = sync_engine
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
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));

        coordinator.sync_discovered_accounts().await;

        let accounts = coordinator.accounts().await;
        assert!(!accounts.is_empty());
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

        let first_id = accounts[0].id.clone();
        coordinator
            .switch_account(&first_id)
            .await
            .expect("switch succeeds");
        let active3 = coordinator.active_account().await.expect("has active");
        assert_eq!(active3.id, first_id);
    }

    #[tokio::test]
    async fn orchestrator_mounts_pause_resume_and_file_status() {
        let temp_dir = std::env::temp_dir().join(format!("nc_test_orch_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let mut config = AppConfig::default();
        config.cache_dir = Some(temp_dir.clone());
        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));

        assert!(coordinator.is_orchestrator_running());

        // Add root mount
        let target_root = temp_dir.join("nextcloud");
        let desc_root = MountDescriptor::new("acc1", "/", &target_root, MountMode::VirtualVfs);
        let root_id = coordinator
            .add_mount(desc_root)
            .await
            .expect("add root mount");

        // Add subdirectory mount to distinct target
        let target_photos = temp_dir.join("photos");
        let desc_photos = MountDescriptor::new(
            "acc1",
            "/Photos",
            &target_photos,
            MountMode::BidirectionalSync,
        );
        let photos_id = coordinator
            .add_mount(desc_photos)
            .await
            .expect("add photos mount");

        let mounts = coordinator.list_mounts().await.expect("list mounts");
        assert_eq!(mounts.len(), 2);

        // Pause specific mount
        assert!(
            !coordinator
                .is_mount_paused(photos_id.clone())
                .await
                .unwrap()
        );
        coordinator
            .pause_mount(photos_id.clone())
            .await
            .expect("pause mount");
        assert!(
            coordinator
                .is_mount_paused(photos_id.clone())
                .await
                .unwrap()
        );
        coordinator
            .resume_mount(photos_id.clone())
            .await
            .expect("resume mount");
        assert!(
            !coordinator
                .is_mount_paused(photos_id.clone())
                .await
                .unwrap()
        );

        // Pause globally
        assert!(!coordinator.is_sync_paused().await.unwrap());
        coordinator.pause_all_sync().await.expect("pause all sync");
        assert!(coordinator.is_sync_paused().await.unwrap());
        coordinator
            .resume_all_sync()
            .await
            .expect("resume all sync");
        assert!(!coordinator.is_sync_paused().await.unwrap());

        // File status and icons
        let status = coordinator
            .file_status(root_id, "/Document.txt")
            .await
            .expect("file status");
        assert_eq!(status.icon_name(), "cloud-symbolic");

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
