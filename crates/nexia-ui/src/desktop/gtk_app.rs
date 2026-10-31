//! GTK 4 and Libadwaita desktop user interface application runner and window builder.

use gtk4::{Box as GtkBox, Orientation, Stack, StackTransitionType};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use tokio::sync::mpsc;

use crate::controller::DesktopUiController;
use crate::error::UiError;
use crate::files::CloudFilesPage;
use crate::header::MainHeaderBar;
use crate::pages::{ActivityPage, NavigationSidebar, SettingsPage, SyncFoldersPage};
use crate::tray::{TrayAction, spawn_app_bar_tray};
use crate::widget::ViewComponent;

use std::sync::Once;

static SUPPRESS_GTK_LOGS_ONCE: Once = Once::new();

/// Suppresses noisy GTK and GLib logging to stderr.
pub fn suppress_gtk_logs() {
    SUPPRESS_GTK_LOGS_ONCE.call_once(|| {
        glib::log_set_writer_func(|level, fields| {
            if matches!(
                level,
                glib::LogLevel::Error | glib::LogLevel::Critical | glib::LogLevel::Warning
            ) {
                glib::log_writer_default(level, fields)
            } else {
                glib::LogWriterOutput::Handled
            }
        });
    });
}

/// Application ID for desktop integration and single-instance coordination.
pub const APPLICATION_ID: &str = "eu.luxuride.nexia";

/// Launches the Nexia Client GTK 4 / Libadwaita application.
pub fn launch_gtk_application(
    controller: DesktopUiController,
    start_minimized: bool,
) -> Result<(), UiError> {
    // Suppress noisy GTK/GLib stderr session bus and debug logs
    suppress_gtk_logs();

    let app = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build();

    app.connect_startup(move |_| {
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
            @define-color accent_color #4338ca;
            @define-color accent_bg_color #4338ca;

            .navigation-sidebar-fixed {
                min-width: 260px !important;
                max-width: 260px !important;
                width: 260px !important;
            }

            .nexia-server-banner {
                background: linear-gradient(135deg, #14b8a6 0%, #4338ca 100%);
                color: #ffffff;
                padding: 12px 14px;
                border-radius: 10px;
                margin: 10px 8px 6px 8px;
                box-shadow: 0 2px 6px rgba(0, 0, 0, 0.12);
            }
            .nexia-brand-icon {
                background-color: rgba(255, 255, 255, 0.22);
                border-radius: 8px;
                padding: 6px;
                min-width: 44px;
                min-height: 44px;
                color: #ffffff;
            }
            .nexia-server-title {
                font-weight: 700;
                font-size: 16px;
                color: #ffffff;
            }
            .nexia-server-slogan {
                font-size: 11px;
                color: rgba(255, 255, 255, 0.85);
            }
            .nexia-status-badge {
                font-size: 11px;
                color: rgba(255, 255, 255, 0.90);
                font-weight: 500;
            }
            .status-dot-connected {
                background-color: #46ba61;
                border-radius: 50%;
                min-width: 8px;
                min-height: 8px;
            }
            .status-dot-syncing {
                background-color: #3584e4;
                border-radius: 50%;
                min-width: 8px;
                min-height: 8px;
            }
            .status-dot-offline {
                background-color: #e01b24;
                border-radius: 50%;
                min-width: 8px;
                min-height: 8px;
            }

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
            .sync-emblem-badge {
                background-color: alpha(@card_bg_color, 0.92);
                border-radius: 9999px;
                padding: 2px;
                box-shadow: 0 1px 3px rgba(0, 0, 0, 0.35);
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
        tracing::info!("Application activate event received");
        if hold_for_activate.borrow().is_none() {
            // Keep the GApplication alive when the window is hidden to the background tray
            *hold_for_activate.borrow_mut() = Some(app.hold());
            tracing::info!("Building UI...");
            let win = build_ui(app, controller.clone(), start_minimized);
            *win_for_activate.borrow_mut() = Some(win);
            tracing::info!("UI built successfully");
        } else if let Some(ref win) = *win_for_activate.borrow() {
            tracing::info!("Presenting existing main window");
            win.set_visible(true);
            win.present();
        }
    });

    let empty_args: Vec<String> = Vec::new();
    app.run_with_args(&empty_args);
    Ok(())
}

type RefreshCallback = Rc<dyn Fn()>;
type RefreshCell = Rc<RefCell<Option<RefreshCallback>>>;

fn build_ui(
    app: &adw::Application,
    controller: DesktopUiController,
    start_minimized: bool,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Nexia Client")
        .default_width(1020)
        .default_height(680)
        .width_request(680)
        .height_request(440)
        .build();

    // App bar tray communication channel
    let (tx_tray, mut rx_tray) = mpsc::unbounded_channel::<TrayAction>();
    let tray_availability = spawn_app_bar_tray(tx_tray);

    let content_stack = Stack::new();
    content_stack.set_transition_type(StackTransitionType::Crossfade);
    content_stack.set_hexpand(true);
    content_stack.set_vexpand(true);

    // Refresh trigger callback for CloudFilesPage
    let files_page_cell: Rc<RefCell<Option<CloudFilesPage<DesktopUiController>>>> =
        Rc::new(RefCell::new(None));

    let refresh_cell: RefreshCell = Rc::new(RefCell::new(None));
    let refresh_for_loop = refresh_cell.clone();
    let files_for_cb = files_page_cell.clone();

    let refresh_cb: RefreshCallback = Rc::new(move || {
        if let Some(ref page) = *files_for_cb.borrow() {
            let p = page.clone();
            let cb_trigger = refresh_for_loop
                .borrow()
                .clone()
                .unwrap_or_else(|| Rc::new(|| {}));
            glib::MainContext::default().spawn_local(async move {
                p.refresh(cb_trigger).await;
            });
        }
    });
    *refresh_cell.borrow_mut() = Some(refresh_cb.clone());

    // Top HeaderBar whose buttons/layout never change across pages
    let toast_overlay = adw::ToastOverlay::new();
    let header_bar = MainHeaderBar::new(&controller, &toast_overlay, refresh_cb.clone());

    // Pages
    let files_page = CloudFilesPage::new(&controller, refresh_cb.clone());
    *files_page_cell.borrow_mut() = Some(files_page.clone());

    let sync_page = SyncFoldersPage::new(&controller, &toast_overlay);
    let activity_page = ActivityPage::new(&controller);
    let settings_page = SettingsPage::new(&controller, &toast_overlay);

    content_stack.add_named(files_page.root_widget(), Some("cloud_files"));
    content_stack.add_named(sync_page.root_widget(), Some("sync_folders"));
    content_stack.add_named(activity_page.root_widget(), Some("transfers"));
    content_stack.add_named(settings_page.root_widget(), Some("settings"));

    // Main layout with fixed sidebar width (no dragging or enlarging)
    let main_layout = GtkBox::new(Orientation::Horizontal, 0);
    main_layout.set_vexpand(true);
    main_layout.set_hexpand(true);

    let stack_for_nav = content_stack.clone();
    let header_for_nav = header_bar.clone();
    let sidebar = NavigationSidebar::new(
        &controller,
        Rc::new(move |page| {
            stack_for_nav.set_visible_child_name(page.stack_name());
            header_for_nav.set_page_subtitle(page.title());
        }),
    );
    sidebar.root_widget().set_width_request(260);
    sidebar.root_widget().set_size_request(260, -1);
    sidebar.root_widget().set_hexpand(false);
    sidebar.root_widget().set_halign(gtk4::Align::Start);
    sidebar
        .root_widget()
        .add_css_class("navigation-sidebar-fixed");

    let sidebar_sep = gtk4::Separator::new(Orientation::Vertical);

    content_stack.set_hexpand(true);
    content_stack.set_vexpand(true);

    main_layout.append(sidebar.root_widget());
    main_layout.append(&sidebar_sep);
    main_layout.append(&content_stack);

    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.set_vexpand(true);
    root_box.set_hexpand(true);
    root_box.append(header_bar.root_widget());
    root_box.append(&main_layout);
    toast_overlay.set_child(Some(&root_box));
    window.set_content(Some(&toast_overlay));

    // Support hardware mouse forward (Button 9) and back (Button 8) buttons
    let mouse_nav = gtk4::GestureClick::new();
    mouse_nav.set_button(0);
    mouse_nav.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let ctrl_nav = controller.clone();
    let refresh_nav = refresh_cb.clone();
    mouse_nav.connect_pressed(move |gesture, n_press, _, _| {
        if n_press != 1 {
            return;
        }
        let btn = gesture.current_button();
        if btn == 8 {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            let c = ctrl_nav.clone();
            let r = refresh_nav.clone();
            glib::MainContext::default().spawn_local(async move {
                if c.go_back().await {
                    r();
                }
            });
        } else if btn == 9 {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            let c = ctrl_nav.clone();
            let r = refresh_nav.clone();
            glib::MainContext::default().spawn_local(async move {
                if c.go_forward().await {
                    r();
                }
            });
        }
    });
    window.add_controller(mouse_nav);

    // Intercept window close (X button) to keep running in background app bar
    let availability_for_close = tray_availability.clone();
    window.connect_close_request(move |win| {
        if !availability_for_close.is_registered() {
            tracing::warn!(
                "No app bar indicator is registered; closing the window quits instead of hiding"
            );
            return glib::Propagation::Proceed;
        }
        tracing::info!("Minimizing Nexia Client to the background app bar");
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
                    tracing::info!("App bar requested the main window");
                    win_for_tray.set_visible(true);
                    win_for_tray.present();
                }
                TrayAction::HideWindow => {
                    tracing::info!("App bar requested the window be hidden");
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
                    tracing::info!("Quitting Nexia Client from the app bar tray");
                    ctrl_for_tray.unmount_fuse_vfs().await;
                    app_for_tray.quit();
                    std::process::exit(0);
                }
            }
        }
    });

    // Reactive event subscriber loop for VfsTreeUpdated and sync cycle events with 200ms debounce
    let mut event_rx = controller.subscribe_events();
    let debounce_timer: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    let refresh_for_events = {
        let refresh = refresh_cb.clone();
        let timer = debounce_timer.clone();
        move || {
            if let Some(source) = timer.take() {
                source.remove();
            }
            let refresh = refresh.clone();
            let timer_inner = timer.clone();
            let source =
                glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                    timer_inner.set(None);
                    refresh();
                });
            timer.set(Some(source));
        }
    };
    let direct_refresh = refresh_cb.clone();
    let ctrl_for_events = controller.clone();
    glib::MainContext::default().spawn_local(async move {
        while let Some(event) = nexia_core::events::next_event(&mut event_rx).await {
            match event {
                nexia_core::events::DesktopEvent::NodeHydrationStarted { ref path } => {
                    ctrl_for_events.mark_hydration_started(path).await;
                    refresh_for_events();
                }
                nexia_core::events::DesktopEvent::NodeHydrationFailed { ref path, .. } => {
                    ctrl_for_events.mark_hydration_finished(path).await;
                    refresh_for_events();
                }
                nexia_core::events::DesktopEvent::NodeHydrated { ref path, .. } => {
                    ctrl_for_events.mark_hydration_finished(path).await;
                    refresh_for_events();
                }
                nexia_core::events::DesktopEvent::NodeDehydrated { ref path } => {
                    ctrl_for_events.mark_hydration_finished(path).await;
                    refresh_for_events();
                }
                nexia_core::events::DesktopEvent::AccountSwitched { .. }
                | nexia_core::events::DesktopEvent::AccountSignedOut { .. } => {
                    ctrl_for_events.reload_account_view_state().await;
                    direct_refresh();
                }
                nexia_core::events::DesktopEvent::VfsTreeUpdated { .. }
                | nexia_core::events::DesktopEvent::SyncCompleted { .. } => {
                    refresh_for_events();
                }
                _ => {}
            }
        }
    });

    // Initial render of Cloud Files view from local persistent tree cache (0 ms first paint)
    refresh_cb();

    if start_minimized {
        // The tray service settles on a runtime task, so without waiting the
        // window could hide before its indicator is known to exist.
        let window_for_settle = window.clone();
        let availability_for_settle = tray_availability.clone();
        glib::MainContext::default().spawn_local(async move {
            availability_for_settle.wait_until_settled().await;
            if availability_for_settle.is_registered() {
                tracing::info!("Starting in the background behind the app bar indicator");
            } else {
                tracing::warn!(
                    "Starting in the background without an app bar indicator; showing the window"
                );
                window_for_settle.present();
            }
        });
    } else {
        window.present();
    }

    // A hidden window with no indicator has no way back, so restore it whenever
    // an indicator that was already working goes away.
    let window_for_indicator = window.clone();
    let availability_for_indicator = tray_availability.clone();
    glib::MainContext::default().spawn_local(async move {
        let mut changes = availability_for_indicator.subscribe();
        let mut was_registered = *changes.borrow_and_update() == Some(true);
        while changes.changed().await.is_ok() {
            match *changes.borrow_and_update() {
                Some(true) => was_registered = true,
                Some(false) => {
                    if was_registered && !window_for_indicator.is_visible() {
                        tracing::warn!(
                            "App bar indicator disappeared; restoring the window so it stays reachable"
                        );
                        window_for_indicator.present();
                    }
                    was_registered = false;
                }
                None => {}
            }
        }
    });

    // Background remote tree refresh, automatic subfolder preloading, and active folder sync
    let ctrl_for_init = controller.clone();
    tokio::spawn(async move {
        let _ = ctrl_for_init.refresh_virtual_tree().await;
        let _ = ctrl_for_init.sync_all_mounts().await;
    });

    window
}
