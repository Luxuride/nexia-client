//! Nautilus-style file and folder properties dialog using Libadwaita preferences rows.

use gtk4::Button;
use gtk4::gdk;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use nextcloud_rust_sdk::NextcloudPermissions;

use crate::model::FileViewItem;
use crate::widget::FileExplorerBackend;

/// Dialog window presenting detailed file metadata, size, location, and share options.
pub struct PropertiesDialog;

impl PropertiesDialog {
    /// Presents the modal properties dialog for `item`.
    pub fn present<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        item: &FileViewItem,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let window = adw::Window::builder()
            .title(format!("“{}” Properties", item.name))
            .default_width(440)
            .default_height(480)
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

        let size_desc = format!("{} ({} bytes)", item.size_formatted, item.size_bytes);
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

        let (perms_label, perms_desc) = item
            .permissions
            .as_deref()
            .map(|raw| {
                let p = NextcloudPermissions::parse(raw);
                (p.summary_label().to_string(), p.human_readable())
            })
            .unwrap_or_else(|| {
                (
                    "Read & Write (Full)".to_string(),
                    "Full access (can read, write, delete, and share)".to_string(),
                )
            });

        let row_perms = adw::ActionRow::builder()
            .title("Permissions")
            .subtitle(format!("{perms_label} — {perms_desc}"))
            .build();
        share_group.add(&row_perms);

        let row_share = adw::ActionRow::builder()
            .title("Public Link")
            .subtitle("No public link generated yet")
            .build();

        let share_btn = Button::with_label("Create Public Link");
        share_btn.add_css_class("suggested-action");
        share_btn.set_valign(gtk4::Align::Center);

        let copy_btn = Button::from_icon_name("edit-copy-symbolic");
        copy_btn.set_valign(gtk4::Align::Center);
        copy_btn.set_tooltip_text(Some("Copy Public Link"));
        copy_btn.set_visible(false);

        let unshare_btn = Button::with_label("Unshare");
        unshare_btn.add_css_class("destructive-action");
        unshare_btn.set_valign(gtk4::Align::Center);
        unshare_btn.set_visible(false);

        let backend_clone = backend.clone();
        let path_clone = item.path.clone();
        let toast_clone = toast_overlay.clone();
        let row_share_clone = row_share.clone();
        let share_btn_clone = share_btn.clone();
        let copy_btn_clone = copy_btn.clone();
        let unshare_btn_clone = unshare_btn.clone();

        share_btn.connect_clicked(move |_| {
            share_btn_clone.set_sensitive(false);
            share_btn_clone.set_label("Generating Link…");
            let b = backend_clone.clone();
            let p = path_clone.clone();
            let t = toast_clone.clone();
            let r = row_share_clone.clone();
            let s_btn = share_btn_clone.clone();
            let c_btn = copy_btn_clone.clone();
            let u_btn = unshare_btn_clone.clone();

            glib::MainContext::default().spawn_local(async move {
                match b.create_public_share(&p).await {
                    Ok(url) => {
                        r.set_subtitle(&url);
                        s_btn.set_visible(false);
                        c_btn.set_visible(true);
                        u_btn.set_visible(true);
                        if let Some(display) = gdk::Display::default() {
                            display.clipboard().set_text(&url);
                        }
                        t.add_toast(adw::Toast::new("Public link copied to clipboard"));
                    }
                    Err(e) => {
                        s_btn.set_sensitive(true);
                        s_btn.set_label("Retry Public Link");
                        t.add_toast(adw::Toast::new(&format!("Failed to create link: {e}")));
                    }
                }
            });
        });

        let row_share_for_copy = row_share.clone();
        let toast_for_copy = toast_overlay.clone();
        copy_btn.connect_clicked(move |_| {
            if let Some(url) = row_share_for_copy.subtitle() {
                if let Some(display) = gdk::Display::default() {
                    display.clipboard().set_text(&url);
                }
                toast_for_copy.add_toast(adw::Toast::new("Public link copied to clipboard"));
            }
        });

        let backend_unshare = backend.clone();
        let path_unshare = item.path.clone();
        let toast_unshare = toast_overlay.clone();
        let row_share_unshare = row_share.clone();
        let share_btn_unshare = share_btn.clone();
        let copy_btn_unshare = copy_btn.clone();
        let unshare_btn_self = unshare_btn.clone();
        unshare_btn.connect_clicked(move |_| {
            unshare_btn_self.set_sensitive(false);
            unshare_btn_self.set_label("Removing…");
            let b = backend_unshare.clone();
            let p = path_unshare.clone();
            let t = toast_unshare.clone();
            let r = row_share_unshare.clone();
            let s_btn = share_btn_unshare.clone();
            let c_btn = copy_btn_unshare.clone();
            let u_btn = unshare_btn_self.clone();

            glib::MainContext::default().spawn_local(async move {
                match b.delete_public_share(&p).await {
                    Ok(_) => {
                        r.set_subtitle("No public link generated yet");
                        s_btn.set_visible(true);
                        s_btn.set_sensitive(true);
                        s_btn.set_label("Create Public Link");
                        c_btn.set_visible(false);
                        u_btn.set_visible(false);
                        u_btn.set_sensitive(true);
                        u_btn.set_label("Unshare");
                        t.add_toast(adw::Toast::new("Public link removed"));
                    }
                    Err(e) => {
                        u_btn.set_sensitive(true);
                        u_btn.set_label("Unshare");
                        t.add_toast(adw::Toast::new(&format!("Failed to remove link: {e}")));
                    }
                }
            });
        });

        // Query existing share on load
        {
            let b = backend.clone();
            let p = item.path.clone();
            let r = row_share.clone();
            let s_btn = share_btn.clone();
            let c_btn = copy_btn.clone();
            let u_btn = unshare_btn.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Ok(Some(url)) = b.get_public_share(&p).await {
                    r.set_subtitle(&url);
                    s_btn.set_visible(false);
                    c_btn.set_visible(true);
                    u_btn.set_visible(true);
                }
            });
        }

        row_share.add_suffix(&share_btn);
        row_share.add_suffix(&copy_btn);
        row_share.add_suffix(&unshare_btn);
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
