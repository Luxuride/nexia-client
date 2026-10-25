//! Nautilus-style file and folder properties dialog using Libadwaita preferences rows.

use gtk4::Button;
use gtk4::gdk;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::model::FileViewItem;

/// Dialog window presenting detailed file metadata, size, location, and share options.
pub struct PropertiesDialog;

impl PropertiesDialog {
    /// Presents the modal properties dialog for `item`.
    pub fn present(
        parent: Option<&impl IsA<gtk4::Widget>>,
        item: &FileViewItem,
        toast_overlay: &adw::ToastOverlay,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let window = adw::Window::builder()
            .title(format!("“{}” Properties", item.name))
            .default_width(420)
            .default_height(460)
            .modal(true)
            .build();

        if let Some(ref win) = root_window {
            window.set_transient_for(Some(win));
        }

        let page = adw::PreferencesPage::new();

        // Group 1: General Metadata
        let general_group = adw::PreferencesGroup::builder()
            .title("General Information")
            .build();

        let row_name = adw::ActionRow::builder()
            .title("Name")
            .subtitle(&item.name)
            .build();
        general_group.add(&row_name);

        let type_desc = if let Some(ref mime) = item.mime_type {
            format!("{} ({mime})", item.type_label())
        } else {
            item.type_label().to_string()
        };
        let row_type = adw::ActionRow::builder()
            .title("Type")
            .subtitle(&type_desc)
            .build();
        general_group.add(&row_type);

        let size_desc = if item.is_directory {
            "Folder".to_string()
        } else {
            format!("{} ({} bytes)", item.size_formatted, item.size_bytes)
        };
        let row_size = adw::ActionRow::builder()
            .title("Size")
            .subtitle(&size_desc)
            .build();
        general_group.add(&row_size);

        let row_path = adw::ActionRow::builder()
            .title("Location")
            .subtitle(&item.path)
            .build();
        general_group.add(&row_path);

        let mod_time = item.modified_at.as_deref().unwrap_or("Recently updated");
        let row_modified = adw::ActionRow::builder()
            .title("Last Modified")
            .subtitle(mod_time)
            .build();
        general_group.add(&row_modified);

        let row_status = adw::ActionRow::builder()
            .title("Synchronization")
            .subtitle(item.status.display_label())
            .build();
        general_group.add(&row_status);

        // Group 2: Nextcloud Sharing & Permissions
        let share_group = adw::PreferencesGroup::builder()
            .title("Sharing & Access")
            .build();

        let perms = item.permissions.as_deref().unwrap_or("Read & Write (Full)");
        let row_perms = adw::ActionRow::builder()
            .title("Permissions")
            .subtitle(perms)
            .build();
        share_group.add(&row_perms);

        let share_row = adw::ActionRow::builder()
            .title("Share with Nextcloud")
            .subtitle("Enable public link sharing and team collaboration")
            .build();
        let switch = gtk4::Switch::new();
        switch.set_active(true);
        switch.set_valign(gtk4::Align::Center);
        share_row.add_suffix(&switch);
        share_group.add(&share_row);

        let share_link = format!(
            "https://cloud.nextcloud.com/remote.php/dav/files/system{}",
            item.path
        );
        let row_share = adw::ActionRow::builder()
            .title("Cloud Link")
            .subtitle(&share_link)
            .build();

        let copy_btn = Button::from_icon_name("edit-copy-symbolic");
        copy_btn.set_valign(gtk4::Align::Center);
        copy_btn.set_tooltip_text(Some("Copy Cloud Link"));

        let link_for_copy = share_link;
        let toast = toast_overlay.clone();
        copy_btn.connect_clicked(move |_| {
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_text(&link_for_copy);
                toast.add_toast(adw::Toast::new("Cloud link copied to clipboard"));
            }
        });

        row_share.add_suffix(&copy_btn);
        share_group.add(&row_share);

        page.add(&general_group);
        page.add(&share_group);

        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let header = adw::HeaderBar::new();
        header.set_show_end_title_buttons(true);
        content_box.append(&header);
        content_box.append(&page);

        window.set_content(Some(&content_box));
        window.present();
    }
}
