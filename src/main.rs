//! Nexia Client — a GTK 4 desktop file-sync client for Nextcloud servers.

use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use nexia_core::config::AppConfig;
use nexia_core::events::EventBus;
use nexia_sync::DesktopSyncCoordinator;
use nexia_ui::controller::DesktopUiController;
use nexia_ui::widget::FileExplorerBackend;

#[derive(Parser, Debug)]
#[command(
    name = "nexia-client",
    about = "Nexia Client — GTK 4 file-sync client for Nextcloud servers",
    long_about = "Nexia Client is an independent, unofficial desktop client for Nextcloud \
                  servers. It is not affiliated with, endorsed by, or sponsored by \
                  Nextcloud GmbH.",
    version
)]
struct CliArgs {
    /// Run without launching the graphical GTK interface.
    #[arg(long)]
    headless: bool,

    /// Start minimized directly in the background (app bar tray icon).
    #[arg(long, short = 'b')]
    background: bool,

    /// Immediately execute two-way synchronization on all enabled sync pairs.
    #[arg(long)]
    sync: bool,

    /// Refresh virtual sync metadata tree from the server.
    #[arg(long)]
    refresh_vfs: bool,

    /// Hydrate a virtual file on-demand.
    #[arg(long)]
    hydrate: Option<String>,

    /// Dehydrate a virtual file back to a lightweight placeholder.
    #[arg(long)]
    dehydrate: Option<String>,

    /// Pause synchronization globally across all mounts.
    #[arg(long)]
    pause_sync: bool,

    /// Resume synchronization globally across all mounts.
    #[arg(long)]
    resume_sync: bool,

    /// List active mounts managed by the orchestrator.
    #[arg(long)]
    list_mounts: bool,

    /// Mount the virtual filesystem at the specified or default directory.
    #[arg(long)]
    mount_vfs: Option<Option<PathBuf>>,

    /// Update synchronization mode for a mount (e.g. --set-mount-mode <MOUNT_ID> Fuse).
    #[arg(long, num_args = 2, value_names = ["MOUNT_ID", "MODE"])]
    set_mount_mode: Option<Vec<String>>,

    /// Path to custom configuration file.
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "gtk")]
    nexia_ui::suppress_gtk_logs();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = CliArgs::parse();

    let config_path = args.config.unwrap_or_else(|| {
        AppConfig::default_config_path().unwrap_or_else(|_| PathBuf::from("config.json"))
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _rt_guard = rt.enter();

    let (storage, mut config) = match rt.block_on(nexia_core::DesktopStorage::open_default()) {
        Ok(s) => {
            let loaded = match s.load_app_config() {
                Ok(Some(cfg)) => cfg,
                _ => AppConfig::load_from_path(&config_path).unwrap_or_default(),
            };
            (Some(Arc::new(s)), loaded)
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to open encrypted desktop storage, falling back to JSON");
            (
                None,
                AppConfig::load_from_path(&config_path).unwrap_or_default(),
            )
        }
    };

    if config.cache_dir.is_none() {
        config.cache_dir = dirs::cache_dir()
            .map(|c| c.join("nexia"))
            .or_else(|| Some(std::env::temp_dir().join("nexia_cache")));
    }

    let event_bus = EventBus::new(256);
    let shared_config = Arc::new(RwLock::new(config));
    let mut coordinator =
        DesktopSyncCoordinator::new(shared_config, event_bus).with_config_path(config_path);
    if let Some(s) = storage {
        coordinator = coordinator.with_storage(s);
    }
    rt.block_on(coordinator.persist_config());

    let controller = DesktopUiController::new(coordinator.clone());

    // Register autolaunch on boot
    let _ = nexia_core::register_autostart();

    rt.block_on(async {
        controller.ensure_discovered_accounts().await;
    });

    if args.headless
        || args.sync
        || args.refresh_vfs
        || args.hydrate.is_some()
        || args.dehydrate.is_some()
        || args.pause_sync
        || args.resume_sync
        || args.list_mounts
        || args.mount_vfs.is_some()
        || args.set_mount_mode.is_some()
    {
        rt.block_on(async {
            if args.pause_sync {
                tracing::info!("Pausing synchronization globally...");
                match controller.pause_all_sync().await {
                    Ok(()) => tracing::info!("Synchronization paused successfully"),
                    Err(e) => tracing::error!(error = %e, "Failed to pause synchronization"),
                }
            }

            if args.resume_sync {
                tracing::info!("Resuming synchronization globally...");
                match controller.resume_all_sync().await {
                    Ok(()) => tracing::info!("Synchronization resumed successfully"),
                    Err(e) => tracing::error!(error = %e, "Failed to resume synchronization"),
                }
            }

            if args.list_mounts {
                tracing::info!("Listing configured sync mounts...");
                let configured = controller.sync_mounts().await;
                for m in configured {
                    tracing::info!(
                        id = %m.id,
                        account_id = %m.account_id,
                        remote = %m.remote_path,
                        local = %m.local_target,
                        mode = %m.mode,
                        enabled = m.enabled,
                        "Configured Sync Mount"
                    );
                }

                tracing::info!("Listing active orchestrator mounts...");
                match controller.list_mounts().await {
                    Ok(mounts) => {
                        for (desc, state) in mounts {
                            tracing::info!(
                                id = %desc.id,
                                remote = %desc.remote_path,
                                local = %desc.local_target.display(),
                                status = ?state.status,
                                paused = (state.status == nexia_sync::MountStatus::Paused),
                                "Mount"
                            );
                        }
                    }
                    Err(e) => tracing::error!(error = %e, "Failed to list mounts"),
                }
            }

            if args.refresh_vfs {
                tracing::info!("Refreshing virtual file hierarchy...");
                match controller.refresh_virtual_tree().await {
                    Ok(count) => tracing::info!(count, "Virtual tree updated"),
                    Err(e) => tracing::error!(error = %e, "Failed to refresh virtual tree"),
                }
            }

            if let Some(ref path) = args.hydrate {
                tracing::info!(path = %path, "Hydrating virtual file...");
                match controller.hydrate_file(path).await {
                    Ok(cached) => tracing::info!(cached_path = %cached.display(), "File hydrated successfully"),
                    Err(e) => tracing::error!(error = %e, "Failed to hydrate file"),
                }
            }

            if let Some(ref path) = args.dehydrate {
                tracing::info!(path = %path, "Dehydrating virtual file...");
                match controller.dehydrate_file(path).await {
                    Ok(()) => tracing::info!("File dehydrated successfully"),
                    Err(e) => tracing::error!(error = %e, "Failed to dehydrate file"),
                }
            }

            if args.sync {
                tracing::info!("Running folder synchronization cycle...");
                match controller.trigger_sync().await {
                    Ok(()) => tracing::info!("Synchronization cycle completed"),
                    Err(e) => tracing::error!(error = %e, "Sync cycle failed"),
                }
            }

            if let Some(ref mode_args) = args.set_mount_mode
                && mode_args.len() == 2
            {
                let mount_id = &mode_args[0];
                let mode = &mode_args[1];
                tracing::info!(mount_id, mode, "Setting mount synchronization mode...");
                match controller.update_sync_mount_mode(mount_id, mode).await {
                    Ok(()) => tracing::info!(mount_id, mode, "Mount mode updated successfully"),
                    Err(e) => tracing::error!(error = %e, "Failed to update mount mode"),
                }
            }

            if let Some(ref target) = args.mount_vfs {
                let mp = match target {
                    Some(p) => p.clone(),
                    None => controller.default_fuse_mountpoint().await,
                };
                tracing::info!(mountpoint = %mp.display(), "Mounting Nexia virtual filesystem with FUSE...");
                match controller.mount_fuse_vfs(&mp).await {
                    Ok(()) => {
                        tracing::info!(mountpoint = %mp.display(), "Virtual drive mounted successfully! Press Ctrl+C to unmount.");
                        tokio::signal::ctrl_c().await?;
                        controller.unmount_fuse_vfs().await;
                        tracing::info!("Virtual drive unmounted.");
                    }
                    Err(e) => tracing::error!(error = %e, "Failed to mount virtual drive"),
                }
            }

            if args.headless
                && !args.sync
                && !args.refresh_vfs
                && args.hydrate.is_none()
                && args.dehydrate.is_none()
                && args.mount_vfs.is_none()
                && args.set_mount_mode.is_none()
            {
                tracing::info!("Running Nexia background daemon. Press Ctrl+C to terminate.");
                tokio::signal::ctrl_c().await?;
                tracing::info!("Shutting down Nexia background daemon.");
            }

            Ok::<(), Box<dyn std::error::Error>>(())
        })?;

        return Ok(());
    }

    #[cfg(feature = "gtk")]
    {
        nexia_ui::launch_gtk_application(controller, args.background)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    }

    #[cfg(not(feature = "gtk"))]
    {
        tracing::warn!("Compiled without GTK feature; falling back to headless execution.");
        nexia_ui::run_headless(controller)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    }

    Ok(())
}
