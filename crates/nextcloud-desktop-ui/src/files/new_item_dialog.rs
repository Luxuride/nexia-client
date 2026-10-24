//! Nautilus-style creation dialogs for new folders and new files with stem selection.

use gtk4::Entry;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::rc::Rc;

use crate::widget::FileExplorerBackend;

/// Creation dialog for new files and folders with extension preservation.
pub struct NewItemDialog;

impl NewItemDialog {
    /// Presents a modal dialog to create a new folder in the active directory.
    pub fn present_new_folder<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) {
        Self::present_dialog(
            parent,
            "New Folder",
            "Enter a name for the new folder:",
            "New Folder",
            backend,
            toast_overlay,
            on_refresh,
            false,
        );
    }

    /// Presents a modal dialog to create a new file in the active directory with stem pre-selection.
    pub fn present_new_file<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) {
        Self::present_dialog(
            parent,
            "New File",
            "Enter a name for the new file:",
            "New Document.txt",
            backend,
            toast_overlay,
            on_refresh,
            true,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn present_dialog<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        heading: &str,
        body: &str,
        default_name: &str,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
        is_file: bool,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let dialog = adw::MessageDialog::builder()
            .heading(heading)
            .body(body)
            .build();

        if let Some(ref win) = root_window {
            dialog.set_transient_for(Some(win));
        }

        let entry = Entry::new();
        entry.set_text(default_name);
        entry.set_margin_top(8);
        entry.set_margin_bottom(8);
        entry.set_margin_start(4);
        entry.set_margin_end(4);

        // Calculate stem selection (e.g. "New Document" excluding ".txt")
        let sel_end = if is_file
            && let Some(dot_idx) = default_name.rfind('.')
            && dot_idx > 0
        {
            dot_idx as i32
        } else {
            default_name.len() as i32
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
        dialog.add_response("create", "Create");
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");

        let dialog_for_activate = dialog.clone();
        entry.connect_activate(move |_| {
            dialog_for_activate.response("create");
        });

        let b = backend.clone();
        let toast = toast_overlay.clone();
        let cb = on_refresh;
        let entry_clone = entry;

        dialog.connect_response(None, move |d, response| {
            if response == "create" {
                let name = entry_clone.text().to_string().trim().to_string();
                if name.is_empty() {
                    d.close();
                    return;
                }

                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();

                glib::MainContext::default().spawn_local(async move {
                    if is_file {
                        match b.create_file(&name, b"").await {
                            Ok(_) => {
                                toast.add_toast(adw::Toast::new(&format!("Created file “{name}”")));
                                cb();
                            }
                            Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                        }
                    } else {
                        match b.create_folder(&name).await {
                            Ok(_) => {
                                toast.add_toast(adw::Toast::new(&format!(
                                    "Created folder “{name}”"
                                )));
                                cb();
                            }
                            Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                        }
                    }
                });
            }
            d.close();
        });

        dialog.present();
    }
}
