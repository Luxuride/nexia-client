//! Two-way local folder synchronization page component.

use gtk4::{Align, Button, Image};
use libadwaita as adw;
use libadwaita::prelude::*;
use nextcloud_rust_sdk::format_bytes;

use crate::widget::{FileExplorerBackend, ViewComponent};

/// Preferences page configuring local folder synchronization pairings.
#[derive(Clone)]
pub struct SyncFoldersPage {
    page: adw::PreferencesPage,
}

impl SyncFoldersPage {
    /// Creates a new synchronization folders page bound to `backend`.
    pub fn new<B: FileExplorerBackend>(backend: &B, toast_overlay: &adw::ToastOverlay) -> Self {
        let page = adw::PreferencesPage::new();

        // 1. Virtual Sync Storage Group
        let vfs_group = adw::PreferencesGroup::builder()
            .title("Virtual Sync Mount")
            .description("On-demand virtual filesystem mount cache and local hydration storage.")
            .build();

        let vfs_size_row = adw::ActionRow::builder()
            .title("Downloaded Virtual Files")
            .subtitle("Calculating…")
            .build();
        vfs_size_row.add_prefix(&Image::from_icon_name("folder-download-symbolic"));

        let clear_vfs_btn = Button::with_label("Clear Downloaded Files");
        clear_vfs_btn.set_valign(Align::Center);
        clear_vfs_btn.add_css_class("destructive-action");
        vfs_size_row.add_suffix(&clear_vfs_btn);
        vfs_group.add(&vfs_size_row);

        // Populate virtual sync size & wire clear button
        let b_vfs = backend.clone();
        let row_vfs = vfs_size_row.clone();
        glib::MainContext::default().spawn_local(async move {
            let sz = b_vfs.virtual_sync_downloaded_size().await;
            row_vfs.set_subtitle(&format_bytes(sz));
        });

        let b_clear = backend.clone();
        let row_clear = vfs_size_row.clone();
        let toast_clear = toast_overlay.clone();
        clear_vfs_btn.connect_clicked(move |_| {
            let b = b_clear.clone();
            let row = row_clear.clone();
            let toast = toast_clear.clone();
            glib::MainContext::default().spawn_local(async move {
                match b.clear_virtual_sync_cache().await {
                    Ok(freed) => {
                        let sz = b.virtual_sync_downloaded_size().await;
                        row.set_subtitle(&format_bytes(sz));
                        toast.add_toast(adw::Toast::new(&format!(
                            "Cleared virtual sync cache (freed {})",
                            format_bytes(freed)
                        )));
                    }
                    Err(e) => {
                        toast.add_toast(adw::Toast::new(&format!(
                            "Failed to clear virtual sync cache: {e}"
                        )));
                    }
                }
            });
        });

        page.add(&vfs_group);

        // 2. Folder Synchronization Controls
        let group = adw::PreferencesGroup::builder()
            .title("Local Folder Synchronization")
            .description(
                "Two-way synchronization keeps selected local folders reconciled with remote Nextcloud WebDAV directories.",
            )
            .build();

        let sync_row = adw::ActionRow::builder()
            .title("Automatic Synchronization")
            .subtitle("Active")
            .build();
        sync_row.add_prefix(&Image::from_icon_name("emblem-synchronizing-symbolic"));

        let run_sync_btn = Button::with_label("Sync Now");
        run_sync_btn.set_valign(Align::Center);
        run_sync_btn.add_css_class("suggested-action");

        let backend_clone = backend.clone();
        let toast_sync = toast_overlay.clone();
        run_sync_btn.connect_clicked(move |_| {
            let b = backend_clone.clone();
            let toast = toast_sync.clone();
            glib::MainContext::default().spawn_local(async move {
                match b.trigger_sync().await {
                    Ok(_) => toast.add_toast(adw::Toast::new("Synchronization triggered")),
                    Err(e) => toast.add_toast(adw::Toast::new(&format!("Sync failed: {e}"))),
                }
            });
        });

        let pause_btn = Button::with_label("Pause Sync");
        pause_btn.set_valign(Align::Center);
        pause_btn.add_css_class("flat");

        let b_pause = backend.clone();
        let pause_btn_clone = pause_btn.clone();
        let row_sync_clone = sync_row.clone();
        pause_btn.connect_clicked(move |_| {
            let b = b_pause.clone();
            let btn = pause_btn_clone.clone();
            let row = row_sync_clone.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Ok(paused) = b.is_sync_paused().await {
                    if paused {
                        let _ = b.resume_all_sync().await;
                        btn.set_label("Pause Sync");
                        btn.remove_css_class("destructive-action");
                        row.set_subtitle("Active");
                    } else {
                        let _ = b.pause_all_sync().await;
                        btn.set_label("Resume Sync");
                        btn.add_css_class("destructive-action");
                        row.set_subtitle("Paused");
                    }
                }
            });
        });

        let btn_init = pause_btn.clone();
        let row_init = sync_row.clone();
        let b_init = backend.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok(true) = b_init.is_sync_paused().await {
                btn_init.set_label("Resume Sync");
                btn_init.add_css_class("destructive-action");
                row_init.set_subtitle("Paused");
            } else {
                row_init.set_subtitle("Active");
            }
        });

        sync_row.add_suffix(&pause_btn);
        sync_row.add_suffix(&run_sync_btn);
        group.add(&sync_row);
        page.add(&group);

        Self { page }
    }
}

impl ViewComponent for SyncFoldersPage {
    type RootWidget = adw::PreferencesPage;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.page
    }
}
