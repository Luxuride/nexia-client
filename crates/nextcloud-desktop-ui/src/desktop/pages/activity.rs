//! Transfers and synchronization activity page component.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::{Align, Button, Image, ProgressBar};
use libadwaita as adw;
use libadwaita::prelude::*;
use nextcloud_desktop_core::events::DesktopEvent;
use nextcloud_rust_sdk::{MountStatus, format_bytes};

use super::MountActivityDialog;
use crate::widget::{FileExplorerBackend, ViewComponent};

/// Preferences page displaying per-mount sync activities, background thumbnails, and file transfers.
#[derive(Clone)]
pub struct ActivityPage {
    page: adw::PreferencesPage,
}

impl ActivityPage {
    /// Creates a new activity and transfers page connected to live desktop events.
    pub fn new<B: FileExplorerBackend>(backend: &B) -> Self {
        let page = adw::PreferencesPage::new();

        // 1. Per-Mount Synchronization Activities
        let mounts_group = adw::PreferencesGroup::builder()
            .title("Folder Synchronizations")
            .description("Active filesystem and virtual drive synchronization status and progress.")
            .build();

        let mount_rows: Rc<RefCell<HashMap<String, (adw::ActionRow, ProgressBar)>>> =
            Rc::new(RefCell::new(HashMap::new()));

        let empty_mounts_row = adw::ActionRow::builder()
            .title("Loading Sync Folders…")
            .subtitle("Querying configured synchronization mounts")
            .build();
        empty_mounts_row.add_prefix(&Image::from_icon_name("folder-download-symbolic"));
        mounts_group.add(&empty_mounts_row);
        page.add(&mounts_group);

        // Populate mounts dynamically from the SDK
        let b_mounts = backend.clone();
        let m_grp = mounts_group.clone();
        let em_m_row = empty_mounts_row.clone();
        let m_rows_map = mount_rows.clone();

        let refresh_mounts = {
            let b_mounts = b_mounts.clone();
            let m_grp = m_grp.clone();
            let em_m_row = em_m_row.clone();
            let m_rows_map = m_rows_map.clone();

            Rc::new(move || {
                let b = b_mounts.clone();
                let grp = m_grp.clone();
                let empty_row = em_m_row.clone();
                let rows_map = m_rows_map.clone();

                glib::MainContext::default().spawn_local(async move {
                    let mounts = b.sync_mounts().await;

                    // Remove previously tracked rows from the group
                    for (_id, (row, _pb)) in rows_map.borrow_mut().drain() {
                        grp.remove(&row);
                    }
                    grp.remove(&empty_row);

                    if !mounts.is_empty() {
                        for mount in mounts {
                            let mount_id_str = mount.id.as_str().to_string();
                            let display_title = mount
                                .display_name
                                .clone()
                                .unwrap_or_else(|| mount.remote_path.clone());

                            let row = adw::ActionRow::builder()
                                .title(&display_title)
                                .subtitle("All files synchronized • Up to date")
                                .activatable(true)
                                .build();

                            let icon_name = if mount_id_str.starts_with("app-")
                                || mount_id_str == "app-default-vfs"
                            {
                                "folder-remote-symbolic"
                            } else {
                                "folder-symbolic"
                            };
                            row.add_prefix(&Image::from_icon_name(icon_name));

                            let pbar = ProgressBar::new();
                            pbar.set_valign(Align::Center);
                            pbar.set_size_request(120, -1);
                            pbar.set_fraction(1.0);
                            row.add_suffix(&pbar);

                            let view_btn = Button::from_icon_name("go-next-symbolic");
                            view_btn.add_css_class("flat");
                            view_btn.set_valign(Align::Center);
                            view_btn.set_tooltip_text(Some("View synchronization details"));

                            let b_for_btn = b.clone();
                            let m_for_btn = mount.clone();
                            let row_for_btn = row.clone();
                            view_btn.connect_clicked(move |_| {
                                MountActivityDialog::present(
                                    Some(&row_for_btn),
                                    &b_for_btn,
                                    m_for_btn.clone(),
                                );
                            });
                            row.add_suffix(&view_btn);

                            let b_for_act = b.clone();
                            let m_for_act = mount.clone();
                            let row_for_act = row.clone();
                            row.connect_activated(move |_| {
                                MountActivityDialog::present(
                                    Some(&row_for_act),
                                    &b_for_act,
                                    m_for_act.clone(),
                                );
                            });

                            grp.add(&row);
                            rows_map
                                .borrow_mut()
                                .insert(mount_id_str.clone(), (row.clone(), pbar.clone()));

                            // Fetch initial mount progress
                            let b_init = b.clone();
                            let m_id_init = mount.id.clone();
                            let row_init = row.clone();
                            let pb_init = pbar.clone();

                            glib::MainContext::default().spawn_local(async move {
                                if let Ok(snap) = b_init.mount_progress(&m_id_init).await {
                                    let frac = if snap.in_progress_items == 0 {
                                        1.0
                                    } else {
                                        let tot = snap.total_items.max(1);
                                        (snap.completed_items as f64 / tot as f64).clamp(0.0, 1.0)
                                    };
                                    pb_init.set_fraction(frac);

                                    match snap.status {
                                        MountStatus::Paused => {
                                            row_init.set_subtitle("Synchronization paused");
                                        }
                                        MountStatus::Error(ref err) => {
                                            row_init.set_subtitle(&format!("Error: {err}"));
                                        }
                                        _ => {
                                            if snap.in_progress_items == 0 {
                                                if snap.completed_items > 0 {
                                                    row_init.set_subtitle(&format!(
                                                        "All files synchronized ({} items • Up to date)",
                                                        snap.completed_items
                                                    ));
                                                } else {
                                                    row_init.set_subtitle(
                                                        "All files synchronized • Up to date",
                                                    );
                                                }
                                            } else {
                                                row_init.set_subtitle(&format!(
                                                    "Syncing: {} of {} completed ({} in progress)…",
                                                    snap.completed_items,
                                                    snap.total_items,
                                                    snap.in_progress_items
                                                ));
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    } else {
                        empty_row.set_title("No Folders Configured");
                        empty_row.set_subtitle(
                            "Configure synchronization mounts in the Sync Folders tab",
                        );
                        grp.add(&empty_row);
                    }
                });
            })
        };
        refresh_mounts();

        // 2. Thumbnail Downloads with total progress and subwindow launcher
        let thumbs_group = adw::PreferencesGroup::builder()
            .title("Media Downloads")
            .description("Background prefetching and caching of image and document thumbnails.")
            .build();

        let thumbs_row = adw::ActionRow::builder()
            .title("Thumbnail Downloads")
            .subtitle("All thumbnails downloaded • Up to date")
            .activatable(true)
            .build();
        thumbs_row.add_prefix(&Image::from_icon_name("image-x-generic-symbolic"));

        let pbar = ProgressBar::new();
        pbar.set_valign(Align::Center);
        pbar.set_size_request(120, -1);
        pbar.set_fraction(1.0);
        thumbs_row.add_suffix(&pbar);

        let view_btn = Button::from_icon_name("go-next-symbolic");
        view_btn.add_css_class("flat");
        view_btn.set_valign(Align::Center);
        view_btn.set_tooltip_text(Some("View thumbnail download details"));
        let b_for_btn = backend.clone();
        let row_for_btn = thumbs_row.clone();
        view_btn.connect_clicked(move |_| {
            crate::files::ThumbnailDownloadsDialog::present(Some(&row_for_btn), &b_for_btn);
        });
        thumbs_row.add_suffix(&view_btn);

        let b_for_act = backend.clone();
        let row_for_act = thumbs_row.clone();
        thumbs_row.connect_activated(move |_| {
            crate::files::ThumbnailDownloadsDialog::present(Some(&row_for_act), &b_for_act);
        });

        // Initialize progress from SDK coordinator snapshot
        let init_snap = backend.thumbnail_queue_snapshot();
        if init_snap.in_progress == 0 {
            pbar.set_fraction(1.0);
            if init_snap.completed > 0 {
                thumbs_row.set_subtitle(&format!(
                    "All thumbnails downloaded ({} items cached • Up to date)",
                    init_snap.completed
                ));
            } else {
                thumbs_row.set_subtitle("All thumbnails downloaded • Up to date");
            }
        } else {
            let total = init_snap.total.max(1);
            let frac = (init_snap.completed as f64 / total as f64).clamp(0.0, 1.0);
            pbar.set_fraction(frac);
            thumbs_row.set_subtitle(&format!(
                "Downloading: {} of {} completed ({} in progress)…",
                init_snap.completed, init_snap.total, init_snap.in_progress
            ));
        }

        thumbs_group.add(&thumbs_row);
        page.add(&thumbs_group);

        // 3. Recent Transfers & Hydrations
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

        let thumbs_row_clone = thumbs_row.clone();
        let pbar_clone = pbar.clone();
        let act_grp_clone = activity_group.clone();
        let empty_row_clone = empty_row.clone();
        let empty_removed = Rc::new(RefCell::new(false));

        let cur_completed = Rc::new(RefCell::new(init_snap.completed));
        let cur_total = Rc::new(RefCell::new(init_snap.total));
        let cur_in_prog = Rc::new(RefCell::new(init_snap.in_progress));

        let update_summary = {
            let row = thumbs_row_clone.clone();
            let pbar = pbar_clone.clone();
            let c_comp = cur_completed.clone();
            let c_tot = cur_total.clone();
            let c_prog = cur_in_prog.clone();

            Rc::new(move || {
                let comp = *c_comp.borrow();
                let tot = *c_tot.borrow();
                let prog = *c_prog.borrow();

                if prog == 0 {
                    pbar.set_fraction(1.0);
                    if comp > 0 {
                        row.set_subtitle(&format!(
                            "All thumbnails downloaded ({comp} items cached • Up to date)"
                        ));
                    } else {
                        row.set_subtitle("All thumbnails downloaded • Up to date");
                    }
                } else {
                    let full_total = tot.max(comp + prog).max(1);
                    let frac = (comp as f64 / full_total as f64).clamp(0.0, 1.0);
                    pbar.set_fraction(frac);
                    row.set_subtitle(&format!(
                        "Downloading: {comp} of {full_total} completed ({prog} in progress)…"
                    ));
                }
            })
        };

        // Listen for live DesktopEvent notifications
        let mut rx = backend.subscribe_events();
        let mount_rows_for_events = mount_rows.clone();
        let ref_mounts_for_events = refresh_mounts.clone();

        glib::MainContext::default().spawn_local(async move {
            while let Ok(event) = rx.recv().await {
                match event {
                    DesktopEvent::SyncMountAdded { ref mount_id } => {
                        ref_mounts_for_events();
                        if !*empty_removed.borrow() {
                            act_grp_clone.remove(&empty_row_clone);
                            *empty_removed.borrow_mut() = true;
                        }
                        let row = adw::ActionRow::builder()
                            .title("Sync Folder Added")
                            .subtitle(format!("Configured sync mount: {mount_id}"))
                            .build();
                        row.add_prefix(&Image::from_icon_name("folder-new-symbolic"));
                        let badge = Image::from_icon_name("object-select-symbolic");
                        badge.add_css_class("success");
                        row.add_suffix(&badge);
                        act_grp_clone.add(&row);
                    }
                    DesktopEvent::SyncMountUpdated { .. }
                    | DesktopEvent::SyncMountRemoved { .. }
                    | DesktopEvent::AccountSwitched { .. } => {
                        ref_mounts_for_events();
                    }
                    DesktopEvent::MountProgress(snap) => {
                        let m_map = mount_rows_for_events.borrow();
                        if let Some((row, pb)) = m_map.get(snap.mount_id.as_str()) {
                            let frac = if snap.in_progress_items == 0 {
                                1.0
                            } else {
                                let tot = snap.total_items.max(1);
                                (snap.completed_items as f64 / tot as f64).clamp(0.0, 1.0)
                            };
                            pb.set_fraction(frac);

                            match snap.status {
                                MountStatus::Paused => {
                                    row.set_subtitle("Synchronization paused");
                                }
                                MountStatus::Error(ref err) => {
                                    row.set_subtitle(&format!("Error: {err}"));
                                }
                                _ => {
                                    if snap.in_progress_items == 0 {
                                        if snap.completed_items > 0 {
                                            row.set_subtitle(&format!(
                                                "All files synchronized ({} items • Up to date)",
                                                snap.completed_items
                                            ));
                                        } else {
                                            row.set_subtitle("All files synchronized • Up to date");
                                        }
                                    } else {
                                        row.set_subtitle(&format!(
                                            "Syncing: {} of {} completed ({} in progress)…",
                                            snap.completed_items,
                                            snap.total_items,
                                            snap.in_progress_items
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    DesktopEvent::ThumbnailDownloadStarted { .. } => {
                        let t = *cur_total.borrow();
                        *cur_total.borrow_mut() = t + 1;
                        let p = *cur_in_prog.borrow();
                        *cur_in_prog.borrow_mut() = p + 1;
                        update_summary();
                    }
                    DesktopEvent::ThumbnailReady { .. } => {
                        let c = *cur_completed.borrow();
                        *cur_completed.borrow_mut() = c + 1;
                        let p = *cur_in_prog.borrow();
                        *cur_in_prog.borrow_mut() = p.saturating_sub(1);
                        update_summary();
                    }
                    DesktopEvent::ThumbnailFailed { .. } => {
                        let p = *cur_in_prog.borrow();
                        *cur_in_prog.borrow_mut() = p.saturating_sub(1);
                        update_summary();
                    }
                    DesktopEvent::ThumbnailProgress {
                        completed,
                        total,
                        in_progress,
                    } => {
                        *cur_completed.borrow_mut() = completed;
                        *cur_total.borrow_mut() = total;
                        *cur_in_prog.borrow_mut() = in_progress;
                        update_summary();
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
                        let badge = Image::from_icon_name("object-select-symbolic");
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
                        let badge = Image::from_icon_name("object-select-symbolic");
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
                        row.add_prefix(&Image::from_icon_name("view-refresh-symbolic"));
                        let badge = Image::from_icon_name("object-select-symbolic");
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
