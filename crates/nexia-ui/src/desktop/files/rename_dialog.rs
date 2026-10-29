//! Nautilus-style rename dialog using Libadwaita MessageDialog with extension preservation.

use gtk4::Entry;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::rc::Rc;

use crate::model::FileViewItem;
use crate::widget::FileExplorerBackend;

/// Nautilus-compliant modal rename dialog.
pub struct RenameDialog;

impl RenameDialog {
    /// Presents the modal rename dialog for `item`, matching Nautilus selection and submission behavior.
    pub fn present<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        item: &FileViewItem,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let dialog = adw::MessageDialog::builder()
            .heading("Rename")
            .body(format!("Enter a new name for “{}”:", item.name))
            .build();

        if let Some(ref win) = root_window {
            dialog.set_transient_for(Some(win));
        }

        let entry = Entry::new();
        entry.set_text(&item.name);
        entry.set_margin_top(8);
        entry.set_margin_bottom(8);
        entry.set_margin_start(4);
        entry.set_margin_end(4);

        // Mirror Nautilus filename pre-selection: select only the stem, leaving the extension unselected
        let sel_end = if !item.is_directory
            && let Some(dot_idx) = item.name.rfind('.')
            && dot_idx > 0
        {
            dot_idx as i32
        } else {
            item.name.len() as i32
        };
        entry.select_region(0, sel_end);

        entry.connect_map(move |e| {
            e.grab_focus();
            e.select_region(0, sel_end);
        });

        let entry_idle = entry.clone();
        glib::idle_add_local_once(move || {
            entry_idle.grab_focus();
            entry_idle.select_region(0, sel_end);
        });

        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("rename", "Rename");
        dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
        dialog.set_close_response("cancel");

        // Keep the Rename button unselected initially until the user enters a modified, valid name
        dialog.set_response_enabled("rename", false);

        let dialog_for_change = dialog.clone();
        let old_name_for_change = item.name.clone();
        entry.connect_text_notify(move |e| {
            let text = e.text().to_string().trim().to_string();
            let is_valid = !text.is_empty() && text != old_name_for_change && !text.contains('/');
            dialog_for_change.set_response_enabled("rename", is_valid);
        });

        // Pressing Enter in the text entry triggers the "rename" response if valid
        let dialog_for_activate = dialog.clone();
        entry.connect_activate(move |_| {
            if dialog_for_activate.is_response_enabled("rename") {
                dialog_for_activate.response("rename");
            }
        });

        let b = backend.clone();
        let toast = toast_overlay.clone();
        let cb = on_refresh;
        let item_path = item.path.clone();
        let old_name = item.name.clone();
        let entry_clone = entry;

        dialog.connect_response(None, move |d, response| {
            if response == "rename" {
                let new_name = entry_clone.text().to_string().trim().to_string();
                if new_name.is_empty() || new_name == old_name {
                    d.close();
                    return;
                }

                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();
                let path = item_path.clone();

                glib::MainContext::default().spawn_local(async move {
                    match b.rename_item(&path, &new_name).await {
                        Ok(_) => {
                            toast.add_toast(adw::Toast::new(&format!("Renamed to “{new_name}”")));
                            cb();
                        }
                        Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                    }
                });
            }
            d.close();
        });

        dialog.present();
    }
}
