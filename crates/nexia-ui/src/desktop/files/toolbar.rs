//! In-page navigation and action toolbar for the Cloud Files view.

use gtk4::{Align, Box as GtkBox, Button, Orientation, SearchEntry, ToggleButton};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::files::new_folder::NewFolderPopover;
use crate::files::path_bar::PathBar;
use crate::files::sort_popover::SortPopover;
use crate::model::{FileViewMode, ZoomLevel};
use crate::widget::{FileExplorerBackend, ViewComponent};

/// Top toolbar panel embedded directly inside the Cloud Files page view.
///
/// Houses Back/Forward navigation buttons, the interactive [`PathBar`],
/// search entry and toggle, zoom level adjustments, [`NewFolderPopover`],
/// and the Grid/List view mode toggle.
#[derive(Clone)]
pub struct PathToolbar {
    container: GtkBox,
    back_btn: Button,
    forward_btn: Button,
    path_bar: PathBar,
    search_entry: SearchEntry,
    search_toggle: ToggleButton,
    zoom_out_btn: Button,
    zoom_state_btn: Button,
    zoom_in_btn: Button,
    grid_toggle: ToggleButton,
    sort_popover: SortPopover,
}

impl PathToolbar {
    /// Creates a new in-page toolbar wired to `backend`, `on_change`, and `on_search`.
    pub fn new<B: FileExplorerBackend>(
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_change: Rc<dyn Fn()>,
        on_search: Rc<dyn Fn(Option<String>)>,
    ) -> Self {
        let container = GtkBox::new(Orientation::Horizontal, 10);
        container.set_margin_top(10);
        container.set_margin_bottom(10);
        container.set_margin_start(16);
        container.set_margin_end(16);
        container.set_valign(Align::Center);

        // Linked Back / Forward buttons
        let nav_box = GtkBox::new(Orientation::Horizontal, 0);
        nav_box.add_css_class("linked");

        let back_btn = Button::from_icon_name("go-previous-symbolic");
        back_btn.set_tooltip_text(Some("Back"));
        back_btn.set_sensitive(false);

        let forward_btn = Button::from_icon_name("go-next-symbolic");
        forward_btn.set_tooltip_text(Some("Forward"));
        forward_btn.set_sensitive(false);

        nav_box.append(&back_btn);
        nav_box.append(&forward_btn);

        // Interactive breadcrumb path bar (expands in the middle of the page toolbar)
        let path_bar = PathBar::new();
        path_bar.root_widget().set_hexpand(true);

        // Search entry replacing breadcrumbs when search is toggled
        let search_entry = SearchEntry::new();
        search_entry.set_placeholder_text(Some("Search files and folders…"));
        search_entry.set_hexpand(true);
        search_entry.set_visible(false);

        // Search toggle button
        let search_toggle = ToggleButton::new();
        search_toggle.set_icon_name("edit-find-symbolic");
        search_toggle.set_tooltip_text(Some("Search files (Ctrl+F)"));

        // Zoom controls
        let zoom_box = GtkBox::new(Orientation::Horizontal, 0);
        zoom_box.add_css_class("linked");

        let zoom_out_btn = Button::from_icon_name("zoom-out-symbolic");
        zoom_out_btn.set_tooltip_text(Some("Zoom Out (Ctrl+-)"));

        let zoom_state_btn = Button::with_label("100%");
        zoom_state_btn.add_css_class("flat");
        zoom_state_btn.set_tooltip_text(Some("Current zoom level (click to reset to 100%)"));

        let zoom_in_btn = Button::from_icon_name("zoom-in-symbolic");
        zoom_in_btn.set_tooltip_text(Some("Zoom In (Ctrl++)"));

        zoom_box.append(&zoom_out_btn);
        zoom_box.append(&zoom_state_btn);
        zoom_box.append(&zoom_in_btn);

        // Right-side actions: New Folder + Sort + Grid/List Switcher
        let new_folder = NewFolderPopover::new(backend, toast_overlay, on_change.clone());
        let sort_popover = SortPopover::new(backend, on_change.clone());

        let view_mode_box = GtkBox::new(Orientation::Horizontal, 0);
        view_mode_box.add_css_class("linked");

        let grid_toggle = ToggleButton::new();
        grid_toggle.set_icon_name("view-grid-symbolic");
        grid_toggle.set_tooltip_text(Some("Grid View"));
        grid_toggle.set_active(true);

        let list_toggle = ToggleButton::new();
        list_toggle.set_icon_name("view-list-symbolic");
        list_toggle.set_tooltip_text(Some("List View"));
        list_toggle.set_group(Some(&grid_toggle));

        view_mode_box.append(&grid_toggle);
        view_mode_box.append(&list_toggle);

        container.append(&nav_box);
        container.append(path_bar.root_widget());
        container.append(&search_entry);
        container.append(new_folder.root_widget());
        container.append(&search_toggle);
        container.append(sort_popover.widget());
        container.append(&zoom_box);
        container.append(&view_mode_box);

        // Wire Back button
        {
            let b = backend.clone();
            let cb = on_change.clone();
            back_btn.connect_clicked(move |_| {
                let b = b.clone();
                let cb = cb.clone();
                glib::MainContext::default().spawn_local(async move {
                    if b.go_back().await {
                        cb();
                    }
                });
            });
        }

        // Wire Forward button
        {
            let b = backend.clone();
            let cb = on_change.clone();
            forward_btn.connect_clicked(move |_| {
                let b = b.clone();
                let cb = cb.clone();
                glib::MainContext::default().spawn_local(async move {
                    if b.go_forward().await {
                        cb();
                    }
                });
            });
        }

        // Wire Search Toggle
        {
            let path_bar_widget = path_bar.root_widget().clone();
            let entry = search_entry.clone();
            let search_cb = on_search.clone();
            search_toggle.connect_toggled(move |btn| {
                if btn.is_active() {
                    path_bar_widget.set_visible(false);
                    entry.set_visible(true);
                    entry.grab_focus();
                } else {
                    entry.set_text("");
                    entry.set_visible(false);
                    path_bar_widget.set_visible(true);
                    search_cb(None);
                }
            });
        }

        // Wire Search Entry with debouncing and Enter key immediate trigger
        {
            let search_cb = on_search;
            let pending_timeout: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

            let pending_timeout_changed = pending_timeout.clone();
            let search_cb_changed = search_cb.clone();
            search_entry.connect_search_changed(move |entry| {
                if let Some(source) = pending_timeout_changed.borrow_mut().take() {
                    source.remove();
                }

                let text = entry.text().trim().to_string();
                if text.is_empty() {
                    // Empty search does not search anything; restore normal directory view
                    search_cb_changed(None);
                    return;
                }

                let cb = search_cb_changed.clone();
                let pending = pending_timeout_changed.clone();
                let query = text;
                let source = glib::timeout_add_local_once(
                    std::time::Duration::from_millis(300),
                    move || {
                        *pending.borrow_mut() = None;
                        cb(Some(query));
                    },
                );
                *pending_timeout_changed.borrow_mut() = Some(source);
            });

            let pending_timeout_activate = pending_timeout;
            let search_cb_activate = search_cb;
            search_entry.connect_activate(move |entry| {
                if let Some(source) = pending_timeout_activate.borrow_mut().take() {
                    source.remove();
                }

                let text = entry.text().trim().to_string();
                if text.is_empty() {
                    // Empty file search should not search anything!
                    return;
                }

                search_cb_activate(Some(text));
            });
        }

        {
            let toggle = search_toggle.clone();
            search_entry.connect_stop_search(move |_| {
                toggle.set_active(false);
            });
        }

        // Wire Zoom Out button
        {
            let b = backend.clone();
            let cb = on_change.clone();
            zoom_out_btn.connect_clicked(move |_| {
                let b = b.clone();
                let cb = cb.clone();
                glib::MainContext::default().spawn_local(async move {
                    let cur = b.zoom_level().await;
                    b.set_zoom_level(cur.zoom_out()).await;
                    cb();
                });
            });
        }

        // Wire Zoom Reset button (middle state)
        {
            let b = backend.clone();
            let cb = on_change.clone();
            zoom_state_btn.connect_clicked(move |_| {
                let b = b.clone();
                let cb = cb.clone();
                glib::MainContext::default().spawn_local(async move {
                    b.set_zoom_level(ZoomLevel::Medium).await;
                    cb();
                });
            });
        }

        // Wire Zoom In button
        {
            let b = backend.clone();
            let cb = on_change.clone();
            zoom_in_btn.connect_clicked(move |_| {
                let b = b.clone();
                let cb = cb.clone();
                glib::MainContext::default().spawn_local(async move {
                    let cur = b.zoom_level().await;
                    b.set_zoom_level(cur.zoom_in()).await;
                    cb();
                });
            });
        }

        // Wire Grid/List toggle
        {
            let b = backend.clone();
            let cb = on_change;
            grid_toggle.connect_toggled(move |btn| {
                let mode = if btn.is_active() {
                    FileViewMode::Grid
                } else {
                    FileViewMode::List
                };
                let b = b.clone();
                let cb = cb.clone();
                glib::MainContext::default().spawn_local(async move {
                    b.set_view_mode(mode).await;
                    cb();
                });
            });
        }

        Self {
            container,
            back_btn,
            forward_btn,
            path_bar,
            search_entry,
            search_toggle,
            zoom_out_btn,
            zoom_state_btn,
            zoom_in_btn,
            grid_toggle,
            sort_popover,
        }
    }

    /// Synchronizes the Back/Forward button sensitivity, breadcrumbs, and zoom buttons.
    pub async fn sync_state<B: FileExplorerBackend>(&self, backend: &B, on_navigate: Rc<dyn Fn()>) {
        let cur_path = backend.current_path().await;
        self.back_btn.set_sensitive(backend.can_go_back().await);
        self.forward_btn
            .set_sensitive(backend.can_go_forward().await);
        self.path_bar.update(&cur_path, backend, on_navigate);

        let zoom = backend.zoom_level().await;
        self.zoom_state_btn.set_label(&zoom.display_label());
        self.zoom_out_btn.set_sensitive(zoom > ZoomLevel::Small);
        self.zoom_in_btn.set_sensitive(zoom < ZoomLevel::ExtraLarge);

        let is_grid = backend.view_mode().await == FileViewMode::Grid;
        if self.grid_toggle.is_active() != is_grid {
            self.grid_toggle.set_active(is_grid);
        }

        self.sort_popover.sync_state(backend);
    }

    /// Access the embedded search entry.
    pub fn search_entry(&self) -> &SearchEntry {
        &self.search_entry
    }

    /// Access the search toggle button.
    pub fn search_toggle(&self) -> &ToggleButton {
        &self.search_toggle
    }
}

impl ViewComponent for PathToolbar {
    type RootWidget = GtkBox;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.container
    }
}
