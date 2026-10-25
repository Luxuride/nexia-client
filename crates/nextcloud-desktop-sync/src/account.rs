//! Account management backed directly by a list of Nextcloud [`Client`]s.

use nextcloud_rust_sdk::{BasicAuth, Client, SecretString, ServerUrl};

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;
use nextcloud_desktop_core::account::{Account, AccountId, ServerConfig};

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

    /// Fetches server visual theming metadata for the active account, if available.
    pub async fn server_theming(&self) -> Option<nextcloud_rust_sdk::ThemeInfo> {
        if let Some((_, client, auth)) = self.active_dav_context().await {
            client.theming(&auth).get_theme().await.ok()
        } else {
            None
        }
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

        let mut discovered_accounts = Vec::new();

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

            let secret = SecretString::new(&password);

            let client_exists = {
                let clients = self.clients.read().await;
                clients.iter().any(|c| {
                    (c.server_url().as_str() == server_url.as_str()
                        || c.server_url().authority() == server_url.authority())
                        && c.current_user_id().as_deref() == Some(&disc.username)
                })
            };

            if !client_exists
                && let Ok(client) = Client::builder()
                    .server_url(server_url.clone())
                    .initial_credentials(&disc.username, secret)
                    .build()
                    .await
            {
                self.add_client(client).await;
            }

            let display_name = disc
                .display_name
                .filter(|d| !d.contains('@') && !d.trim().is_empty())
                .unwrap_or_else(|| disc.username.clone());
            let sso_acc = Account::system_sso_account(
                format!(
                    "sso-{}-{}",
                    server_url.authority().replace([':', '/'], "_"),
                    disc.username
                ),
                display_name,
                ServerConfig::new(server_url.as_str()),
                nextcloud_desktop_core::account::AccountCredentials::new(&disc.username, password),
                None,
            );
            discovered_accounts.push(sso_acc);
        }

        if !discovered_accounts.is_empty() {
            let mut conf = self.config.write().await;
            conf.sync_system_accounts(discovered_accounts);
            if let Some(ref path) = self.config_path {
                let _ = conf.save_to_path(path);
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
        let target_account = {
            let mut conf = self.config.write().await;
            let Some(acc) = conf.accounts.iter().find(|a| &a.id == account_id).cloned() else {
                return Err(SyncError::NotFound(format!(
                    "Account {} not found",
                    account_id.0
                )));
            };
            conf.active_account_id = Some(account_id.clone());
            if let Some(ref path) = self.config_path {
                let _ = conf.save_to_path(path);
            }
            acc
        };

        // Match existing running client by server URL and username, or create it if not present
        let target_idx = {
            let clients = self.clients.read().await;
            clients.iter().position(|c| {
                let target_authority = ServerUrl::parse(&target_account.server.server_url)
                    .map(|u| u.authority().to_string())
                    .unwrap_or_default();
                (c.server_url().as_str() == target_account.server.server_url
                    || c.server_url().authority() == target_authority)
                    && c.current_user_id().as_deref() == Some(&target_account.credentials.username)
            })
        };

        let idx = match target_idx {
            Some(i) => i,
            None => {
                let server_url = ServerUrl::parse(&target_account.server.server_url)
                    .map_err(|e| SyncError::InvalidPath(e.to_string()))?;
                let secret = SecretString::new(target_account.credentials.secret());
                let client = Client::builder()
                    .server_url(server_url)
                    .initial_credentials(&target_account.credentials.username, secret)
                    .build()
                    .await
                    .map_err(|e| SyncError::InvalidPath(e.to_string()))?;
                self.add_client(client).await
            }
        };

        self.switch_client(idx).await?;

        // Reset default mount id so a new mount is opened for the switched account
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
