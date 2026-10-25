//! Virtual file system entry summary model and WebDAV path helper.

use nextcloud_rust_sdk::{DavPath, ResourceIconCategory, VirtualNode};
use std::path::PathBuf;

use crate::error::SyncError;

/// Constructs a normalized [`DavPath`] from user name and relative path.
pub fn make_dav_path(user: &str, relative: &str) -> Result<DavPath, SyncError> {
    let clean = relative.trim_matches('/');
    if clean.is_empty() {
        DavPath::root(user).map_err(|err| SyncError::InvalidPath(err.to_string()))
    } else {
        DavPath::new(user, clean).map_err(|err| SyncError::InvalidPath(err.to_string()))
    }
}

/// Summary representation of a virtual file or directory entry for UI display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VirtualEntrySummary {
    /// Canonical relative path within the virtual mount (e.g. `/Photos/cat.jpg`).
    pub path: String,
    /// Display file or folder name.
    pub name: String,
    /// Whether this entry represents a directory.
    pub is_directory: bool,
    /// File size in bytes (as reported by metadata).
    pub size: u64,
    /// ETag identifier.
    pub etag: Option<String>,
    /// Last modification timestamp as formatted string.
    pub modified_at: Option<String>,
    /// Permission flags string reported by Nextcloud.
    pub permissions: Option<String>,
    /// Content MIME type.
    pub mime_type: Option<String>,
    /// Semantic Freedesktop icon category determined by the SDK.
    pub icon_category: ResourceIconCategory,
    /// Whether the file is currently a metadata-only placeholder or hydrated.
    pub is_hydrated: bool,
    /// Local disk cache path if hydrated.
    pub cached_path: Option<PathBuf>,
}

impl From<&VirtualNode> for VirtualEntrySummary {
    fn from(node: &VirtualNode) -> Self {
        Self {
            path: node.metadata.path.clone(),
            name: node.metadata.name.clone(),
            is_directory: node.metadata.is_directory,
            size: node.metadata.size,
            etag: node.metadata.etag.clone(),
            modified_at: node.metadata.modified_at.clone(),
            permissions: node.metadata.permissions.clone(),
            mime_type: node.metadata.mime_type.clone(),
            icon_category: node.metadata.icon_category(),
            is_hydrated: node.hydration.is_hydrated(),
            cached_path: node.hydration.cached_path().map(|p| p.to_path_buf()),
        }
    }
}
