//! Synchronization coordinator module.

pub use crate::coordinator::DesktopSyncCoordinator;

#[cfg(test)]
mod tests {
    use super::*;
    use nexia_core::account::{Account, AccountCredentials, AccountId, ServerConfig};
    use nexia_core::config::AppConfig;
    use nexia_core::events::EventBus;
    use nexia_nextcloud_sdk::{MountDescriptor, MountMode};
    use std::sync::Arc;

    #[tokio::test]
    async fn account_management_and_switching() {
        let config = AppConfig::default();
        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));

        let acc_alice = Account::new(
            "Alice Example",
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
            "Bob Example",
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

    /// Drains whatever the bus has buffered, returning it as a vector.
    fn drain_events(
        rx: &mut tokio::sync::broadcast::Receiver<nexia_core::events::DesktopEvent>,
    ) -> Vec<nexia_core::events::DesktopEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn first_sign_in_from_zero_accounts_announces_active_account() {
        let config = AppConfig::default();
        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));
        let mut rx = coordinator.event_bus().subscribe();

        assert!(coordinator.accounts().await.is_empty());
        assert!(coordinator.active_account().await.is_none());
        let _ = drain_events(&mut rx);

        let account = Account::new(
            "First Example",
            ServerConfig::new("https://cloud.first.example"),
            AccountCredentials::new("first", "token"),
        );
        let id = account.id.clone();
        coordinator
            .add_account(account)
            .await
            .expect("add first account succeeds");

        let switched: Vec<_> = drain_events(&mut rx)
            .into_iter()
            .filter(|e| matches!(e, nexia_core::events::DesktopEvent::AccountSwitched { .. }))
            .collect();
        assert_eq!(
            switched.len(),
            1,
            "signing in from zero accounts must emit AccountSwitched so account-scoped UI state repopulates"
        );
        assert!(matches!(
            &switched[0],
            nexia_core::events::DesktopEvent::AccountSwitched { account_id }
                if account_id == &id.0
        ));
        assert_eq!(coordinator.active_account().await.map(|a| a.id), Some(id));
    }

    #[tokio::test]
    async fn empty_discovery_purges_stale_system_accounts() {
        let mut config = AppConfig::default();
        let stale = Account::system_sso_account(
            "sso-cloud.stale.example-ghost",
            "Ghost",
            ServerConfig::new("https://cloud.stale.example"),
            AccountCredentials::new("ghost", "token"),
            None,
        );
        let stale_id = stale.id.clone();
        config.accounts.push(stale);
        config.active_account_id = Some(stale_id.clone());

        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));
        let mut rx = coordinator.event_bus().subscribe();
        assert_eq!(
            coordinator.active_account().await.map(|a| a.id),
            Some(stale_id)
        );
        let _ = drain_events(&mut rx);

        coordinator.reconcile_system_accounts(Vec::new()).await;

        assert!(
            coordinator.accounts().await.is_empty(),
            "an account the OS no longer reports must not survive reconciliation"
        );
        assert!(
            coordinator.active_account().await.is_none(),
            "active account must not dangle once nothing is left"
        );
        assert!(
            drain_events(&mut rx)
                .iter()
                .any(|e| matches!(e, nexia_core::events::DesktopEvent::AccountSignedOut { .. })),
            "losing the last account must notify the UI so it can clear account-scoped state"
        );
    }

    #[tokio::test]
    async fn reconciliation_promotes_remaining_account_as_active() {
        let mut config = AppConfig::default();
        let stale = Account::system_sso_account(
            "sso-cloud.gone.example-old",
            "Old SSO",
            ServerConfig::new("https://cloud.gone.example"),
            AccountCredentials::new("old", "token"),
            None,
        );
        config.accounts.push(stale);

        let manual = Account::new(
            "Manual",
            ServerConfig::new("https://cloud.manual.example"),
            AccountCredentials::new("manual", "token"),
        );
        let manual_id = manual.id.clone();
        config.accounts.push(manual);
        config.active_account_id = Some(AccountId("sso-cloud.gone.example-old".to_string()));

        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));
        let mut rx = coordinator.event_bus().subscribe();
        let _ = drain_events(&mut rx);

        coordinator.reconcile_system_accounts(Vec::new()).await;

        assert_eq!(
            coordinator.accounts().await.len(),
            1,
            "only the manually added account survives an empty discovery run"
        );
        assert_eq!(
            coordinator.active_account().await.map(|a| a.id),
            Some(manual_id.clone()),
            "the surviving account becomes active instead of leaving a dangling id"
        );
        assert!(
            drain_events(&mut rx).iter().any(|e| matches!(
                e,
                nexia_core::events::DesktopEvent::AccountSwitched { account_id }
                    if account_id == &manual_id.0
            )),
            "promoting a new active account must repopulate the UI"
        );
    }

    #[tokio::test]
    async fn switching_accounts_drops_cached_branding_and_logo() {
        let temp_dir = std::env::temp_dir().join(format!("nc_test_theme_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let theming_dir = temp_dir.join("theming");
        std::fs::create_dir_all(&theming_dir).expect("create theming dir");
        std::fs::write(theming_dir.join("cloud.alice.org_logo.svg"), b"<svg/>")
            .expect("write alice logo");

        let config = AppConfig {
            cache_dir: Some(temp_dir.clone()),
            ..Default::default()
        };
        let config_lock = Arc::new(tokio::sync::RwLock::new(config));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(100));

        coordinator
            .add_account(Account::new(
                "Alice Example",
                ServerConfig::new("https://cloud.alice.org"),
                AccountCredentials::new("alice", "token123"),
            ))
            .await
            .expect("add alice succeeds");
        let bob = Account::new(
            "Bob Example",
            ServerConfig::new("https://cloud.bob.org"),
            AccountCredentials::new("bob", "token456"),
        );
        let bob_id = bob.id.clone();
        coordinator
            .add_account(bob)
            .await
            .expect("add bob succeeds");

        assert!(
            coordinator.cached_theme_logo().is_none(),
            "bob has no logo, so alice's logo must not be picked up"
        );

        *coordinator.cached_theme_data.write().expect("theme lock") =
            Some(nexia_nextcloud_sdk::ThemeInfo {
                name: Some("Alice".to_string()),
                ..Default::default()
            });
        assert_eq!(
            coordinator.cached_theme().and_then(|t| t.name),
            Some("Alice".to_string())
        );

        coordinator
            .switch_account(&bob_id)
            .await
            .expect("switch to bob succeeds");
        assert!(
            coordinator.cached_theme().is_none(),
            "branding must be repopulated for the newly active account"
        );

        let _ = std::fs::remove_dir_all(temp_dir);
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

        // Uploading should be disabled while sync is paused
        let upload_err = coordinator
            .create_file("/paused_upload.txt", b"test")
            .await
            .expect_err("upload must be disabled while paused");
        assert!(matches!(
            upload_err,
            crate::error::SyncError::OperationDisabled(_)
        ));

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

        let vfs = coordinator.active_vfs().await.expect("active vfs");
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

    #[tokio::test]
    async fn primary_account_vfs_protection_and_legacy_purge() {
        let temp_dir = std::env::temp_dir().join(format!(
            "nextcloud_vfs_protect_test_{}",
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

        // 1. Verify internal VFS identification
        assert!(DesktopSyncCoordinator::is_internal_vfs("app-default-vfs"));
        assert!(DesktopSyncCoordinator::is_internal_vfs("app-vfs-user-1"));
        assert!(!DesktopSyncCoordinator::is_internal_vfs("custom-sync-uuid"));

        // 2. Attempting to remove an internal account VFS must be rejected
        let res = coordinator
            .remove_sync_mount_with_cleanup("app-vfs-user-1", true)
            .await;
        assert!(res.is_err());

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn multi_account_etag_polling_tracks_state() {
        let temp_dir = std::env::temp_dir().join(format!(
            "nextcloud_etag_poll_test_{}",
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

        // When no accounts exist, checking remote changes completes cleanly
        coordinator.check_remote_changes_all_accounts().await;

        // Register two accounts
        let acc1 = Account::new(
            "Account 1",
            ServerConfig::new("https://server1.local"),
            AccountCredentials::new("user1", "tok1"),
        );
        let id1 = acc1.id.0.clone();
        coordinator.add_account(acc1).await.unwrap();

        let acc2 = Account::new(
            "Account 2",
            ServerConfig::new("https://server2.local"),
            AccountCredentials::new("user2", "tok2"),
        );
        let id2 = acc2.id.0.clone();
        coordinator.add_account(acc2).await.unwrap();

        // Simulate recorded root etags across multiple accounts
        coordinator
            .last_root_etags
            .write()
            .await
            .insert(id1.clone(), "\"etag-acc1-v1\"".to_string());
        coordinator
            .last_root_etags
            .write()
            .await
            .insert(id2.clone(), "\"etag-acc2-v1\"".to_string());

        let etags = coordinator.last_root_etags.read().await;
        assert_eq!(etags.get(&id1).unwrap(), "\"etag-acc1-v1\"");
        assert_eq!(etags.get(&id2).unwrap(), "\"etag-acc2-v1\"");

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn sync_claim_is_exclusive_per_mount() {
        let config_lock = Arc::new(tokio::sync::RwLock::new(AppConfig::default()));
        let coordinator = DesktopSyncCoordinator::new(config_lock, EventBus::new(10));

        assert!(coordinator.try_claim_sync("mount-a"));
        // A second cycle for the same mount must not start.
        assert!(!coordinator.try_claim_sync("mount-a"));
        // Other mounts are unaffected.
        assert!(coordinator.try_claim_sync("mount-b"));

        coordinator.release_sync("mount-a");
        assert!(coordinator.try_claim_sync("mount-a"));
        // Releasing a mount nobody holds is not an error.
        coordinator.release_sync("mount-unknown");
    }
}
