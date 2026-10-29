//! Header bar account switcher button with dropdown popover and profile picture support.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Image, Label, Orientation, Popover, Separator};
use libadwaita as adw;
use nexia_core::events::DesktopEvent;
use std::rc::Rc;

use crate::account::add_dialog::AddAccountDialog;
use crate::account::sign_out_dialog::SignOutAccountDialog;
use crate::widget::FileExplorerBackend;

/// Helper constructing an [`adw::Avatar`] displaying either a user's photo or their initials.
fn make_avatar(name: &str, avatar_path: Option<&std::path::Path>, size: i32) -> adw::Avatar {
    let avatar = adw::Avatar::new(size, Some(name), true);
    if let Some(path) = avatar_path
        && path.exists()
    {
        let file = gtk4::gio::File::for_path(path);
        if let Ok(texture) = gtk4::gdk::Texture::from_file(&file) {
            avatar.set_custom_image(Some(&texture));
        }
    }
    avatar
}

/// Account switcher widget placed in the top-left of the application header bar.
#[derive(Clone)]
pub struct AccountSwitcher {
    button: Button,
}

/// Button text shown while no account profile exists.
const NO_ACCOUNT_LABEL: &str = "No Account";

/// Rebinds the switcher button to the currently active account's name and avatar.
async fn refresh_active_account<B: FileExplorerBackend>(
    backend: &B,
    button: &Button,
    label: &Label,
    avatar_bin: &GtkBox,
) {
    let accounts = backend.accounts().await;
    let active_id = backend.active_account_id().await;
    let active = accounts
        .into_iter()
        .find(|a| Some(&a.id) == active_id.as_ref());
    let (name, avatar_path) = match active {
        Some(acc) => (acc.name, acc.avatar_path),
        None => (NO_ACCOUNT_LABEL.to_string(), None),
    };
    label.set_label(&name);
    button.set_tooltip_text(Some(&format!("Nexia accounts — {name}")));
    while let Some(child) = avatar_bin.first_child() {
        avatar_bin.remove(&child);
    }
    avatar_bin.append(&make_avatar(&name, avatar_path.as_deref(), 24));
}

impl AccountSwitcher {
    /// Constructs a new account switcher wired to the backend and UI refresh callbacks.
    pub fn new<B: FileExplorerBackend>(
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) -> Self {
        let button = Button::builder().has_frame(false).build();
        button.add_css_class("flat");

        let hbox = GtkBox::new(Orientation::Horizontal, 8);
        hbox.set_can_target(false);

        let avatar_bin = GtkBox::new(Orientation::Horizontal, 0);
        avatar_bin.set_can_target(false);
        let default_avatar = make_avatar(NO_ACCOUNT_LABEL, None, 24);
        avatar_bin.append(&default_avatar);

        let active_label = Label::new(Some(NO_ACCOUNT_LABEL));
        active_label.set_can_target(false);

        let arrow = Image::from_icon_name("pan-down-symbolic");
        arrow.set_can_target(false);

        hbox.append(&avatar_bin);
        hbox.append(&active_label);
        hbox.append(&arrow);
        button.set_child(Some(&hbox));

        let popover = Popover::new();
        popover.set_has_arrow(true);
        popover.set_parent(&button);

        let popover_content = GtkBox::new(Orientation::Vertical, 4);
        popover_content.set_margin_top(8);
        popover_content.set_margin_bottom(8);
        popover_content.set_margin_start(8);
        popover_content.set_margin_end(8);
        popover.set_child(Some(&popover_content));

        // Keep the button label and avatar in sync with the active account, including switches
        // triggered outside the popover (sign-out, add-dialog, tray actions).
        {
            let b = backend.clone();
            let button_init = button.clone();
            let label_init = active_label.clone();
            let avatar_bin_init = avatar_bin.clone();
            let mut event_rx = backend.subscribe_events();
            glib::MainContext::default().spawn_local(async move {
                refresh_active_account(&b, &button_init, &label_init, &avatar_bin_init).await;
                while let Ok(event) = event_rx.recv().await {
                    if matches!(
                        event,
                        DesktopEvent::AccountSwitched { .. }
                            | DesktopEvent::AccountSignedOut { .. }
                    ) {
                        refresh_active_account(&b, &button_init, &label_init, &avatar_bin_init)
                            .await;
                    }
                }
            });
        }

        // On button click, rebuild menu content and popup
        {
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_refresh;
            let pop = popover.clone();
            let btn_clone = button.clone();
            let content_box = popover_content;

            button.connect_clicked(move |_| {
                let b_async = b.clone();
                let toast_async = toast.clone();
                let cb_async = cb.clone();
                let pop_async = pop.clone();
                let btn_async = btn_clone.clone();
                let container = content_box.clone();

                glib::MainContext::default().spawn_local(async move {
                    while let Some(child) = container.first_child() {
                        container.remove(&child);
                    }

                    let accounts = b_async.accounts().await;
                    let active_id = b_async.active_account_id().await;

                    // Header title
                    let header_lbl = Label::new(Some("Nexia Accounts"));
                    header_lbl.add_css_class("heading");
                    header_lbl.set_xalign(0.0);
                    header_lbl.set_margin_bottom(6);
                    header_lbl.set_margin_start(4);
                    container.append(&header_lbl);

                    if accounts.is_empty() {
                        let empty_lbl = Label::new(Some(
                            "No accounts yet. Sign in to a Nextcloud server to get started.",
                        ));
                        empty_lbl.set_wrap(true);
                        empty_lbl.set_xalign(0.0);
                        empty_lbl.set_margin_bottom(6);
                        empty_lbl.add_css_class("dim-label");
                        empty_lbl.add_css_class("caption");
                        container.append(&empty_lbl);
                    }

                    // Accounts list
                    for acc in accounts {
                        let is_active = Some(&acc.id) == active_id.as_ref();
                        let row_btn = Button::builder().has_frame(false).build();
                        row_btn.add_css_class("flat");

                        let row_box = GtkBox::new(Orientation::Horizontal, 10);
                        row_box.set_can_target(false);
                        row_box.set_margin_top(4);
                        row_box.set_margin_bottom(4);

                        // Profile picture or initials avatar
                        let row_avatar = make_avatar(&acc.name, acc.avatar_path.as_deref(), 32);
                        row_avatar.set_can_target(false);
                        row_box.append(&row_avatar);

                        // Text details (name + username/server)
                        let text_box = GtkBox::new(Orientation::Vertical, 2);
                        text_box.set_can_target(false);
                        text_box.set_hexpand(true);

                        let name_row = GtkBox::new(Orientation::Horizontal, 6);
                        name_row.set_can_target(false);
                        let row_name = Label::new(Some(&acc.name));
                        row_name.set_can_target(false);
                        row_name.set_xalign(0.0);
                        name_row.append(&row_name);

                        if acc.is_system {
                            let sys_badge = Label::new(Some("(System)"));
                            sys_badge.set_can_target(false);
                            sys_badge.add_css_class("dim-label");
                            sys_badge.add_css_class("caption");
                            name_row.append(&sys_badge);
                        }
                        text_box.append(&name_row);

                        let sub_text = format!(
                            "{} • {}",
                            acc.credentials.username,
                            acc.server
                                .server_url
                                .trim_start_matches("https://")
                                .trim_start_matches("http://")
                        );
                        let sub_lbl = Label::new(Some(&sub_text));
                        sub_lbl.set_can_target(false);
                        sub_lbl.set_xalign(0.0);
                        sub_lbl.add_css_class("dim-label");
                        sub_lbl.add_css_class("caption");
                        text_box.append(&sub_lbl);

                        row_box.append(&text_box);

                        if is_active {
                            let check = Image::from_icon_name("object-select-symbolic");
                            check.set_can_target(false);
                            check.add_css_class("success");
                            row_box.append(&check);
                        }

                        row_btn.set_child(Some(&row_box));
                        row_btn.set_hexpand(true);

                        let acc_id = acc.id.clone();
                        let pop_click = pop_async.clone();
                        let b_click = b_async.clone();
                        let cb_click = cb_async.clone();

                        row_btn.connect_clicked(move |_| {
                            pop_click.popdown();

                            if !is_active {
                                let b = b_click.clone();
                                let cb = cb_click.clone();
                                let id = acc_id.clone();
                                glib::MainContext::default().spawn_local(async move {
                                    let _ = b.switch_account(&id).await;
                                    cb();
                                });
                            }
                        });

                        let item_container = GtkBox::new(Orientation::Horizontal, 4);
                        item_container.append(&row_btn);

                        if !acc.is_system {
                            let sign_out_btn = Button::from_icon_name("system-log-out-symbolic");
                            sign_out_btn.add_css_class("flat");
                            sign_out_btn.set_valign(gtk4::Align::Center);
                            sign_out_btn.set_tooltip_text(Some("Sign out of account"));
                            let b_so = b_async.clone();
                            let toast_so = toast_async.clone();
                            let pop_so = pop_async.clone();
                            let acc_so = acc.clone();
                            let cb_so = cb_async.clone();
                            let parent_so = btn_async.clone();

                            sign_out_btn.connect_clicked(move |_| {
                                pop_so.popdown();
                                let cb = cb_so.clone();
                                SignOutAccountDialog::present(
                                    Some(&parent_so),
                                    &b_so,
                                    &toast_so,
                                    acc_so.clone(),
                                    move || cb(),
                                );
                            });
                            item_container.append(&sign_out_btn);
                        }

                        container.append(&item_container);
                    }

                    container.append(&Separator::new(Orientation::Horizontal));

                    // Add Account button
                    let add_btn = Button::builder().has_frame(false).build();
                    add_btn.add_css_class("flat");

                    let add_box = GtkBox::new(Orientation::Horizontal, 8);
                    add_box.set_can_target(false);
                    let add_icon = Image::from_icon_name("list-add-symbolic");
                    add_icon.set_can_target(false);
                    let add_lbl = Label::new(Some("Add Account…"));
                    add_lbl.set_can_target(false);
                    add_lbl.set_xalign(0.0);
                    add_box.append(&add_icon);
                    add_box.append(&add_lbl);
                    add_btn.set_child(Some(&add_box));

                    let pop_add = pop_async.clone();
                    let b_add = b_async.clone();
                    let toast_add = toast_async.clone();
                    let cb_add = cb_async.clone();
                    let parent_add = btn_async.clone();

                    add_btn.connect_clicked(move |_| {
                        pop_add.popdown();
                        AddAccountDialog::present(
                            Some(&parent_add),
                            &b_add,
                            &toast_add,
                            cb_add.clone(),
                        );
                    });

                    container.append(&add_btn);
                    pop_async.popup();
                });
            });
        }

        Self { button }
    }

    /// Returns a reference to the root button widget.
    pub fn widget(&self) -> &Button {
        &self.button
    }
}
