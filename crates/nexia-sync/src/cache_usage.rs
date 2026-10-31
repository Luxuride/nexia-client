//! Disk-usage accounting for the cache directories this application owns.

use std::path::Path;

/// Files and bytes found below a directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DirUsage {
    /// Number of regular files found.
    pub files: usize,
    /// Sum of their sizes in bytes.
    pub bytes: u64,
}

impl DirUsage {
    /// Adds another usage reading to this one.
    fn add(&mut self, other: Self) {
        self.files += other.files;
        self.bytes += other.bytes;
    }
}

/// Sums the size and count of every regular file below `root`.
///
/// The walk is iterative and ignores what it cannot stat: a cache directory may hold entries for
/// files another process is deleting, and a size report is not worth failing over.
pub(crate) fn scan_dir_usage(root: &Path) -> DirUsage {
    let mut usage = DirUsage::default();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file()
                && let Ok(meta) = entry.metadata()
            {
                usage.files += 1;
                usage.bytes += meta.len();
            }
        }
    }

    usage
}

/// Sums the usage of several cache directories.
pub(crate) fn scan_dirs_usage<'a>(
    dirs: impl IntoIterator<Item = &'a std::path::PathBuf>,
) -> DirUsage {
    let mut usage = DirUsage::default();
    for dir in dirs {
        usage.add(scan_dir_usage(dir));
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_dir_usage_sums_nested_files_only() {
        let root = std::env::temp_dir().join(format!("nexia-usage-{}", std::process::id()));
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).expect("create dirs");
        std::fs::write(root.join("one.bin"), b"12345").expect("write");
        std::fs::write(nested.join("two.bin"), b"123").expect("write");

        let usage = scan_dir_usage(&root);
        assert_eq!(usage.files, 2);
        assert_eq!(usage.bytes, 8);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_dir_usage_of_a_missing_directory_is_empty() {
        let missing = std::env::temp_dir().join("nexia-usage-does-not-exist");
        assert_eq!(scan_dir_usage(&missing), DirUsage::default());
    }
}
