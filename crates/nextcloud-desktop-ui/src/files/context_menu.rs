//! Context menu popover providing Open, Rename, and Delete actions for file items.

#[cfg(feature = "gtk")]
use gtk4::{
    Align, Box as GtkBox, Button, Entry, GestureClick, Label, Orientation, Popover, Separator,
};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use std::rc::Rc;

#[cfg(feature = "gtk")]
use crate::model::FileViewItem;
#[cfg(feature = "gtk")]
use crate::widget::FileExplorerBackend;

/// Right-click context menu offering Open, Rename, and Delete actions.
#[cfg(feature = "gtk")]
pub struct ItemContextMenu;

#[cfg(feature = "gtk")]
impl ItemContextMenu {
    /// Attaches a right-click secondary click gesture and popup to `target`.
    pub fn attach<B: FileExplorerBackend>(
        target: &impl IsA<gtk4::Widget>,
        item: &FileViewItem,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) {
        let popover = Popover::new();
        popover.set_has_arrow(true);

        let menu_box = GtkBox::new(Orientation::Vertical, 6);
        menu_box.set_margin_top(8);
        menu_box.set_margin_bottom(8);
        menu_box.set_margin_start(8);
        menu_box.set_margin_end(8);

        // 1. Open action
        let open_label = if item.is_directory {
            "Open Folder"
        } else {
            "Open"
        };
        let open_btn = Button::with_label(open_label);
        open_btn.add_css_class("flat");

        {
            let pop = popover.clone();
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_refresh.clone();
            let item_clone = item.clone();
            open_btn.connect_clicked(move |_| {
                pop.popdown();
                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();
                let item = item_clone.clone();
                glib::MainContext::default().spawn_local(async move {
                    if item.is_directory {
                        b.navigate_into(&item.name).await;
                        cb();
                    } else {
                        match b.open_file(&item.path).await {
                            Ok(path) => {
                                toast.add_toast(adw::Toast::new(&format!(
                                    "Opened “{}” ({})",
                                    item.name,
                                    path.display()
                                )));
                            }
                            Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                        }
                    }
                });
            });
        }

        // 2. Rename action (inline rename input row)
        let rename_box = GtkBox::new(Orientation::Horizontal, 6);
        let rename_entry = Entry::new();
        rename_entry.set_text(&item.name);
        rename_entry.set_width_chars(14);

        let rename_btn = Button::from_icon_name("edit-rename-symbolic");
        rename_btn.add_css_class("flat");
        rename_btn.set_tooltip_text(Some("Confirm Rename"));

        rename_box.append(&rename_entry);
        rename_box.append(&rename_btn);

        {
            let pop = popover.clone();
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_refresh.clone();
            let path = item.path.clone();
            let entry = rename_entry;
            rename_btn.connect_clicked(move |_| {
                let new_name = entry.text().to_string();
                pop.popdown();
                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();
                let p = path.clone();
                glib::MainContext::default().spawn_local(async move {
                    match b.rename_item(&p, &new_name).await {
                        Ok(new_path) => {
                            toast.add_toast(adw::Toast::new(&format!("Renamed to “{new_name}”")));
                            cb();
                            tracing::info!(renamed_path = %new_path, "Item renamed successfully");
                        }
                        Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                    }
                });
            });
        }

        // 3. Delete action
        let delete_btn = Button::with_label("Delete");
        delete_btn.add_css_class("flat");
        delete_btn.add_css_class("destructive-action");

        {
            let pop = popover.clone();
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_refresh;
            let path = item.path.clone();
            let name = item.name.clone();
            delete_btn.connect_clicked(move |_| {
                pop.popdown();
                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();
                let p = path.clone();
                let n = name.clone();
                glib::MainContext::default().spawn_local(async move {
                    match b.delete_item(&p).await {
                        Ok(_) => {
                            toast.add_toast(adw::Toast::new(&format!("Deleted “{n}”")));
                            cb();
                        }
                        Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                    }
                });
            });
        }

        let rename_label = Label::new(Some("Rename"));
        rename_label.set_halign(Align::Start);
        rename_label.add_css_class("caption");
        rename_label.add_css_class("dim-label");

        menu_box.append(&open_btn);
        menu_box.append(&Separator::new(Orientation::Horizontal));
        menu_box.append(&rename_label);
        menu_box.append(&rename_box);
        menu_box.append(&Separator::new(Orientation::Horizontal));
        menu_box.append(&delete_btn);
        popover.set_child(Some(&menu_box));
        popover.set_parent(target);

        let gesture = GestureClick::new();
        gesture.set_button(3); // Secondary click
        let pop_for_click = popover;
        gesture.connect_pressed(move |_, _, _, _| {
            pop_for_click.popup();
        });
        target.add_controller(gesture);
    }
}
