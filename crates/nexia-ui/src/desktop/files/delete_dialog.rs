//! Confirmation dialog for deleting files or folders in Cloud Files.

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Presents an Adwaita confirmation dialog before deleting a resource.
pub struct DeleteConfirmationDialog;

impl DeleteConfirmationDialog {
    /// Shows a modal confirmation dialog for deleting `item_name`.
    pub fn show<F>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        item_name: &str,
        is_directory: bool,
        on_confirm: F,
    ) where
        F: FnOnce() + 'static,
    {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let kind = if is_directory { "folder" } else { "file" };
        let heading = format!("Delete “{item_name}”?");
        let body = format!(
            "Are you sure you want to delete this {kind}?\n\nIt will be moved to your Nextcloud trashbin."
        );

        let dialog = adw::MessageDialog::builder()
            .heading(heading)
            .body(body)
            .build();

        if let Some(ref win) = root_window {
            dialog.set_transient_for(Some(win));
        }

        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let on_confirm_cell = Rc::new(RefCell::new(Some(on_confirm)));

        dialog.connect_response(None, move |_, response| {
            if response == "delete"
                && let Some(action) = on_confirm_cell.borrow_mut().take()
            {
                action();
            }
        });

        dialog.present();
    }
}
