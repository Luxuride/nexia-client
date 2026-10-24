//! In-page navigation and action toolbar for the Cloud Files view.

#[cfg(feature = "gtk")]
use gtk4::{Align, Box as GtkBox, Button, Orientation, ToggleButton};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use std::rc::Rc;

#[cfg(feature = "gtk")]
use crate::files::new_folder::NewFolderPopover;
#[cfg(feature = "gtk")]
use crate::files::path_bar::PathBar;
#[cfg(feature = "gtk")]
use crate::model::FileViewMode;
#[cfg(feature = "gtk")]
use crate::widget::{FileExplorerBackend, ViewComponent};

/// Top toolbar panel embedded directly inside the Cloud Files page view.
///
/// Houses Back/Forward navigation buttons, the interactive [`PathBar`],
/// the [`NewFolderPopover`] button, and the Grid/List view mode toggle.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct PathToolbar {
    container: GtkBox,
    back_btn: Button,
    forward_btn: Button,
    path_bar: PathBar,
}

#[cfg(feature = "gtk")]
impl PathToolbar {
    /// Creates a new in-page toolbar wired to `backend` and `on_change`.
    pub fn new<B: FileExplorerBackend>(
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_change: Rc<dyn Fn()>,
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

        // Right-side actions: New Folder + Grid/List Switcher
        let new_folder = NewFolderPopover::new(backend, toast_overlay, on_change.clone());

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
        container.append(new_folder.root_widget());
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
        }
    }

    /// Synchronizes the Back/Forward button sensitivity and breadcrumb buttons with `backend`.
    pub async fn sync_state<B: FileExplorerBackend>(&self, backend: &B, on_navigate: Rc<dyn Fn()>) {
        let cur_path = backend.current_path().await;
        self.back_btn.set_sensitive(backend.can_go_back().await);
        self.forward_btn
            .set_sensitive(backend.can_go_forward().await);
        self.path_bar.update(&cur_path, backend, on_navigate);
    }
}

#[cfg(feature = "gtk")]
impl ViewComponent for PathToolbar {
    type RootWidget = GtkBox;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.container
    }
}
