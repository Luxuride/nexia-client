//! Account and application settings page component.

use gtk4::{Button, Image};
use libadwaita as adw;
use libadwaita::prelude::*;
use nextcloud_rust_sdk::format_bytes;

use crate::widget::{FileExplorerBackend, ViewComponent};

/// Preferences page displaying Nextcloud account security, local VFS cache, and thumbnail configuration.
#[derive(Clone)]
pub struct SettingsPage {
    page: adw::PreferencesPage,
}

impl SettingsPage {
    /// Creates a new settings page bound to `backend`.
    pub fn new<B: FileExplorerBackend>(backend: &B) -> Self {
        let page = adw::PreferencesPage::new();

        // 1. Nextcloud Account & System Startup
        let account_group = adw::PreferencesGroup::builder()
            .title("Application & System Startup")
            .description("System integration and autostart preferences.")
            .build();

        let autostart_switch = gtk4::Switch::new();
        autostart_switch.set_valign(gtk4::Align::Center);
        autostart_switch.set_active(nextcloud_desktop_core::is_autostart_enabled());
        autostart_switch.connect_active_notify(|sw| {
            if sw.is_active() {
                let _ = nextcloud_desktop_core::register_autostart();
            } else {
                let _ = nextcloud_desktop_core::unregister_autostart();
            }
        });

        let autostart_row = adw::ActionRow::builder()
            .title("Launch on System Boot")
            .subtitle("Automatically start Nextcloud Desktop upon logging in")
            .build();
        autostart_row.add_suffix(&autostart_switch);
        autostart_row.set_activatable_widget(Some(&autostart_switch));
        account_group.add(&autostart_row);

        let security_row = adw::ActionRow::builder()
            .title("Credential Redaction")
            .subtitle(
                "App tokens and passwords are automatically redacted from logs and diagnostics",
            )
            .build();
        security_row.add_prefix(&Image::from_icon_name("network-server-symbolic"));
        account_group.add(&security_row);

        // 2. Virtual File System Cache & Disk Usage
        let storage_group = adw::PreferencesGroup::builder()
            .title("Local Storage & Cache")
            .description(
                "Local disk storage used for encrypted databases, thumbnails, and hydrated files.",
            )
            .build();

        let cache_dir_display = backend.cache_dir_display();
        let safe_cache = glib::markup_escape_text(&cache_dir_display);
        let cache_row = adw::ActionRow::builder()
            .title("Hydration Cache Directory")
            .subtitle(safe_cache.as_str())
            .build();
        cache_row.add_prefix(&Image::from_icon_name("drive-harddisk-symbolic"));
        storage_group.add(&cache_row);

        let db_size_row = adw::ActionRow::builder()
            .title("Cache Database Size")
            .subtitle("Calculating…")
            .build();
        db_size_row.add_prefix(&Image::from_icon_name("drive-removable-media-symbolic"));
        storage_group.add(&db_size_row);

        let files_size_row = adw::ActionRow::builder()
            .title("Cached Files Size")
            .subtitle("Calculating…")
            .build();
        files_size_row.add_prefix(&Image::from_icon_name("folder-symbolic"));
        storage_group.add(&files_size_row);

        // Thumbnail prefetching toggle
        let autodownload_switch = gtk4::Switch::new();
        autodownload_switch.set_valign(gtk4::Align::Center);
        autodownload_switch.set_active(true);

        let autodownload_row = adw::ActionRow::builder()
            .title("Auto-download Smallest Thumbnails")
            .subtitle("Prefetch low-resolution thumbnails in background for instant previewing")
            .build();
        autodownload_row.add_suffix(&autodownload_switch);
        autodownload_row.set_activatable_widget(Some(&autodownload_switch));

        let b_sw = backend.clone();
        let sw_clone = autodownload_switch.clone();
        glib::MainContext::default().spawn_local(async move {
            sw_clone.set_active(b_sw.is_autodownload_thumbnails_enabled().await);
            let b = b_sw.clone();
            sw_clone.connect_active_notify(move |sw| {
                let b = b.clone();
                let active = sw.is_active();
                glib::MainContext::default().spawn_local(async move {
                    let _ = b.set_autodownload_thumbnails_enabled(active).await;
                });
            });
        });
        storage_group.add(&autodownload_row);

        // Clean actions
        let actions_group = adw::PreferencesGroup::builder()
            .title("Cache Cleanup Actions")
            .description("Free up disk space occupied by cached media and temporary files.")
            .build();

        let clean_thumbs_row = adw::ActionRow::builder()
            .title("Clean Thumbnails")
            .subtitle("Delete all downloaded and cached thumbnail images")
            .build();
        let clean_thumbs_btn = Button::builder()
            .label("Clean Thumbnails")
            .valign(gtk4::Align::Center)
            .build();
        clean_thumbs_btn.add_css_class("destructive-action");
        clean_thumbs_row.add_suffix(&clean_thumbs_btn);
        actions_group.add(&clean_thumbs_row);

        let clean_all_row = adw::ActionRow::builder()
            .title("Clean Everything")
            .subtitle("Reset cache database, delete all hydrated files and thumbnails")
            .build();
        let clean_all_btn = Button::builder()
            .label("Clean Everything")
            .valign(gtk4::Align::Center)
            .build();
        clean_all_btn.add_css_class("destructive-action");
        clean_all_row.add_suffix(&clean_all_btn);
        actions_group.add(&clean_all_row);

        // Asynchronously populate cache sizes and wire button actions
        let b = backend.clone();
        let db_row_c = db_size_row.clone();
        let files_row_c = files_size_row.clone();
        let refresh_sizes = move || {
            let b = b.clone();
            let db_r = db_row_c.clone();
            let files_r = files_row_c.clone();
            glib::MainContext::default().spawn_local(async move {
                let db_sz = b.cache_database_size().await;
                db_r.set_subtitle(&format_bytes(db_sz));
                let files_sz = b.cached_files_size().await;
                files_r.set_subtitle(&format_bytes(files_sz));
            });
        };

        refresh_sizes();

        {
            let b = backend.clone();
            let ref_fn = refresh_sizes.clone();
            clean_thumbs_btn.connect_clicked(move |_| {
                let b = b.clone();
                let ref_fn = ref_fn.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _ = b.clean_thumbnails().await;
                    ref_fn();
                });
            });
        }

        {
            let b = backend.clone();
            let ref_fn = refresh_sizes;
            clean_all_btn.connect_clicked(move |_| {
                let b = b.clone();
                let ref_fn = ref_fn.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _ = b.clean_everything().await;
                    ref_fn();
                });
            });
        }

        page.add(&account_group);
        page.add(&storage_group);
        page.add(&actions_group);

        Self { page }
    }
}

impl ViewComponent for SettingsPage {
    type RootWidget = adw::PreferencesPage;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.page
    }
}
