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
        };

        if let Ok(rt_handle) = tokio::runtime::Handle::try_current() {
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
                            _ => {}
                        }
                    }
                });
            }

            // Periodic background WebDAV synchronizer
            let periodic_coord = coord.clone();
            rt_handle.spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    if periodic_coord.active_client().await.is_some() {
                        let _ = periodic_coord.refresh_virtual_tree().await;
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

    /// Access the thread-safe application configuration.
    pub fn config(&self) -> &Arc<RwLock<AppConfig>> {
        &self.config
    }

    /// Access the virtual filesystem handle for the active account mount.
    pub async fn active_vfs(&self) -> VfsHandle {
        let active_user = self
            .active_client()
            .await
            .and_then(|c| c.current_user_id().map(|s| s.to_string()))
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
            let initial_id = MountId::new();
            let vfs_dir = self
                .cache_dir()
                .join("mounts")
                .join(initial_id.as_str())
                .join("vfs");
            let tree_file = vfs_dir.join("tree.json");
            if !tree_file.exists() {
                let _ = std::fs::create_dir_all(&vfs_dir);
                let tree = VirtualTree::new("Nextcloud");
                if let Ok(json) = serde_json::to_string_pretty(&tree) {
                    let _ = std::fs::write(&tree_file, json);
                }
            }

            let mut desc = MountDescriptor::new(&active_user, "/", vfs_dir, MountMode::VirtualVfs);
            desc.id = initial_id.clone();
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
            let clients = self.clients.read().await;
            for c in clients.iter() {
                if c.server_url().as_str() == active_acc.server.server_url
                    && c.current_user_id().as_deref() == Some(&active_acc.credentials.username)
                {
                    return Some(c.clone());
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
}
