//! Headless runner used when the crate is built without the GTK feature.

use crate::controller::DesktopUiController;
use crate::error::UiError;

/// Runs the desktop controller without creating any GUI.
///
/// Kept outside the `desktop` module so builds with `--no-default-features`
/// still have an entry point. It runs until `Ctrl+C`, mirroring the `--headless`
/// daemon: the coordinator's etag poller and filesystem watcher do the work, so
/// returning immediately would leave the process without a single running task.
pub fn run_headless(controller: DesktopUiController) -> Result<(), UiError> {
    tracing::info!("Running Nexia Client in headless mode");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| UiError::initialization_with_source("build the headless runtime", err))?;

    rt.block_on(async {
        let path = controller.current_path().await;
        tracing::info!(current_path = %path, "Virtual mount ready");
        tracing::info!("Headless daemon running; press Ctrl+C to stop");

        if let Err(err) = tokio::signal::ctrl_c().await {
            return Err(UiError::initialization_with_source("wait for Ctrl+C", err));
        }

        controller.unmount_fuse_vfs().await;
        tracing::info!("Headless daemon stopped");
        Ok(())
    })
}
