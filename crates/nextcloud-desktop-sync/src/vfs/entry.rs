//! Virtual file system entry summary re-export and WebDAV path helper.

use nextcloud_rust_sdk::DavPath;
pub use nextcloud_rust_sdk::VirtualEntrySummary;

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
