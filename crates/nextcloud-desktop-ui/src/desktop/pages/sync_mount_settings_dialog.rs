//! Subwindow dialog displaying storage metrics, cache capacity, and cleanup operations for a sync mount.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Button, Image};
use libadwaita as adw;
use libadwaita::prelude::*;
use nextcloud_rust_sdk::{StoredSyncMount, format_bytes};

use crate::widget::FileExplorerBackend;

/// Modal subwindow presenting per-mount settings, disk consumption, and safe cleanup actions with confirmation.
pub struct SyncMountSettingsDialog;

impl SyncMountSettingsDialog {
    /// Presents the sync mount settings dialog for `mount`.
    pub fn present<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        mount: StoredSyncMount,
        on_change: Rc<dyn Fn()>,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let window = adw::Window::builder()
            .title(
                mount
                    .display_name
                    .as_deref()
                    .unwrap_or("Sync Folder Settings"),
            )
            .default_width(520)
            .default_height(580)
            .modal(true)
            .build();

        if let Some(ref win) = root_window {
            window.set_transient_for(Some(win));
        }

        let header_bar = adw::HeaderBar::new();
        header_bar.set_show_end_title_buttons(true);
        header_bar.set_show_start_title_buttons(false);

        let page = adw::PreferencesPage::new();

        // 1. Mount details group
        let details_group = adw::PreferencesGroup::builder()
            .title("Folder Pairing Details")
            .description("Configuration and location of this synchronization pairing.")
            .build();

        let local_row = adw::ActionRow::builder()
            .title("Local Folder")
            .subtitle(&mount.local_target)
            .build();
        local_row.add_prefix(&Image::from_icon_name("folder-symbolic"));
        details_group.add(&local_row);

        let remote_row = adw::ActionRow::builder()
            .title("Remote Nextcloud Path")
            .subtitle(&mount.remote_path)
            .build();
        remote_row.add_prefix(&Image::from_icon_name("network-server-symbolic"));
        details_group.add(&remote_row);

        // Changeable Synchronization Mode
        let mode_options = [
            "Two-Way Synchronization (Local files mirrored to cloud)",
            "Virtual Filesystem (On-demand hydrated placeholders)",
        ];
        let mode_model = gtk4::StringList::new(&mode_options);
        let initial_selected = if mount.mode == "VirtualVfs" { 1 } else { 0 };

        let mode_combo = adw::ComboRow::builder()
            .title("Synchronization Mode")
            .model(&mode_model)
            .selected(initial_selected)
            .build();
        mode_combo.add_prefix(&Image::from_icon_name("view-refresh-symbolic"));
        details_group.add(&mode_combo);

        let b_mode = backend.clone();
        let m_id_mode = mount.id.clone();
        let toast_mode = toast_overlay.clone();
        let on_change_mode = on_change.clone();

        mode_combo.connect_selected_notify(move |combo| {
            let b = b_mode.clone();
            let m_id = m_id_mode.clone();
            let toast = toast_mode.clone();
            let cb = on_change_mode.clone();
            let new_mode = match combo.selected() {
                1 => "VirtualVfs",
                _ => "BidirectionalSync",
            };
            glib::MainContext::default().spawn_local(async move {
                if b.update_sync_mount_mode(&m_id, new_mode).await.is_ok() {
                    let desc = if new_mode == "VirtualVfs" {
                        "Switched to Virtual Filesystem"
                    } else {
                        "Switched to Two-Way Synchronization"
                    };
                    toast.add_toast(adw::Toast::new(desc));
                    cb();
                }
            });
        });

        page.add(&details_group);

        // 2. Local Disk Usage Group
        let storage_group = adw::PreferencesGroup::builder()
            .title("Local Storage Usage")
            .description("Disk space consumed on your local drive by this synchronization mount.")
            .build();

        let local_usage_row = adw::ActionRow::builder()
            .title(if mount.mode == "VirtualVfs" {
                "Downloaded & Hydrated Files"
            } else {
                "Local Synced Files"
            })
            .subtitle("Calculating…")
            .build();
        local_usage_row.add_prefix(&Image::from_icon_name("drive-harddisk-symbolic"));
        storage_group.add(&local_usage_row);

        let thumbs_usage_row = adw::ActionRow::builder()
            .title("Shared Thumbnail Previews")
            .subtitle("Calculating…")
            .build();
        thumbs_usage_row.add_prefix(&Image::from_icon_name("image-x-generic-symbolic"));
        storage_group.add(&thumbs_usage_row);

        page.add(&storage_group);

        // 3. Cleanup Actions Group
        let clean_group = adw::PreferencesGroup::builder()
            .title("Storage Cleanup")
            .description("Free up local storage space safely. Remote files remain untouched.")
            .build();

        let clear_files_row = adw::ActionRow::builder()
            .title(if mount.mode == "VirtualVfs" {
                "Clear Downloaded Files"
            } else {
                "Clear Local Sync Cache"
            })
            .subtitle(if mount.mode == "VirtualVfs" {
                "Dehydrate local files back to placeholders to reclaim storage"
            } else {
                "Reset temporary synchronization journal and cache"
            })
            .build();

        let clear_files_btn = Button::with_label("Clear Files");
        clear_files_btn.set_valign(Align::Center);
        clear_files_btn.add_css_class("destructive-action");
        clear_files_row.add_suffix(&clear_files_btn);
        clean_group.add(&clear_files_row);

        let clear_thumbs_row = adw::ActionRow::builder()
            .title("Clear Shared Previews")
            .subtitle("Remove cached thumbnail images (will re-download when viewed)")
            .build();

        let clear_thumbs_btn = Button::with_label("Clear Previews");
        clear_thumbs_btn.set_valign(Align::Center);
        clear_thumbs_btn.add_css_class("destructive-action");
        clear_thumbs_row.add_suffix(&clear_thumbs_btn);
        clean_group.add(&clear_thumbs_row);

        page.add(&clean_group);

        // 4. Remove sync pairing (for user-added mounts)
        let is_app_default = mount.id == "app-default-vfs";
        if !is_app_default {
            let remove_group = adw::PreferencesGroup::builder()
                .title("Remove Synchronization")
                .description("Stop synchronizing this folder with Nextcloud.")
                .build();

            let remove_row = adw::ActionRow::builder()
                .title("Remove Folder Pairing")
                .subtitle("Local files will remain on your computer, but will no longer sync")
                .build();

            let remove_btn = Button::with_label("Remove Sync Folder");
            remove_btn.set_valign(Align::Center);
            remove_btn.add_css_class("destructive-action");
            remove_row.add_suffix(&remove_btn);
            remove_group.add(&remove_row);
            page.add(&remove_group);

            let b_rm = backend.clone();
            let toast_rm = toast_overlay.clone();
            let win_rm = window.clone();
            let cb_rm = on_change.clone();
            let m_id_rm = mount.id.clone();
            let m_name_rm = mount
                .display_name
                .clone()
                .unwrap_or_else(|| mount.remote_path.clone());

            remove_btn.connect_clicked(move |_| {
                let confirm = adw::MessageDialog::builder()
                    .heading("Remove Sync Folder?")
                    .body(format!(
                        "Choose how you want to remove the pairing for “{m_name_rm}”:\n\n• Unlink: Stop synchronizing, but keep all local files on disk.\n• Remove & Delete: Stop synchronizing and delete all local files and placeholders from your disk."
                    ))
                    .build();
                confirm.set_transient_for(Some(&win_rm));
                confirm.add_response("cancel", "Cancel");
                confirm.add_response("unlink", "Unlink (Keep Local Files)");
                confirm.add_response("cleanup", "Remove & Delete Local Files");
                confirm.set_response_appearance("cleanup", adw::ResponseAppearance::Destructive);
                confirm.set_default_response(Some("cancel"));
                confirm.set_close_response("cancel");

                let b = b_rm.clone();
                let toast = toast_rm.clone();
                let win = win_rm.clone();
                let cb = cb_rm.clone();
                let m_id = m_id_rm.clone();

                confirm.connect_response(None, move |d, response| {
                    if response == "unlink" || response == "cleanup" {
                        let clean_local = response == "cleanup";
                        let b = b.clone();
                        let toast = toast.clone();
                        let win = win.clone();
                        let cb = cb.clone();
                        let m_id = m_id.clone();
                        glib::MainContext::default().spawn_local(async move {
                            match b.remove_sync_mount_with_cleanup(&m_id, clean_local).await {
                                Ok(()) => {
                                    let msg = if clean_local {
                                        "Sync folder removed and local files deleted"
                                    } else {
                                        "Sync folder unlinked successfully (local files kept)"
                                    };
                                    toast.add_toast(adw::Toast::new(msg));
                                    cb();
                                    win.close();
                                }
                                Err(e) => {
                                    toast.add_toast(adw::Toast::new(&format!(
                                        "Failed to remove sync mount: {e}"
                                    )));
                                }
                            }
                        });
                    }
                    d.close();
                });
                confirm.present();
            });
        }

        // Live storage query & labels refresher
        let b_refresh = backend.clone();
        let m_id_c = mount.id.clone();
        let row_loc_c = local_usage_row.clone();
        let row_thm_c = thumbs_usage_row.clone();
        let btn_files_c = clear_files_btn.clone();
        let btn_thm_c = clear_thumbs_btn.clone();

        let bytes_files_ref = Rc::new(std::cell::Cell::new(0u64));
        let bytes_thm_ref = Rc::new(std::cell::Cell::new(0u64));

        let b_cell = bytes_files_ref.clone();
        let t_cell = bytes_thm_ref.clone();

        let refresh_usage = Rc::new(move || {
            let b = b_refresh.clone();
            let m_id = m_id_c.clone();
            let row_loc = row_loc_c.clone();
            let row_thm = row_thm_c.clone();
            let btn_files = btn_files_c.clone();
            let btn_thm = btn_thm_c.clone();
            let b_c = b_cell.clone();
            let t_c = t_cell.clone();

            glib::MainContext::default().spawn_local(async move {
                match b.mount_storage_usage(&m_id).await {
                    Ok(usage) => {
                        b_c.set(usage.local_bytes);
                        t_c.set(usage.shared_thumbnail_bytes);

                        let loc_str = format_bytes(usage.local_bytes);
                        let thm_str = format_bytes(usage.shared_thumbnail_bytes);

                        row_loc.set_subtitle(&format!(
                            "{loc_str} ({} files)",
                            usage.hydrated_files_count
                        ));
                        row_thm.set_subtitle(&thm_str);

                        btn_files.set_label(&format!("Clear ({loc_str})"));
                        btn_thm.set_label(&format!("Clear ({thm_str})"));
                    }
                    Err(_) => {
                        row_loc.set_subtitle("0 B (0 files)");
                        row_thm.set_subtitle("0 B");
                        btn_files.set_label("Clear (0 B)");
                        btn_thm.set_label("Clear (0 B)");
                    }
                }
            });
        });

        refresh_usage();

        // Wire Clear Files with strict confirmation
        {
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let win = window.clone();
            let ref_fn = refresh_usage.clone();
            let m_id = mount.id.clone();
            let b_c = bytes_files_ref;

            clear_files_btn.connect_clicked(move |_| {
                let bytes = b_c.get();
                let size_str = format_bytes(bytes);
                let confirm = adw::MessageDialog::builder()
                    .heading("Clear Downloaded Files?")
                    .body(format!(
                        "This will delete {size_str} of locally cached files.\n\nYour files remain safe on your Nextcloud server and will be re-downloaded when opened."
                    ))
                    .build();
                confirm.set_transient_for(Some(&win));
                confirm.add_response("cancel", "Cancel");
                confirm.add_response("clear", &format!("Clear {size_str}"));
                confirm.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
                confirm.set_default_response(Some("cancel"));
                confirm.set_close_response("cancel");

                let b = b.clone();
                let toast = toast.clone();
                let ref_fn = ref_fn.clone();
                let m_id = m_id.clone();

                confirm.connect_response(None, move |d, response| {
                    if response == "clear" {
                        let b = b.clone();
                        let toast = toast.clone();
                        let ref_fn = ref_fn.clone();
                        let m_id = m_id.clone();
                        glib::MainContext::default().spawn_local(async move {
                            match b.clear_mount_cache(&m_id, true).await {
                                Ok(freed) => {
                                    toast.add_toast(adw::Toast::new(&format!(
                                        "Freed {} of local file storage",
                                        format_bytes(freed)
                                    )));
                                    ref_fn();
                                }
                                Err(e) => {
                                    toast.add_toast(adw::Toast::new(&format!(
                                        "Failed to clear mount cache: {e}"
                                    )));
                                }
                            }
                        });
                    }
                    d.close();
                });
                confirm.present();
            });
        }

        // Wire Clear Shared Thumbnails with strict confirmation
        {
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let win = window.clone();
            let ref_fn = refresh_usage;
            let t_c = bytes_thm_ref;

            clear_thumbs_btn.connect_clicked(move |_| {
                let bytes = t_c.get();
                let size_str = format_bytes(bytes);
                let confirm = adw::MessageDialog::builder()
                    .heading("Clear Shared Previews?")
                    .body(format!(
                        "This will delete {size_str} of cached thumbnail images.\n\nPreviews will be re-downloaded when viewed in Cloud Files."
                    ))
                    .build();
                confirm.set_transient_for(Some(&win));
                confirm.add_response("cancel", "Cancel");
                confirm.add_response("clear", &format!("Clear {size_str}"));
                confirm.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
                confirm.set_default_response(Some("cancel"));
                confirm.set_close_response("cancel");

                let b = b.clone();
                let toast = toast.clone();
                let ref_fn = ref_fn.clone();

                confirm.connect_response(None, move |d, response| {
                    if response == "clear" {
                        let b = b.clone();
                        let toast = toast.clone();
                        let ref_fn = ref_fn.clone();
                        glib::MainContext::default().spawn_local(async move {
                            match b.clear_shared_thumbnails().await {
                                Ok(freed) => {
                                    toast.add_toast(adw::Toast::new(&format!(
                                        "Freed {} of preview thumbnail storage",
                                        format_bytes(freed)
                                    )));
                                    ref_fn();
                                }
                                Err(e) => {
                                    toast.add_toast(adw::Toast::new(&format!(
                                        "Failed to clear thumbnail cache: {e}"
                                    )));
                                }
                            }
                        });
                    }
                    d.close();
                });
                confirm.present();
            });
        }

        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content_box.append(&header_bar);
        content_box.append(&page);
        window.set_content(Some(&content_box));
        window.present();
    }
}
