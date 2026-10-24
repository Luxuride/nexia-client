//! Context menu displayed when right-clicking empty space in the file browser.

use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, GestureClick, Orientation, Popover, Separator};
use libadwaita as adw;
use std::rc::Rc;

use crate::files::new_item_dialog::NewItemDialog;
use crate::widget::FileExplorerBackend;

/// Context menu shown on empty canvas right-clicks.
pub struct CanvasContextMenu;

impl CanvasContextMenu {
    /// Attaches canvas right-click context menu capabilities to `target`.
    pub fn attach<B: FileExplorerBackend>(
        target: &impl IsA<gtk4::Widget>,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
        is_hit_child: Rc<dyn Fn(f64, f64) -> bool>,
    ) {
        let popover = Popover::new();
        popover.set_has_arrow(false);
        let menu_box = GtkBox::new(Orientation::Vertical, 4);
        menu_box.set_margin_top(6);
        menu_box.set_margin_bottom(6);
        menu_box.set_margin_start(6);
        menu_box.set_margin_end(6);

        // 1. New Folder button
        let new_folder_btn = Button::builder()
            .label("New Folder…")
            .icon_name("folder-new-symbolic")
            .has_frame(false)
            .build();
        let target_clone = target.clone().upcast::<gtk4::Widget>();
        let b = backend.clone();
        let toast = toast_overlay.clone();
        let cb = on_refresh.clone();
        let popover_clone = popover.clone();
        new_folder_btn.connect_clicked(move |_| {
            popover_clone.popdown();
            NewItemDialog::present_new_folder(Some(&target_clone), &b, &toast, cb.clone());
        });

        // 2. New File button
        let new_file_btn = Button::builder()
            .label("New Document…")
            .icon_name("document-new-symbolic")
            .has_frame(false)
            .build();
        let target_clone2 = target.clone().upcast::<gtk4::Widget>();
        let b2 = backend.clone();
        let toast2 = toast_overlay.clone();
        let cb2 = on_refresh.clone();
        let popover_clone2 = popover.clone();
        new_file_btn.connect_clicked(move |_| {
            popover_clone2.popdown();
            NewItemDialog::present_new_file(Some(&target_clone2), &b2, &toast2, cb2.clone());
        });

        // 3. Refresh button
        let refresh_btn = Button::builder()
            .label("Refresh")
            .icon_name("view-refresh-symbolic")
            .has_frame(false)
            .build();
        let cb3 = on_refresh;
        let popover_clone3 = popover.clone();
        refresh_btn.connect_clicked(move |_| {
            popover_clone3.popdown();
            cb3();
        });

        menu_box.append(&new_folder_btn);
        menu_box.append(&new_file_btn);
        menu_box.append(&Separator::new(Orientation::Horizontal));
        menu_box.append(&refresh_btn);

        popover.set_child(Some(&menu_box));
        popover.set_parent(target);

        let click = GestureClick::new();
        click.set_button(3); // Secondary click
        let popover_click = popover;
        click.connect_pressed(move |_, _, x, y| {
            if !is_hit_child(x, y) {
                let rect = gdk::Rectangle::new(x as i32, y as i32, 1, 1);
                popover_click.set_pointing_to(Some(&rect));
                popover_click.popup();
            }
        });
        target.add_controller(click);
    }
}
