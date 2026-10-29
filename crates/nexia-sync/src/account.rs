//! Account management backed directly by a list of SDK [`Client`]s.

use std::path::PathBuf;

use nexia_core::account::{Account, AccountId, ServerConfig};
use nexia_core::events::DesktopEvent;
use nexia_nextcloud_sdk::{
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

        let client = Client::builder()
            .server_url(server_url)
            .storage(paths)
            .store_key(store_key)
            .initial_credentials(username, secret)
            .build()
            .await
            .map_err(|e| SyncError::InvalidPath(e.to_string()))?;

        let mut client_events = client.subscribe_events();
        let bus = self.event_bus.clone();
        tokio::spawn(async move {
            while let Ok(evt) = client_events.recv().await {
                match evt {
                    nexia_nextcloud_sdk::ClientEvent::SyncMountAdded(m) => {
                        bus.emit(DesktopEvent::SyncMountAdded { mount_id: m.id });
                    }
                    nexia_nextcloud_sdk::ClientEvent::SyncMountRemoved(id) => {
                        bus.emit(DesktopEvent::SyncMountRemoved { mount_id: id });
                    }
                    nexia_nextcloud_sdk::ClientEvent::SyncMountUpdated(m) => {
                        bus.emit(DesktopEvent::SyncMountUpdated { mount_id: m.id });
                    }
                    _ => {}
                }
            }
        });

        Ok(client)
    }

    /// Returns the filesystem-safe identifier used for `server`'s shared theming assets.
    fn theme_server_key(server: &str) -> String {
        ServerUrl::parse(server)
            .map(|url| url.authority().replace([':', '/'], "_"))
            .unwrap_or_else(|_| server.replace([':', '/'], "_"))
    }

    /// Returns synchronous in-memory cached visual theming metadata if available.
    pub fn cached_theme(&self) -> Option<ThemeInfo> {
        self.cached_theme_data.read().ok().and_then(|t| t.clone())
    }

    /// Returns the local cached server logo file path if it already exists on disk.
    ///
    /// Only the active server's logo is considered, so branding never leaks between accounts.
    pub fn cached_theme_logo(&self) -> Option<PathBuf> {
        let server = self
            .config
            .try_read()
            .ok()
            .and_then(|conf| conf.active_account_id.clone())
            .and_then(|id| {
                self.config
                    .try_read()
                    .ok()
                    .and_then(|conf| conf.get_account(&id).map(|a| a.server.server_url.clone()))
            });
        let server = server?;
        let theming_dir = self.cache_dir().join("theming");
        let prefix = format!("{}_logo.", Self::theme_server_key(&server));
        let entries = std::fs::read_dir(theming_dir).ok()?;
        for entry in entries.flatten() {
            let p = entry.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with(&prefix) && (name.ends_with(".svg") || name.ends_with(".png")) {
                return Some(p);
            }
        }
        None
    }

    /// Drops cached branding metadata so the next theming read reflects a different server.
    pub fn clear_cached_theme(&self) {
        if let Ok(mut lock) = self.cached_theme_data.write() {
            *lock = None;
        }
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

    /// Returns the active `nexia-nextcloud-sdk` [`Client`] handle if configured.
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

    /// Checks whether a mount ID represents an internal account virtual drive (which cannot be removed).
    pub fn is_internal_vfs(mount_id: &str) -> bool {
        mount_id.starts_with("app-") || mount_id == "app-default-vfs"
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

        // 1. Purge any legacy or stale internal VFS mounts that don't match the active account
        let Some(account) = self.active_account().await else {
            return mounts;
        };

        let active_vfs_id = format!("app-vfs-{}", account.id.0);

        let stale_internal_ids: Vec<String> = mounts
            .iter()
            .filter(|m| {
                (Self::is_internal_vfs(&m.id) && m.id != active_vfs_id)
                    || m.id == "app-default-vfs"
                    || m.local_target.contains("mounts/app-default-vfs")
            })
            .map(|m| m.id.clone())
            .collect();

        if !stale_internal_ids.is_empty() {
            if let Some(client) = self.active_client().await {
                for id in &stale_internal_ids {
                    let _ = client.remove_sync_mount(id).await;
                }
            }
            if let Some(ref h) = *self.orchestrator_handle.read().await {
                for id in &stale_internal_ids {
                    let _ = h.mounts().remove_mount(MountId::from(id.clone())).await;
                }
            }
            let legacy_dir = self.cache_dir().join("mounts").join("app-default-vfs");
            if legacy_dir.exists() {
                let _ = tokio::fs::remove_dir_all(&legacy_dir).await;
            }
            mounts.retain(|m| !stale_internal_ids.contains(&m.id));
        }

        // 2. Filter to only mounts belonging to the active account (or unassigned)
        mounts.retain(|m| m.account_id.is_empty() || m.account_id == account.id.0);

        // 3. Ensure per-account virtual mount exists
        let has_app_vfs = mounts.iter().any(|m| m.id == active_vfs_id);
        if !has_app_vfs {
            let vfs_dir = self.account_vfs_dir(&account.id.0);
            let default_app_mount = StoredSyncMount {
                id: active_vfs_id.clone(),
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

        // 3. Deduplicate mounts by ID
        let mut seen = std::collections::HashSet::new();
        mounts.retain(|m| seen.insert(m.id.clone()));

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
        self.uncancel_mount(&mount_id).await;

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

        if mode == "Fuse" {
            let _ = self
                .mount_fuse_folder(&mount_id, &remote_str, &local_buf)
                .await;
        } else {
            if let Some(ref h) = *self.orchestrator_handle.read().await {
                let _ = h.mounts().add_mount(desc).await;
            }

            // Register with filesystem watcher to monitor for local edits & additions
            self.watch_mount(&local_buf, &mount_id).await;
        }

        self.event_bus.emit(DesktopEvent::SyncMountAdded {
            mount_id: mount_id.clone(),
        });

        if mode != "Fuse" {
            // Immediately trigger initial sync for the newly registered mount
            let coord = self.clone();
            let mid = mount_id.clone();
            tokio::spawn(async move {
                let _ = coord.sync_mount_folder(&mid).await;
            });
        }

        Ok(mount_id)
    }

    /// Removes a sync mount by ID from both the SDK database and the orchestrator.
    pub async fn remove_sync_mount(&self, id: &str) -> Result<(), SyncError> {
        self.remove_sync_mount_with_cleanup(id, false).await
    }

    /// Removes a sync mount by ID, optionally deleting local synced files from disk.
    pub async fn remove_sync_mount_with_cleanup(
        &self,
        id: &str,
        clean_local_files: bool,
    ) -> Result<(), SyncError> {
        if Self::is_internal_vfs(id) {
            return Err(SyncError::InvalidPath(
                "Cannot remove primary account virtual drive".to_string(),
            ));
        }

        // 1. Immediately mark mount as cancelled so any running sync loop terminates
        self.cancel_mount(id).await;
        self.unmount_fuse_folder(id).await;

        let mounts = self.sync_mounts().await;
        let target_opt = mounts.into_iter().find(|m| m.id == id);

        if let Some(ref target) = target_opt {
            let local_path = std::path::PathBuf::from(&target.local_target);
            self.unwatch_mount(&local_path).await;
        }

        if let Some(client) = self.active_client().await {
            let _ = client.remove_sync_mount(id).await;
        }
        if let Some(ref h) = *self.orchestrator_handle.read().await {
            let _ = h.mounts().remove_mount(MountId::from(id.to_string())).await;
        }

        // Brief yield so any in-flight step checks cancellation and completes safely
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;

        if let Some(ref target) = target_opt {
            let local_path = std::path::PathBuf::from(&target.local_target);
            let mount_cache = self.account_mounts_dir(&target.account_id).join(id);
            if mount_cache.exists() {
                let _ = tokio::fs::remove_dir_all(&mount_cache).await;
            }

            if clean_local_files && local_path.exists() {
                let _ = tokio::fs::remove_dir_all(&local_path).await;
                // Double check to eliminate any resurrected file from a racing write
                if local_path.exists() {
                    let _ = tokio::fs::remove_dir_all(&local_path).await;
                }
            }
        }

        self.event_bus.emit(DesktopEvent::SyncMountRemoved {
            mount_id: id.to_string(),
        });

        Ok(())
    }

    /// Updates the synchronization mode for an existing sync mount in the SDK database and orchestrator.
    pub async fn update_sync_mount_mode(&self, id: &str, new_mode: &str) -> Result<(), SyncError> {
        let Some(client) = self.active_client().await else {
            return Err(SyncError::NotFound("No active account".to_string()));
        };

        let mounts = client.list_sync_mounts().await.unwrap_or_default();
        if let Some(target) = mounts.into_iter().find(|m| m.id == id) {
            let old_mode = target.mode.clone();
            let mut updated = target.clone();
            updated.mode = new_mode.to_string();
            let _ = client.store_sync_mount(updated).await;

            let local_path = std::path::PathBuf::from(&target.local_target);

            if old_mode == "Fuse" && new_mode != "Fuse" {
                self.unmount_fuse_folder(id).await;
            }

            if new_mode == "Fuse" {
                let _ = self
                    .mount_fuse_folder(id, &target.remote_path, &local_path)
                    .await;
                self.unwatch_mount(&local_path).await;
                if let Some(ref h) = *self.orchestrator_handle.read().await {
                    let _ = h.mounts().remove_mount(MountId::from(id.to_string())).await;
                }
            } else {
                self.watch_mount(&local_path, id).await;

                if let Some(ref h) = *self.orchestrator_handle.read().await {
                    let mount_mode = match new_mode {
                        "BidirectionalSync" => MountMode::BidirectionalSync,
                        _ => MountMode::VirtualVfs,
                    };
                    let mut desc = MountDescriptor::new(
                        &target.account_id,
                        &target.remote_path,
                        local_path,
                        mount_mode,
                    )
                    .with_id(MountId::from(id.to_string()));
                    if let Some(name) = target.display_name {
                        desc = desc.with_display_name(name);
                    }
                    let _ = h.mounts().add_mount(desc).await;
                }
            }

            self.event_bus.emit(DesktopEvent::SyncMountUpdated {
                mount_id: id.to_string(),
            });
        }
        Ok(())
    }

    /// Helper resolving active user name, SDK client, and auth credentials.
    pub(crate) async fn active_dav_context(&self) -> Option<(String, Client, BasicAuth)> {
        let client = self.active_client().await?;
        let auth = client.credentials().or_else(|| {
            let conf = self.config.try_read().ok()?;
            let acc = conf
                .active_account_id
                .as_ref()
                .and_then(|id| conf.get_account(id))
                .or_else(|| conf.accounts.first())?;
            Some(BasicAuth::new(
                &acc.credentials.username,
                SecretString::new(acc.credentials.secret()),
            ))
        })?;
        let user = client
            .current_user_id()
            .or_else(|| {
                let conf = self.config.try_read().ok()?;
                let acc = conf
                    .active_account_id
                    .as_ref()
                    .and_then(|id| conf.get_account(id))
                    .or_else(|| conf.accounts.first())?;
                Some(acc.credentials.username.clone())
            })
            .unwrap_or_else(|| "user".to_string());
        Some((user, client, auth))
    }

    /// Merges freshly discovered system accounts into the configuration and republishes the
    /// account-scoped runtime state if the active profile changed as a result.
    pub async fn reconcile_system_accounts(&self, discovered_accounts: Vec<Account>) {
        // Reconcile unconditionally: an empty run is exactly how system accounts the OS no
        // longer reports get purged, and how a dangling active account gets repaired.
        let (previous_active, purged) = {
            let mut conf = self.config.write().await;
            let previous_active = conf.active_account_id.clone();
            let before: Vec<Account> = conf.accounts.clone();
            conf.sync_system_accounts(discovered_accounts);
            let purged: Vec<Account> = before
                .into_iter()
                .filter(|old| !conf.accounts.iter().any(|a| a.id == old.id))
                .collect();
            (previous_active, purged)
        };
        self.persist_config().await;

        for gone in &purged {
            self.drop_client_for(gone).await;
        }

        let active_after = {
            let conf = self.config.read().await;
            conf.active_account_id.clone()
        };
        if active_after == previous_active {
            return;
        }

        *self.default_mount_id.write().await = None;
        self.clear_cached_theme();
        let _ = self.refresh_virtual_tree().await;
        match active_after {
            Some(id) => self.emit_account_switched(&id),
            None => self.event_bus.emit(DesktopEvent::AccountSignedOut {
                account_id: previous_active.map(|a| a.0).unwrap_or_default(),
            }),
        }
    }

    /// Helper resolving user name, SDK client, and auth credentials for any specific account.
    pub(crate) async fn dav_context_for_account(
        &self,
        account: &Account,
    ) -> Option<(String, Client, BasicAuth)> {
        let clients = self.clients.read().await;
        let client = clients
            .iter()
            .find(|c| {
                let target_authority = ServerUrl::parse(&account.server.server_url)
                    .map(|u| u.authority().to_string())
                    .unwrap_or_default();
                (c.server_url().as_str() == account.server.server_url
                    || c.server_url().authority() == target_authority)
                    && c.current_user_id().as_deref() == Some(&account.credentials.username)
            })?
            .clone();

        let auth = client.credentials().unwrap_or_else(|| {
            BasicAuth::new(
                &account.credentials.username,
                SecretString::new(account.credentials.secret()),
            )
        });
        let user = account.credentials.username.clone();
        Some((user, client, auth))
    }

    /// Synchronizes all active folder mount pairings belonging to a specific account.
    pub async fn sync_account_mounts(&self, account_id: &str) {
        let engine = crate::folder_sync::FolderSyncEngine::new(self.clone());
        let mounts = self.sync_mounts().await;
        for mount in mounts {
            if mount.account_id == account_id && !Self::is_internal_vfs(&mount.id) && mount.enabled
            {
                let _ = engine.sync_mount(&mount.id).await;
            }
        }
    }

    /// Discovers external desktop system accounts (SSO), resolves credentials, and instantiates [`Client`]s.
    pub async fn sync_discovered_accounts(&self) {
        let sso = nexia_nextcloud_sdk::DesktopSso::new();
        let keyring = nexia_nextcloud_sdk::PlatformKeyring::new();
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
                nexia_core::account::AccountCredentials::new(&disc.username, password),
                None,
            );
            discovered_accounts.push(sso_acc);
        }

        self.reconcile_system_accounts(discovered_accounts).await;

        // Ensure every configured account has a registered client
        let configured = {
            let conf = self.config.read().await;
            conf.accounts.clone()
        };
        for acc in configured {
            if self.client_index_for(&acc).await.is_some() {
                continue;
            }
            let Ok(server_url) = ServerUrl::parse(&acc.server.server_url) else {
                continue;
            };
            let secret = SecretString::new(acc.credentials.secret());
            if let Ok(client) = self
                .build_account_client(server_url, &acc.credentials.username, secret, &acc.id.0)
                .await
            {
                self.add_client(client).await;
            }
        }

        // Initialize mounts and watchers for discovered accounts
        self.init_watcher().await;
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

        // Branding metadata belongs to the previous account; drop it before the switch
        // event lets the UI repopulate from the new one.
        self.clear_cached_theme();

        // Reset default mount id so a new mount is opened for the switched account
        *self.default_mount_id.write().await = None;
        let _ = self.refresh_virtual_tree().await;
        self.event_bus.emit(DesktopEvent::AccountSwitched {
            account_id: account_id.0.clone(),
        });
        Ok(())
    }

    /// Adds a new account profile and makes it active, registering a [`Client`] for it.
    pub async fn add_account(&self, account: Account) -> Result<(), SyncError> {
        // The OAuth flow registers a live client before calling here, so reuse it rather than
        // registering a second client for the same profile.
        let idx = match self.client_index_for(&account).await {
            Some(i) => Some(i),
            None => match ServerUrl::parse(&account.server.server_url) {
                Ok(server_url) => {
                    let secret = SecretString::new(account.credentials.secret());
                    match self
                        .build_account_client(
                            server_url,
                            &account.credentials.username,
                            secret,
                            &account.id.0,
                        )
                        .await
                    {
                        Ok(client) => Some(self.add_client(client).await),
                        Err(_) => None,
                    }
                }
                Err(_) => None,
            },
        };

        let account_id = account.id.clone();
        {
            let mut conf = self.config.write().await;
            conf.upsert_account(account);
            conf.active_account_id = Some(account_id.clone());
        }
        self.persist_config().await;

        if let Some(i) = idx {
            let _ = self.switch_client(i).await;
        }
        *self.default_mount_id.write().await = None;
        self.clear_cached_theme();
        let _ = self.refresh_virtual_tree().await;

        // The first sign-in goes through here rather than `switch_account`, so the UI has no
        // other signal to repopulate its account-scoped state from.
        self.emit_account_switched(&account_id);
        Ok(())
    }

    /// Emits the event every account-scoped view subscribes to when its state must be re-read.
    fn emit_account_switched(&self, account_id: &AccountId) {
        self.event_bus.emit(DesktopEvent::AccountSwitched {
            account_id: account_id.0.clone(),
        });
    }

    /// Finds the index of an already registered client matching `account`, if any.
    async fn client_index_for(&self, account: &Account) -> Option<usize> {
        let target_authority = ServerUrl::parse(&account.server.server_url)
            .map(|u| u.authority().to_string())
            .unwrap_or_default();
        let clients = self.clients.read().await;
        clients.iter().position(|c| {
            (c.server_url().as_str() == account.server.server_url
                || (!target_authority.is_empty() && c.server_url().authority() == target_authority))
                && c.current_user_id().as_deref() == Some(&account.credentials.username)
        })
    }

    /// Forgets the registered [`Client`] of an account that is no longer configured.
    async fn drop_client_for(&self, account: &Account) {
        let idx = {
            let target_authority = ServerUrl::parse(&account.server.server_url)
                .map(|u| u.authority().to_string())
                .unwrap_or_default();
            let clients = self.clients.read().await;
            clients.iter().position(|c| {
                (c.server_url().as_str() == account.server.server_url
                    || (!target_authority.is_empty()
                        && c.server_url().authority() == target_authority))
                    && c.current_user_id().as_deref() == Some(&account.credentials.username)
            })
        };
        let Some(idx) = idx else {
            return;
        };
        let mut clients = self.clients.write().await;
        if idx >= clients.len() {
            return;
        }
        clients.remove(idx);
        let mut active_idx = self.active_client_idx.write().await;
        *active_idx = (*active_idx).min(clients.len().saturating_sub(1));
    }

    /// Signs out and removes a non-system account, optionally cleaning up synchronized files and pairings.
    pub async fn sign_out_account(
        &self,
        account_id: &AccountId,
        clean_syncs: bool,
    ) -> Result<(), SyncError> {
        let account = {
            let conf = self.config.read().await;
            conf.get_account(account_id).cloned()
        };

        let Some(acc) = account else {
            return Err(SyncError::NotFound(format!(
                "Account {} not found",
                account_id.0
            )));
        };

        if acc.is_system {
            return Err(SyncError::OperationDisabled(
                "System accounts cannot be signed out from the desktop application. Please manage system accounts in your OS Settings.".to_string(),
            ));
        }

        // 1. Process all sync mounts for this account
        let mounts = self.sync_mounts().await;
        for m in mounts {
            if m.account_id == acc.id.0 {
                let local_path = PathBuf::from(&m.local_target);
                self.unwatch_mount(&local_path).await;
                let _ = self.remove_mount(MountId::from(m.id.clone())).await;

                if clean_syncs && local_path.exists() {
                    let _ = std::fs::remove_dir_all(&local_path);
                }

                if let Some(client) = self.active_client().await {
                    let _ = client.remove_sync_mount(&m.id).await;
                }
            }
        }

        // 2. Remove account cache directory
        let account_cache = self.cache_dir().join("accounts").join(&acc.id.0);
        if account_cache.exists() {
            let _ = std::fs::remove_dir_all(&account_cache);
        }

        // 3. Remove client from self.clients, keeping the active index in range
        {
            let mut clients = self.clients.write().await;
            clients.retain(|c| {
                !(c.server_url().as_str() == acc.server.server_url
                    && c.current_user_id().as_deref() == Some(&acc.credentials.username))
            });
            let mut active_idx = self.active_client_idx.write().await;
            *active_idx = (*active_idx).min(clients.len().saturating_sub(1));
        }

        // 4. Remove account from self.config and determine new active account
        let next_account_id = {
            let mut conf = self.config.write().await;
            conf.remove_account(account_id);
            conf.active_account_id.clone()
        };
        self.persist_config().await;

        // 5. Switch to the remaining account if there is one, otherwise drop the account-scoped
        // runtime state so nothing is left pointing at the account that just went away.
        match next_account_id {
            Some(next_id) => {
                // `switch_account` emits `AccountSwitched`, which is what repopulates the UI.
                let _ = self.switch_account(&next_id).await;
            }
            None => {
                *self.default_mount_id.write().await = None;
                *self.active_client_idx.write().await = 0;
                self.clear_cached_theme();
            }
        }

        self.event_bus.emit(DesktopEvent::AccountSignedOut {
            account_id: account_id.0.clone(),
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_server_key_flattens_authority() {
        assert_eq!(
            DesktopSyncCoordinator::theme_server_key("https://cloud.example.com"),
            "cloud.example.com"
        );
        assert_eq!(
            DesktopSyncCoordinator::theme_server_key("https://cloud.example.com:8443/nextcloud"),
            "cloud.example.com_8443"
        );
    }
}
