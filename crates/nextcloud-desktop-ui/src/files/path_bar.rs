//! Interactive Nautilus-style breadcrumb path bar rendered inside the Cloud Files page toolbar.

use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};
use libadwaita::prelude::*;
use std::rc::Rc;

use crate::model::FileViewItem;
use crate::widget::{FileExplorerBackend, ViewComponent};

/// Clickable breadcrumb path bar displaying the active virtual directory hierarchy.
#[derive(Clone)]
pub struct PathBar {
    container: GtkBox,
}

impl PathBar {
    /// Creates an empty interactive path bar.
    pub fn new() -> Self {
        let container = GtkBox::new(Orientation::Horizontal, 0);
        container.add_css_class("linked");
        container.set_valign(Align::Center);
        container.set_halign(Align::Start);
        Self { container }
    }

    /// Rebuilds the clickable breadcrumb buttons for `current_path`.
    pub fn update<B: FileExplorerBackend>(
        &self,
        current_path: &str,
        backend: &B,
        on_navigate: Rc<dyn Fn()>,
    ) {
        while let Some(child) = self.container.first_child() {
            self.container.remove(&child);
        }

        let segments = FileViewItem::breadcrumb_segments(current_path);
        let last_idx = segments.len().saturating_sub(1);

        for (idx, seg) in segments.into_iter().enumerate() {
            let btn = Button::new();
            let is_current = idx == last_idx;

            if seg.is_root {
                let inner = GtkBox::new(Orientation::Horizontal, 6);
                inner.set_can_target(false);
                let icon = Image::from_icon_name("folder-remote-symbolic");
                icon.set_can_target(false);
                let label = Label::new(Some(&seg.name));
                label.set_can_target(false);
                if is_current {
                    label.add_css_class("heading");
                }
                inner.append(&icon);
                inner.append(&label);
                btn.set_child(Some(&inner));
            } else {
                let label = Label::new(Some(&seg.name));
                label.set_can_target(false);
                if is_current {
                    label.add_css_class("heading");
                }
                btn.set_child(Some(&label));
            }

            let target_path = if seg.is_root {
                "/".to_string()
            } else {
                seg.full_path
            };
            let backend_clone = backend.clone();
            let nav_cb = on_navigate.clone();
            btn.connect_clicked(move |_| {
                let b = backend_clone.clone();
                let cb = nav_cb.clone();
                let target = target_path.clone();
                glib::MainContext::default().spawn_local(async move {
                    b.set_current_path(&target).await;
                    cb();
                });
            });

            self.container.append(&btn);
        }
    }
}

impl Default for PathBar {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewComponent for PathBar {
    type RootWidget = GtkBox;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.container
    }
}
