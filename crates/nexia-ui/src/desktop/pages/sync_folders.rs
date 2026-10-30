//! Two-way local folder synchronization page component.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Button, Image};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::desktop::pages::add_sync_dialog::AddSyncFolderDialog;
use crate::desktop::pages::sync_mount_settings_dialog::SyncMountSettingsDialog;
use crate::widget::{FileExplorerBackend, ViewComponent};

type RefreshCallback = Rc<dyn Fn()>;
type RefreshHolder = Rc<RefCell<Option<RefreshCallback>>>;

/// Renders the global pause row and master button for the active account.
async fn refresh_global_pause_state<B: FileExplorerBackend>(
    backend: &B,
    button: &Button,
    row: &adw::ActionRow,
) {
    if let Ok(true) = backend.is_sync_paused().await {
        button.set_label("Resume All Sync");
        button.add_css_class("destructive-action");
        row.set_subtitle("Paused");
    } else {
        button.set_label("Pause All Sync");
        button.remove_css_class("destructive-action");
        row.set_subtitle("Active");
    }
}

/// Preferences page configuring local folder synchronization pairings.
#[derive(Clone)]
pub struct SyncFoldersPage {
    page: adw::PreferencesPage,
}

impl SyncFoldersPage {
    /// Creates a new synchronization folders page bound to `backend`.
    pub fn new<B: FileExplorerBackend>(backend: &B, toast_overlay: &adw::ToastOverlay) -> Self {
        let page = adw::PreferencesPage::new();

        // 1. Global Synchronization Controls Group
        let global_group = adw::PreferencesGroup::builder()
            .title("Global Synchronization")
            .description("Master controls to pause, resume, or add synchronization folders across Nextcloud.")
            .build();

        let global_row = adw::ActionRow::builder()
            .title("All Synchronization")
            .subtitle("Active")
            .build();
        global_row.add_prefix(&Image::from_icon_name("view-refresh-symbolic"));

        let global_pause_btn = Button::with_label("Pause All Sync");
        global_pause_btn.set_valign(Align::Center);
        global_pause_btn.add_css_class("flat");

        let add_sync_btn = Button::with_label("Add Folder Sync…");
        add_sync_btn.set_valign(Align::Center);
        add_sync_btn.add_css_class("suggested-action");

        global_row.add_suffix(&global_pause_btn);
        global_row.add_suffix(&add_sync_btn);
        global_group.add(&global_row);
        page.add(&global_group);

        // 2. Configured Sync Folders Group
        let syncs_group = adw::PreferencesGroup::builder()
            .title("Synchronization Folders")
            .description("Configured folder pairings managed by the Nextcloud SDK.")
            .build();
        page.add(&syncs_group);

        // Wire Global Pause / Resume
        let b_gp = backend.clone();
        let btn_gp = global_pause_btn.clone();
        let row_gp = global_row.clone();
        let toast_gp = toast_overlay.clone();

        global_pause_btn.connect_clicked(move |_| {
            let b = b_gp.clone();
            let btn = btn_gp.clone();
            let row = row_gp.clone();
            let toast = toast_gp.clone();

            glib::MainContext::default().spawn_local(async move {
                if let Ok(paused) = b.is_sync_paused().await {
                    if paused {
                        let _ = b.resume_all_sync().await;
                        btn.set_label("Pause All Sync");
                        btn.remove_css_class("destructive-action");
                        row.set_subtitle("Active");
                        toast.add_toast(adw::Toast::new("All synchronization resumed"));
                    } else {
                        let _ = b.pause_all_sync().await;
                        btn.set_label("Resume All Sync");
                        btn.add_css_class("destructive-action");
                        row.set_subtitle("Paused");
                        toast.add_toast(adw::Toast::new("All synchronization paused"));
                    }
                }
            });
        });

        // Global pause state is per-account, so it is re-read on account changes rather than
        // captured once at construction.
        let pause_state: RefreshCallback = {
            let b = backend.clone();
            let btn = global_pause_btn.clone();
            let row = global_row.clone();
            Rc::new(move || {
                let b = b.clone();
                let btn = btn.clone();
                let row = row.clone();
                glib::MainContext::default().spawn_local(async move {
                    refresh_global_pause_state(&b, &btn, &row).await;
                });
            })
        };
        pause_state();

        // Dynamic list populator
        let b_list = backend.clone();
        let toast_list = toast_overlay.clone();
        let grp_list = syncs_group;
        let page_for_dialog = page.clone();
        let tracked_rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));
        let tracked_list = tracked_rows.clone();
        let refresh_gen: Rc<std::cell::Cell<u64>> = Rc::new(std::cell::Cell::new(0));
        let gen_counter = refresh_gen.clone();

        let refresh_holder: RefreshHolder = Rc::new(RefCell::new(None));
        let rh = refresh_holder.clone();

        let refresh_list = Rc::new(move || {
            let b = b_list.clone();
            let toast = toast_list.clone();
            let grp = grp_list.clone();
            let pg = page_for_dialog.clone();
            let tracked = tracked_list.clone();
            let rh_c = rh.clone();
            let counter = gen_counter.clone();

            let my_gen = counter.get().wrapping_add(1);
            counter.set(my_gen);

            glib::MainContext::default().spawn_local(async move {
                let mounts = b.sync_mounts().await;

                // Discard render if a newer refresh was triggered while awaiting
                if counter.get() != my_gen {
                    return;
                }

                // Clear existing mount rows right before populating
                for r in tracked.borrow_mut().drain(..) {
                    grp.remove(&r);
                }
                if mounts.is_empty() {
                    let empty_row = adw::ActionRow::builder()
                        .title("No Folders Configured")
                        .subtitle(
                            "Click “Add Folder Sync…” to configure two-way local directory sync",
                        )
                        .build();
                    empty_row.add_prefix(&Image::from_icon_name("folder-symbolic"));
                    grp.add(&empty_row);
                    tracked.borrow_mut().push(empty_row);
                    return;
                }

                for mount in mounts {
                    let is_on_demand = mount.mode != "BidirectionalSync";
                    let is_app_vfs = mount.id.starts_with("app-") || mount.id == "app-default-vfs";

                    let title_text = mount.display_name.clone().unwrap_or_else(|| {
                        if is_app_vfs {
                            "Application Virtual Drive".to_string()
                        } else {
                            mount.remote_path.clone()
                        }
                    });

                    let mode_badge = if is_app_vfs {
                        "Internal Cache"
                    } else if is_on_demand {
                        "On-Demand (FUSE)"
                    } else {
                        "Two-Way Sync"
                    };

                    let row = adw::ActionRow::builder()
                        .title(&title_text)
                        .subtitle(format!(
                            "Local: {}  •  Remote: {}  •  {}",
                            mount.local_target, mount.remote_path, mode_badge
                        ))
                        .activatable(true)
                        .build();

                    let icon_name = if is_on_demand {
                        "folder-remote-symbolic"
                    } else {
                        "folder-download-symbolic"
                    };
                    row.add_prefix(&Image::from_icon_name(icon_name));

                    if !is_app_vfs {
                        // Per-mount Sync Now button
                        let sync_btn = Button::with_label("Sync Now");
                        sync_btn.set_valign(Align::Center);
                        sync_btn.add_css_class("flat");

                        let b_sync = b.clone();
                        let m_id_sync = mount.id.clone();
                        let t_sync = toast.clone();

                        sync_btn.connect_clicked(move |_| {
                            let b = b_sync.clone();
                            let m_id = m_id_sync.clone();
                            let toast = t_sync.clone();

                            glib::MainContext::default().spawn_local(async move {
                                toast.add_toast(adw::Toast::new("Synchronizing folder…"));
                                let _ = b.sync_mount(&m_id).await;
                            });
                        });

                        row.add_suffix(&sync_btn);

                        // Per-mount Pause / Resume button
                        let pause_btn = Button::with_label("Pause");
                        pause_btn.set_valign(Align::Center);
                        pause_btn.add_css_class("flat");

                        let b_p = b.clone();
                        let m_id_p = mount.id.clone();
                        let btn_p = pause_btn.clone();
                        let t_p = toast.clone();

                        pause_btn.connect_clicked(move |_| {
                            let b = b_p.clone();
                            let m_id = m_id_p.clone();
                            let btn = btn_p.clone();
                            let toast = t_p.clone();

                            glib::MainContext::default().spawn_local(async move {
                                if let Ok(paused) = b.is_mount_paused(&m_id).await {
                                    if paused {
                                        let _ = b.resume_mount(&m_id).await;
                                        btn.set_label("Pause");
                                        btn.remove_css_class("destructive-action");
                                        toast.add_toast(adw::Toast::new(
                                            "Folder synchronization resumed",
                                        ));
                                    } else {
                                        let _ = b.pause_mount(&m_id).await;
                                        btn.set_label("Resume");
                                        btn.add_css_class("destructive-action");
                                        toast.add_toast(adw::Toast::new(
                                            "Folder synchronization paused",
                                        ));
                                    }
                                }
                            });
                        });

                        // Initialize per-mount pause button status
                        let b_init_p = b.clone();
                        let m_id_init_p = mount.id.clone();
                        let btn_init_p = pause_btn.clone();
                        glib::MainContext::default().spawn_local(async move {
                            if let Ok(true) = b_init_p.is_mount_paused(&m_id_init_p).await {
                                btn_init_p.set_label("Resume");
                                btn_init_p.add_css_class("destructive-action");
                            }
                        });

                        row.add_suffix(&pause_btn);
                    }

                    // Configure / Settings button
                    let settings_btn = Button::from_icon_name("emblem-system-symbolic");
                    settings_btn.set_valign(Align::Center);
                    settings_btn.add_css_class("flat");
                    settings_btn.set_tooltip_text(Some("Sync folder settings & storage cleanup"));

                    let b_s = b.clone();
                    let t_s = toast.clone();
                    let m_s = mount.clone();
                    let pg_s = pg.clone();

                    let rh_for_settings = rh_c.clone();
                    let open_settings = Rc::new(move || {
                        let rh_call = rh_for_settings.clone();
                        SyncMountSettingsDialog::present(
                            Some(&pg_s),
                            &b_s,
                            &t_s,
                            m_s.clone(),
                            Rc::new(move || {
                                if let Some(ref f) = *rh_call.borrow() {
                                    f();
                                }
                            }),
                        );
                    });

                    let os1 = open_settings.clone();
                    settings_btn.connect_clicked(move |_| {
                        os1();
                    });

                    let os2 = open_settings;
                    row.connect_activated(move |_| {
                        os2();
                    });

                    row.add_suffix(&settings_btn);
                    grp.add(&row);
                    tracked.borrow_mut().push(row);
                }
            });
        });

        *refresh_holder.borrow_mut() = Some(refresh_list.clone());
        refresh_list();

        // Subscribe to account switches and sync events to update the sync folder pairings list
        let mut event_rx = backend.subscribe_events();
        let ref_for_events = refresh_list.clone();
        let pause_for_events = pause_state.clone();
        glib::MainContext::default().spawn_local(async move {
            while let Ok(event) = event_rx.recv().await {
                match event {
                    nexia_core::events::DesktopEvent::AccountSwitched { .. }
                    | nexia_core::events::DesktopEvent::AccountSignedOut { .. } => {
                        pause_for_events();
                        ref_for_events();
                    }
                    nexia_core::events::DesktopEvent::SyncMountAdded { .. }
                    | nexia_core::events::DesktopEvent::SyncMountUpdated { .. }
                    | nexia_core::events::DesktopEvent::SyncMountRemoved { .. } => {
                        ref_for_events();
                    }
                    _ => {}
                }
            }
        });

        // Wire Add Sync Folder button
        let b_add = backend.clone();
        let toast_add = toast_overlay.clone();
        let ref_add = refresh_list;
        let pg_add = page.clone();

        add_sync_btn.connect_clicked(move |_| {
            let ref_fn = ref_add.clone();
            AddSyncFolderDialog::present(
                Some(&pg_add),
                &b_add,
                &toast_add,
                Rc::new(move || {
                    ref_fn();
                }),
            );
        });

        Self { page }
    }
}

impl ViewComponent for SyncFoldersPage {
    type RootWidget = adw::PreferencesPage;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.page
    }
}
