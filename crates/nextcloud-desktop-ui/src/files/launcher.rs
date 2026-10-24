//! External application launcher for opening hydrated files in default desktop applications.

use gtk4::gio;
use std::path::Path;

/// Launches the default desktop application to open the file at `path`.
pub fn launch_file_externally(path: &Path) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let uri = format!("file://{}", canonical.display());
    if let Err(err) = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>) {
        tracing::warn!("Failed to launch default application for {uri}: {err}");
        let _ = std::process::Command::new("xdg-open")
            .arg(&canonical)
            .spawn();
    }
}
