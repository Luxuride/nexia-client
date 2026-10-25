//! Filesystem and WebDAV scanners utilizing nextcloud-rust-sdk.

use nextcloud_rust_sdk::{BasicAuth, DavClient, Depth, PropfindBody, ReqwestTransport};
pub use nextcloud_rust_sdk::{DiscoveredLocalFile, DiscoveredRemoteFile, scan_local_directory};

use crate::error::SyncError;
use crate::vfs::make_dav_path;

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
