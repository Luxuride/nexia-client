//! Synchronization coordinator module.

pub use crate::coordinator::DesktopSyncCoordinator;

#[cfg(test)]
mod tests {
    use super::*;
    use nextcloud_desktop_core::account::{Account, AccountCredentials, ServerConfig};
    use nextcloud_desktop_core::config::AppConfig;
    use nextcloud_desktop_core::events::EventBus;
    use nextcloud_rust_sdk::{MountDescriptor, MountMode};
    use std::sync::Arc;

    #[tokio::test]
    async fn account_management_and_switching() {
        let config = AppConfig::default();
        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));

        let acc_alice = Account::new(
            "Alice Nextcloud",
            ServerConfig::new("https://cloud.alice.org"),
            AccountCredentials::new("alice", "token123"),
        );
        let alice_id = acc_alice.id.clone();
        coordinator
            .add_account(acc_alice)
            .await
            .expect("add alice succeeds");

        let accounts = coordinator.accounts().await;
        assert_eq!(accounts.len(), 1);
        assert_eq!(coordinator.active_account().await.unwrap().id, alice_id);

        let acc_bob = Account::new(
            "Bob Nextcloud",
            ServerConfig::new("https://cloud.bob.org"),
            AccountCredentials::new("bob", "token456"),
        );
        let bob_id = acc_bob.id.clone();
        coordinator
            .add_account(acc_bob)
            .await
            .expect("add bob succeeds");

        assert_eq!(coordinator.accounts().await.len(), 2);
        assert_eq!(coordinator.active_account().await.unwrap().id, bob_id);

        coordinator
            .switch_account(&alice_id)
            .await
            .expect("switch to alice succeeds");
        assert_eq!(coordinator.active_account().await.unwrap().id, alice_id);
    }

    #[tokio::test]
    async fn orchestrator_mounts_pause_resume_and_file_status() {
        let temp_dir = std::env::temp_dir().join(format!("nc_test_orch_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let config = AppConfig {
            cache_dir: Some(temp_dir.clone()),
            ..Default::default()
        };
        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));

        assert!(coordinator.is_orchestrator_running());

        // Add root mount
        let target_root = temp_dir.join("nextcloud");
        let desc_root = MountDescriptor::new("acc1", "/", &target_root, MountMode::VirtualVfs);
        let root_id = coordinator
            .add_mount(desc_root)
            .await
            .expect("add root mount");

        // Add subdirectory mount to distinct target
        let target_photos = temp_dir.join("photos");
        let desc_photos = MountDescriptor::new(
            "acc1",
            "/Photos",
            &target_photos,
            MountMode::BidirectionalSync,
        );
        let photos_id = coordinator
            .add_mount(desc_photos)
            .await
            .expect("add photos mount");

        let mounts = coordinator.list_mounts().await.expect("list mounts");
        assert_eq!(mounts.len(), 2);

        // Pause specific mount
        assert!(
            !coordinator
                .is_mount_paused(photos_id.clone())
                .await
                .unwrap()
        );
        coordinator
            .pause_mount(photos_id.clone())
            .await
            .expect("pause mount");
        assert!(
            coordinator
                .is_mount_paused(photos_id.clone())
                .await
                .unwrap()
        );
        coordinator
            .resume_mount(photos_id.clone())
            .await
            .expect("resume mount");
        assert!(
            !coordinator
                .is_mount_paused(photos_id.clone())
                .await
                .unwrap()
        );

        // Pause globally
        assert!(!coordinator.is_sync_paused().await.unwrap());
        coordinator.pause_all_sync().await.expect("pause all sync");
        assert!(coordinator.is_sync_paused().await.unwrap());
        coordinator
            .resume_all_sync()
            .await
            .expect("resume all sync");
        assert!(!coordinator.is_sync_paused().await.unwrap());

        // File status and icons
        let status = coordinator
            .file_status(root_id, "/Document.txt")
            .await
            .expect("file status");
        assert_eq!(status.icon_name(), "cloud-symbolic");

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn orchestrator_vfs_file_mutations() {
        let temp_dir = std::env::temp_dir().join(format!("nc_test_vfs_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let config = AppConfig {
            cache_dir: Some(temp_dir.clone()),
            ..Default::default()
        };
        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));

        let vfs = coordinator.active_vfs().await;
        let root_entries = vfs.list_children("/").await.expect("list root");
        assert!(root_entries.is_empty());

        coordinator
            .create_directory("/TestFolder")
            .await
            .expect("create directory");
        coordinator
            .create_file("/TestFolder/sample.txt", b"orchestrator data")
            .await
            .expect("create file");

        let folder_entries = vfs.list_children("/TestFolder").await.expect("list folder");
        assert!(folder_entries.iter().any(|e| e.name == "sample.txt"));

        let renamed = coordinator
            .rename_resource("/TestFolder/sample.txt", "renamed.txt")
            .await
            .expect("rename");
        assert_eq!(renamed, "/TestFolder/renamed.txt");

        coordinator
            .delete_resource("/TestFolder/renamed.txt")
            .await
            .expect("delete");

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn empty_search_returns_empty_and_valid_search_finds_entries() {
        let temp_dir = std::env::temp_dir().join(format!(
            "nextcloud_search_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let config = AppConfig {
            cache_dir: Some(temp_dir.clone()),
            ..Default::default()
        };
        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));

        // 1. Empty or whitespace search must return empty immediately without searching
        let empty1 = coordinator.search("").await.expect("empty search");
        assert!(empty1.is_empty());

        let empty2 = coordinator.search("   ").await.expect("whitespace search");
        assert!(empty2.is_empty());

        // 2. Add an item and verify non-empty search runs asynchronously and finds it
        coordinator
            .create_file("/budget_report_2026.pdf", b"data")
            .await
            .expect("create file");

        let found = coordinator.search("budget").await.expect("search budget");
        assert!(found.iter().any(|m| m.name == "budget_report_2026.pdf"));

        let not_found = coordinator
            .search("nonexistent")
            .await
            .expect("search none");
        assert!(not_found.is_empty());

        let _ = std::fs::remove_dir_all(temp_dir);
    }
}
