//! Context menu displayed when right-clicking empty space in the file browser.

use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, GestureClick, Image, Label, Orientation, Popover, Separator};
use libadwaita as adw;
use std::rc::Rc;

use crate::files::new_item_dialog::NewItemDialog;
use crate::files::properties_dialog::PropertiesDialog;
use crate::files::settings_dialog::CloudFilesSettingsDialog;
use crate::model::{FileSyncStatus, FileViewItem};
use crate::widget::FileExplorerBackend;

/// Helper creating a flat context menu button with both an icon and a label.
fn make_menu_item(icon_name: &str, label_text: &str) -> Button {
    let btn = Button::builder().has_frame(false).build();
    let hbox = GtkBox::new(Orientation::Horizontal, 10);
    hbox.set_can_target(false);

    let icon = Image::from_icon_name(icon_name);
    icon.set_can_target(false);

    let label = Label::new(Some(label_text));
    label.set_can_target(false);
    label.set_xalign(0.0);
    label.set_hexpand(true);

    hbox.append(&icon);
    hbox.append(&label);
    btn.set_child(Some(&hbox));
    btn.add_css_class("flat");
    btn
}

/// Context menu shown on empty canvas right-clicks and top-right actions menu.
pub struct CanvasContextMenu;

impl CanvasContextMenu {
    /// Builds the canvas context menu popover with New Folder, New Document, Refresh, Properties, and Settings.
    pub fn create_menu_popover<B: FileExplorerBackend>(
        parent_widget: &impl IsA<gtk4::Widget>,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) -> Popover {
        let popover = Popover::new();
        popover.set_has_arrow(false);
        let menu_box = GtkBox::new(Orientation::Vertical, 4);
        menu_box.set_margin_top(6);
        menu_box.set_margin_bottom(6);
        menu_box.set_margin_start(6);
        menu_box.set_margin_end(6);

        // 1. New Folder button
        let new_folder_btn = make_menu_item("folder-new-symbolic", "New Folder…");
        let parent_clone1 = parent_widget.clone().upcast::<gtk4::Widget>();
        let b1 = backend.clone();
        let toast1 = toast_overlay.clone();
        let cb1 = on_refresh.clone();
        let popover_clone1 = popover.clone();
        new_folder_btn.connect_clicked(move |_| {
            popover_clone1.popdown();
            NewItemDialog::present_new_folder(Some(&parent_clone1), &b1, &toast1, cb1.clone());
        });

        // 2. New File button
        let new_file_btn = make_menu_item("document-new-symbolic", "New Document…");
        let parent_clone2 = parent_widget.clone().upcast::<gtk4::Widget>();
        let b2 = backend.clone();
        let toast2 = toast_overlay.clone();
        let cb2 = on_refresh.clone();
        let popover_clone2 = popover.clone();
        new_file_btn.connect_clicked(move |_| {
            popover_clone2.popdown();
            NewItemDialog::present_new_file(Some(&parent_clone2), &b2, &toast2, cb2.clone());
        });

        // 3. Refresh button
        let refresh_btn = make_menu_item("view-refresh-symbolic", "Refresh");
        let cb3 = on_refresh;
        let popover_clone3 = popover.clone();
        refresh_btn.connect_clicked(move |_| {
            popover_clone3.popdown();
            cb3();
        });

        // 4. Properties button
        let prop_btn = make_menu_item("document-properties-symbolic", "Properties");
        let parent_clone4 = parent_widget.clone().upcast::<gtk4::Widget>();
        let b4 = backend.clone();
        let toast4 = toast_overlay.clone();
        let popover_clone4 = popover.clone();
        prop_btn.connect_clicked(move |_| {
            popover_clone4.popdown();
            let b = b4.clone();
            let toast = toast4.clone();
            let parent = parent_clone4.clone();
            glib::MainContext::default().spawn_local(async move {
                let current_path = b.current_path().await;
                let items = b.list_current_items().await.unwrap_or_default();
                let total_size: u64 = items.iter().map(|i| i.size_bytes).sum();
                let name = if current_path == "/" {
                    "Nexia".to_string()
                } else {
                    current_path
                        .split('/')
                        .rfind(|s| !s.is_empty())
                        .unwrap_or("Folder")
                        .to_string()
                };
                let folder_item = FileViewItem {
                    path: current_path,
                    name,
                    is_directory: true,
                    size_formatted: FileViewItem::format_bytes(total_size),
                    size_bytes: total_size,
                    status: FileSyncStatus::CloudPlaceholder,
                    mime_type: Some("httpd/unix-directory".to_string()),
                    file_id: None,
                    icon_category: nexia_nextcloud_sdk::ResourceIconCategory::Folder,
                    modified_at: None,
                    permissions: Some("RDNVWC".to_string()),
                    directory_summary: None,
                };
                PropertiesDialog::present(Some(&parent), &folder_item, &b, &toast);
            });
        });

        // 5. Settings button
        let settings_btn = make_menu_item("emblem-system-symbolic", "Settings…");
        let parent_clone5 = parent_widget.clone().upcast::<gtk4::Widget>();
        let b5 = backend.clone();
        let popover_clone5 = popover.clone();
        settings_btn.connect_clicked(move |_| {
            popover_clone5.popdown();
            CloudFilesSettingsDialog::present(Some(&parent_clone5), &b5);
        });

        menu_box.append(&new_folder_btn);
        menu_box.append(&new_file_btn);
        menu_box.append(&Separator::new(Orientation::Horizontal));
        menu_box.append(&refresh_btn);
        menu_box.append(&Separator::new(Orientation::Horizontal));
        menu_box.append(&prop_btn);
        menu_box.append(&settings_btn);

        popover.set_child(Some(&menu_box));
        popover.set_parent(parent_widget);
        popover
    }

    /// Attaches canvas right-click context menu capabilities to `target`.
    pub fn attach<B: FileExplorerBackend>(
        target: &impl IsA<gtk4::Widget>,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
        is_hit_child: Rc<dyn Fn(f64, f64) -> bool>,
    ) {
        let popover = Self::create_menu_popover(target, backend, toast_overlay, on_refresh);

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
