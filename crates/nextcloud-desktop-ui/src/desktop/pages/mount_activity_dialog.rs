//! Subwindow dialog displaying real-time synchronization activity and file transfers for a mount point.

use gtk4::prelude::*;
use gtk4::{Align, Image, Label, Orientation, ProgressBar, ScrolledWindow, Spinner};
use libadwaita as adw;
use libadwaita::prelude::*;
use nextcloud_rust_sdk::{MountStatus, StoredSyncMount, format_bytes};

use crate::widget::FileExplorerBackend;

/// Modal subwindow presenting live synchronization activity and active transfers for an individual mount.
pub struct MountActivityDialog;

impl MountActivityDialog {
    /// Presents the mount synchronization activity subwindow.
    pub fn present<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        backend: &B,
        mount: StoredSyncMount,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let title_str = format!(
            "{} Activity",
            mount.display_name.as_deref().unwrap_or(&mount.remote_path)
        );

        let window = adw::Window::builder()
            .title(&title_str)
            .default_width(480)
            .default_height(520)
            .modal(true)
            .build();

        if let Some(ref win) = root_window {
            window.set_transient_for(Some(win));
        }

        let header_bar = adw::HeaderBar::new();
        header_bar.set_show_end_title_buttons(true);
        header_bar.set_show_start_title_buttons(false);

        let content_box = gtk4::Box::new(Orientation::Vertical, 16);
        content_box.set_margin_top(16);
        content_box.set_margin_bottom(16);
        content_box.set_margin_start(16);
        content_box.set_margin_end(16);

        // Header status summary
        let status_box = gtk4::Box::new(Orientation::Vertical, 8);
        status_box.set_hexpand(true);

        let title_label = Label::new(Some(&title_str));
        title_label.add_css_class("title-3");
        title_label.set_halign(Align::Start);
        status_box.append(&title_label);

        let subtitle_label = Label::new(Some("Querying status…"));
        subtitle_label.add_css_class("dim-label");
        subtitle_label.set_halign(Align::Start);
        status_box.append(&subtitle_label);

        let pbar = ProgressBar::new();
        pbar.set_hexpand(true);
        pbar.set_size_request(-1, 8);
        pbar.set_fraction(1.0);
        status_box.append(&pbar);

        content_box.append(&status_box);

        // Details group
        let details_group = adw::PreferencesGroup::builder()
            .title("Synchronization Metrics")
            .build();

        let status_row = adw::ActionRow::builder()
            .title("Current State")
            .subtitle("Checking…")
            .build();
        status_row.add_prefix(&Image::from_icon_name("view-refresh-symbolic"));
        details_group.add(&status_row);

        let progress_row = adw::ActionRow::builder()
            .title("Items Synchronized")
            .subtitle("0 items")
            .build();
        progress_row.add_prefix(&Image::from_icon_name("folder-download-symbolic"));
        details_group.add(&progress_row);

        let bytes_row = adw::ActionRow::builder()
            .title("Volume Transferred")
            .subtitle("0 B")
            .build();
        bytes_row.add_prefix(&Image::from_icon_name("network-transmit-receive-symbolic"));
        details_group.add(&bytes_row);

        content_box.append(&details_group);

        // Active files list
        let active_group = adw::PreferencesGroup::builder()
            .title("Active Transfers")
            .description("Files currently being transferred or reconciled with Nextcloud.")
            .build();

        let empty_row = adw::ActionRow::builder()
            .title("All Files Up to Date")
            .subtitle("No active file transfers in progress")
            .build();
        empty_row.add_prefix(&Image::from_icon_name("object-select-symbolic"));
        active_group.add(&empty_row);

        let scrolled = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .min_content_height(140)
            .vexpand(true)
            .child(&active_group)
            .build();

        content_box.append(&scrolled);

        // Populate metrics from SDK
        let b = backend.clone();
        let m_id = mount.id.clone();
        let sub_lbl = subtitle_label;
        let pb = pbar;
        let st_row = status_row;
        let pr_row = progress_row;
        let by_row = bytes_row;
        let act_grp = active_group;
        let em_row = empty_row;

        glib::MainContext::default().spawn_local(async move {
            if let Ok(snap) = b.mount_progress(&m_id).await {
                let status_text = match snap.status {
                    MountStatus::Paused => "Paused",
                    MountStatus::Syncing => "Syncing",
                    MountStatus::Error(ref e) => e.as_str(),
                    _ => "Idle / Up to date",
                };
                st_row.set_subtitle(status_text);

                if snap.in_progress_items == 0 {
                    pb.set_fraction(1.0);
                    sub_lbl.set_text("All files synchronized • Up to date");
                    pr_row.set_subtitle(&format!("{} items completed", snap.completed_items));
                } else {
                    let total = snap.total_items.max(1);
                    let frac = (snap.completed_items as f64 / total as f64).clamp(0.0, 1.0);
                    pb.set_fraction(frac);
                    sub_lbl.set_text(&format!(
                        "Syncing: {} of {} items ({} active)",
                        snap.completed_items, snap.total_items, snap.in_progress_items
                    ));
                    pr_row.set_subtitle(&format!(
                        "{} of {} items",
                        snap.completed_items, snap.total_items
                    ));
                }

                by_row.set_subtitle(&format!(
                    "{} transferred (total {})",
                    format_bytes(snap.bytes_transferred),
                    format_bytes(snap.bytes_total)
                ));

                if !snap.active_items.is_empty() {
                    act_grp.remove(&em_row);
                    for item in snap.active_items {
                        let row = adw::ActionRow::builder()
                            .title(&item)
                            .subtitle("Transferring…")
                            .build();
                        let spinner = Spinner::new();
                        spinner.start();
                        spinner.set_valign(Align::Center);
                        row.add_suffix(&spinner);
                        act_grp.add(&row);
                    }
                }
            }
        });

        let main_box = gtk4::Box::new(Orientation::Vertical, 0);
        main_box.append(&header_bar);
        main_box.append(&content_box);
        window.set_content(Some(&main_box));
        window.present();
    }
}
