//! GTK 4 and Libadwaita desktop user interface implementation.

#[cfg(feature = "gtk")]
use gtk4::{
    Box as GtkBox, Button, ListBox, Orientation, Paned, PolicyType, ScrolledWindow, SelectionMode,
    Stack, StackTransitionType,
};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use tokio::sync::mpsc;

use crate::controller::DesktopUiController;
use crate::error::UiError;
#[cfg(feature = "gtk")]
use crate::nautilus_view::{build_nautilus_browser, refresh_nautilus_view};
#[cfg(feature = "gtk")]
use crate::tray::{TrayAction, spawn_app_bar_tray};
#[cfg(feature = "gtk")]
use crate::views::{
    build_settings_page, build_sidebar_row, build_sync_folders_page, build_transfers_page,
};

/// Launches the Nextcloud Desktop GTK 4 / Libadwaita application.
#[cfg(feature = "gtk")]
pub fn launch_gtk_application(
    controller: DesktopUiController,
    start_minimized: bool,
) -> Result<(), UiError> {
    let app = adw::Application::builder()
        .application_id("com.nextcloud.desktop.rs")
        .build();

    let hold_guard = std::rc::Rc::new(std::cell::RefCell::new(None));
    let main_window: std::rc::Rc<std::cell::RefCell<Option<adw::ApplicationWindow>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));

    let hold_for_activate = hold_guard.clone();
    let win_for_activate = main_window.clone();

    app.connect_activate(move |app| {
        if hold_for_activate.borrow().is_none() {
            // Keep the GApplication alive when all windows are hidden to the background tray
            *hold_for_activate.borrow_mut() = Some(app.hold());
            let win = build_ui(app, controller.clone(), start_minimized);
            *win_for_activate.borrow_mut() = Some(win);
        } else if let Some(ref win) = *win_for_activate.borrow() {
            win.set_visible(true);
            win.present();
        }
    });

    let empty_args: Vec<String> = Vec::new();
    app.run_with_args(&empty_args);
    Ok(())
}

#[cfg(feature = "gtk")]
fn build_ui(
    app: &adw::Application,
    controller: DesktopUiController,
    start_minimized: bool,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Nextcloud")
        .default_width(1020)
        .default_height(680)
        .width_request(680)
        .height_request(440)
        .build();

    // App bar tray communication channel
    let (tx_tray, mut rx_tray) = mpsc::unbounded_channel::<TrayAction>();
    spawn_app_bar_tray(tx_tray);

    let nautilus = build_nautilus_browser(&controller);

    // Header bar following Nautilus / GNOME Files layout
    let header_bar = adw::HeaderBar::new();

    // Left: Linked Back / Forward buttons + Refresh & Sync
    let nav_box = GtkBox::new(Orientation::Horizontal, 0);
    nav_box.add_css_class("linked");
    nav_box.append(&nautilus.back_btn);
    nav_box.append(&nautilus.forward_btn);
    header_bar.pack_start(&nav_box);

    // Center: Stack switching between Nautilus Breadcrumb Path Bar (on Cloud Files) and WindowTitle (on other pages)
    let title_stack = Stack::new();
    let window_title = adw::WindowTitle::new("Nextcloud", "Sync Folders");
    title_stack.add_named(&nautilus.breadcrumb_box, Some("breadcrumbs"));
    title_stack.add_named(&window_title, Some("title"));
    title_stack.set_visible_child_name("breadcrumbs");
    header_bar.set_title_widget(Some(&title_stack));

    // Right: View Mode Switcher (Grid/List), New Folder, Refresh, Sync
    let refresh_btn = Button::from_icon_name("view-refresh-symbolic");
    refresh_btn.set_tooltip_text(Some("Refresh Virtual Drive"));

    let sync_btn = Button::from_icon_name("emblem-synchronizing-symbolic");
    sync_btn.set_tooltip_text(Some("Sync All Folders"));

    let view_mode_box = GtkBox::new(Orientation::Horizontal, 0);
    view_mode_box.add_css_class("linked");
    view_mode_box.append(&nautilus.grid_toggle);
    view_mode_box.append(&nautilus.list_toggle);

    header_bar.pack_end(&sync_btn);
    header_bar.pack_end(&refresh_btn);
    header_bar.pack_end(&nautilus.new_folder_btn);
    header_bar.pack_end(&view_mode_box);

    // Intercept window close (X button) to keep running in background / app bar
    window.connect_close_request(move |win| {
        tracing::info!("Minimizing Nextcloud Desktop to background app bar");
        win.set_visible(false);
        glib::Propagation::Stop
    });

    // Main split layout: Navigation Sidebar + Content Stack
    let paned = Paned::new(Orientation::Horizontal);
    paned.set_position(210);
    paned.set_shrink_start_child(false);
    paned.set_shrink_end_child(false);
    paned.set_resize_start_child(false);
    paned.set_vexpand(true);

    let sidebar_list = ListBox::new();
    sidebar_list.set_selection_mode(SelectionMode::Single);
    sidebar_list.add_css_class("navigation-sidebar");

    let cloud_row = build_sidebar_row("folder-remote-symbolic", "Cloud Files");
    let sync_row = build_sidebar_row("emblem-synchronizing-symbolic", "Sync Folders");
    let transfer_row = build_sidebar_row("network-transmit-receive-symbolic", "Activity");
    let settings_row = build_sidebar_row("emblem-system-symbolic", "Settings");

    sidebar_list.append(&cloud_row);
    sidebar_list.append(&sync_row);
    sidebar_list.append(&transfer_row);
    sidebar_list.append(&settings_row);
    sidebar_list.select_row(Some(&cloud_row));

    let sidebar_scroll = ScrolledWindow::new();
    sidebar_scroll.set_hscrollbar_policy(PolicyType::Never);
    sidebar_scroll.set_width_request(200);
    sidebar_scroll.set_vexpand(true);
    sidebar_scroll.set_child(Some(&sidebar_list));
    paned.set_start_child(Some(&sidebar_scroll));

    // Content Stack
    let content_stack = Stack::new();
    content_stack.set_transition_type(StackTransitionType::Crossfade);
    content_stack.set_hexpand(true);
    content_stack.set_vexpand(true);

    let sync_page = build_sync_folders_page(&controller);
    let transfers_page = build_transfers_page();
    let settings_page = build_settings_page(&controller);

    content_stack.add_named(&nautilus.toast_overlay, Some("cloud_files"));
    content_stack.add_named(&sync_page, Some("sync_folders"));
    content_stack.add_named(&transfers_page, Some("transfers"));
    content_stack.add_named(&settings_page, Some("settings"));

    paned.set_end_child(Some(&content_stack));

    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.append(&header_bar);
    root_box.append(&paned);
    window.set_content(Some(&root_box));

    // Connect sidebar selection to stack pages and toggle header bar controls
    let stack_clone = content_stack.clone();
    let title_stack_clone = title_stack.clone();
    let title_clone = window_title.clone();
    let nav_box_clone = nav_box.clone();
    let view_mode_clone = view_mode_box.clone();
    let new_folder_clone = nautilus.new_folder_btn.clone();
    sidebar_list.connect_row_selected(move |_, row| {
        if let Some(r) = row {
            let is_files = r.index() == 0;
            nav_box_clone.set_visible(is_files);
            view_mode_clone.set_visible(is_files);
            new_folder_clone.set_visible(is_files);

            match r.index() {
                0 => {
                    stack_clone.set_visible_child_name("cloud_files");
                    title_stack_clone.set_visible_child_name("breadcrumbs");
                }
                1 => {
                    stack_clone.set_visible_child_name("sync_folders");
                    title_clone.set_subtitle("Sync Folders");
                    title_stack_clone.set_visible_child_name("title");
                }
                2 => {
                    stack_clone.set_visible_child_name("transfers");
                    title_clone.set_subtitle("Activity");
                    title_stack_clone.set_visible_child_name("title");
                }
                3 => {
                    stack_clone.set_visible_child_name("settings");
                    title_clone.set_subtitle("Settings");
                    title_stack_clone.set_visible_child_name("title");
                }
                _ => {}
            }
        }
    });

    // Header bar refresh action
    let ctrl_for_refresh = controller.clone();
    let nav_refresh = nautilus.clone();
    refresh_btn.connect_clicked(move |_| {
        let ctrl = ctrl_for_refresh.clone();
        let nav = nav_refresh.clone();
        glib::MainContext::default().spawn_local(async move {
            let _ = ctrl.refresh_virtual_tree().await;
            refresh_nautilus_view(&nav, &ctrl).await;
        });
    });

    // Header bar sync action
    let ctrl_for_sync = controller.clone();
    sync_btn.connect_clicked(move |_| {
        let ctrl = ctrl_for_sync.clone();
        glib::MainContext::default().spawn_local(async move {
            let _ = ctrl.trigger_sync().await;
        });
    });

    // Background tray action dispatcher loop
    let win_for_tray = window.clone();
    let app_for_tray = app.clone();
    let ctrl_for_tray = controller.clone();
    let nav_tray = nautilus.clone();

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
                    let _ = ctrl_for_tray.trigger_sync().await;
                }
                TrayAction::RefreshVfs => {
                    let _ = ctrl_for_tray.refresh_virtual_tree().await;
                    refresh_nautilus_view(&nav_tray, &ctrl_for_tray).await;
                }
                TrayAction::Quit => {
                    app_for_tray.quit();
                }
            }
        }
    });

    // Initial render
    let ctrl_init = controller;
    let nav_init = nautilus;
    glib::MainContext::default().spawn_local(async move {
        refresh_nautilus_view(&nav_init, &ctrl_init).await;
    });

    if !start_minimized {
        window.present();
    }

    window
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
