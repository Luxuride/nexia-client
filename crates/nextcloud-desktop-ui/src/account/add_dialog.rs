//! Modal dialog for connecting a new Nextcloud account profile.

use gtk4::prelude::*;
use gtk4::{Button, PasswordEntry};
use libadwaita as adw;
use libadwaita::prelude::*;
use nextcloud_desktop_core::account::{Account, AccountCredentials, ServerConfig};
use std::rc::Rc;

use crate::widget::FileExplorerBackend;

/// Modal dialog allowing users to enter server credentials and register a new account.
pub struct AddAccountDialog;

impl AddAccountDialog {
    /// Presents the modal Add Account dialog.
    pub fn present<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_account_added: Rc<dyn Fn()>,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let window = adw::Window::builder()
            .title("Add Nextcloud Account")
            .default_width(420)
            .default_height(460)
            .modal(true)
            .build();

        if let Some(ref win) = root_window {
            window.set_transient_for(Some(win));
        }

        let header_bar = adw::HeaderBar::new();
        header_bar.set_show_end_title_buttons(false);
        header_bar.set_show_start_title_buttons(false);

        let cancel_btn = Button::with_label("Cancel");
        header_bar.pack_start(&cancel_btn);

        let add_btn = Button::with_label("Add");
        add_btn.add_css_class("suggested-action");
        header_bar.pack_end(&add_btn);

        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder()
            .title("Connection Details")
            .description("Enter your Nextcloud server address and login credentials.")
            .build();

        let server_row = adw::EntryRow::builder()
            .title("Server URL")
            .text("https://cloud.nextcloud.com")
            .build();
        group.add(&server_row);

        let name_row = adw::EntryRow::builder()
            .title("Account Name")
            .text("Personal")
            .build();
        group.add(&name_row);

        let user_row = adw::EntryRow::builder().title("Username").build();
        group.add(&user_row);

        let pass_row = adw::ActionRow::builder()
            .title("Password / App Token")
            .build();
        let pass_entry = PasswordEntry::new();
        pass_entry.set_valign(gtk4::Align::Center);
        pass_entry.set_hexpand(true);
        pass_entry.set_show_peek_icon(true);
        pass_row.add_suffix(&pass_entry);
        group.add(&pass_row);

        page.add(&group);

        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content_box.append(&header_bar);
        content_box.append(&page);
        window.set_content(Some(&content_box));

        {
            let win = window.clone();
            cancel_btn.connect_clicked(move |_| {
                win.close();
            });
        }

        {
            let win = window.clone();
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_account_added;
            add_btn.connect_clicked(move |_| {
                let s_text = server_row.text().to_string();
                let n_text = name_row.text().to_string();
                let u_text = user_row.text().to_string();
                let p_text = pass_entry.text().to_string();

                let server_clean = s_text.trim();
                let user_clean = u_text.trim();

                if server_clean.is_empty() || user_clean.is_empty() {
                    toast.add_toast(adw::Toast::new("Server URL and username are required"));
                    return;
                }

                let account_name = if n_text.trim().is_empty() {
                    user_clean.to_string()
                } else {
                    n_text.trim().to_string()
                };

                let new_account = Account::new(
                    account_name,
                    ServerConfig::new(server_clean),
                    AccountCredentials::new(user_clean, &p_text),
                );

                let b_clone = b.clone();
                let toast_clone = toast.clone();
                let cb_clone = cb.clone();
                let win_clone = win.clone();

                glib::MainContext::default().spawn_local(async move {
                    match b_clone.add_account(new_account).await {
                        Ok(_) => {
                            toast_clone.add_toast(adw::Toast::new("Account successfully added"));
                            win_clone.close();
                            cb_clone();
                        }
                        Err(err) => {
                            toast_clone.add_toast(adw::Toast::new(&format!(
                                "Failed to add account: {err}"
                            )));
                        }
                    }
                });
            });
        }

        window.present();
    }
}
