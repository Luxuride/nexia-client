//! Cross-platform automatic launch on system boot.

use std::path::{Path, PathBuf};

/// Application identifier used for autostart entries.
pub const AUTOSTART_APP_NAME: &str = "eu.luxuride.nexia";

/// Command line the autostart entry launches, presented as a single executable.
#[cfg(target_os = "linux")]
const AUTOSTART_BACKGROUND_ARG: &str = "--background";

/// Resolves the current running executable path.
pub fn current_exe_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Resolves the directory a session reads XDG autostart entries from.
///
/// Inside a Flatpak sandbox `dirs::config_dir()` honours the private
/// `XDG_CONFIG_HOME`, which the host desktop never scans, so the host user's
/// config directory is used instead.
#[cfg(target_os = "linux")]
fn autostart_config_dir() -> Option<PathBuf> {
    if crate::sandbox::is_flatpak() {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        return Some(home.join(".config"));
    }
    dirs::config_dir()
}

/// Builds the `Exec=` value for the autostart entry.
#[cfg(target_os = "linux")]
fn autostart_exec_line() -> String {
    if let Some(app_id) = crate::sandbox::flatpak_app_id() {
        return format!("flatpak run {app_id} {AUTOSTART_BACKGROUND_ARG}");
    }
    let exe = current_exe_path().unwrap_or_else(|| PathBuf::from("nexia-client"));
    format!(
        "{} {AUTOSTART_BACKGROUND_ARG}",
        escape_exec_argument(&exe.to_string_lossy())
    )
}

/// Quotes one argument of a desktop-entry `Exec=` value.
///
/// The value is parsed as an argv, not by a shell: a space splits arguments, `"` starts a
/// quoted argument and `%` introduces a field code. An unquoted path containing any of
/// those either truncates the command or is replaced by the session.
#[cfg(target_os = "linux")]
fn escape_exec_argument(argument: &str) -> String {
    let mut escaped = String::with_capacity(argument.len() + 2);
    escaped.push('"');
    for character in argument.chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '%' => escaped.push_str("%%"),
            _ => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

/// Reports whether a session would actually honour an autostart entry.
///
/// Desktop environments disable an entry by setting `Hidden=true` or
/// `X-GNOME-Autostart-enabled=false` and leaving the file in place, so the file's
/// existence alone does not mean the application starts at login.
#[cfg(target_os = "linux")]
fn autostart_entry_is_enabled(desktop_path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(desktop_path) else {
        return false;
    };
    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let disabled = match key.trim() {
            "Hidden" => value.eq_ignore_ascii_case("true"),
            "X-GNOME-Autostart-enabled" => value.eq_ignore_ascii_case("false"),
            _ => false,
        };
        if disabled {
            return false;
        }
    }
    true
}

/// Checks whether autostart on system boot is currently configured.
pub fn is_autostart_enabled() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Some(config_dir) = autostart_config_dir() {
            let desktop_path = config_dir
                .join("autostart")
                .join(format!("{AUTOSTART_APP_NAME}.desktop"));
            return autostart_entry_is_enabled(&desktop_path);
        }
        false
    }

    #[cfg(target_os = "windows")]
    {
        false
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let plist_path = home
                .join("Library/LaunchAgents")
                .join(format!("{AUTOSTART_APP_NAME}.plist"));
            return plist_path.exists();
        }
        false
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        false
    }
}

/// Registers the application to automatically launch on system boot.
pub fn register_autostart() -> Result<(), std::io::Error> {
    #[cfg(target_os = "macos")]
    let exe = current_exe_path().unwrap_or_else(|| PathBuf::from("nexia-client"));
    #[cfg(target_os = "macos")]
    let exe_str = exe.to_string_lossy();

    #[cfg(target_os = "linux")]
    {
        let Some(config_dir) = autostart_config_dir() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "User config directory not found",
            ));
        };

        let exec_line = autostart_exec_line();
        let flatpak_line = crate::sandbox::flatpak_app_id()
            .map(|id| format!("X-Flatpak={id}\n"))
            .unwrap_or_default();
        let autostart_dir = config_dir.join("autostart");
        std::fs::create_dir_all(&autostart_dir)?;

        let desktop_file = autostart_dir.join(format!("{AUTOSTART_APP_NAME}.desktop"));
        let content = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.0\n\
             Name=Nexia Client\n\
             Comment=Nexia Client — independent desktop client for Nextcloud servers\n\
             Exec={exec_line}\n\
             Icon={AUTOSTART_APP_NAME}\n\
             Terminal=false\n\
             Categories=Network;FileTransfer;\n\
             StartupNotify=false\n\
             X-GNOME-Autostart-enabled=true\n\
             {flatpak_line}"
        );

        std::fs::write(desktop_file, content)?;
        tracing::info!("Registered Nexia Client in XDG autostart");
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        let Some(home) = dirs::home_dir() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Home directory not found",
            ));
        };

        let agents_dir = home.join("Library/LaunchAgents");
        std::fs::create_dir_all(&agents_dir)?;

        let plist_path = agents_dir.join(format!("{AUTOSTART_APP_NAME}.plist"));
        let content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
                 <key>Label</key>\n\
                 <string>{AUTOSTART_APP_NAME}</string>\n\
                 <key>ProgramArguments</key>\n\
                 <array>\n\
                     <string>{exe_str}</string>\n\
                 </array>\n\
                 <key>RunAtLoad</key>\n\
                 <true/>\n\
             </dict>\n\
             </plist>\n"
        );

        std::fs::write(plist_path, content)?;
        tracing::info!("Registered Nexia Client in macOS LaunchAgents");
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Ok(())
    }
}

/// Unregisters the application from system boot autostart.
pub fn unregister_autostart() -> Result<(), std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        if let Some(config_dir) = autostart_config_dir() {
            let desktop_file = config_dir
                .join("autostart")
                .join(format!("{AUTOSTART_APP_NAME}.desktop"));
            if desktop_file.exists() {
                std::fs::remove_file(desktop_file)?;
                tracing::info!("Removed Nexia Client from XDG autostart");
            }
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let plist_path = home
                .join("Library/LaunchAgents")
                .join(format!("{AUTOSTART_APP_NAME}.plist"));
            if plist_path.exists() {
                std::fs::remove_file(plist_path)?;
                tracing::info!("Removed Nexia Client from macOS LaunchAgents");
            }
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Ok(())
    }
}
