//! Presentation models and UI state representations backed by `nextcloud-rust-sdk`.

pub use nextcloud_rust_sdk::{
    BreadcrumbSegment as PathSegment, ResourceIconCategory, breadcrumb_segments, format_bytes,
};

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

/// Layout mode for the Nautilus-style Cloud Files browser.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileViewMode {
    /// Icon grid view (Nautilus default).
    #[default]
    Grid,
    /// Detailed list view with columns.
    List,
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
    /// Formats the status as a human-readable display string.
    pub fn display_label(&self) -> &'static str {
        match self {
            Self::CloudPlaceholder => "Cloud Only",
            Self::HydratedOffline => "Available Offline",
            Self::Syncing => "Syncing…",
        }
    }

    /// Returns the Freedesktop symbolic icon name representing this status.
    pub fn icon_name(&self) -> &'static str {
        match self {
            Self::CloudPlaceholder => "weather-overcast-symbolic",
            Self::HydratedOffline => "emblem-ok-symbolic",
            Self::Syncing => "emblem-synchronizing-symbolic",
        }
    }

    /// Returns the Libadwaita CSS style class associated with this status.
    pub fn css_class(&self) -> &'static str {
        match self {
            Self::CloudPlaceholder => "dim-label",
            Self::HydratedOffline => "success",
            Self::Syncing => "accent",
        }
    }
}

/// Representation of an entry shown in the virtual file browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileViewItem {
    /// Canonical relative path in the virtual mount (e.g. `/Documents/Report.pdf`).
    pub path: String,
    /// Name of the file or folder.
    pub name: String,
    /// Whether this entry is a directory.
    pub is_directory: bool,
    /// Formatted human-readable file size (e.g. `4.2 MB`, or `Folder` for dirs).
    pub size_formatted: String,
    /// Raw size in bytes.
    pub size_bytes: u64,
    /// Synchronization / hydration state.
    pub status: FileSyncStatus,
    /// MIME type string if known.
    pub mime_type: Option<String>,
    /// Semantic icon category computed by `nextcloud-rust-sdk`.
    pub icon_category: ResourceIconCategory,
}

impl FileViewItem {
    /// Formats a raw byte count into human-readable units via `nextcloud-rust-sdk`.
    pub fn format_bytes(bytes: u64) -> String {
        format_bytes(bytes)
    }

    /// Returns the full-color Adwaita / Nautilus icon name matching this item.
    pub const fn nautilus_icon_name(&self) -> &'static str {
        self.icon_category.freedesktop_icon_name()
    }

    /// Returns the symbolic icon name matching this item.
    pub const fn symbolic_icon_name(&self) -> &'static str {
        self.icon_category.symbolic_icon_name()
    }

    /// Splits a canonical virtual path into clickable Nautilus-style breadcrumb segments via `nextcloud-rust-sdk`.
    pub fn breadcrumb_segments(current_path: &str) -> Vec<PathSegment> {
        breadcrumb_segments(current_path, "Nextcloud")
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
            "Cloud Only"
        );
        assert_eq!(
            FileSyncStatus::HydratedOffline.display_label(),
            "Available Offline"
        );
        assert_eq!(
            FileSyncStatus::CloudPlaceholder.icon_name(),
            "weather-overcast-symbolic"
        );
        assert_eq!(
            FileSyncStatus::HydratedOffline.icon_name(),
            "emblem-ok-symbolic"
        );
    }

    #[test]
    fn breadcrumb_segments_splitting() {
        let root_segs = FileViewItem::breadcrumb_segments("/");
        assert_eq!(root_segs.len(), 1);
        assert_eq!(root_segs[0].name, "Nextcloud");
        assert_eq!(root_segs[0].full_path, "/");

        let nested = FileViewItem::breadcrumb_segments("/Documents/Projects");
        assert_eq!(nested.len(), 3);
        assert_eq!(nested[1].name, "Documents");
        assert_eq!(nested[1].full_path, "/Documents");
        assert_eq!(nested[2].name, "Projects");
        assert_eq!(nested[2].full_path, "/Documents/Projects");
    }
}
