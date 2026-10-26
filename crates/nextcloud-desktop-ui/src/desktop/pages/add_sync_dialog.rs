//! Subwindow dialog for configuring and adding a new two-way local folder synchronization pairing.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Button, FileChooserAction, FileChooserNative, Image, ResponseType};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::widget::FileExplorerBackend;

/// Modal subwindow allowing the user to configure and pair a local filesystem folder with Nextcloud.
pub struct AddSyncFolderDialog;

impl AddSyncFolderDialog {
    /// Presents the Add Sync Folder subwindow.
    pub fn present<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_change: Rc<dyn Fn()>,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let window = adw::Window::builder()
            .title("Add Folder Synchronization")
            .default_width(500)
            .default_height(480)
            .modal(true)
            .build();

        if let Some(ref win) = root_window {
            window.set_transient_for(Some(win));
        }

        let header_bar = adw::HeaderBar::new();
        header_bar.set_show_end_title_buttons(true);
        header_bar.set_show_start_title_buttons(false);

        let page = adw::PreferencesPage::new();

        let group = adw::PreferencesGroup::builder()
            .title("Sync Pairing Configuration")
            .description("Choose a local folder and corresponding remote Nextcloud directory to keep in sync.")
            .build();

        // 1. Local folder selection row
        let default_local = glib::home_dir().join("Nextcloud");

        let selected_local_path = Rc::new(std::cell::RefCell::new(default_local.clone()));

        let local_row = adw::ActionRow::builder()
            .title("Local Directory")
            .subtitle(default_local.to_string_lossy().as_ref())
            .build();
        local_row.add_prefix(&Image::from_icon_name("folder-symbolic"));

        let choose_btn = Button::with_label("Choose Folder…");
        choose_btn.set_valign(Align::Center);
        local_row.add_suffix(&choose_btn);
        group.add(&local_row);

        // Native folder picker
        let win_for_picker = window.clone();
        let path_cell = selected_local_path.clone();
        let row_local_clone = local_row.clone();

        choose_btn.connect_clicked(move |_| {
            let file_chooser = FileChooserNative::new(
                Some("Select Local Sync Folder"),
                Some(&win_for_picker),
                FileChooserAction::SelectFolder,
                Some("Select"),
                Some("Cancel"),
            );

            let path_cell_c = path_cell.clone();
            let row_c = row_local_clone.clone();

            file_chooser.connect_response(move |chooser, response| {
                if response == ResponseType::Accept
                    && let Some(file) = chooser.file()
                    && let Some(path) = file.path()
                {
                    row_c.set_subtitle(path.to_string_lossy().as_ref());
                    *path_cell_c.borrow_mut() = path;
                }
            });

            file_chooser.show();
        });

        // 2. Remote directory selection row with visual browser
        let selected_remote_path = Rc::new(std::cell::RefCell::new("/".to_string()));

        let remote_row = adw::ActionRow::builder()
            .title("Remote Directory")
            .subtitle("/")
            .build();
        remote_row.add_prefix(&Image::from_icon_name("network-server-symbolic"));

        let browse_remote_btn = Button::with_label("Browse Remote…");
        browse_remote_btn.set_valign(Align::Center);
        remote_row.add_suffix(&browse_remote_btn);
        group.add(&remote_row);

        let win_for_remote = window.clone();
        let b_for_remote = backend.clone();
        let rem_cell = selected_remote_path.clone();
        let row_remote_clone = remote_row.clone();

        browse_remote_btn.connect_clicked(move |_| {
            let row_c = row_remote_clone.clone();
            let rem_c = rem_cell.clone();
            let init = rem_c.borrow().clone();

            super::RemoteDirectoryBrowserDialog::present(
                Some(&win_for_remote),
                &b_for_remote,
                &init,
                Rc::new(move |chosen| {
                    row_c.set_subtitle(&chosen);
                    *rem_c.borrow_mut() = chosen;
                }),
            );
        });

        // 3. Mode selector
        let mode_options = [
            "Two-Way Synchronization (Bidirectional)",
            "Virtual Filesystem (Sparse placeholders on ext4/btrfs)",
            "FUSE Virtual Filesystem (Transparent on-demand kernel hydration)",
        ];
        let mode_model = gtk4::StringList::new(&mode_options);
        let mode_row = adw::ComboRow::builder()
            .title("Synchronization Mode")
            .model(&mode_model)
            .selected(0)
            .build();
        group.add(&mode_row);

        // 4. Custom Display Name
        let name_entry = adw::EntryRow::builder()
            .title("Display Name (Optional)")
            .build();
        group.add(&name_entry);

        page.add(&group);

        // Action row: Add button
        let action_group = adw::PreferencesGroup::new();
        let add_btn = Button::with_label("Add Sync Folder");
        add_btn.set_halign(Align::End);
        add_btn.set_margin_top(16);
        add_btn.set_margin_bottom(16);
        add_btn.set_margin_end(16);
        add_btn.add_css_class("suggested-action");
        action_group.add(&add_btn);
        page.add(&action_group);

        // Wire Add Sync Folder
        let b = backend.clone();
        let toast = toast_overlay.clone();
        let win = window.clone();
        let path_ref = selected_local_path;
        let rem_ref = selected_remote_path;
        let m_row = mode_row;
        let n_entry = name_entry;

        add_btn.connect_clicked(move |_| {
            let local_path = path_ref.borrow().clone();
            let remote_path = rem_ref.borrow().clone();
            let remote_path = if remote_path.is_empty() || !remote_path.starts_with('/') {
                format!("/{remote_path}")
            } else {
                remote_path
            };

            let mode = match m_row.selected() {
                2 => "Fuse".to_string(),
                1 => "VirtualVfs".to_string(),
                _ => "BidirectionalSync".to_string(),
            };

            let name_text = n_entry.text().to_string().trim().to_string();
            let display_name = if name_text.is_empty() {
                None
            } else {
                Some(name_text)
            };

            let b = b.clone();
            let toast = toast.clone();
            let win = win.clone();
            let cb = on_change.clone();

            glib::MainContext::default().spawn_local(async move {
                match b
                    .add_sync_mount(remote_path, local_path, mode, display_name)
                    .await
                {
                    Ok(_) => {
                        toast.add_toast(adw::Toast::new("Sync folder added successfully"));
                        cb();
                        win.close();
                    }
                    Err(e) => {
                        toast.add_toast(adw::Toast::new(&format!("Failed to add sync mount: {e}")));
                    }
                }
            });
        });

        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content_box.append(&header_bar);
        content_box.append(&page);
        window.set_content(Some(&content_box));
        window.present();
    }
}
