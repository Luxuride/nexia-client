//! FreeDesktop Thumbnail Managing Standard integration re-exported from `nextcloud_rust_sdk`.

pub use nextcloud_rust_sdk::{
    crc32, embed_freedesktop_metadata, ensure_png_bytes, freedesktop_thumbnail_dirs,
    install_freedesktop_thumbnail, install_freedesktop_thumbnail_with_mtime, make_png_text_chunk,
    md5_hex, path_to_file_uri,
};
