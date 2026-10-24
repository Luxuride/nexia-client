//! Popover menu button for creating a new directory inside the active Cloud Files folder.

#[cfg(feature = "gtk")]
use gtk4::{Align, Box as GtkBox, Button, Entry, Label, MenuButton, Orientation, Popover};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use std::rc::Rc;

#[cfg(feature = "gtk")]
use crate::widget::{FileExplorerBackend, ViewComponent};

/// Menu button and popover dialog for creating a new folder in the current directory.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct NewFolderPopover {
    button: MenuButton,
}

#[cfg(feature = "gtk")]
impl NewFolderPopover {
    /// Creates a new folder menu button wired to `backend` and `toast_overlay`.
    pub fn new<B: FileExplorerBackend>(
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_created: Rc<dyn Fn()>,
    ) -> Self {
        let button = MenuButton::builder()
            .icon_name("folder-new-symbolic")
            .tooltip_text("New Folder")
            .build();

        let popover = Popover::new();
        let box_layout = GtkBox::new(Orientation::Vertical, 8);
        box_layout.set_margin_top(12);
        box_layout.set_margin_bottom(12);
        box_layout.set_margin_start(12);
        box_layout.set_margin_end(12);

        let title = Label::new(Some("Folder Name"));
        title.set_halign(Align::Start);
        title.add_css_class("heading");

        let entry = Entry::new();
        entry.set_placeholder_text(Some("New Folder"));
        entry.set_width_chars(22);

        let create_btn = Button::with_label("Create");
        create_btn.add_css_class("suggested-action");
        create_btn.set_halign(Align::End);

        box_layout.append(&title);
        box_layout.append(&entry);
        box_layout.append(&create_btn);
        popover.set_child(Some(&box_layout));
        button.set_popover(Some(&popover));

        let backend_clone = backend.clone();
        let toast_clone = toast_overlay.clone();
        let entry_clone = entry.clone();
        let pop_clone = popover;
        create_btn.connect_clicked(move |_| {
            let name = entry_clone.text().to_string();
            entry_clone.set_text("");
            pop_clone.popdown();

            let b = backend_clone.clone();
            let toast = toast_clone.clone();
            let cb = on_created.clone();
            glib::MainContext::default().spawn_local(async move {
                match b.create_folder(&name).await {
                    Ok(_) => {
                        toast.add_toast(adw::Toast::new(&format!("Created folder “{name}”")));
                        cb();
                    }
                    Err(err) => {
                        toast.add_toast(adw::Toast::new(&err.to_string()));
                    }
                }
            });
        });

        Self { button }
    }
}

#[cfg(feature = "gtk")]
impl ViewComponent for NewFolderPopover {
    type RootWidget = MenuButton;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.button
    }
}
