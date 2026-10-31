//! Typed access to the mode stored on an SDK sync-mount record.
//!
//! `StoredSyncMount::mode` is a plain string, so callers compared it against literals. A typo in
//! one of those comparisons silently selected the wrong branch — treating a FUSE mount as a plain
//! virtual one, for instance, which then wrote payloads into a directory the mount also serves.
//! Reading it once through the SDK's [`MountMode`] removes both the literals and that mistake.

use nexia_nextcloud_sdk::{MountMode, StoredSyncMount};

/// Reads the mode of a stored mount record as the SDK's typed mode.
pub trait StoredMountMode {
    /// Returns how this mount serves its files.
    fn mount_mode(&self) -> MountMode;
}

impl StoredMountMode for StoredSyncMount {
    fn mount_mode(&self) -> MountMode {
        match self.mode.as_str() {
            "BidirectionalSync" => MountMode::BidirectionalSync,
            "Fuse" => MountMode::Fuse,
            // The SDK's default, and the least destructive reading of a value written by a newer
            // build: a placeholder mount never overwrites the files in its target directory.
            _ => MountMode::VirtualVfs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(mode: &str) -> StoredSyncMount {
        StoredSyncMount {
            id: "mount".to_string(),
            account_id: "account".to_string(),
            remote_path: "/".to_string(),
            local_target: "/tmp/target".to_string(),
            mode: mode.to_string(),
            display_name: None,
            enabled: true,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn stored_modes_map_onto_the_typed_mode() {
        assert_eq!(stored("VirtualVfs").mount_mode(), MountMode::VirtualVfs);
        assert_eq!(stored("Fuse").mount_mode(), MountMode::Fuse);
        assert_eq!(
            stored("BidirectionalSync").mount_mode(),
            MountMode::BidirectionalSync
        );
        assert_eq!(stored("FuseV2").mount_mode(), MountMode::VirtualVfs);
    }
}
