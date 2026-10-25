//! Presentation models and UI state representations backed by `nextcloud-rust-sdk`.

pub use nextcloud_rust_sdk::{
    BreadcrumbSegment as PathSegment, FileSortCriteria, FileSortField, FileSortOrder,
    ResourceIconCategory, ThemeInfo, breadcrumb_segments, format_bytes,
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

impl NavigationPage {
    /// Returns all top-level navigation pages in display order.
    pub const fn all() -> [Self; 4] {
        [
            Self::CloudFiles,
            Self::SyncFolders,
            Self::Transfers,
            Self::Settings,
        ]
    }

    /// Returns the human-readable title of this page.
    pub const fn title(&self) -> &'static str {
        match self {
            Self::CloudFiles => "Cloud Files",
            Self::SyncFolders => "Sync Folders",
            Self::Transfers => "Activity",
            Self::Settings => "Settings",
        }
    }

    /// Returns the internal stack identifier for this page.
    pub const fn stack_name(&self) -> &'static str {
        match self {
            Self::CloudFiles => "cloud_files",
            Self::SyncFolders => "sync_folders",
            Self::Transfers => "transfers",
            Self::Settings => "settings",
        }
    }

    /// Returns the symbolic icon name for the sidebar row.
    pub const fn icon_name(&self) -> &'static str {
        match self {
            Self::CloudFiles => "folder-remote-symbolic",
            Self::SyncFolders => "emblem-synchronizing-symbolic",
            Self::Transfers => "network-transmit-receive-symbolic",
            Self::Settings => "emblem-system-symbolic",
        }
    }

    /// Maps a sidebar row index to its [`NavigationPage`].
    pub const fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Self::CloudFiles),
            1 => Some(Self::SyncFolders),
            2 => Some(Self::Transfers),
            3 => Some(Self::Settings),
            _ => None,
        }
    }
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

/// Nautilus-style zoom levels for the file grid and list views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ZoomLevel {
    /// Smallest icon presentation (48px grid icon, compact list).
    Small,
    /// Standard / default icon presentation (96px grid icon).
    #[default]
    Medium,
    /// Large icon presentation (128px grid icon).
    Large,
    /// Extra large icon presentation (192px grid icon).
    ExtraLarge,
}

impl ZoomLevel {
    /// Returns the pixel dimension for grid view icons / thumbnails.
    pub const fn grid_icon_size(&self) -> i32 {
        match self {
            Self::Small => 48,
            Self::Medium => 96,
            Self::Large => 128,
            Self::ExtraLarge => 192,
        }
    }

    /// Returns the target pixel width of the item container card in grid view.
    pub const fn grid_item_width(&self) -> i32 {
        match self {
            Self::Small => 90,
            Self::Medium => 120,
            Self::Large => 160,
            Self::ExtraLarge => 220,
        }
    }

    /// Returns the pixel dimension for list view row icons.
    pub const fn list_icon_size(&self) -> i32 {
        match self {
            Self::Small => 24,
            Self::Medium => 32,
            Self::Large => 48,
            Self::ExtraLarge => 64,
        }
    }

    /// Steps zoom in one level up, if not already at maximum.
    pub const fn zoom_in(&self) -> Self {
        match self {
            Self::Small => Self::Medium,
            Self::Medium => Self::Large,
            Self::Large | Self::ExtraLarge => Self::ExtraLarge,
        }
    }

    /// Steps zoom out one level down, if not already at minimum.
    pub const fn zoom_out(&self) -> Self {
        match self {
            Self::ExtraLarge => Self::Large,
            Self::Large => Self::Medium,
            Self::Medium | Self::Small => Self::Small,
        }
    }

    /// Returns the zoom percentage (50%, 100%, 150%, 200%).
    pub const fn percentage(&self) -> u32 {
        match self {
            Self::Small => 50,
            Self::Medium => 100,
            Self::Large => 150,
            Self::ExtraLarge => 200,
        }
    }

    /// Formatted display label (e.g. "100%").
    pub fn display_label(&self) -> String {
        format!("{}%", self.percentage())
    }

    /// Converts this zoom level to a static string key for configuration persistence.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::ExtraLarge => "extra_large",
        }
    }

    /// Parses a zoom level from a configuration string.
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "small" | "50" | "50%" => Self::Small,
            "large" | "150" | "150%" => Self::Large,
            "extra_large" | "extralarge" | "200" | "200%" => Self::ExtraLarge,
            _ => Self::Medium,
        }
    }
}

/// Status of an individual file or directory in the virtual browser view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSyncStatus {
    /// Remote metadata placeholder; 0 bytes consumed on local disk (Virtual file / contains unhydrated files).
    CloudPlaceholder,
    /// File content downloaded and available offline in local cache (Hydrated file).
    HydratedOffline,
    /// Local-only file or changes waiting to sync to Nextcloud.
    LocalOnly,
    /// Contains both unhydrated files and unsynced local modifications.
    UnhydratedAndUnsynced,
    /// File currently undergoing upload or download.
    Syncing,
}

impl FileSyncStatus {
    /// Formats the status as a human-readable display string.
    pub fn display_label(&self) -> &'static str {
        match self {
            Self::CloudPlaceholder => "Virtual File (Cloud Only)",
            Self::HydratedOffline => "Hydrated (Available Offline)",
            Self::LocalOnly => "Local Only (Waiting to Sync)",
            Self::UnhydratedAndUnsynced => "Unhydrated & Unsynced",
            Self::Syncing => "Syncing…",
        }
    }

    /// Returns the Freedesktop symbolic icon name representing this status.
    pub fn icon_name(&self) -> &'static str {
        match self {
            Self::CloudPlaceholder => "folder-remote-symbolic",
            Self::HydratedOffline => "emblem-ok-symbolic",
            Self::LocalOnly => "go-up-symbolic",
            Self::UnhydratedAndUnsynced => "folder-remote-symbolic",
            Self::Syncing => "view-refresh-symbolic",
        }
    }

    /// Returns the Libadwaita CSS style class associated with this status.
    pub fn css_class(&self) -> &'static str {
        match self {
            Self::CloudPlaceholder => "dim-label",
            Self::HydratedOffline => "success",
            Self::LocalOnly => "warning",
            Self::UnhydratedAndUnsynced => "warning",
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
    /// Last modification timestamp if available.
    pub modified_at: Option<String>,
    /// Permissions flags reported by Nextcloud if available.
    pub permissions: Option<String>,
}

impl FileViewItem {
    /// Formats a raw byte count into human-readable units via `nextcloud-rust-sdk`.
    pub fn format_bytes(bytes: u64) -> String {
        format_bytes(bytes)
    }

    /// Returns the full-color Adwaita / Nautilus icon name matching this item.
    pub fn nautilus_icon_name(&self) -> &'static str {
        if self.is_directory {
            match self.status {
                FileSyncStatus::CloudPlaceholder | FileSyncStatus::UnhydratedAndUnsynced => {
                    "folder-remote"
                }
                FileSyncStatus::LocalOnly => "folder-download",
                FileSyncStatus::HydratedOffline | FileSyncStatus::Syncing => "folder",
            }
        } else {
            self.icon_category.freedesktop_icon_name()
        }
    }

    /// Sorts a slice of [`FileViewItem`] in place according to the specified criteria.
    pub fn sort_items(items: &mut [Self], criteria: &FileSortCriteria) {
        items.sort_by(|a, b| {
            if criteria.folders_first && a.is_directory != b.is_directory {
                return if a.is_directory {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                };
            }

            let ord = match criteria.field {
                FileSortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                FileSortField::Size => a.size_bytes.cmp(&b.size_bytes),
                FileSortField::Modified => a.modified_at.cmp(&b.modified_at),
                FileSortField::Type => {
                    let ext_a = std::path::Path::new(&a.name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let ext_b = std::path::Path::new(&b.name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    ext_a
                        .cmp(&ext_b)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
            };

            match criteria.order {
                FileSortOrder::Ascending => ord,
                FileSortOrder::Descending => ord.reverse(),
            }
        });
    }

    /// Returns the symbolic icon name matching this item.
    pub const fn symbolic_icon_name(&self) -> &'static str {
        self.icon_category.symbolic_icon_name()
    }

    /// Returns a human-readable category type label (e.g. `"Folder"`, `"Document"`, `"Image"`).
    pub const fn type_label(&self) -> &'static str {
        match self.icon_category {
            ResourceIconCategory::Folder
            | ResourceIconCategory::DocumentsFolder
            | ResourceIconCategory::PicturesFolder
            | ResourceIconCategory::MusicFolder
            | ResourceIconCategory::VideosFolder
            | ResourceIconCategory::DownloadsFolder
            | ResourceIconCategory::TemplatesFolder
            | ResourceIconCategory::PublicShareFolder => "Folder",
            ResourceIconCategory::Document => "Document",
            ResourceIconCategory::Spreadsheet => "Spreadsheet",
            ResourceIconCategory::Presentation => "Presentation",
            ResourceIconCategory::Image => "Image",
            ResourceIconCategory::Audio => "Audio",
            ResourceIconCategory::Video => "Video",
            ResourceIconCategory::Archive => "Archive",
            ResourceIconCategory::Text => "Plain Text",
        }
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
            "Virtual File (Cloud Only)"
        );
        assert_eq!(
            FileSyncStatus::HydratedOffline.display_label(),
            "Hydrated (Available Offline)"
        );
        assert_eq!(
            FileSyncStatus::LocalOnly.display_label(),
            "Local Only (Waiting to Sync)"
        );
        assert_eq!(
            FileSyncStatus::CloudPlaceholder.icon_name(),
            "folder-remote-symbolic"
        );
        assert_eq!(
            FileSyncStatus::HydratedOffline.icon_name(),
            "emblem-ok-symbolic"
        );
        assert_eq!(FileSyncStatus::LocalOnly.icon_name(), "go-up-symbolic");
        assert_eq!(FileSyncStatus::Syncing.icon_name(), "view-refresh-symbolic");
        assert_eq!(FileSyncStatus::Syncing.display_label(), "Syncing…");
        assert_eq!(FileSyncStatus::Syncing.css_class(), "accent");
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
