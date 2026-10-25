//! Nextcloud Desktop GTK application with virtual file sync.

use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use nextcloud_desktop_core::config::AppConfig;
use nextcloud_desktop_core::events::EventBus;
use nextcloud_desktop_sync::DesktopSyncCoordinator;
use nextcloud_desktop_ui::controller::DesktopUiController;

#[derive(Parser, Debug)]
#[command(
    name = "nextcloud-desktop",
    about = "Nextcloud Desktop GTK Application with Virtual File Sync",
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

    /// Path to custom configuration file.
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "gtk")]
    nextcloud_desktop_ui::suppress_gtk_logs();

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

    let mut config = AppConfig::load_from_path(&config_path).unwrap_or_default();
    if config.cache_dir.is_none() {
        config.cache_dir = dirs::cache_dir()
            .map(|c| c.join("nextcloud-desktop"))
            .or_else(|| Some(std::env::temp_dir().join("nc_cache")));
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _rt_guard = rt.enter();

    let event_bus = EventBus::new(256);
    let shared_config = Arc::new(RwLock::new(config));
    let coordinator =
        DesktopSyncCoordinator::new(shared_config, event_bus).with_config_path(config_path);
    let controller = DesktopUiController::new(coordinator.clone());

    // Register autolaunch on boot
    let _ = nextcloud_desktop_core::register_autostart();

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
                tracing::info!("Listing active orchestrator mounts...");
                match controller.list_mounts().await {
                    Ok(mounts) => {
                        for (desc, state) in mounts {
                            tracing::info!(
                                id = %desc.id,
                                remote = %desc.remote_path,
                                local = %desc.local_target.display(),
                                status = ?state.status,
                                paused = (state.status == nextcloud_desktop_sync::MountStatus::Paused),
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

            if args.headless && !args.sync && !args.refresh_vfs && args.hydrate.is_none() && args.dehydrate.is_none() {
                tracing::info!("Running Nextcloud background daemon. Press Ctrl+C to terminate.");
                tokio::signal::ctrl_c().await?;
                tracing::info!("Shutting down Nextcloud background daemon.");
            }

            Ok::<(), Box<dyn std::error::Error>>(())
        })?;

        return Ok(());
    }

    #[cfg(feature = "gtk")]
    {
        nextcloud_desktop_ui::launch_gtk_application(controller, args.background)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    }

    #[cfg(not(feature = "gtk"))]
    {
        tracing::warn!("Compiled without GTK feature; falling back to headless execution.");
        nextcloud_desktop_ui::run_headless(controller)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    }

    Ok(())
}
