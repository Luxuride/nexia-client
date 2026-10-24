//! Filesystem and WebDAV scanners for discovering local and remote items.

use nextcloud_rust_sdk::{BasicAuth, DavClient, Depth, PropfindBody, ReqwestTransport};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::SyncError;
use crate::vfs::make_dav_path;

/// An item discovered on the local filesystem during traversal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredLocalFile {
    /// Relative path within the local sync root (e.g. `/Documents/report.txt`).
    pub relative_path: String,
    /// Absolute host path on disk.
    pub absolute_path: PathBuf,
    /// Size in bytes.
    pub size: u64,
    /// Last modification timestamp.
    pub mtime: SystemTime,
    /// Whether this entry is a directory.
    pub is_directory: bool,
}

/// An item discovered on the remote Nextcloud instance via WebDAV.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredRemoteFile {
    /// Relative path within the remote sync root (e.g. `/Documents/report.txt`).
    pub relative_path: String,
    /// Full WebDAV resource href.
    pub href: String,
    /// Remote file size in bytes.
    pub size: u64,
    /// Remote ETag string.
    pub etag: Option<String>,
    /// Whether this entry is a collection/directory.
    pub is_directory: bool,
}

/// Scans a local directory recursively, collecting all files and folders.
pub fn scan_local_directory(root: &Path) -> Result<Vec<DiscoveredLocalFile>, SyncError> {
    let mut files = Vec::new();
    scan_dir_recursive(root, root, &mut files)?;
    Ok(files)
}

fn scan_dir_recursive(
    root: &Path,
    current: &Path,
    collected: &mut Vec<DiscoveredLocalFile>,
) -> Result<(), SyncError> {
    if !current.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(current).map_err(|err| SyncError::LocalIo {
        path: current.to_path_buf(),
        context: "reading local directory".to_string(),
        source: err,
    })?;

    for entry in entries {
        let entry = entry.map_err(|err| SyncError::LocalIo {
            path: current.to_path_buf(),
            context: "reading local directory entry".to_string(),
            source: err,
        })?;

        let path = entry.path();
        let meta = entry.metadata().map_err(|err| SyncError::LocalIo {
            path: path.clone(),
            context: "reading local metadata".to_string(),
            source: err,
        })?;

        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let canonical_rel = format!("/{}", rel.trim_start_matches('/'));

        let is_dir = meta.is_dir();
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        collected.push(DiscoveredLocalFile {
            relative_path: canonical_rel,
            absolute_path: path.clone(),
            size: meta.len(),
            mtime,
            is_directory: is_dir,
        });

        if is_dir {
            scan_dir_recursive(root, &path, collected)?;
        }
    }

    Ok(())
}

/// Scans remote Nextcloud WebDAV hierarchy starting from `base_remote_path`.
pub async fn scan_remote_hierarchy(
    dav_client: &DavClient<'_, ReqwestTransport, BasicAuth>,
    user: &str,
    base_remote_path: &str,
    dav_prefix: &str,
) -> Result<Vec<DiscoveredRemoteFile>, SyncError> {
    let dav_path = make_dav_path(user, base_remote_path)?;
    let body = PropfindBody::all();

    let resources = dav_client
        .propfind(&dav_path, Depth::Infinity, &body)
        .await
        .map_err(SyncError::WebDav)?;

    let mut discovered = Vec::new();
    let prefix = dav_prefix.trim_end_matches('/');

    for res in resources {
        let relative = if res.href.starts_with(prefix) {
            &res.href[prefix.len()..]
        } else {
            &res.href
        };

        let clean_path = if relative.is_empty() || relative == "/" {
            "/".to_string()
        } else {
            format!("/{}", relative.trim_matches('/'))
        };

        if clean_path == "/" {
            continue;
        }

        discovered.push(DiscoveredRemoteFile {
            relative_path: clean_path,
            href: res.href,
            size: res.content_length.unwrap_or(0),
            etag: res.etag,
            is_directory: res.is_collection,
        });
    }

    Ok(discovered)
}
