//! Lazy context menu popover providing Open, Rename, Delete, and Properties actions for file items.

use gtk4::{Box as GtkBox, Button, GestureClick, Orientation, Popover, Separator};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::rc::Rc;

use crate::files::delete_dialog::DeleteConfirmationDialog;
use crate::files::rename_dialog::RenameDialog;
use crate::model::{FileSyncStatus, FileViewItem};
use crate::widget::FileExplorerBackend;

/// Right-click context menu offering Open, Rename, Delete, and Properties actions.
pub struct ItemContextMenu;

impl ItemContextMenu {
    /// Attaches a right-click secondary click gesture that builds and presents the context menu lazily on demand.
    pub fn attach<B: FileExplorerBackend>(
        target: &impl IsA<gtk4::Widget>,
        item: &FileViewItem,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) {
        let gesture = GestureClick::new();
        gesture.set_button(3); // Secondary click
        let target_widget = target.clone().upcast::<gtk4::Widget>();
        let item_clone = item.clone();
        let b = backend.clone();
        let toast = toast_overlay.clone();
        let cb = on_refresh;

        gesture.connect_pressed(move |_, _, _, _| {
            Self::show_menu(&target_widget, &item_clone, &b, &toast, cb.clone());
        });
        target.add_controller(gesture);
    }

    /// Lazily constructs and presents the context menu popover anchored to `target`.
    fn show_menu<B: FileExplorerBackend>(
        target: &gtk4::Widget,
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
                                crate::files::launcher::launch_file_externally(&path);
                                toast
                                    .add_toast(adw::Toast::new(&format!("Opened “{}”", item.name)));
                            }
                            Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                        }
                    }
                });
            });
        }

        // Hydration download / free space action for files
        let hydration_btn = if !item.is_directory {
            match item.status {
                FileSyncStatus::CloudPlaceholder => {
                    let btn = Button::with_label("Download (Keep Offline)");
                    btn.add_css_class("flat");
                    let pop = popover.clone();
                    let b = backend.clone();
                    let toast = toast_overlay.clone();
                    let cb = on_refresh.clone();
                    let p = item.path.clone();
                    let n = item.name.clone();
                    btn.connect_clicked(move |_| {
                        pop.popdown();
                        let b = b.clone();
                        let toast = toast.clone();
                        let cb = cb.clone();
                        let p = p.clone();
                        let n = n.clone();
                        glib::MainContext::default().spawn_local(async move {
                            match b.hydrate_file(&p).await {
                                Ok(_) => {
                                    toast.add_toast(adw::Toast::new(&format!("Downloaded “{n}”")));
                                    cb();
                                }
                                Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                            }
                        });
                    });
                    Some(btn)
                }
                FileSyncStatus::HydratedOffline => {
                    let btn = Button::with_label("Free Up Space");
                    btn.add_css_class("flat");
                    let pop = popover.clone();
                    let b = backend.clone();
                    let toast = toast_overlay.clone();
                    let cb = on_refresh.clone();
                    let p = item.path.clone();
                    let n = item.name.clone();
                    btn.connect_clicked(move |_| {
                        pop.popdown();
                        let b = b.clone();
                        let toast = toast.clone();
                        let cb = cb.clone();
                        let p = p.clone();
                        let n = n.clone();
                        glib::MainContext::default().spawn_local(async move {
                            match b.dehydrate_file(&p).await {
                                Ok(_) => {
                                    toast.add_toast(adw::Toast::new(&format!(
                                        "Freed space for “{n}”"
                                    )));
                                    cb();
                                }
                                Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                            }
                        });
                    });
                    Some(btn)
                }
                FileSyncStatus::Syncing => {
                    let btn = Button::with_label("Syncing…");
                    btn.add_css_class("flat");
                    btn.set_sensitive(false);
                    Some(btn)
                }
                _ => None,
            }
        } else {
            let btn = Button::with_label("Download Folder (Keep Offline)");
            btn.add_css_class("flat");
            let pop = popover.clone();
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_refresh.clone();
            let p = item.path.clone();
            let n = item.name.clone();
            btn.connect_clicked(move |_| {
                pop.popdown();
                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();
                let p = p.clone();
                let n = n.clone();
                toast.add_toast(adw::Toast::new(&format!("Downloading files in “{n}”…")));
                glib::MainContext::default().spawn_local(async move {
                    match b.hydrate_directory(&p).await {
                        Ok(count) => {
                            toast.add_toast(adw::Toast::new(&format!(
                                "Downloaded {count} files in “{n}”"
                            )));
                            cb();
                        }
                        Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                    }
                });
            });
            Some(btn)
        };

        // 2. Rename action (opens Nautilus modal Rename dialog)
        let rename_btn = Button::with_label("Rename…");
        rename_btn.add_css_class("flat");

        {
            let pop = popover.clone();
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_refresh.clone();
            let item_clone = item.clone();
            let target_widget = target.clone();
            rename_btn.connect_clicked(move |_| {
                pop.popdown();
                RenameDialog::present(Some(&target_widget), &item_clone, &b, &toast, cb.clone());
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
            let is_dir = item.is_directory;
            delete_btn.connect_clicked(move |btn| {
                pop.popdown();
                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();
                let p = path.clone();
                let n = name.clone();
                DeleteConfirmationDialog::show(Some(btn), &name, is_dir, move || {
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
            });
        }

        // 4. Properties action
        let properties_btn = Button::with_label("Properties");
        properties_btn.add_css_class("flat");

        {
            let pop = popover.clone();
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let item_clone = item.clone();
            properties_btn.connect_clicked(move |btn| {
                pop.popdown();
                crate::files::PropertiesDialog::present(Some(btn), &item_clone, &b, &toast);
            });
        }

        menu_box.append(&open_btn);
        if let Some(ref btn) = hydration_btn {
            menu_box.append(btn);
        }
        menu_box.append(&Separator::new(Orientation::Horizontal));
        menu_box.append(&rename_btn);
        menu_box.append(&delete_btn);
        menu_box.append(&Separator::new(Orientation::Horizontal));
        menu_box.append(&properties_btn);

        popover.set_child(Some(&menu_box));
        popover.set_parent(target);
        popover.popup();
    }
}
