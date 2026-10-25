//! Account management backed directly by a list of Nextcloud [`Client`]s.

use std::path::PathBuf;

use nextcloud_desktop_core::account::{Account, AccountId, ServerConfig};
use nextcloud_rust_sdk::{
    BasicAuth, CachedTheme, Client, MountDescriptor, MountId, MountMode, SecretString, ServerUrl,
    StoragePaths, StoreKey, StoredSyncMount, ThemeInfo,
};

use crate::coordinator::DesktopSyncCoordinator;
use crate::error::SyncError;

impl DesktopSyncCoordinator {
    /// Builds an isolated SDK [`Client`] instance with dedicated per-account encrypted databases.
    pub async fn build_account_client(
        &self,
        server_url: ServerUrl,
        username: &str,
        secret: SecretString,
        account_id: &str,
    ) -> Result<Client, SyncError> {
        let account_dir = self.cache_dir().join("accounts").join(account_id);
        let paths = StoragePaths::in_directory(&account_dir);
        let _ = paths.ensure_directories();

        // Deterministic StoreKey derived from account_id and secret
        let mut key_bytes = [0u8; 32];
        let seed = format!("{account_id}:{}", secret.expose());
        for (i, b) in seed.as_bytes().iter().cycle().take(32).enumerate() {
            key_bytes[i] = b ^ (i as u8).wrapping_mul(31);
        }
        let store_key =
            StoreKey::from_bytes(key_bytes).unwrap_or_else(|_| StoreKey::generate().unwrap());

        Client::builder()
            .server_url(server_url)
            .storage(paths)
            .store_key(store_key)
            .initial_credentials(username, secret)
            .build()
            .await
            .map_err(|e| SyncError::InvalidPath(e.to_string()))
    }

    /// Returns synchronous in-memory cached visual theming metadata if available.
    pub fn cached_theme(&self) -> Option<ThemeInfo> {
        self.cached_theme_data.read().ok().and_then(|t| t.clone())
    }

    /// Returns the local cached server logo file path if it already exists on disk.
    pub fn cached_theme_logo(&self) -> Option<PathBuf> {
        let cache_dir = self.cache_dir();
        let theming_dir = cache_dir.join("theming");
        if let Ok(entries) = std::fs::read_dir(theming_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if (name.ends_with(".svg") || name.ends_with(".png")) && name.contains("logo") {
                    return Some(p);
                }
            }
        }
        None
    }

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

    /// Fetches server visual theming metadata for the active account, checking local permanent DB first.
    pub async fn server_theming(&self) -> Option<ThemeInfo> {
        let client = self.active_client().await?;

        // 1. If cached in permanent DB, load and return immediately
        if let Ok(Some(cached)) = client.load_cached_theme().await {
            let info = ThemeInfo {
                name: cached.name,
                slogan: cached.slogan,
                color: cached.color,
                color_text: cached.color_text,
                background: cached.background_url,
                ..Default::default()
            };
            if let Ok(mut lock) = self.cached_theme_data.write() {
                *lock = Some(info.clone());
            }
            return Some(info);
        }

        // 2. Otherwise fetch fresh theming from network
        let theme = if let Some(auth) = client.credentials()
            && let Ok(theme) = client.theming(&auth).get_theme().await
        {
            Some(theme)
        } else {
            client.anonymous_theming().get_theme().await.ok()
        };

        if let Some(ref t) = theme {
            let cached = CachedTheme {
                server: client.server_url().as_str().to_string(),
                name: t.name().map(ToString::to_string),
                slogan: t.slogan().map(ToString::to_string),
                color: t.color().map(ToString::to_string),
                color_text: t.color_text().map(ToString::to_string),
                logo_path: None,
                background_url: t.background().map(ToString::to_string),
                updated_at: 0,
            };
            let _ = client.store_cached_theme(cached).await;
            if let Ok(mut lock) = self.cached_theme_data.write() {
                *lock = Some(t.clone());
            }
        }

        theme
    }

    /// Fetches and caches server theme logo, checking disk cache first before doing network calls.
    pub async fn fetch_theme_logo(&self) -> Option<PathBuf> {
        let client = self.active_client().await?;
        let cache_dir = self.cache_dir();
        let theming_dir = cache_dir.join("theming");
        let _ = tokio::fs::create_dir_all(&theming_dir).await;

        let server_clean = client.server_url().authority().replace([':', '/'], "_");
        let logo_svg = theming_dir.join(format!("{server_clean}_logo.svg"));
        let logo_png = theming_dir.join(format!("{server_clean}_logo.png"));

        if logo_svg.exists() {
            return Some(logo_svg);
        }
        if logo_png.exists() {
            return Some(logo_png);
        }

        let bytes = if let Some(auth) = client.credentials() {
            client.theming(&auth).fetch_logo_bytes().await.ok()?
        } else {
            client.anonymous_theming().fetch_logo_bytes().await.ok()?
        };

        if bytes.is_empty() {
            return None;
        }

        let is_svg = bytes.starts_with(b"<?xml")
            || bytes.starts_with(b"<svg")
            || bytes.windows(4).any(|w| w == b"<svg");
        let ext = if is_svg { "svg" } else { "png" };
        let logo_path = theming_dir.join(format!("{server_clean}_logo.{ext}"));

        if tokio::fs::write(&logo_path, &bytes).await.is_ok() {
            Some(logo_path)
        } else {
            None
        }
    }

    /// Returns the size in bytes of the cache database for the active account.
    pub async fn cache_database_size(&self) -> u64 {
        let Some(account) = self.active_account().await else {
            return 0;
        };
        let account_dir = self.cache_dir().join("accounts").join(&account.id.0);
        let db_path = account_dir.join("cache.db");
        std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0)
    }

    /// Returns the total size in bytes of all cached files on local disk.
    pub async fn cached_files_size(&self) -> u64 {
        let cache_dir = self.cache_dir();
        fn dir_size(path: &std::path::Path) -> u64 {
            let mut total = 0;
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        total += dir_size(&p);
                    } else if let Ok(meta) = entry.metadata() {
                        total += meta.len();
                    }
                }
            }
            total
        }
        dir_size(&cache_dir)
    }

    /// Checks if automatic prefetching of smallest thumbnails is enabled.
    pub async fn is_autodownload_thumbnails_enabled(&self) -> bool {
        if let Some(ref storage) = self.storage
            && let Ok(Some(val)) = storage.get_setting("autodownload_thumbnails")
        {
            return val != "false";
        }
        if let Some(client) = self.active_client().await
            && let Ok(Some(val)) = client.get_setting("autodownload_thumbnails").await
        {
            return val != "false";
        }
        true
    }

    /// Sets whether automatic prefetching of smallest thumbnails is enabled.
    pub async fn set_autodownload_thumbnails_enabled(
        &self,
        enabled: bool,
    ) -> Result<(), SyncError> {
        let val = if enabled { "true" } else { "false" };
        if let Some(ref storage) = self.storage {
            let _ = storage.set_setting("autodownload_thumbnails", val);
        }
        if let Some(client) = self.active_client().await {
            let _ = client.set_setting("autodownload_thumbnails", val).await;
        }
        Ok(())
    }

    /// Lists all sync mounts stored in the SDK database for the active account, ensuring the application virtual drive is registered.
    pub async fn sync_mounts(&self) -> Vec<StoredSyncMount> {
        let mut mounts = if let Some(client) = self.active_client().await
            && let Ok(list) = client.list_sync_mounts().await
        {
            list
        } else {
            Vec::new()
        };

        if let Some(account) = self.active_account().await {
            let has_app_vfs = mounts.iter().any(|m| {
                m.id == "app-default-vfs" || (m.remote_path == "/" && m.mode == "VirtualVfs")
            });
            if !has_app_vfs {
                let vfs_dir = self
                    .cache_dir()
                    .join("mounts")
                    .join("app-default-vfs")
                    .join("vfs");
                let default_app_mount = StoredSyncMount {
                    id: "app-default-vfs".to_string(),
                    account_id: account.id.0.clone(),
                    remote_path: "/".to_string(),
                    local_target: vfs_dir.to_string_lossy().to_string(),
                    mode: "VirtualVfs".to_string(),
                    display_name: Some("Application Virtual Drive".to_string()),
                    enabled: true,
                    created_at: 0,
                    updated_at: 0,
                };
                if let Some(client) = self.active_client().await {
                    let _ = client.store_sync_mount(default_app_mount.clone()).await;
                }
                mounts.insert(0, default_app_mount);
            }
        }
        mounts
    }

    /// Stores a new sync mount in the SDK database and registers it with the orchestrator.
    pub async fn add_sync_mount(
        &self,
        remote_path: impl Into<String>,
        local_target: impl Into<PathBuf>,
        mode: &str,
        display_name: Option<String>,
    ) -> Result<String, SyncError> {
        let Some(client) = self.active_client().await else {
            return Err(SyncError::NotFound("No active account".to_string()));
        };
        let Some(account) = self.active_account().await else {
            return Err(SyncError::NotFound("No active account".to_string()));
        };

        let remote_str = remote_path.into();
        let local_buf = local_target.into();
        let mount_id = uuid::Uuid::new_v4().to_string();

        let stored = StoredSyncMount {
            id: mount_id.clone(),
            account_id: account.id.0.clone(),
            remote_path: remote_str.clone(),
            local_target: local_buf.to_string_lossy().to_string(),
            mode: mode.to_string(),
            display_name: display_name.clone(),
            enabled: true,
            created_at: 0,
            updated_at: 0,
        };

        client
            .store_sync_mount(stored)
            .await
            .map_err(|e| SyncError::InvalidPath(e.to_string()))?;

        let mount_mode = match mode {
            "BidirectionalSync" => MountMode::BidirectionalSync,
            _ => MountMode::VirtualVfs,
        };

        let mut desc = MountDescriptor::new(&account.id.0, &remote_str, &local_buf, mount_mode)
            .with_id(MountId::from(mount_id.clone()));
        if let Some(name) = display_name {
            desc = desc.with_display_name(name);
        }

        if let Some(ref h) = *self.orchestrator_handle.read().await {
            let _ = h.mounts().add_mount(desc).await;
        }

        Ok(mount_id)
    }

    /// Removes a sync mount by ID from both the SDK database and the orchestrator.
    pub async fn remove_sync_mount(&self, id: &str) -> Result<(), SyncError> {
        if let Some(client) = self.active_client().await {
            let _ = client.remove_sync_mount(id).await;
        }
        if let Some(ref h) = *self.orchestrator_handle.read().await {
            let _ = h.mounts().remove_mount(MountId::from(id.to_string())).await;
        }
        Ok(())
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

            let sso_account_id = format!(
                "sso-{}-{}",
                server_url.authority().replace([':', '/'], "_"),
                disc.username
            );

            if !client_exists
                && let Ok(client) = self
                    .build_account_client(
                        server_url.clone(),
                        &disc.username,
                        secret,
                        &sso_account_id,
                    )
                    .await
            {
                self.add_client(client).await;
            }

            let display_name = disc
                .display_name
                .filter(|d| !d.contains('@') && !d.trim().is_empty())
                .unwrap_or_else(|| disc.username.clone());
            let sso_acc = Account::system_sso_account(
                sso_account_id,
                display_name,
                ServerConfig::new(server_url.as_str()),
                nextcloud_desktop_core::account::AccountCredentials::new(&disc.username, password),
                None,
            );
            discovered_accounts.push(sso_acc);
        }

        if !discovered_accounts.is_empty() {
            {
                let mut conf = self.config.write().await;
                conf.sync_system_accounts(discovered_accounts);
            }
            self.persist_config().await;
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
            acc
        };
        self.persist_config().await;

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
                let client = self
                    .build_account_client(
                        server_url,
                        &target_account.credentials.username,
                        secret,
                        &target_account.id.0,
                    )
                    .await?;
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
            if let Ok(client) = self
                .build_account_client(
                    server_url,
                    &account.credentials.username,
                    secret,
                    &account.id.0,
                )
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
        }
        self.persist_config().await;

        *self.default_mount_id.write().await = None;
        let _ = self.refresh_virtual_tree().await;
        Ok(())
    }
}
