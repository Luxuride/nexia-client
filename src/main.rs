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

    /// Path to custom configuration file.
    #[arg(long)]
    config: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let config = AppConfig::load_from_path(&config_path).unwrap_or_default();
    let cache_dir = config.cache_dir.clone().unwrap_or_else(|| {
        dirs::cache_dir()
            .map(|c| c.join("nextcloud-desktop"))
            .unwrap_or_else(|| std::env::temp_dir().join("nc_cache"))
    });

    let event_bus = EventBus::new(256);
    let shared_config = Arc::new(RwLock::new(config));
    let coordinator = DesktopSyncCoordinator::new(shared_config, cache_dir, event_bus)
        .with_config_path(config_path);
    let controller = DesktopUiController::new(coordinator.clone());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let _rt_guard = rt.enter();

    rt.block_on(async {
        controller.ensure_discovered_accounts().await;
    });

    if args.headless
        || args.sync
        || args.refresh_vfs
        || args.hydrate.is_some()
        || args.dehydrate.is_some()
    {
        rt.block_on(async {
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
