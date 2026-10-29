//! Headless runner used when the crate is built without the GTK feature.

use crate::controller::DesktopUiController;
use crate::error::UiError;

/// Runs the desktop controller without creating any GUI.
///
/// Kept outside the `desktop` module so builds with `--no-default-features`
/// still have an entry point.
pub fn run_headless(controller: DesktopUiController) -> Result<(), UiError> {
    tracing::info!("Running Nexia Client in headless mode");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| UiError::Initialization(err.to_string()))?;

    rt.block_on(async {
        tracing::info!("Headless desktop controller initialized");
        let path = controller.current_path().await;
        tracing::info!(current_path = %path, "Virtual mount ready");
    });

    Ok(())
}
