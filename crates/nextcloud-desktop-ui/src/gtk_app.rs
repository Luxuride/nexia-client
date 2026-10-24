//! GTK 4 desktop user interface implementation.

#[cfg(feature = "gtk")]
use gtk4::prelude::*;
#[cfg(feature = "gtk")]
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, HeaderBar, Label, ListBox, ListBoxRow,
    Orientation, Paned, ScrolledWindow, Stack,
};
#[cfg(feature = "gtk")]
use std::cell::RefCell;
#[cfg(feature = "gtk")]
use std::rc::Rc;
#[cfg(feature = "gtk")]
use tokio::sync::mpsc;

use crate::controller::DesktopUiController;
use crate::error::UiError;
#[cfg(feature = "gtk")]
use crate::model::FileSyncStatus;
#[cfg(feature = "gtk")]
use crate::tray::{TrayAction, spawn_app_bar_tray};

/// Launches the Nextcloud Desktop GTK application.
#[cfg(feature = "gtk")]
pub fn launch_gtk_application(
    controller: DesktopUiController,
    start_minimized: bool,
) -> Result<(), UiError> {
    let app = Application::builder()
        .application_id("com.nextcloud.desktop.rs")
        .build();

    let controller = Rc::new(RefCell::new(controller));

    app.connect_activate(move |app| {
        build_ui(app, controller.clone(), start_minimized);
    });

    let empty_args: Vec<String> = Vec::new();
    app.run_with_args(&empty_args);
    Ok(())
}

#[cfg(feature = "gtk")]
fn build_ui(
    app: &Application,
    controller: Rc<RefCell<DesktopUiController>>,
    start_minimized: bool,
) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Nextcloud Desktop")
        .default_width(950)
        .default_height(650)
        .build();

    // App bar tray communication channel
    let (tx_tray, mut rx_tray) = mpsc::unbounded_channel::<TrayAction>();
    spawn_app_bar_tray(tx_tray);

    // Header bar
    let header_bar = HeaderBar::new();
    let title_label = Label::new(Some("Nextcloud Desktop"));
    title_label.add_css_class("title");
    header_bar.set_title_widget(Some(&title_label));

    let refresh_btn = Button::with_label("🔄 Refresh VFS");
    let sync_btn = Button::with_label("⚡ Sync Folders");
    header_bar.pack_start(&refresh_btn);
    header_bar.pack_start(&sync_btn);

    window.set_titlebar(Some(&header_bar));

    // Intercept window close to keep running in background / app bar
    window.connect_close_request(move |win| {
        tracing::info!("Minimizing Nextcloud Desktop to background app bar");
        win.set_visible(false);
        glib::Propagation::Stop
    });

    // Main layout: Paned with Sidebar and Content Stack
    let paned = Paned::new(Orientation::Horizontal);
    paned.set_position(220);

    // Sidebar
    let sidebar_box = GtkBox::new(Orientation::Vertical, 6);
    sidebar_box.set_margin_top(12);
    sidebar_box.set_margin_bottom(12);
    sidebar_box.set_margin_start(12);
    sidebar_box.set_margin_end(12);

    let sidebar_title = Label::new(Some("Navigation"));
    sidebar_title.add_css_class("heading");
    sidebar_box.append(&sidebar_title);

    let sidebar_list = ListBox::new();
    sidebar_list.add_css_class("navigation-sidebar");

    let cloud_row = ListBoxRow::new();
    cloud_row.set_child(Some(&Label::new(Some("☁️  Cloud Files (Virtual)"))));
    sidebar_list.append(&cloud_row);

    let sync_row = ListBoxRow::new();
    sync_row.set_child(Some(&Label::new(Some("🔄  Local Sync Folders"))));
    sidebar_list.append(&sync_row);

    let transfer_row = ListBoxRow::new();
    transfer_row.set_child(Some(&Label::new(Some("📊  Transfers & Activity"))));
    sidebar_list.append(&transfer_row);

    let settings_row = ListBoxRow::new();
    settings_row.set_child(Some(&Label::new(Some("⚙️  Settings"))));
    sidebar_list.append(&settings_row);

    sidebar_box.append(&sidebar_list);
    paned.set_start_child(Some(&sidebar_box));

    // Content Stack
    let content_stack = Stack::new();

    // Page 1: Cloud Files (Virtual Sync Browser)
    let vfs_page = GtkBox::new(Orientation::Vertical, 8);
    vfs_page.set_margin_top(12);
    vfs_page.set_margin_bottom(12);
    vfs_page.set_margin_start(12);
    vfs_page.set_margin_end(12);

    // Breadcrumb and path bar
    let path_bar = GtkBox::new(Orientation::Horizontal, 8);
    let up_btn = Button::with_label("⬆ Up");
    let current_path_label = Label::new(Some("/"));
    current_path_label.set_hexpand(true);
    current_path_label.set_halign(gtk4::Align::Start);
    current_path_label.add_css_class("caption");

    path_bar.append(&up_btn);
    path_bar.append(&current_path_label);
    vfs_page.append(&path_bar);

    // File list
    let file_list = ListBox::new();
    let scrolled = ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&file_list));
    vfs_page.append(&scrolled);

    content_stack.add_named(&vfs_page, Some("cloud_files"));

    // Page 2: Local Sync Folders
    let sync_page = GtkBox::new(Orientation::Vertical, 12);
    sync_page.set_margin_top(16);
    sync_page.set_margin_start(16);
    let sync_heading = Label::new(Some("Configured Local Sync Pairs"));
    sync_heading.add_css_class("heading");
    sync_page.append(&sync_heading);
    let sync_info = Label::new(Some(
        "Two-way full sync synchronizes selected local directories with Nextcloud WebDAV.",
    ));
    sync_page.append(&sync_info);
    content_stack.add_named(&sync_page, Some("sync_folders"));

    // Page 3: Transfers & Activity
    let transfers_page = GtkBox::new(Orientation::Vertical, 12);
    transfers_page.set_margin_top(16);
    transfers_page.set_margin_start(16);
    let transfers_heading = Label::new(Some("Transfers & Sync Activity"));
    transfers_heading.add_css_class("heading");
    transfers_page.append(&transfers_heading);
    let transfers_info = Label::new(Some(
        "Recent uploads, downloads, and on-demand file hydration events.",
    ));
    transfers_page.append(&transfers_info);
    content_stack.add_named(&transfers_page, Some("transfers"));

    // Page 4: Settings
    let settings_page = GtkBox::new(Orientation::Vertical, 12);
    settings_page.set_margin_top(16);
    settings_page.set_margin_start(16);
    let settings_heading = Label::new(Some("Account & Storage Settings"));
    settings_heading.add_css_class("heading");
    settings_page.append(&settings_heading);
    content_stack.add_named(&settings_page, Some("settings"));

    paned.set_end_child(Some(&content_stack));
    window.set_child(Some(&paned));

    // Connect sidebar selection to stack pages
    let stack_clone = content_stack.clone();
    sidebar_list.connect_row_selected(move |_, row| {
        if let Some(r) = row {
            match r.index() {
                0 => stack_clone.set_visible_child_name("cloud_files"),
                1 => stack_clone.set_visible_child_name("sync_folders"),
                2 => stack_clone.set_visible_child_name("transfers"),
                3 => stack_clone.set_visible_child_name("settings"),
                _ => {}
            }
        }
    });

    // Populate initial files
    let ctrl_for_refresh = controller.clone();
    let fl_clone = file_list.clone();
    let cpl_clone = current_path_label.clone();
    refresh_btn.connect_clicked(move |_| {
        let ctrl = ctrl_for_refresh.clone();
        let fl = fl_clone.clone();
        let cpl = cpl_clone.clone();

        glib::MainContext::default().spawn_local(async move {
            let _ = ctrl.borrow().refresh_virtual_tree().await;
            update_file_list(&ctrl.borrow(), &fl, &cpl).await;
        });
    });

    let ctrl_for_sync = controller.clone();
    sync_btn.connect_clicked(move |_| {
        let ctrl = ctrl_for_sync.clone();
        glib::MainContext::default().spawn_local(async move {
            let _ = ctrl.borrow().trigger_sync().await;
        });
    });

    let ctrl_for_up = controller.clone();
    let fl_for_up = file_list.clone();
    let cpl_for_up = current_path_label.clone();
    up_btn.connect_clicked(move |_| {
        let ctrl = ctrl_for_up.clone();
        let fl = fl_for_up.clone();
        let cpl = cpl_for_up.clone();
        glib::MainContext::default().spawn_local(async move {
            ctrl.borrow().navigate_up().await;
            update_file_list(&ctrl.borrow(), &fl, &cpl).await;
        });
    });

    // Background tray action dispatcher loop
    let win_for_tray = window.clone();
    let app_for_tray = app.clone();
    let ctrl_for_tray = controller.clone();
    let fl_for_tray = file_list.clone();
    let cpl_for_tray = current_path_label.clone();

    glib::MainContext::default().spawn_local(async move {
        while let Some(action) = rx_tray.recv().await {
            match action {
                TrayAction::ShowWindow => {
                    win_for_tray.set_visible(true);
                    win_for_tray.present();
                }
                TrayAction::HideWindow => {
                    win_for_tray.set_visible(false);
                }
                TrayAction::TriggerSync => {
                    let _ = ctrl_for_tray.borrow().trigger_sync().await;
                }
                TrayAction::RefreshVfs => {
                    let _ = ctrl_for_tray.borrow().refresh_virtual_tree().await;
                    update_file_list(&ctrl_for_tray.borrow(), &fl_for_tray, &cpl_for_tray).await;
                }
                TrayAction::Quit => {
                    app_for_tray.quit();
                }
            }
        }
    });

    // Initial render
    let ctrl_init = controller.clone();
    let fl_init = file_list.clone();
    let cpl_init = current_path_label.clone();
    glib::MainContext::default().spawn_local(async move {
        update_file_list(&ctrl_init.borrow(), &fl_init, &cpl_init).await;
    });

    if !start_minimized {
        window.present();
    }
}

#[cfg(feature = "gtk")]
async fn update_file_list(
    controller: &DesktopUiController,
    list_box: &ListBox,
    path_label: &Label,
) {
    let cur_path = controller.current_path().await;
    path_label.set_text(&format!("Current Location: {}", cur_path));

    // Clear previous rows
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    if let Ok(items) = controller.list_current_items().await {
        for item in items {
            let row = ListBoxRow::new();
            let row_box = GtkBox::new(Orientation::Horizontal, 12);
            row_box.set_margin_start(8);
            row_box.set_margin_end(8);
            row_box.set_margin_top(6);
            row_box.set_margin_bottom(6);

            let icon_str = if item.is_directory { "📁" } else { "📄" };
            let icon_label = Label::new(Some(icon_str));

            let name_label = Label::new(Some(&item.name));
            name_label.set_hexpand(true);
            name_label.set_halign(gtk4::Align::Start);

            let size_label = Label::new(Some(&item.size_formatted));
            size_label.add_css_class("dim-label");

            let status_label = Label::new(Some(item.status.display_label()));
            match item.status {
                FileSyncStatus::CloudPlaceholder => status_label.add_css_class("dim-label"),
                FileSyncStatus::HydratedOffline => status_label.add_css_class("success"),
                FileSyncStatus::Syncing => status_label.add_css_class("accent"),
            }

            row_box.append(&icon_label);
            row_box.append(&name_label);
            row_box.append(&size_label);
            row_box.append(&status_label);

            if !item.is_directory {
                if item.status == FileSyncStatus::CloudPlaceholder {
                    let hydrate_btn = Button::with_label("Hydrate ⬇");
                    let ctrl = controller.clone();
                    let target_path = item.path.clone();
                    hydrate_btn.connect_clicked(move |_| {
                        let c = ctrl.clone();
                        let p = target_path.clone();
                        glib::MainContext::default().spawn_local(async move {
                            let _ = c.hydrate_file(&p).await;
                        });
                    });
                    row_box.append(&hydrate_btn);
                } else {
                    let dehydrate_btn = Button::with_label("Dehydrate ☁");
                    let ctrl = controller.clone();
                    let target_path = item.path.clone();
                    dehydrate_btn.connect_clicked(move |_| {
                        let c = ctrl.clone();
                        let p = target_path.clone();
                        glib::MainContext::default().spawn_local(async move {
                            let _ = c.dehydrate_file(&p).await;
                        });
                    });
                    row_box.append(&dehydrate_btn);
                }
            }

            row.set_child(Some(&row_box));
            list_box.append(&row);
        }
    }
}

/// Fallback runner for headless or non-GUI execution environments.
pub fn run_headless(controller: DesktopUiController) -> Result<(), UiError> {
    tracing::info!("Running Nextcloud Desktop in headless mode");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| UiError::Initialization(err.to_string()))?;

    rt.block_on(async {
        tracing::info!("Headless desktop controller initialized");
        let path = controller.current_path().await;
        tracing::info!(current_path = %path, "Virtual mount ready");
    });

    Ok(())
}
