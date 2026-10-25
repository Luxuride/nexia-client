//! Modal dialog for configuring Cloud Files presentation and explorer preferences.

use gtk4::prelude::*;
use gtk4::{Align, Orientation, Switch};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::widget::FileExplorerBackend;

/// Helper function to build a Libadwaita switch row.
fn make_switch_row(title: &str, subtitle: &str) -> (adw::ActionRow, Switch) {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .activatable(true)
        .build();
    let sw = Switch::new();
    sw.set_valign(Align::Center);
    row.add_suffix(&sw);
    row.set_activatable_widget(Some(&sw));
    (row, sw)
}

/// Modal dialog presenting Cloud Files presentation and display options.
pub struct CloudFilesSettingsDialog;

impl CloudFilesSettingsDialog {
    /// Presents the Cloud Files settings window transient to `parent`.
    pub fn present<B: FileExplorerBackend>(parent: Option<&impl IsA<gtk4::Widget>>, backend: &B) {
        let window = adw::Window::builder()
            .title("Cloud Files Settings")
            .modal(true)
            .default_width(460)
            .default_height(520)
            .destroy_with_parent(true)
            .build();

        if let Some(p) = parent
            && let Some(root) = p.root().and_then(|r| r.downcast::<gtk4::Window>().ok())
        {
            window.set_transient_for(Some(&root));
        }

        let header_bar = adw::HeaderBar::new();
        header_bar.set_show_end_title_buttons(true);
        header_bar.set_show_start_title_buttons(false);

        let content_box = gtk4::Box::new(Orientation::Vertical, 16);
        content_box.set_margin_top(16);
        content_box.set_margin_bottom(16);
        content_box.set_margin_start(16);
        content_box.set_margin_end(16);

        let group = adw::PreferencesGroup::builder()
            .title("File & Folder Display Options")
            .description(
                "Configure what information is shown for files and folders in Cloud Files.",
            )
            .build();

        let (r_direct_items, sw_direct_items) = make_switch_row(
            "Direct items inside folder",
            "Show item count directly inside folder (e.g. “3 items”)",
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
            "Show total size of files directly inside folder",
        );
        let (r_total_size, sw_total_size) = make_switch_row(
            "Recursive total folder size",
            "Show total aggregated size of entire folder tree",
        );
        let (r_last_mod, sw_last_mod) = make_switch_row(
            "Last modified date",
            "Show last edit timestamp under file and folder names",
        );

        let (r_auto_thumb, sw_auto_thumb) = make_switch_row(
            "Automatic thumbnail downloads",
            "Download and cache image thumbnails progressively in background",
        );

        let b = backend.clone();
        let s_di = sw_direct_items.clone();
        let s_ti = sw_total_items.clone();
        let s_fs = sw_file_size.clone();
        let s_ds = sw_direct_size.clone();
        let s_ts = sw_total_size.clone();
        let s_lm = sw_last_mod.clone();
        let s_at = sw_auto_thumb.clone();

        glib::MainContext::default().spawn_local(async move {
            let cfg = b.effective_display_config().await;
            s_di.set_active(cfg.show_direct_item_count);
            s_ti.set_active(cfg.show_total_item_count);
            s_fs.set_active(cfg.show_file_size);
            s_ds.set_active(cfg.show_direct_folder_size);
            s_ts.set_active(cfg.show_total_folder_size);
            s_lm.set_active(cfg.show_last_modified);
            s_at.set_active(b.is_autodownload_thumbnails_enabled().await);
        });

        let save_display_cfg = {
            let b = backend.clone();
            let s_di = sw_direct_items.clone();
            let s_ti = sw_total_items.clone();
            let s_fs = sw_file_size.clone();
            let s_ds = sw_direct_size.clone();
            let s_ts = sw_total_size.clone();
            let s_lm = sw_last_mod.clone();
            move || {
                let b = b.clone();
                let show_di = s_di.is_active();
                let show_ti = s_ti.is_active();
                let show_fs = s_fs.is_active();
                let show_ds = s_ds.is_active();
                let show_ts = s_ts.is_active();
                let show_lm = s_lm.is_active();
                glib::MainContext::default().spawn_local(async move {
                    let mut cfg = b.effective_display_config().await;
                    cfg.show_direct_item_count = show_di;
                    cfg.show_total_item_count = show_ti;
                    cfg.show_file_size = show_fs;
                    cfg.show_direct_folder_size = show_ds;
                    cfg.show_total_folder_size = show_ts;
                    cfg.show_last_modified = show_lm;
                    if let Some(aid) = b.active_account_id().await {
                        let _ = b.set_account_display_config(&aid, cfg).await;
                    } else {
                        let _ = b.set_global_display_config(cfg).await;
                    }
                });
            }
        };

        {
            let save_fn = save_display_cfg.clone();
            sw_direct_items.connect_active_notify(move |_| save_fn());
        }
        {
            let save_fn = save_display_cfg.clone();
            sw_total_items.connect_active_notify(move |_| save_fn());
        }
        {
            let save_fn = save_display_cfg.clone();
            sw_file_size.connect_active_notify(move |_| save_fn());
        }
        {
            let save_fn = save_display_cfg.clone();
            sw_direct_size.connect_active_notify(move |_| save_fn());
        }
        {
            let save_fn = save_display_cfg.clone();
            sw_total_size.connect_active_notify(move |_| save_fn());
        }
        {
            let save_fn = save_display_cfg;
            sw_last_mod.connect_active_notify(move |_| save_fn());
        }
        {
            let b = backend.clone();
            sw_auto_thumb.connect_active_notify(move |sw| {
                let b = b.clone();
                let val = sw.is_active();
                glib::MainContext::default().spawn_local(async move {
                    let _ = b.set_autodownload_thumbnails_enabled(val).await;
                });
            });
        }

        group.add(&r_direct_items);
        group.add(&r_total_items);
        group.add(&r_file_size);
        group.add(&r_direct_size);
        group.add(&r_total_size);
        group.add(&r_last_mod);
        group.add(&r_auto_thumb);

        let scrolled = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .vexpand(true)
            .build();

        content_box.append(&group);
        scrolled.set_child(Some(&content_box));

        let main_box = gtk4::Box::new(Orientation::Vertical, 0);
        main_box.append(&header_bar);
        main_box.append(&scrolled);

        window.set_content(Some(&main_box));
        window.present();
    }
}
