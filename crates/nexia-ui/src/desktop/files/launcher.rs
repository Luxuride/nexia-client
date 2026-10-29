//! External application launcher for opening hydrated files in default desktop applications.

use gtk4::gio;
use std::path::Path;

/// Launches the default desktop application to open the file at `path`.
pub fn launch_file_externally(path: &Path) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let uri = format!("file://{}", canonical.display());

    // 1. Attempt default desktop launch via GIO AppInfo
    if let Ok(()) = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>) {
        tracing::info!("Launched external viewer for {uri} via GIO AppInfo");
        return;
    }

    // 2. Attempt standard freedesktop xdg-open
    if let Ok(mut child) = std::process::Command::new("xdg-open")
        .arg(&canonical)
        .spawn()
    {
        tracing::info!(
            "Launched external viewer for {} via xdg-open",
            canonical.display()
        );
        let _ = child.wait();
        return;
    }

    // 3. Fallback candidates for containerized or custom desktop environments
    let ext = canonical
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    let candidates: &[&str] = match ext.as_str() {
        "pdf" => &[
            "evince", "okular", "atril", "mupdf", "zathura", "firefox", "chromium",
        ],
        "png" | "jpg" | "jpeg" | "gif" | "svg" => &[
            "loupe", "eog", "gthumb", "feh", "imv", "viewnior", "firefox", "chromium",
        ],
        _ => &[
            "gnome-text-editor",
            "gedit",
            "kate",
            "kwrite",
            "mousepad",
            "xed",
            "leafpad",
            "nano",
            "vi",
        ],
    };

    for &viewer in candidates {
        if let Ok(mut child) = std::process::Command::new(viewer).arg(&canonical).spawn() {
            tracing::info!(
                "Launched external viewer {viewer} for {}",
                canonical.display()
            );
            let _ = child.wait();
            return;
        }
    }

    tracing::warn!(
        "No external application available to open {}",
        canonical.display()
    );
}
