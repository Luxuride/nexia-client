//! Account management backed directly by a list of Nextcloud [`Client`]s.

use nextcloud_rust_sdk::{BasicAuth, Client, SecretString, ServerUrl};

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use nextcloud_desktop_core::account::{Account, AccountId};

impl DesktopSyncCoordinator {
    /// Returns an SDK [`Client`] instance for the specified account.
    pub async fn client_for_account(&self, account: &Account) -> Result<Option<Client>, SyncError> {
        let clients = self.clients.read().await;
        for c in clients.iter() {
            if c.server_url().as_str() == account.server.server_url
                && c.current_user_id().as_deref() == Some(&account.credentials.username)
            {
                return Ok(Some(c.clone()));
            }
        }
        Ok(self.active_client().await)
    }

    /// Returns the active `nextcloud-rust-sdk` [`Client`] handle if configured.
    pub async fn client(&self) -> Result<Option<Client>, SyncError> {
        Ok(self.active_client().await)
    }

    /// Helper resolving active user name, SDK client, and auth credentials.
    pub(crate) async fn active_dav_context(&self) -> Option<(String, Client, BasicAuth)> {
        let client = self.active_client().await?;
        let auth = client.credentials()?;
        let user = client
            .current_user_id()
            .unwrap_or_else(|| "user".to_string());
        Some((user, client, auth))
    }

    /// Discovers external desktop system accounts (SSO), resolves credentials, and instantiates [`Client`]s.
    pub async fn sync_discovered_accounts(&self) {
        let sso = nextcloud_rust_sdk::DesktopSso::new();
        let keyring = nextcloud_rust_sdk::PlatformKeyring::new();
        let discovered = sso.discover_accounts().await;

        for disc in discovered {
            let Ok(server_url) = ServerUrl::parse(disc.server_url.as_str()) else {
                continue;
            };

            let password = sso
                .resolve_credentials(&disc, &keyring)
                .await
                .ok()
                .flatten()
                .map(|p| p.into_inner())
                .unwrap_or_default();

            let secret = SecretString::new(password);
            if let Ok(client) = Client::builder()
                .server_url(server_url)
                .initial_credentials(&disc.username, secret)
                .build()
                .await
            {
                self.add_client(client).await;
            }
        }
    }

    /// Returns all configured account profiles.
    pub async fn accounts(&self) -> Vec<Account> {
        let conf = self.config.read().await;
        conf.accounts.clone()
    }

    /// Returns the currently active account profile.
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

        let accounts = self.accounts().await;
        if let Some(pos) = accounts.iter().position(|a| &a.id == account_id) {
            let _ = self.switch_client(pos).await;
        }

        *self.default_mount_id.write().await = None;
        let _ = self.refresh_virtual_tree().await;
        Ok(())
    }

    /// Adds a new account profile, instantiating and appending a [`Client`] to the clients list.
    pub async fn add_account(&self, account: Account) -> Result<(), SyncError> {
        if let Ok(server_url) = ServerUrl::parse(&account.server.server_url) {
            let secret = SecretString::new(account.credentials.secret());
            if let Ok(client) = Client::builder()
                .server_url(server_url)
                .initial_credentials(&account.credentials.username, secret)
                .build()
                .await
            {
                self.add_client(client).await;
            }
        }

        let account_id = account.id.clone();
        {
            let mut conf = self.config.write().await;
            conf.upsert_account(account);
            conf.active_account_id = Some(account_id);
            if let Some(ref path) = self.config_path {
                let _ = conf.save_to_path(path);
            }
        }

        *self.default_mount_id.write().await = None;
        let _ = self.refresh_virtual_tree().await;
        Ok(())
    }
}
