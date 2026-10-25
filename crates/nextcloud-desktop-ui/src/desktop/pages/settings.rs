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
    pub fn new<B: FileExplorerBackend>(backend: &B, toast_overlay: &adw::ToastOverlay) -> Self {
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

        // Maximum thumbnail size option
        let size_options = ["64 px", "128 px", "256 px", "512 px"];
        let model = gtk4::StringList::new(&size_options);
        let max_thumb_row = adw::ComboRow::builder()
            .title("Maximum Thumbnail Size")
            .subtitle("Limit preview resolution to conserve disk space and bandwidth")
            .model(&model)
            .build();

        let b_max = backend.clone();
        let max_row_clone = max_thumb_row.clone();
        glib::MainContext::default().spawn_local(async move {
            let max_val = b_max.max_thumbnail_size().await;
            let sel = match max_val {
                0..=64 => 0,
                65..=128 => 1,
                129..=256 => 2,
                _ => 3,
            };
            max_row_clone.set_selected(sel);
            let b = b_max.clone();
            max_row_clone.connect_selected_notify(move |combo| {
                let chosen = match combo.selected() {
                    0 => 64,
                    1 => 128,
                    2 => 256,
                    _ => 512,
                };
                let b = b.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _ = b.set_max_thumbnail_size(chosen).await;
                });
            });
        });
        storage_group.add(&max_thumb_row);

        // Clean actions
        let actions_group = adw::PreferencesGroup::builder()
            .title("Cache Cleanup Actions")
            .description("Free up disk space occupied by cached media and temporary files.")
            .build();

        let clean_oversized_row = adw::ActionRow::builder()
            .title("Remove Oversized Thumbnails")
            .subtitle("Delete thumbnails larger than configured maximum size")
            .build();
        let clean_oversized_btn = Button::builder()
            .label("Clean Oversized")
            .valign(gtk4::Align::Center)
            .build();
        clean_oversized_btn.add_css_class("destructive-action");
        clean_oversized_row.add_suffix(&clean_oversized_btn);
        actions_group.add(&clean_oversized_row);

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
            let toast = toast_overlay.clone();
            let ref_fn = refresh_sizes.clone();
            clean_oversized_btn.connect_clicked(move |_| {
                let b = b.clone();
                let toast = toast.clone();
                let ref_fn = ref_fn.clone();
                glib::MainContext::default().spawn_local(async move {
                    match b.clean_oversized_thumbnails().await {
                        Ok(freed) => {
                            toast.add_toast(adw::Toast::new(&format!(
                                "Removed oversized thumbnails (freed {})",
                                format_bytes(freed)
                            )));
                        }
                        Err(e) => {
                            toast.add_toast(adw::Toast::new(&format!(
                                "Failed to clean oversized thumbnails: {e}"
                            )));
                        }
                    }
                    ref_fn();
                });
            });
        }

        {
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let ref_fn = refresh_sizes.clone();
            clean_thumbs_btn.connect_clicked(move |_| {
                let b = b.clone();
                let toast = toast.clone();
                let ref_fn = ref_fn.clone();
                glib::MainContext::default().spawn_local(async move {
                    match b.clean_thumbnails().await {
                        Ok(freed) => {
                            toast.add_toast(adw::Toast::new(&format!(
                                "Cleaned thumbnails (freed {})",
                                format_bytes(freed)
                            )));
                        }
                        Err(e) => {
                            toast.add_toast(adw::Toast::new(&format!(
                                "Failed to clean thumbnails: {e}"
                            )));
                        }
                    }
                    ref_fn();
                });
            });
        }

        {
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let ref_fn = refresh_sizes;
            clean_all_btn.connect_clicked(move |_| {
                let b = b.clone();
                let toast = toast.clone();
                let ref_fn = ref_fn.clone();
                glib::MainContext::default().spawn_local(async move {
                    match b.clean_everything().await {
                        Ok(freed) => {
                            toast.add_toast(adw::Toast::new(&format!(
                                "Cleaned all cache data (freed {})",
                                format_bytes(freed)
                            )));
                        }
                        Err(e) => {
                            toast.add_toast(adw::Toast::new(&format!(
                                "Failed to clean cache data: {e}"
                            )));
                        }
                    }
                    ref_fn();
                });
            });
        }

        // Group: Cloud Files Presentation Preferences
        let display_group = adw::PreferencesGroup::builder()
            .title("Cloud Files Settings")
            .description("Configure item metadata presentation and folder contents for the Cloud Files explorer.")
            .build();

        fn make_switch_row(title: &str, subtitle: &str) -> (adw::ActionRow, gtk4::Switch) {
            let row = adw::ActionRow::builder()
                .title(title)
                .subtitle(subtitle)
                .activatable(true)
                .build();
            let sw = gtk4::Switch::new();
            sw.set_valign(gtk4::Align::Center);
            row.add_suffix(&sw);
            row.set_activatable_widget(Some(&sw));
            (row, sw)
        }

        let (r_direct_items, sw_direct_items) = make_switch_row(
            "Direct items inside folder",
            "Show direct child item count on folders (e.g. “3 items”)",
        );
        let (r_total_items, sw_total_items) = make_switch_row(
            "Recursive total items",
            "Show total item count in all subdirectories (e.g. “124 total items”)",
        );
        let (r_file_size, sw_file_size) = make_switch_row(
            "File sizes",
            "Show formatted file size under file names (e.g. “2.4 MB”)",
        );
        let (r_direct_size, sw_direct_size) = make_switch_row(
            "Direct folder files size",
            "Show total byte size of files directly inside folder",
        );
        let (r_total_size, sw_total_size) = make_switch_row(
            "Recursive total folder size",
            "Show total aggregated byte size of entire folder tree",
        );
        let (r_last_mod, sw_last_mod) = make_switch_row(
            "Last modified date",
            "Show last edit timestamp under file and folder names",
        );

        let b_disp = backend.clone();
        let s_di = sw_direct_items.clone();
        let s_ti = sw_total_items.clone();
        let s_fs = sw_file_size.clone();
        let s_ds = sw_direct_size.clone();
        let s_ts = sw_total_size.clone();
        let s_lm = sw_last_mod.clone();

        glib::MainContext::default().spawn_local(async move {
            let cfg = b_disp.effective_display_config().await;
            s_di.set_active(cfg.show_direct_item_count);
            s_ti.set_active(cfg.show_total_item_count);
            s_fs.set_active(cfg.show_file_size);
            s_ds.set_active(cfg.show_direct_folder_size);
            s_ts.set_active(cfg.show_total_folder_size);
            s_lm.set_active(cfg.show_last_modified);

            let b_di = b_disp.clone();
            s_di.connect_active_notify(move |sw| {
                let act = sw.is_active();
                let b = b_di.clone();
                glib::MainContext::default().spawn_local(async move {
                    let mut cur = b.effective_display_config().await;
                    cur.show_direct_item_count = act;
                    if let Some(acc_id) = b.active_account_id().await {
                        let _ = b.set_account_display_config(&acc_id, cur.clone()).await;
                    }
                    let _ = b.set_global_display_config(cur).await;
                });
            });

            let b_ti = b_disp.clone();
            s_ti.connect_active_notify(move |sw| {
                let act = sw.is_active();
                let b = b_ti.clone();
                glib::MainContext::default().spawn_local(async move {
                    let mut cur = b.effective_display_config().await;
                    cur.show_total_item_count = act;
                    if let Some(acc_id) = b.active_account_id().await {
                        let _ = b.set_account_display_config(&acc_id, cur.clone()).await;
                    }
                    let _ = b.set_global_display_config(cur).await;
                });
            });

            let b_fs = b_disp.clone();
            s_fs.connect_active_notify(move |sw| {
                let act = sw.is_active();
                let b = b_fs.clone();
                glib::MainContext::default().spawn_local(async move {
                    let mut cur = b.effective_display_config().await;
                    cur.show_file_size = act;
                    if let Some(acc_id) = b.active_account_id().await {
                        let _ = b.set_account_display_config(&acc_id, cur.clone()).await;
                    }
                    let _ = b.set_global_display_config(cur).await;
                });
            });

            let b_ds = b_disp.clone();
            s_ds.connect_active_notify(move |sw| {
                let act = sw.is_active();
                let b = b_ds.clone();
                glib::MainContext::default().spawn_local(async move {
                    let mut cur = b.effective_display_config().await;
                    cur.show_direct_folder_size = act;
                    if let Some(acc_id) = b.active_account_id().await {
                        let _ = b.set_account_display_config(&acc_id, cur.clone()).await;
                    }
                    let _ = b.set_global_display_config(cur).await;
                });
            });

            let b_ts = b_disp.clone();
            s_ts.connect_active_notify(move |sw| {
                let act = sw.is_active();
                let b = b_ts.clone();
                glib::MainContext::default().spawn_local(async move {
                    let mut cur = b.effective_display_config().await;
                    cur.show_total_folder_size = act;
                    if let Some(acc_id) = b.active_account_id().await {
                        let _ = b.set_account_display_config(&acc_id, cur.clone()).await;
                    }
                    let _ = b.set_global_display_config(cur).await;
                });
            });

            let b_lm = b_disp.clone();
            s_lm.connect_active_notify(move |sw| {
                let act = sw.is_active();
                let b = b_lm.clone();
                glib::MainContext::default().spawn_local(async move {
                    let mut cur = b.effective_display_config().await;
                    cur.show_last_modified = act;
                    if let Some(acc_id) = b.active_account_id().await {
                        let _ = b.set_account_display_config(&acc_id, cur.clone()).await;
                    }
                    let _ = b.set_global_display_config(cur).await;
                });
            });
        });

        display_group.add(&r_direct_items);
        display_group.add(&r_total_items);
        display_group.add(&r_file_size);
        display_group.add(&r_direct_size);
        display_group.add(&r_total_size);
        display_group.add(&r_last_mod);

        page.add(&account_group);
        page.add(&display_group);
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
