//! Subwindow dialog displaying background thumbnail download progress and queue.

use gtk4::prelude::*;
use gtk4::{Align, Image, Label, Orientation, ProgressBar, ScrolledWindow, Spinner};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::widget::FileExplorerBackend;
use nextcloud_desktop_core::events::DesktopEvent;

/// Modal subwindow presenting background thumbnail downloads with live progress.
pub struct ThumbnailDownloadsDialog;

impl ThumbnailDownloadsDialog {
    /// Presents the Thumbnail Downloads subwindow.
    pub fn present<B: FileExplorerBackend>(parent: Option<&impl IsA<gtk4::Widget>>, backend: &B) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let window = adw::Window::builder()
            .title("Thumbnail Downloads")
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

        let title_label = Label::new(Some("Background Thumbnail Downloads"));
        title_label.add_css_class("title-3");
        title_label.set_halign(Align::Start);
        status_box.append(&title_label);

        let subtitle_label = Label::new(Some(""));
        subtitle_label.add_css_class("dim-label");
        subtitle_label.set_halign(Align::Start);
        status_box.append(&subtitle_label);

        let pbar = ProgressBar::new();
        pbar.set_hexpand(true);
        pbar.set_size_request(-1, 8);
        status_box.append(&pbar);

        content_box.append(&status_box);

        // Recent & Active downloads list
        let list_group = adw::PreferencesGroup::builder()
            .title("Active & Recent Downloads")
            .description("Thumbnails generated on-demand by Nextcloud and cached locally.")
            .build();

        let initial_snap = backend.thumbnail_queue_snapshot();
        let total_started = Rc::new(RefCell::new(initial_snap.total));
        let total_finished = Rc::new(RefCell::new(initial_snap.completed));
        let in_progress = Rc::new(RefCell::new(initial_snap.in_progress));

        let update_progress_ui = {
            let pbar = pbar.clone();
            let subtitle = subtitle_label.clone();
            let started = total_started.clone();
            let finished = total_finished.clone();
            let in_prog = in_progress.clone();

            Rc::new(move || {
                let st = *started.borrow();
                let fin = *finished.borrow();
                let prog = *in_prog.borrow();

                if prog == 0 {
                    // When there are no thumbnail downloads, it means it is done: full progress bar!
                    pbar.set_fraction(1.0);
                    if fin > 0 {
                        subtitle.set_text(&format!(
                            "All thumbnails downloaded ({fin} items cached • Up to date)"
                        ));
                    } else {
                        subtitle.set_text("All thumbnails downloaded • Up to date");
                    }
                } else {
                    let total = st.max(fin + prog).max(1);
                    let fraction = (fin as f64 / total as f64).clamp(0.0, 1.0);
                    pbar.set_fraction(fraction);
                    subtitle.set_text(&format!(
                        "Downloading: {fin} of {total} completed ({prog} active)…"
                    ));
                }
            })
        };

        // Initialize progress UI from SDK snapshot
        update_progress_ui();

        struct RowEntry {
            row: adw::ActionRow,
            spinner: Spinner,
            icon: Image,
        }

        let rows_map: Rc<RefCell<HashMap<String, RowEntry>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let list_group_clone = list_group.clone();

        // Populate initial active items from snapshot
        for item_path in &initial_snap.active_items {
            let filename = std::path::Path::new(item_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(item_path)
                .to_string();

            let row = adw::ActionRow::builder()
                .title(&filename)
                .subtitle(item_path)
                .build();

            let icon = Image::from_icon_name("image-x-generic-symbolic");
            row.add_prefix(&icon);

            let spinner = Spinner::new();
            spinner.set_spinning(true);
            row.add_suffix(&spinner);

            list_group.add(&row);
            rows_map
                .borrow_mut()
                .insert(item_path.clone(), RowEntry { row, spinner, icon });
        }

        let scroll = ScrolledWindow::new();
        scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scroll.set_vexpand(true);
        scroll.set_hexpand(true);
        scroll.set_child(Some(&list_group));

        content_box.append(&scroll);

        // Listen for live events from the sync engine
        let update_ui_for_events = update_progress_ui.clone();
        let rows_for_events = rows_map.clone();
        let total_started_events = total_started.clone();
        let total_finished_events = total_finished.clone();
        let in_prog_events = in_progress.clone();
        let list_group_events = list_group_clone;

        let mut rx = backend.subscribe_events();
        glib::MainContext::default().spawn_local(async move {
            while let Ok(event) = rx.recv().await {
                match event {
                    DesktopEvent::ThumbnailDownloadStarted { path } => {
                        *total_started_events.borrow_mut() += 1;
                        *in_prog_events.borrow_mut() += 1;
                        update_ui_for_events();

                        let mut rows = rows_for_events.borrow_mut();
                        rows.entry(path.clone()).or_insert_with(|| {
                            let filename = std::path::Path::new(&path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(&path)
                                .to_string();

                            let row = adw::ActionRow::builder()
                                .title(&filename)
                                .subtitle(&path)
                                .build();

                            let icon = Image::from_icon_name("image-x-generic-symbolic");
                            row.add_prefix(&icon);

                            let spinner = Spinner::new();
                            spinner.set_spinning(true);
                            row.add_suffix(&spinner);

                            list_group_events.add(&row);
                            RowEntry { row, spinner, icon }
                        });
                    }
                    DesktopEvent::ThumbnailReady { path, .. } => {
                        *total_finished_events.borrow_mut() += 1;
                        let cur_prog = *in_prog_events.borrow();
                        *in_prog_events.borrow_mut() = cur_prog.saturating_sub(1);
                        update_ui_for_events();

                        let rows = rows_for_events.borrow();
                        if let Some(entry) = rows.get(&path) {
                            entry.spinner.set_spinning(false);
                            entry.spinner.set_visible(false);
                            entry.icon.set_icon_name(Some("emblem-ok-symbolic"));
                            entry.row.set_subtitle(&format!("Downloaded • {path}"));
                        }
                    }
                    DesktopEvent::ThumbnailFailed { path } => {
                        let cur_prog = *in_prog_events.borrow();
                        *in_prog_events.borrow_mut() = cur_prog.saturating_sub(1);
                        update_ui_for_events();

                        let rows = rows_for_events.borrow();
                        if let Some(entry) = rows.get(&path) {
                            entry.spinner.set_spinning(false);
                            entry.spinner.set_visible(false);
                            entry.icon.set_icon_name(Some("dialog-warning-symbolic"));
                            entry.row.set_subtitle(&format!("Unavailable • {path}"));
                        }
                    }
                    DesktopEvent::ThumbnailProgress {
                        completed,
                        total,
                        in_progress: in_p,
                    } => {
                        *total_finished_events.borrow_mut() = completed;
                        *total_started_events.borrow_mut() = total;
                        *in_prog_events.borrow_mut() = in_p;
                        update_ui_for_events();
                    }
                    _ => {}
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
