//! Header bar account switcher button with dropdown popover and profile picture support.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Image, Label, Orientation, Popover, Separator};
use libadwaita as adw;
use nextcloud_desktop_core::Account;
use std::rc::Rc;

use crate::account::add_dialog::AddAccountDialog;
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
        let default_avatar = make_avatar("System", None, 24);
        avatar_bin.append(&default_avatar);

        let active_label = Label::new(Some("System Account"));
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

        // Initial setup
        {
            let b = backend.clone();
            let label_init = active_label.clone();
            let avatar_bin_init = avatar_bin.clone();
            glib::MainContext::default().spawn_local(async move {
                let accounts = b.accounts().await;
                let active_id = b.active_account_id().await;
                if let Some(active) = accounts.iter().find(|a| Some(&a.id) == active_id.as_ref()) {
                    label_init.set_label(&active.name);
                    while let Some(child) = avatar_bin_init.first_child() {
                        avatar_bin_init.remove(&child);
                    }
                    let av = make_avatar(&active.name, active.avatar_path.as_deref(), 24);
                    avatar_bin_init.append(&av);
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
            let current_label = active_label;
            let current_avatar_bin = avatar_bin;

            button.connect_clicked(move |_| {
                let b_async = b.clone();
                let toast_async = toast.clone();
                let cb_async = cb.clone();
                let pop_async = pop.clone();
                let btn_async = btn_clone.clone();
                let container = content_box.clone();
                let label_async = current_label.clone();
                let avatar_bin_async = current_avatar_bin.clone();

                glib::MainContext::default().spawn_local(async move {
                    while let Some(child) = container.first_child() {
                        container.remove(&child);
                    }

                    let accounts = b_async.accounts().await;
                    let active_id = b_async.active_account_id().await;

                    // Header title
                    let header_lbl = Label::new(Some("Nextcloud Accounts"));
                    header_lbl.add_css_class("heading");
                    header_lbl.set_xalign(0.0);
                    header_lbl.set_margin_bottom(6);
                    header_lbl.set_margin_start(4);
                    container.append(&header_lbl);

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
                            let check = Image::from_icon_name("emblem-ok-symbolic");
                            check.set_can_target(false);
                            check.add_css_class("success");
                            row_box.append(&check);

                            label_async.set_label(&acc.name);
                            while let Some(child) = avatar_bin_async.first_child() {
                                avatar_bin_async.remove(&child);
                            }
                            let av = make_avatar(&acc.name, acc.avatar_path.as_deref(), 24);
                            avatar_bin_async.append(&av);
                        }

                        row_btn.set_child(Some(&row_box));

                        let acc_id = acc.id.clone();
                        let pop_click = pop_async.clone();
                        let b_click = b_async.clone();
                        let cb_click = cb_async.clone();
                        let lbl_click = label_async.clone();
                        let av_bin_click = avatar_bin_async.clone();
                        let acc_clone: Account = acc.clone();

                        row_btn.connect_clicked(move |_| {
                            pop_click.popdown();
                            lbl_click.set_label(&acc_clone.name);
                            while let Some(child) = av_bin_click.first_child() {
                                av_bin_click.remove(&child);
                            }
                            let av =
                                make_avatar(&acc_clone.name, acc_clone.avatar_path.as_deref(), 24);
                            av_bin_click.append(&av);

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

                        container.append(&row_btn);
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
