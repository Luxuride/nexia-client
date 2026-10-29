//! Detection of the Flatpak sandbox.

use std::path::Path;

/// Marker file Flatpak mounts into every sandboxed process.
const FLATPAK_INFO_PATH: &str = "/.flatpak-info";

/// Environment variable Flatpak sets to the sandboxed application id.
const FLATPAK_ID_ENV: &str = "FLATPAK_ID";

/// Returns the Flatpak application id when the process runs inside a Flatpak sandbox.
pub fn flatpak_app_id() -> Option<String> {
    let id = std::env::var(FLATPAK_ID_ENV).ok()?;
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Reports whether the process runs inside a Flatpak sandbox.
pub fn is_flatpak() -> bool {
    flatpak_app_id().is_some() || Path::new(FLATPAK_INFO_PATH).exists()
}
