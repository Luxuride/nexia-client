//! Cross-platform automatic launch on system boot.

use std::path::PathBuf;

/// Application identifier used for autostart entries.
pub const AUTOSTART_APP_NAME: &str = "eu.luxuride.nextcloud-desktop";

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
    let exe = current_exe_path().unwrap_or_else(|| PathBuf::from("nextcloud-desktop-rs"));
    format!("{} {AUTOSTART_BACKGROUND_ARG}", exe.to_string_lossy())
}

/// Checks whether autostart on system boot is currently configured.
pub fn is_autostart_enabled() -> bool {
    #[cfg(target_os = "linux")]
    {
        if let Some(config_dir) = autostart_config_dir() {
            let desktop_path = config_dir
                .join("autostart")
                .join(format!("{AUTOSTART_APP_NAME}.desktop"));
            return desktop_path.exists();
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
    #[cfg(not(target_os = "linux"))]
    let exe = current_exe_path().unwrap_or_else(|| PathBuf::from("nextcloud-desktop-rs"));
    #[cfg(not(target_os = "linux"))]
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
             Name=Nextcloud Desktop\n\
             Comment=Nextcloud Desktop Sync Client\n\
             Exec={exec_line}\n\
             Icon={AUTOSTART_APP_NAME}\n\
             Terminal=false\n\
             Categories=Network;FileTransfer;\n\
             StartupNotify=false\n\
             X-GNOME-Autostart-enabled=true\n\
             {flatpak_line}"
        );

        std::fs::write(desktop_file, content)?;
        tracing::info!("Registered Nextcloud Desktop in XDG autostart");
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
        tracing::info!("Registered Nextcloud Desktop in macOS LaunchAgents");
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
                tracing::info!("Removed Nextcloud Desktop from XDG autostart");
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
                tracing::info!("Removed Nextcloud Desktop from macOS LaunchAgents");
            }
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Ok(())
    }
}
