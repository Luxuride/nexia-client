//! Presentation models and UI state representations.

/// Available navigation views in the application sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationPage {
    /// Virtual drive and cloud file hierarchy.
    CloudFiles,
    /// Local bidirectional synchronized folders.
    SyncFolders,
    /// Active transfers and sync activity log.
    Transfers,
    /// Account and general application settings.
    Settings,
}

/// Status of an individual file in the virtual browser view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSyncStatus {
    /// Remote metadata placeholder; 0 bytes consumed on local disk.
    CloudPlaceholder,
    /// File content downloaded and available offline in local cache.
    HydratedOffline,
    /// File currently undergoing upload or download.
    Syncing,
}

impl FileSyncStatus {
    /// Formats the status as a human-readable display string with icon indicator.
    pub fn display_label(&self) -> &'static str {
        match self {
            Self::CloudPlaceholder => "☁️ Virtual (Cloud Only)",
            Self::HydratedOffline => "💾 Offline Ready",
            Self::Syncing => "🔄 Syncing...",
        }
    }
}

/// Representation of an entry shown in the virtual file browser table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileViewItem {
    /// Canonical relative path in the virtual mount (e.g. `/Documents/Report.pdf`).
    pub path: String,
    /// Name of the file or folder.
    pub name: String,
    /// Whether this entry is a directory.
    pub is_directory: bool,
    /// Formatted human-readable file size (e.g. `4.2 MB`, or `--` for dirs).
    pub size_formatted: String,
    /// Raw size in bytes.
    pub size_bytes: u64,
    /// Synchronization / hydration state.
    pub status: FileSyncStatus,
    /// MIME type string if known.
    pub mime_type: Option<String>,
}

impl FileViewItem {
    /// Formats a raw byte count into human-readable units.
    pub fn format_bytes(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.1} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.1} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.1} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}

/// Representation of an active or recent file transfer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferItem {
    /// Path of the file being transferred.
    pub path: String,
    /// Transfer progress as percentage (0 to 100).
    pub progress_pct: u8,
    /// Transfer category label (Upload, Download, Hydration).
    pub kind_label: String,
    /// Human-readable transfer status.
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_calculation() {
        assert_eq!(FileViewItem::format_bytes(500), "500 B");
        assert_eq!(FileViewItem::format_bytes(2048), "2.0 KB");
        assert_eq!(FileViewItem::format_bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn status_labels() {
        assert_eq!(
            FileSyncStatus::CloudPlaceholder.display_label(),
            "☁️ Virtual (Cloud Only)"
        );
        assert_eq!(
            FileSyncStatus::HydratedOffline.display_label(),
            "💾 Offline Ready"
        );
    }
}
