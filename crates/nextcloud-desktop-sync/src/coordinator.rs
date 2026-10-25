//! Master desktop coordinator wrapping the Nextcloud background orchestrator and client list.

use nextcloud_rust_sdk::{
    Client, MountDescriptor, MountId, MountMode, MountsHandle, NextcloudOrchestrator,
    OrchestratorConfig, OrchestratorEvent, OrchestratorHandle, ServerUrl, SessionState, VfsHandle,
    VirtualTree,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::SyncError;
use nextcloud_desktop_core::account::{Account, AccountCredentials, ServerConfig};
use nextcloud_desktop_core::config::AppConfig;
use nextcloud_desktop_core::events::{DesktopEvent, EventBus};

/// Snapshot of background thumbnail downloading queue and progress.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ThumbnailQueueSnapshot {
    /// Total number of thumbnail tasks queued in this session.
    pub total: usize,
    /// Number of successfully downloaded and cached thumbnails.
    pub completed: usize,
    /// Number of failed thumbnail attempts.
    pub failed: usize,
    /// Number of thumbnail downloads currently in progress.
    pub in_progress: usize,
    /// List of paths currently being downloaded.
    pub active_items: Vec<String>,
}

/// Master coordinator managing background orchestrator worker, mounts, VFS, and clients.
#[derive(Clone)]
pub struct DesktopSyncCoordinator {
    pub(crate) config: Arc<RwLock<AppConfig>>,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) event_bus: EventBus,
    pub(crate) clients: Arc<RwLock<Vec<Client>>>,
    pub(crate) active_client_idx: Arc<RwLock<usize>>,
    pub(crate) orchestrator: Arc<std::sync::Mutex<Option<NextcloudOrchestrator>>>,
    pub(crate) orchestrator_handle: Arc<RwLock<Option<OrchestratorHandle>>>,
    pub(crate) default_mount_id: Arc<RwLock<Option<MountId>>>,
    pub(crate) preview_semaphore: Arc<tokio::sync::Semaphore>,
    pub(crate) cached_theme_data: Arc<std::sync::RwLock<Option<nextcloud_rust_sdk::ThemeInfo>>>,
    pub(crate) thumbnail_stats: Arc<std::sync::RwLock<ThumbnailQueueSnapshot>>,
    pub(crate) is_preloading_tree: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) storage: Option<Arc<nextcloud_desktop_core::DesktopStorage>>,
    pub(crate) watcher: Arc<tokio::sync::Mutex<Option<crate::watcher::FileSystemWatcher>>>,
    pub(crate) mount_progress_cache: Arc<
        std::sync::RwLock<
            std::collections::HashMap<String, nextcloud_rust_sdk::MountProgressSnapshot>,
        >,
    >,
}

impl DesktopSyncCoordinator {
    /// Returns the live snapshot of background thumbnail download progress.
    pub fn thumbnail_queue_snapshot(&self) -> ThumbnailQueueSnapshot {
        self.thumbnail_stats
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Creates a new synchronization coordinator launching the background orchestrator actor.
    pub fn new(config: Arc<RwLock<AppConfig>>, event_bus: EventBus) -> Self {
        let cache_dir = if let Ok(conf) = config.try_read() {
            conf.resolved_cache_dir()
        } else {
            AppConfig::default_cache_dir()
        };

        let orch_config = OrchestratorConfig::new(cache_dir);
        let orch = NextcloudOrchestrator::start(orch_config).ok();
        let handle = orch.as_ref().map(|o| o.handle());

        let coord = Self {
            config,
            config_path: None,
            event_bus: event_bus.clone(),
            clients: Arc::new(RwLock::new(Vec::new())),
            active_client_idx: Arc::new(RwLock::new(0)),
            orchestrator: Arc::new(std::sync::Mutex::new(orch)),
            orchestrator_handle: Arc::new(RwLock::new(handle.clone())),
            default_mount_id: Arc::new(RwLock::new(None)),
            preview_semaphore: Arc::new(tokio::sync::Semaphore::new(8)),
            cached_theme_data: Arc::new(std::sync::RwLock::new(None)),
            thumbnail_stats: Arc::new(std::sync::RwLock::new(ThumbnailQueueSnapshot::default())),
            is_preloading_tree: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            storage: None,
            watcher: Arc::new(tokio::sync::Mutex::new(None)),
            mount_progress_cache: Arc::new(
                std::sync::RwLock::new(std::collections::HashMap::new()),
            ),
        };

        if let Ok(rt_handle) = tokio::runtime::Handle::try_current() {
            let watcher_coord = coord.clone();
            rt_handle.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                watcher_coord.init_watcher().await;
            });

            if let Some(ref h) = handle {
                let mut orch_rx = h.subscribe_events();
                let bus = event_bus;
                rt_handle.spawn(async move {
                    while let Ok(event) = orch_rx.recv().await {
                        match event {
                            OrchestratorEvent::VfsTreeChanged { .. } => {
                                bus.emit(DesktopEvent::VfsTreeUpdated { node_count: 0 });
                            }
                            OrchestratorEvent::FileHydrated { remote_path, .. } => {
                                bus.emit(DesktopEvent::NodeHydrated {
                                    path: remote_path,
                                    size: 0,
                                });
                            }
                            OrchestratorEvent::FileDehydrated { remote_path, .. } => {
                                bus.emit(DesktopEvent::NodeDehydrated { path: remote_path });
                            }
                            OrchestratorEvent::SyncCycleCompleted {
                                items_processed, ..
                            } => {
                                bus.emit(DesktopEvent::SyncCompleted {
                                    pairing_id: "default".to_string(),
                                    uploaded: 0,
                                    downloaded: items_processed as usize,
                                    conflicts: 0,
                                });
                            }
                            OrchestratorEvent::MountProgress(snapshot) => {
                                bus.emit(DesktopEvent::MountProgress(snapshot));
                            }
                            _ => {}
                        }
                    }
                });
            }

            // Periodic background WebDAV synchronizer
            let periodic_coord = coord.clone();
            rt_handle.spawn(async move {
                // Short delay after startup before running initial sync
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                if periodic_coord.active_client().await.is_some() {
                    periodic_coord.sync_all_mounts().await;
                }

                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    if periodic_coord.active_client().await.is_some() {
                        let _ = periodic_coord.refresh_virtual_tree().await;
                        periodic_coord.sync_all_mounts().await;
                    }
                }
            });
        }

        coord
    }

    /// Attaches a configuration file path for persistent state storage.
    #[must_use]
    pub fn with_config_path(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    /// Attaches an encrypted desktop storage database for persistent state storage.
    #[must_use]
    pub fn with_storage(mut self, storage: Arc<nextcloud_desktop_core::DesktopStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Access the encrypted desktop storage database if initialized.
    pub fn storage(&self) -> Option<&Arc<nextcloud_desktop_core::DesktopStorage>> {
        self.storage.as_ref()
    }

    /// Persists current application configuration into the encrypted database and optionally JSON file.
    pub async fn persist_config(&self) {
        let conf = self.config.read().await.clone();
        if let Some(ref storage) = self.storage
            && let Err(e) = storage.save_app_config(&conf)
        {
            tracing::warn!(error = %e, "Failed to persist app config to encrypted storage");
        }
        if let Some(ref path) = self.config_path {
            let _ = conf.save_to_path(path);
        }
    }

    /// Access the base cache directory path.
    pub fn cache_dir(&self) -> PathBuf {
        if let Ok(conf) = self.config.try_read() {
            return conf.resolved_cache_dir();
        }
        AppConfig::default_cache_dir()
    }

    /// Returns the cache directory for a specific account.
    pub fn account_dir(&self, account_id: &str) -> PathBuf {
        self.cache_dir().join("accounts").join(account_id)
    }

    /// Returns the thumbnails directory for a specific account.
    pub fn account_thumbnails_dir(&self, account_id: &str) -> PathBuf {
        self.account_dir(account_id).join("thumbnails")
    }

    /// Returns the active account's thumbnails directory synchronously, or global fallback.
    pub fn active_thumbnails_dir_sync(&self) -> PathBuf {
        if let Ok(conf) = self.config.try_read()
            && let Some(ref acc_id) = conf.active_account_id
        {
            self.account_thumbnails_dir(&acc_id.0)
        } else {
            self.cache_dir().join("thumbnails")
        }
    }

    /// Returns the active account's thumbnails directory, or global fallback if no active account.
    pub async fn active_thumbnails_dir(&self) -> PathBuf {
        if let Some(account) = self.active_account().await {
            self.account_thumbnails_dir(&account.id.0)
        } else {
            self.active_thumbnails_dir_sync()
        }
    }

    /// Returns the virtual filesystem directory for a specific account.
    pub fn account_vfs_dir(&self, account_id: &str) -> PathBuf {
        self.account_dir(account_id).join("vfs")
    }

    /// Returns the mount cache directory for a specific account.
    pub fn account_mounts_dir(&self, account_id: &str) -> PathBuf {
        self.account_dir(account_id).join("mounts")
    }

    /// Returns `true` if the background orchestrator actor thread is active.
    pub fn is_orchestrator_running(&self) -> bool {
        self.orchestrator
            .lock()
            .map(|o| o.is_some())
            .unwrap_or(false)
    }

    /// Access the thread-safe application configuration.
    pub fn config(&self) -> &Arc<RwLock<AppConfig>> {
        &self.config
    }

    /// Access the virtual filesystem handle for the active account mount.
    pub async fn active_vfs(&self) -> VfsHandle {
        let active_account = self.active_account().await;
        let account_id_str = active_account
            .as_ref()
            .map(|a| a.id.0.clone())
            .unwrap_or_else(|| "default".to_string());
        let active_user = active_account
            .as_ref()
            .map(|a| a.credentials.username.clone())
            .unwrap_or_else(|| "default".to_string());

        let handle = self
            .orchestrator_handle
            .read()
            .await
            .clone()
            .expect("orchestrator actor must be running");

        let mut mount_lock = self.default_mount_id.write().await;
        let mount_id = if let Some(ref id) = *mount_lock {
            id.clone()
        } else {
            let initial_id = MountId::from(format!("app-vfs-{}", account_id_str));
            let vfs_dir = self.account_vfs_dir(&account_id_str);
            let tree_file = vfs_dir.join("tree.json");
            if !tree_file.exists() {
                let _ = std::fs::create_dir_all(&vfs_dir);
                let legacy_tree = self
                    .cache_dir()
                    .join("mounts")
                    .join("app-default-vfs")
                    .join("vfs")
                    .join("tree.json");
                if legacy_tree.exists() {
                    let _ = std::fs::copy(&legacy_tree, &tree_file);
                } else {
                    let tree = VirtualTree::new("Nextcloud");
                    if let Ok(json) = serde_json::to_string_pretty(&tree) {
                        let _ = std::fs::write(&tree_file, json);
                    }
                }
            }

            let mut desc = MountDescriptor::new(&active_user, "/", vfs_dir, MountMode::VirtualVfs);
            desc.id = initial_id.clone();
            desc.display_name = Some("Application Virtual Drive".to_string());
            let registered = handle
                .mounts()
                .add_mount(desc)
                .await
                .unwrap_or_else(|_| initial_id.clone());
            *mount_lock = Some(registered.clone());
            registered
        };

        handle.vfs(mount_id)
    }

    /// Access the shared event bus.
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Caches the current mount progress snapshot in memory.
    pub fn set_mount_progress(&self, snapshot: nextcloud_rust_sdk::MountProgressSnapshot) {
        if let Ok(mut map) = self.mount_progress_cache.write() {
            map.insert(snapshot.mount_id.as_str().to_string(), snapshot);
        }
    }

    /// Returns the active orchestrator handle communicating with the background worker thread.
    pub async fn orchestrator_handle(&self) -> Option<OrchestratorHandle> {
        self.orchestrator_handle.read().await.clone()
    }

    /// Access the mounts management interface.
    pub async fn mounts(&self) -> Option<MountsHandle> {
        self.orchestrator_handle
            .read()
            .await
            .as_ref()
            .map(|h| h.mounts())
    }

    /// Returns the list of managed Nextcloud [`Client`]s.
    pub async fn clients(&self) -> Vec<Client> {
        self.clients.read().await.clone()
    }

    /// Returns the currently active Nextcloud [`Client`].
    pub async fn active_client(&self) -> Option<Client> {
        if let Some(active_acc) = self.active_account().await {
            let target_authority =
                nextcloud_rust_sdk::ServerUrl::parse(&active_acc.server.server_url)
                    .map(|u| u.authority().to_string())
                    .unwrap_or_default();
            {
                let clients = self.clients.read().await;
                for c in clients.iter() {
                    let c_auth = c.server_url().authority().to_string();
                    if (c.server_url().as_str().trim_end_matches('/')
                        == active_acc.server.server_url.trim_end_matches('/')
                        || (!target_authority.is_empty() && c_auth == target_authority))
                        && c.current_user_id().as_deref() == Some(&active_acc.credentials.username)
                    {
                        return Some(c.clone());
                    }
                }
            }

            // Client not yet initialized for active account: instantiate and cache
            if let Ok(server_url) =
                nextcloud_rust_sdk::ServerUrl::parse(&active_acc.server.server_url)
            {
                let secret = nextcloud_rust_sdk::SecretString::new(active_acc.credentials.secret());
                if let Ok(client) = self
                    .build_account_client(
                        server_url,
                        &active_acc.credentials.username,
                        secret,
                        &active_acc.id.0,
                    )
                    .await
                {
                    self.add_client(client.clone()).await;
                    return Some(client);
                }
            }
        }
        let clients = self.clients.read().await;
        let idx = *self.active_client_idx.read().await;
        clients
            .get(idx)
            .cloned()
            .or_else(|| clients.first().cloned())
    }

    /// Appends a new [`Client`] to the list of managed clients, returning its index.
    pub async fn add_client(&self, client: Client) -> usize {
        let mut clients = self.clients.write().await;
        clients.push(client);
        clients.len() - 1
    }

    /// Switches the active client by index in the clients list.
    pub async fn switch_client(&self, index: usize) -> Result<(), SyncError> {
        let clients = self.clients.read().await;
        if index >= clients.len() {
            return Err(SyncError::NotFound(format!(
                "Client index {index} out of range"
            )));
        }
        *self.active_client_idx.write().await = index;
        Ok(())
    }

    /// Initiates a browser-based OAuth (Login Flow v2) session with a server.
    pub async fn start_oauth_login(
        &self,
        server_url_str: &str,
    ) -> Result<(String, Client), SyncError> {
        let server_url =
            ServerUrl::parse(server_url_str).map_err(|e| SyncError::InvalidPath(e.to_string()))?;
        let client = Client::builder()
            .server_url(server_url)
            .build()
            .await
            .map_err(|e| SyncError::InvalidPath(e.to_string()))?;
        let flow = client
            .start_login()
            .await
            .map_err(|e| SyncError::InvalidPath(e.to_string()))?;
        Ok((flow.login_url().to_string(), client))
    }

    /// Polls a pending OAuth login flow on the provided client handle.
    ///
    /// If the user authorized the login in their browser, this saves the account
    /// profile to configuration, registers the client, and returns `Ok(Some(account))`.
    /// If authorization is still pending, returns `Ok(None)`.
    pub async fn poll_oauth_login(&self, client: &Client) -> Result<Option<Account>, SyncError> {
        let state = client
            .poll_login()
            .await
            .map_err(|e| SyncError::InvalidPath(e.to_string()))?;
        match state {
            SessionState::Authenticated(acc) => {
                let username = acc.user_id().to_string();
                let password = acc.password().expose().to_string();
                let server_url = acc.server().as_str().to_string();
                let account_name = username.clone();
                let account = Account::new(
                    account_name,
                    ServerConfig::new(&server_url),
                    AccountCredentials::new(&username, password),
                );
                let _ = self.add_client(client.clone()).await;
                self.add_account(account.clone()).await?;
                Ok(Some(account))
            }
            _ => Ok(None),
        }
    }

    /// Triggers an immediate folder synchronization cycle for a specific mount point.
    pub async fn sync_mount_folder(&self, mount_id: &str) -> Result<usize, SyncError> {
        let engine = crate::folder_sync::FolderSyncEngine::new(self.clone());
        engine.sync_mount(mount_id).await
    }

    /// Triggers an immediate folder synchronization cycle across all active mount points.
    pub async fn sync_all_mounts(&self) {
        let engine = crate::folder_sync::FolderSyncEngine::new(self.clone());
        engine.sync_all_mounts().await;
    }

    /// Initializes and starts the filesystem watcher for all active folder pairings.
    pub async fn init_watcher(&self) {
        let mut watcher_guard = self.watcher.lock().await;
        if watcher_guard.is_none() {
            let mut w = crate::watcher::FileSystemWatcher::new(self.clone());
            if let Ok(()) = w.start() {
                *watcher_guard = Some(w);
            }
        }

        let orch_handle = self.orchestrator_handle.read().await.clone();
        for mount in self.sync_mounts().await {
            if mount.id == "app-default-vfs" || !mount.enabled {
                continue;
            }
            if let Some(ref h) = orch_handle {
                let m_mode = match mount.mode.as_str() {
                    "BidirectionalSync" => nextcloud_rust_sdk::MountMode::BidirectionalSync,
                    _ => nextcloud_rust_sdk::MountMode::VirtualVfs,
                };
                let mut desc = nextcloud_rust_sdk::MountDescriptor::new(
                    &mount.account_id,
                    &mount.remote_path,
                    PathBuf::from(&mount.local_target),
                    m_mode,
                )
                .with_id(MountId::from(mount.id.clone()));
                if let Some(name) = mount.display_name.clone() {
                    desc = desc.with_display_name(name);
                }
                let _ = h.mounts().add_mount(desc).await;
            }
            if let Some(ref mut w) = *watcher_guard {
                let local_path = PathBuf::from(&mount.local_target);
                let _ = w.watch_folder(&local_path, &mount.id);
            }
        }
    }

    /// Registers a local directory path with the filesystem watcher.
    pub async fn watch_mount(&self, local_path: &std::path::Path, mount_id: &str) {
        let mut watcher_guard = self.watcher.lock().await;
        if let Some(ref mut w) = *watcher_guard {
            let _ = w.watch_folder(local_path, mount_id);
        }
    }

    /// Unregisters a local directory path from the filesystem watcher.
    pub async fn unwatch_mount(&self, local_path: &std::path::Path) {
        let mut watcher_guard = self.watcher.lock().await;
        if let Some(ref mut w) = *watcher_guard {
            w.unwatch_folder(local_path);
        }
    }
}
