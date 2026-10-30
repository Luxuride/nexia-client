//! Presentation models and UI state representations backed by `nexia-nextcloud-sdk`.

pub use nexia_nextcloud_sdk::{
    BreadcrumbSegment as PathSegment, DirectorySizeSummary, FileSortCriteria, FileSortField,
    FileSortOrder, ResourceIconCategory, ThemeInfo, breadcrumb_segments, format_bytes,
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
            Self::SyncFolders => "folder-download-symbolic",
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
    /// Standard / default icon presentation (64px grid icon).
    #[default]
    Medium,
    /// Large icon presentation (96px grid icon).
    Large,
    /// Extra large icon presentation (128px grid icon).
    ExtraLarge,
}

impl ZoomLevel {
    /// Returns the pixel dimension for grid view icons / thumbnails.
    pub const fn grid_icon_size(&self) -> i32 {
        match self {
            Self::Small => 64,
            Self::Medium => 96,
            Self::Large => 128,
            Self::ExtraLarge => 192,
        }
    }

    /// Returns the target pixel width of the item container card in grid view.
    pub const fn grid_item_width(&self) -> i32 {
        match self {
            Self::Small => 130,
            Self::Medium => 160,
            Self::Large => 200,
            Self::ExtraLarge => 260,
        }
    }

    /// Returns the pixel dimension for list view row icons.
    pub const fn list_icon_size(&self) -> i32 {
        match self {
            Self::Small => 28,
            Self::Medium => 36,
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

    /// Returns the zoom percentage (75%, 100%, 150%, 200%).
    pub const fn percentage(&self) -> u32 {
        match self {
            Self::Small => 75,
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
            Self::HydratedOffline => "object-select-symbolic",
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
    /// Nextcloud internal numeric file ID (`oc:fileid`) if available.
    pub file_id: Option<u64>,
    /// Semantic icon category computed by `nexia-nextcloud-sdk`.
    pub icon_category: ResourceIconCategory,
    /// Last modification timestamp if available.
    pub modified_at: Option<String>,
    /// Permissions flags reported by Nextcloud if available.
    pub permissions: Option<String>,
    /// Aggregated directory details if this item is a folder.
    pub directory_summary: Option<DirectorySizeSummary>,
}

impl FileViewItem {
    /// Formats a raw byte count into human-readable units via `nexia-nextcloud-sdk`.
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
        // The field pass uses `sort_by_cached_key`, which lowercases each name once instead of
        // allocating two lowercase copies for every comparison. The folder rank is a second,
        // stable pass: it lifts directories above files while keeping the field order inside
        // each group, which is why only the field pass is reversed for a descending sort.
        match criteria.field {
            FileSortField::Name => items.sort_by_cached_key(|i| i.name.to_lowercase()),
            FileSortField::Size => items.sort_by_cached_key(|i| i.size_bytes),
            FileSortField::Modified => items.sort_by_cached_key(|i| i.modified_at.clone()),
            FileSortField::Type => items.sort_by_cached_key(|i| {
                let ext = std::path::Path::new(&i.name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                (ext, i.name.to_lowercase())
            }),
        }

        if criteria.order == FileSortOrder::Descending {
            items.reverse();
        }

        if criteria.folders_first {
            items.sort_by_key(|i| !i.is_directory);
        }
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

    /// Splits a canonical virtual path into clickable Nautilus-style breadcrumb segments via `nexia-nextcloud-sdk`.
    pub fn breadcrumb_segments(current_path: &str) -> Vec<PathSegment> {
        breadcrumb_segments(current_path, "Nexia")
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

    /// Builds the minimal item a sort test needs: name, kind and size.
    fn item(name: &str, is_directory: bool, size_bytes: u64) -> FileViewItem {
        FileViewItem {
            path: format!("/{name}"),
            name: name.to_string(),
            is_directory,
            size_formatted: String::new(),
            size_bytes,
            status: FileSyncStatus::CloudPlaceholder,
            mime_type: None,
            file_id: None,
            icon_category: if is_directory {
                ResourceIconCategory::Folder
            } else {
                ResourceIconCategory::Document
            },
            modified_at: None,
            permissions: None,
            directory_summary: None,
        }
    }

    fn names(items: &[FileViewItem]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn sort_items_is_case_insensitive_by_name() {
        let mut items = vec![item("beta.txt", false, 0), item("Alpha.txt", false, 0)];
        FileViewItem::sort_items(
            &mut items,
            &FileSortCriteria::new(FileSortField::Name, FileSortOrder::Ascending, false),
        );
        assert_eq!(names(&items), ["Alpha.txt", "beta.txt"]);
    }

    #[test]
    fn sort_items_keeps_folders_first_for_both_orders() {
        for order in [FileSortOrder::Ascending, FileSortOrder::Descending] {
            let mut items = vec![
                item("a.txt", false, 3),
                item("b-folder", true, 1),
                item("c.txt", false, 1),
                item("d-folder", true, 9),
            ];
            FileViewItem::sort_items(
                &mut items,
                &FileSortCriteria::new(FileSortField::Size, order, true),
            );
            assert!(items[0].is_directory, "folders stay on top for {order:?}");
            assert!(items[1].is_directory, "folders stay on top for {order:?}");
            assert!(!items[2].is_directory);
            assert!(!items[3].is_directory);
        }
    }

    #[test]
    fn sort_items_descending_reverses_only_the_field() {
        let mut items = vec![
            item("small.txt", false, 1),
            item("large.txt", false, 10),
            item("folder", true, 0),
        ];
        FileViewItem::sort_items(
            &mut items,
            &FileSortCriteria::new(FileSortField::Size, FileSortOrder::Descending, true),
        );
        assert_eq!(names(&items), ["folder", "large.txt", "small.txt"]);
    }

    #[test]
    fn sort_items_by_type_groups_extensions() {
        let mut items = vec![
            item("z.txt", false, 0),
            item("a.png", false, 0),
            item("m.png", false, 0),
        ];
        FileViewItem::sort_items(
            &mut items,
            &FileSortCriteria::new(FileSortField::Type, FileSortOrder::Ascending, false),
        );
        assert_eq!(names(&items), ["a.png", "m.png", "z.txt"]);
    }

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
            "object-select-symbolic"
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
        assert_eq!(root_segs[0].name, "Nexia");
        assert_eq!(root_segs[0].full_path, "/");

        let nested = FileViewItem::breadcrumb_segments("/Documents/Projects");
        assert_eq!(nested.len(), 3);
        assert_eq!(nested[1].name, "Documents");
        assert_eq!(nested[1].full_path, "/Documents");
        assert_eq!(nested[2].name, "Projects");
        assert_eq!(nested[2].full_path, "/Documents/Projects");
    }
}
