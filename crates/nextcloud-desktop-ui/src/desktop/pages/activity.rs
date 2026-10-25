//! Transfers and hydration activity page component.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::{Align, Image, ProgressBar, Spinner};
use libadwaita as adw;
use libadwaita::prelude::*;
use nextcloud_desktop_core::events::DesktopEvent;
use nextcloud_rust_sdk::format_bytes;

use crate::widget::{FileExplorerBackend, ViewComponent};

/// Preferences page displaying recent file transfers and virtual hydration status.
#[derive(Clone)]
pub struct ActivityPage {
    page: adw::PreferencesPage,
}

impl ActivityPage {
    /// Creates a new activity and transfers page connected to live desktop events.
    pub fn new<B: FileExplorerBackend>(backend: &B) -> Self {
        let page = adw::PreferencesPage::new();

        // 1. Thumbnail Downloads with total progress and expandable item progress
        let thumbs_group = adw::PreferencesGroup::builder()
            .title("Media Downloads")
            .description("Background prefetching and caching of image and document thumbnails.")
            .build();

        let thumbs_expander = adw::ExpanderRow::builder()
            .title("Thumbnail Downloads")
            .subtitle("Idle (No active downloads)")
            .build();
        thumbs_expander.add_prefix(&Image::from_icon_name("image-x-generic-symbolic"));

        let pbar = ProgressBar::new();
        pbar.set_valign(Align::Center);
        pbar.set_size_request(120, -1);
        pbar.set_fraction(0.0);
        thumbs_expander.add_action(&pbar);

        thumbs_group.add(&thumbs_expander);
        page.add(&thumbs_group);

        // 2. Recent Transfers & Hydrations
        let activity_group = adw::PreferencesGroup::builder()
            .title("Recent Transfers and Hydrations")
            .description(
                "Real-time log of file uploads, downloads, and on-demand hydration events.",
            )
            .build();

        let empty_row = adw::ActionRow::builder()
            .title("No Recent Activity")
            .subtitle("Transfers and hydration requests will appear here as they occur")
            .build();
        empty_row.add_prefix(&Image::from_icon_name("document-send-symbolic"));
        activity_group.add(&empty_row);
        page.add(&activity_group);

        // State trackers
        struct ThumbState {
            _row: adw::ActionRow,
            spinner: Spinner,
            icon: Image,
        }

        let thumb_rows: Rc<RefCell<HashMap<String, ThumbState>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let total_started = Rc::new(RefCell::new(0usize));
        let total_finished = Rc::new(RefCell::new(0usize));

        let thumbs_exp_clone = thumbs_expander.clone();
        let pbar_clone = pbar.clone();
        let act_grp_clone = activity_group.clone();
        let empty_row_clone = empty_row.clone();
        let empty_removed = Rc::new(RefCell::new(false));

        let mut event_rx = backend.subscribe_events();
        glib::MainContext::default().spawn_local(async move {
            while let Ok(event) = event_rx.recv().await {
                match event {
                    DesktopEvent::ThumbnailDownloadStarted { path } => {
                        let mut rows = thumb_rows.borrow_mut();
                        if !rows.contains_key(&path) {
                            *total_started.borrow_mut() += 1;
                            let started = *total_started.borrow();
                            let finished = *total_finished.borrow();

                            let filename = std::path::Path::new(&path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(&path)
                                .to_string();

                            let row = adw::ActionRow::builder()
                                .title(&filename)
                                .subtitle(&path)
                                .build();

                            let spinner = Spinner::new();
                            spinner.start();
                            spinner.set_valign(Align::Center);

                            let icon = Image::from_icon_name("emblem-ok-symbolic");
                            icon.add_css_class("success");
                            icon.set_valign(Align::Center);
                            icon.set_visible(false);

                            row.add_suffix(&spinner);
                            row.add_suffix(&icon);
                            thumbs_exp_clone.add_row(&row);

                            rows.insert(
                                path.clone(),
                                ThumbState {
                                    _row: row,
                                    spinner,
                                    icon,
                                },
                            );

                            let fraction = if started > 0 {
                                finished as f64 / started as f64
                            } else {
                                0.0
                            };
                            pbar_clone.set_fraction(fraction);
                            let percent = (fraction * 100.0) as u32;
                            thumbs_exp_clone.set_subtitle(&format!(
                                "Downloading: {} of {} ({}%)",
                                finished, started, percent
                            ));
                        }
                    }
                    DesktopEvent::ThumbnailReady { path, .. } => {
                        let rows = thumb_rows.borrow();
                        if let Some(state) = rows.get(&path) {
                            state.spinner.stop();
                            state.spinner.set_visible(false);
                            state.icon.set_icon_name(Some("emblem-ok-symbolic"));
                            state.icon.remove_css_class("warning");
                            state.icon.add_css_class("success");
                            state.icon.set_visible(true);

                            *total_finished.borrow_mut() += 1;
                            let started = *total_started.borrow();
                            let finished = *total_finished.borrow();

                            let fraction = if started > 0 {
                                (finished as f64 / started as f64).min(1.0)
                            } else {
                                1.0
                            };
                            pbar_clone.set_fraction(fraction);
                            let percent = (fraction * 100.0) as u32;

                            if finished >= started {
                                thumbs_exp_clone
                                    .set_subtitle(&format!("All {} downloaded", started));
                            } else {
                                thumbs_exp_clone.set_subtitle(&format!(
                                    "Downloading: {} of {} ({}%)",
                                    finished, started, percent
                                ));
                            }
                        }
                    }
                    DesktopEvent::ThumbnailFailed { path } => {
                        let rows = thumb_rows.borrow();
                        if let Some(state) = rows.get(&path) {
                            state.spinner.stop();
                            state.spinner.set_visible(false);
                            state.icon.set_icon_name(Some("dialog-warning-symbolic"));
                            state.icon.remove_css_class("success");
                            state.icon.add_css_class("warning");
                            state.icon.set_visible(true);

                            *total_finished.borrow_mut() += 1;
                            let started = *total_started.borrow();
                            let finished = *total_finished.borrow();

                            let fraction = if started > 0 {
                                (finished as f64 / started as f64).min(1.0)
                            } else {
                                1.0
                            };
                            pbar_clone.set_fraction(fraction);
                            let percent = (fraction * 100.0) as u32;

                            if finished >= started {
                                thumbs_exp_clone.set_subtitle(&format!(
                                    "Completed (with warnings) {} files",
                                    started
                                ));
                            } else {
                                thumbs_exp_clone.set_subtitle(&format!(
                                    "Downloading: {} of {} ({}%)",
                                    finished, started, percent
                                ));
                            }
                        }
                    }
                    DesktopEvent::NodeHydrated { path, size } => {
                        if !*empty_removed.borrow() {
                            act_grp_clone.remove(&empty_row_clone);
                            *empty_removed.borrow_mut() = true;
                        }
                        let filename = std::path::Path::new(&path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&path);
                        let row = adw::ActionRow::builder()
                            .title(format!("Hydrated “{}”", filename))
                            .subtitle(format!("{} ({})", path, format_bytes(size)))
                            .build();
                        row.add_prefix(&Image::from_icon_name("network-transmit-receive-symbolic"));
                        let badge = Image::from_icon_name("emblem-ok-symbolic");
                        badge.add_css_class("success");
                        row.add_suffix(&badge);
                        act_grp_clone.add(&row);
                    }
                    DesktopEvent::FileTransferCompleted {
                        relative_path,
                        kind,
                    } => {
                        if !*empty_removed.borrow() {
                            act_grp_clone.remove(&empty_row_clone);
                            *empty_removed.borrow_mut() = true;
                        }
                        let filename = std::path::Path::new(&relative_path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&relative_path);
                        let kind_str = match kind {
                            nextcloud_desktop_core::events::TransferKind::Upload => "Uploaded",
                            nextcloud_desktop_core::events::TransferKind::Download => "Downloaded",
                            nextcloud_desktop_core::events::TransferKind::Hydration => "Hydrated",
                        };
                        let row = adw::ActionRow::builder()
                            .title(format!("{} “{}”", kind_str, filename))
                            .subtitle(&relative_path)
                            .build();
                        row.add_prefix(&Image::from_icon_name("document-send-symbolic"));
                        let badge = Image::from_icon_name("emblem-ok-symbolic");
                        badge.add_css_class("success");
                        row.add_suffix(&badge);
                        act_grp_clone.add(&row);
                    }
                    DesktopEvent::SyncCompleted {
                        uploaded,
                        downloaded,
                        ..
                    } => {
                        if !*empty_removed.borrow() {
                            act_grp_clone.remove(&empty_row_clone);
                            *empty_removed.borrow_mut() = true;
                        }
                        let row = adw::ActionRow::builder()
                            .title("Synchronization Finished")
                            .subtitle(format!(
                                "Uploaded: {}, Downloaded: {}",
                                uploaded, downloaded
                            ))
                            .build();
                        row.add_prefix(&Image::from_icon_name("emblem-synchronizing-symbolic"));
                        let badge = Image::from_icon_name("emblem-ok-symbolic");
                        badge.add_css_class("success");
                        row.add_suffix(&badge);
                        act_grp_clone.add(&row);
                    }
                    _ => {}
                }
            }
        });

        Self { page }
    }
}

impl ViewComponent for ActivityPage {
    type RootWidget = adw::PreferencesPage;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.page
    }
}
