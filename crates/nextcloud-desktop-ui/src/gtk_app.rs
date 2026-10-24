//! GTK 4 and Libadwaita desktop user interface application runner and window builder.

#[cfg(feature = "gtk")]
use gtk4::{Box as GtkBox, Orientation, Paned, Stack, StackTransitionType};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use std::cell::RefCell;
#[cfg(feature = "gtk")]
use std::rc::Rc;
#[cfg(feature = "gtk")]
use tokio::sync::mpsc;

use crate::controller::DesktopUiController;
use crate::error::UiError;
#[cfg(feature = "gtk")]
use crate::files::CloudFilesPage;
#[cfg(feature = "gtk")]
use crate::header::MainHeaderBar;
#[cfg(feature = "gtk")]
use crate::pages::{ActivityPage, NavigationSidebar, SettingsPage, SyncFoldersPage};
#[cfg(feature = "gtk")]
use crate::tray::{TrayAction, spawn_app_bar_tray};
#[cfg(feature = "gtk")]
use crate::widget::ViewComponent;

/// Launches the Nextcloud Desktop GTK 4 / Libadwaita application.
#[cfg(feature = "gtk")]
pub fn launch_gtk_application(
    controller: DesktopUiController,
    start_minimized: bool,
) -> Result<(), UiError> {
    let app = adw::Application::builder()
        .application_id("com.nextcloud.desktop.rs")
        .build();

    app.connect_startup(|_| {
        if let Some(settings) = gtk4::Settings::default() {
            let cursor_size = std::env::var("XCURSOR_SIZE")
                .ok()
                .and_then(|s| s.trim_start_matches('-').parse::<i32>().ok())
                .filter(|&sz| sz > 0)
                .unwrap_or(24);
            settings.set_gtk_cursor_theme_size(cursor_size);

            if let Ok(theme) = std::env::var("XCURSOR_THEME") {
                let clean_theme = theme.trim_start_matches('-');
                if !clean_theme.is_empty() {
                    settings.set_gtk_cursor_theme_name(Some(clean_theme));
                }
            }
        }

        let provider = gtk4::CssProvider::new();
        provider.load_from_data(
            "
            flowboxchild {
                border-radius: 8px;
                padding: 2px;
                margin: 0;
                transition: background-color 120ms ease-in-out;
            }
            flowboxchild:hover {
                background-color: alpha(currentColor, 0.07);
            }
            flowboxchild:selected {
                background-color: alpha(@accent_color, 0.18);
                color: currentColor;
            }
            flowboxchild:selected:hover {
                background-color: alpha(@accent_color, 0.24);
            }
            flowbox > flowboxchild:focus,
            flowbox > flowboxchild:focus-visible,
            flowboxchild:focus,
            flowboxchild:focus-visible {
                outline: none !important;
                box-shadow: none !important;
            }
            .file-tile {
                background: none;
                background-color: transparent;
                border: none;
            }
            .file-tile:hover {
                background: none;
                background-color: transparent;
            }
            .file-list-view list > row {
                border-radius: 6px;
                margin: 1px 8px;
                padding: 0;
                transition: background-color 120ms ease-in-out;
            }
            .file-list-view list > row:hover {
                background-color: alpha(currentColor, 0.06);
            }
            .file-list-view list > row:selected {
                background-color: alpha(@accent_color, 0.18);
                color: currentColor;
            }
            .file-list-view list > row:selected:hover {
                background-color: alpha(@accent_color, 0.24);
            }
            .file-list-view list > row:focus,
            .file-list-view list > row:focus-visible,
            list > row:focus,
            list > row:focus-visible {
                outline: none !important;
                box-shadow: none !important;
            }
            .drop-hover {
                outline: 2px solid @accent_color !important;
                background-color: alpha(@accent_color, 0.18) !important;
                border-radius: 8px;
            }
            ",
        );
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });

    let hold_guard = Rc::new(RefCell::new(None));
    let main_window: Rc<RefCell<Option<adw::ApplicationWindow>>> = Rc::new(RefCell::new(None));

    let hold_for_activate = hold_guard.clone();
    let win_for_activate = main_window.clone();

    app.connect_activate(move |app| {
        if hold_for_activate.borrow().is_none() {
            // Keep the GApplication alive when the window is hidden to the background tray
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

    let content_stack = Stack::new();
    content_stack.set_transition_type(StackTransitionType::Crossfade);
    content_stack.set_hexpand(true);
    content_stack.set_vexpand(true);

    // Refresh trigger callback for CloudFilesPage
    let files_page_cell: Rc<RefCell<Option<CloudFilesPage<DesktopUiController>>>> =
        Rc::new(RefCell::new(None));

    let files_for_cb = files_page_cell.clone();
    let refresh_cb: Rc<dyn Fn()> = Rc::new({
        let files_cell = files_for_cb.clone();
        move || {
            if let Some(ref page) = *files_cell.borrow() {
                let p = page.clone();
                let files_again = files_cell.clone();
                let inner_cb: Rc<dyn Fn()> = Rc::new(move || {
                    if let Some(ref p2) = *files_again.borrow() {
                        let p2_c = p2.clone();
                        glib::MainContext::default().spawn_local(async move {
                            let empty_cb: Rc<dyn Fn()> = Rc::new(|| {});
                            p2_c.refresh(empty_cb).await;
                        });
                    }
                });
                glib::MainContext::default().spawn_local(async move {
                    p.refresh(inner_cb).await;
                });
            }
        }
    });

    // Top HeaderBar whose buttons/layout never change across pages
    let header_bar = MainHeaderBar::new(&controller, refresh_cb.clone());

    // Pages
    let files_page = CloudFilesPage::new(&controller, refresh_cb.clone());
    *files_page_cell.borrow_mut() = Some(files_page.clone());

    let sync_page = SyncFoldersPage::new(&controller);
    let activity_page = ActivityPage::new();
    let settings_page = SettingsPage::new(&controller);

    content_stack.add_named(files_page.root_widget(), Some("cloud_files"));
    content_stack.add_named(sync_page.root_widget(), Some("sync_folders"));
    content_stack.add_named(activity_page.root_widget(), Some("transfers"));
    content_stack.add_named(settings_page.root_widget(), Some("settings"));

    // Main layout
    let paned = Paned::new(Orientation::Horizontal);
    paned.set_position(210);
    paned.set_shrink_start_child(false);
    paned.set_shrink_end_child(false);
    paned.set_resize_start_child(false);
    paned.set_vexpand(true);

    let stack_for_nav = content_stack.clone();
    let header_for_nav = header_bar.clone();
    let sidebar = NavigationSidebar::new(Rc::new(move |page| {
        stack_for_nav.set_visible_child_name(page.stack_name());
        header_for_nav.set_page_subtitle(page.title());
    }));

    paned.set_start_child(Some(sidebar.root_widget()));
    paned.set_end_child(Some(&content_stack));

    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.append(header_bar.root_widget());
    root_box.append(&paned);
    window.set_content(Some(&root_box));

    // Intercept window close (X button) to keep running in background app bar
    window.connect_close_request(move |win| {
        tracing::info!("Minimizing Nextcloud Desktop to background app bar");
        win.set_visible(false);
        glib::Propagation::Stop
    });

    // Background tray action dispatcher loop
    let win_for_tray = window.clone();
    let app_for_tray = app.clone();
    let ctrl_for_tray = controller.clone();
    let refresh_for_tray = refresh_cb.clone();

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
                    refresh_for_tray();
                }
                TrayAction::Quit => {
                    app_for_tray.quit();
                }
            }
        }
    });

    // Initial render of Cloud Files view
    refresh_cb();

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
