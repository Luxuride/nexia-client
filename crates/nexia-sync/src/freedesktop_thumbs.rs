//! FreeDesktop Thumbnail Managing Standard integration re-exported from `nexia_nextcloud_sdk`.

/// Returns whether `bytes` look like a thumbnail this application installed.
///
/// System thumbnail caches are shared with every other application, so scoped
/// accounting and cleanup has to identify its own entries. The PNG `Software`
/// chunk written by `crate::THUMBNAIL_SOFTWARE` is that marker.
pub(crate) fn is_app_thumbnail(bytes: &[u8]) -> bool {
    bytes
        .windows(crate::THUMBNAIL_SOFTWARE.len())
        .any(|w| w == crate::THUMBNAIL_SOFTWARE.as_bytes())
}

pub use nexia_nextcloud_sdk::{
    crc32, embed_freedesktop_metadata, ensure_png_bytes, freedesktop_thumbnail_dirs,
    install_freedesktop_fail_marker, install_freedesktop_thumbnail,
    install_freedesktop_thumbnail_with_mtime, make_png_text_chunk, md5_hex, path_to_file_uri,
    remove_freedesktop_fail_marker,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_thumbnails_tagged_by_this_app() {
        let tagged = make_png_text_chunk("Software", crate::THUMBNAIL_SOFTWARE);
        assert!(is_app_thumbnail(&tagged));
    }

    #[test]
    fn ignores_thumbnails_tagged_by_another_program() {
        // A different client's thumbnail can live in the same shared cache, so a
        // product name that is not ours must not be counted or deleted.
        let foreign = make_png_text_chunk("Software", "Nextcloud Desktop");
        assert!(!is_app_thumbnail(&foreign));

        let generic = make_png_text_chunk("Software", "GNOME Thumbnail Factory");
        assert!(!is_app_thumbnail(&generic));
    }

    #[test]
    fn ignores_an_untagged_thumbnail() {
        // Fail markers written by the SDK carry no Software chunk at all.
        let bare = make_png_text_chunk("Thumb::URI", "file:///tmp/photo.jpg");
        assert!(!is_app_thumbnail(&bare));
    }
}
