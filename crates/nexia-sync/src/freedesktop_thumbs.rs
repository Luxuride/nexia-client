//! FreeDesktop Thumbnail Managing Standard integration re-exported from `nexia_nextcloud_sdk`.

use std::io::Read;
use std::path::Path;

/// Number of leading bytes of a PNG that carry the metadata chunks this application reads.
const THUMBNAIL_HEADER_BYTES: usize = 2048;

/// The size directories the Thumbnail Managing Standard defines.
pub(crate) const THUMBNAIL_SIZE_DIRS: [&str; 4] = ["normal", "large", "x-large", "xx-large"];

/// Returns whether the thumbnail file at `path` was installed by this application.
pub(crate) fn is_app_thumbnail_file(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; THUMBNAIL_HEADER_BYTES];
    match file.read(&mut buf) {
        Ok(read) => is_app_thumbnail(&buf[..read]),
        Err(_) => false,
    }
}

/// The size directories of the shared system thumbnail cache.
fn app_thumbnail_dirs() -> Vec<std::path::PathBuf> {
    let (normal_dir, _) = freedesktop_thumbnail_dirs();
    match normal_dir.parent() {
        Some(parent) => THUMBNAIL_SIZE_DIRS
            .iter()
            .map(|name| parent.join(name))
            .collect(),
        None => Vec::new(),
    }
}

/// Returns the total size in bytes of the thumbnails this application installed in the shared
/// system thumbnail cache.
pub(crate) fn app_thumbnail_bytes() -> u64 {
    app_thumbnail_dirs()
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.flatten())
        .filter(|entry| is_app_thumbnail_file(&entry.path()))
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum()
}

/// Deletes the thumbnails this application installed in the shared system thumbnail cache,
/// returning the number of bytes freed.
pub(crate) fn remove_app_thumbnails() -> u64 {
    let mut freed = 0;
    for dir in app_thumbnail_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_app_thumbnail_file(&path) {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                freed += meta.len();
            }
            let _ = std::fs::remove_file(&path);
        }
    }
    freed
}

/// Returns whether `bytes` look like a thumbnail this application installed.
///
/// System thumbnail caches are shared with every other application, so scoped
/// accounting and cleanup has to identify its own entries. The PNG `Software`
/// chunk written by `crate::THUMBNAIL_SOFTWARE` is that marker.
pub(crate) fn is_app_thumbnail(bytes: &[u8]) -> bool {
    // Anchored on the `Software` text-chunk keyword instead of scanning for the product name
    // alone: a `Thumb::URI` chunk naming a file like `Nexia Client notes.png` would otherwise be
    // mistaken for a thumbnail this application wrote, and then deleted or counted.
    const KEYWORD: &[u8] = b"Software\0";
    let product = crate::THUMBNAIL_SOFTWARE.as_bytes();
    let mut cursor = bytes;
    while let Some(idx) = cursor
        .windows(KEYWORD.len())
        .position(|window| window == KEYWORD)
    {
        let value = &cursor[idx + KEYWORD.len()..];
        if value.starts_with(product) {
            return true;
        }
        cursor = &cursor[idx + KEYWORD.len()..];
    }
    false
}

pub use nexia_nextcloud_sdk::{
    crc32, embed_freedesktop_metadata, ensure_png_bytes, freedesktop_thumbnail_dirs,
    freedesktop_thumbnail_is_current, install_freedesktop_fail_marker,
    install_freedesktop_thumbnail, install_freedesktop_thumbnail_with_mtime, make_png_text_chunk,
    md5_hex, path_to_file_uri, remove_freedesktop_fail_marker,
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
    fn ignores_a_product_name_that_only_appears_in_the_uri() {
        // The product name inside `Thumb::URI` is not a tag: it must not mark the file as ours.
        let mut bytes = make_png_text_chunk("Thumb::URI", "file:///tmp/Nexia Client notes.png");
        bytes.extend_from_slice(&make_png_text_chunk("Software", "GNOME Thumbnail Factory"));
        assert!(!is_app_thumbnail(&bytes));
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
