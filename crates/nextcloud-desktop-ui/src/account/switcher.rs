//! Header bar account switcher button with dropdown popover for multi-account management.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Image, Label, Orientation, Popover, Separator};
use libadwaita as adw;
use std::rc::Rc;

use crate::account::add_dialog::AddAccountDialog;
use crate::widget::FileExplorerBackend;

/// Account switcher widget placed in the top-right header bar.
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

        let hbox = GtkBox::new(Orientation::Horizontal, 6);
        hbox.set_can_target(false);

        let avatar = Image::from_icon_name("avatar-default-symbolic");
        avatar.set_can_target(false);

        let active_label = Label::new(Some("System Account"));
        active_label.set_can_target(false);

        let arrow = Image::from_icon_name("pan-down-symbolic");
        arrow.set_can_target(false);

        hbox.append(&avatar);
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

        // Initial label setup
        {
            let b = backend.clone();
            let label_init = active_label.clone();
            glib::MainContext::default().spawn_local(async move {
                let accounts = b.accounts().await;
                let active_id = b.active_account_id().await;
                if let Some(active) = accounts.iter().find(|a| Some(&a.id) == active_id.as_ref()) {
                    let display_name = if active.is_system {
                        "System Account".to_string()
                    } else {
                        active.name.clone()
                    };
                    label_init.set_label(&display_name);
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

            button.connect_clicked(move |_| {
                let b_async = b.clone();
                let toast_async = toast.clone();
                let cb_async = cb.clone();
                let pop_async = pop.clone();
                let btn_async = btn_clone.clone();
                let container = content_box.clone();
                let label_async = current_label.clone();

                glib::MainContext::default().spawn_local(async move {
                    // Clear previous items
                    while let Some(child) = container.first_child() {
                        container.remove(&child);
                    }

                    let accounts = b_async.accounts().await;
                    let active_id = b_async.active_account_id().await;

                    // Header title
                    let header_lbl = Label::new(Some("Nextcloud Accounts"));
                    header_lbl.add_css_class("heading");
                    header_lbl.set_xalign(0.0);
                    header_lbl.set_margin_bottom(4);
                    container.append(&header_lbl);

                    // Accounts list
                    for acc in accounts {
                        let is_active = Some(&acc.id) == active_id.as_ref();
                        let row_btn = Button::builder().has_frame(false).build();
                        row_btn.add_css_class("flat");

                        let row_box = GtkBox::new(Orientation::Horizontal, 8);
                        row_box.set_can_target(false);

                        let row_avatar = Image::from_icon_name("avatar-default-symbolic");
                        row_avatar.set_can_target(false);
                        row_box.append(&row_avatar);

                        let name_text = if acc.is_system {
                            format!("{} (System)", acc.name)
                        } else {
                            acc.name.clone()
                        };
                        let row_lbl = Label::new(Some(&name_text));
                        row_lbl.set_can_target(false);
                        row_lbl.set_xalign(0.0);
                        row_lbl.set_hexpand(true);
                        row_box.append(&row_lbl);

                        if is_active {
                            let check = Image::from_icon_name("emblem-ok-symbolic");
                            check.set_can_target(false);
                            check.add_css_class("success");
                            row_box.append(&check);
                            let display_label = if acc.is_system {
                                "System Account".to_string()
                            } else {
                                acc.name.clone()
                            };
                            label_async.set_label(&display_label);
                        }

                        row_btn.set_child(Some(&row_box));

                        let acc_id = acc.id.clone();
                        let pop_click = pop_async.clone();
                        let b_click = b_async.clone();
                        let cb_click = cb_async.clone();
                        let lbl_click = label_async.clone();
                        let acc_display = if acc.is_system {
                            "System Account".to_string()
                        } else {
                            acc.name.clone()
                        };

                        row_btn.connect_clicked(move |_| {
                            pop_click.popdown();
                            lbl_click.set_label(&acc_display);
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
