//! Sort options popover menu allowing users to configure file ordering.

use gtk4::{Align, Box as GtkBox, CheckButton, Label, MenuButton, Orientation, Popover, Separator};
use libadwaita::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

use crate::model::{FileSortCriteria, FileSortField, FileSortOrder};
use crate::widget::FileExplorerBackend;

/// Popover button providing options to sort items by Name, Size, Date, or Type.
#[derive(Clone)]
pub struct SortPopover {
    menu_button: MenuButton,
    name_btn: CheckButton,
    size_btn: CheckButton,
    date_btn: CheckButton,
    type_btn: CheckButton,
    asc_btn: CheckButton,
    folders_first_btn: CheckButton,
    syncing: Rc<Cell<bool>>,
}

impl SortPopover {
    /// Constructs a new sort popover wired to `backend` and callback.
    pub fn new<B: FileExplorerBackend>(backend: &B, on_change: Rc<dyn Fn()>) -> Self {
        let menu_button = MenuButton::new();
        menu_button.set_icon_name("view-sort-ascending-symbolic");
        menu_button.set_tooltip_text(Some("Sort options"));

        let popover = Popover::new();
        let content_box = GtkBox::new(Orientation::Vertical, 6);
        content_box.set_margin_top(8);
        content_box.set_margin_bottom(8);
        content_box.set_margin_start(10);
        content_box.set_margin_end(10);

        // 1. Sort Field section
        let field_label = Label::new(Some("Sort by"));
        field_label.set_halign(Align::Start);
        field_label.add_css_class("caption-heading");
        field_label.add_css_class("dim-label");
        content_box.append(&field_label);

        let name_btn = CheckButton::with_label("Name");
        name_btn.set_active(true);
        let size_btn = CheckButton::with_label("Size");
        size_btn.set_group(Some(&name_btn));
        let date_btn = CheckButton::with_label("Date Modified");
        date_btn.set_group(Some(&name_btn));
        let type_btn = CheckButton::with_label("Type");
        type_btn.set_group(Some(&name_btn));

        content_box.append(&name_btn);
        content_box.append(&size_btn);
        content_box.append(&date_btn);
        content_box.append(&type_btn);

        content_box.append(&Separator::new(Orientation::Horizontal));

        // 2. Sort Direction section
        let dir_label = Label::new(Some("Order"));
        dir_label.set_halign(Align::Start);
        dir_label.add_css_class("caption-heading");
        dir_label.add_css_class("dim-label");
        content_box.append(&dir_label);

        let asc_btn = CheckButton::with_label("Ascending (A to Z)");
        asc_btn.set_active(true);
        let desc_btn = CheckButton::with_label("Descending (Z to A)");
        desc_btn.set_group(Some(&asc_btn));

        content_box.append(&asc_btn);
        content_box.append(&desc_btn);

        content_box.append(&Separator::new(Orientation::Horizontal));

        // 3. Folders first option
        let folders_first_btn = CheckButton::with_label("Folders First");
        folders_first_btn.set_active(true);
        content_box.append(&folders_first_btn);

        // Helper closure to apply criteria
        let b = backend.clone();
        let cb = on_change;
        let menu_btn_icon = menu_button.clone();
        let name_c = name_btn.clone();
        let size_c = size_btn.clone();
        let date_c = date_btn.clone();
        let asc_c = asc_btn.clone();
        let folders_c = folders_first_btn.clone();

        let syncing: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let syncing_apply = syncing.clone();

        let apply_sort = Rc::new(move || {
            if syncing_apply.get() {
                return;
            }
            let field = if size_c.is_active() {
                FileSortField::Size
            } else if date_c.is_active() {
                FileSortField::Modified
            } else if name_c.is_active() {
                FileSortField::Name
            } else {
                FileSortField::Type
            };

            let order = if asc_c.is_active() {
                FileSortOrder::Ascending
            } else {
                FileSortOrder::Descending
            };

            let folders_first = folders_c.is_active();

            if order == FileSortOrder::Ascending {
                menu_btn_icon.set_icon_name("view-sort-ascending-symbolic");
            } else {
                menu_btn_icon.set_icon_name("view-sort-descending-symbolic");
            }

            let criteria = FileSortCriteria::new(field, order, folders_first);
            let b = b.clone();
            let cb = cb.clone();
            glib::MainContext::default().spawn_local(async move {
                b.set_sort_criteria(criteria).await;
                cb();
            });
        });

        // Wire toggles
        let apply1 = apply_sort.clone();
        name_btn.connect_toggled(move |btn| {
            if btn.is_active() {
                apply1();
            }
        });
        let apply2 = apply_sort.clone();
        size_btn.connect_toggled(move |btn| {
            if btn.is_active() {
                apply2();
            }
        });
        let apply3 = apply_sort.clone();
        date_btn.connect_toggled(move |btn| {
            if btn.is_active() {
                apply3();
            }
        });
        let apply4 = apply_sort.clone();
        type_btn.connect_toggled(move |btn| {
            if btn.is_active() {
                apply4();
            }
        });
        let apply5 = apply_sort.clone();
        asc_btn.connect_toggled(move |btn| {
            if btn.is_active() {
                apply5();
            }
        });
        let apply6 = apply_sort.clone();
        desc_btn.connect_toggled(move |btn| {
            if btn.is_active() {
                apply6();
            }
        });
        let apply7 = apply_sort;
        folders_first_btn.connect_toggled(move |_| {
            apply7();
        });

        popover.set_child(Some(&content_box));
        menu_button.set_popover(Some(&popover));

        Self {
            menu_button,
            name_btn,
            size_btn,
            date_btn,
            type_btn,
            asc_btn,
            folders_first_btn,
            syncing,
        }
    }

    /// Reflects the backend's active sort criteria without persisting it back.
    pub fn sync_state<B: FileExplorerBackend>(&self, backend: &B) {
        let b = backend.clone();
        let name_btn = self.name_btn.clone();
        let size_btn = self.size_btn.clone();
        let date_btn = self.date_btn.clone();
        let type_btn = self.type_btn.clone();
        let asc_btn = self.asc_btn.clone();
        let folders_first_btn = self.folders_first_btn.clone();
        let syncing = self.syncing.clone();
        let menu_btn_icon = self.menu_button.clone();
        glib::MainContext::default().spawn_local(async move {
            let criteria = b.sort_criteria().await;
            syncing.set(true);
            match criteria.field {
                FileSortField::Size => size_btn.set_active(true),
                FileSortField::Modified => date_btn.set_active(true),
                FileSortField::Type => type_btn.set_active(true),
                FileSortField::Name => name_btn.set_active(true),
            }
            asc_btn.set_active(criteria.order == FileSortOrder::Ascending);
            folders_first_btn.set_active(criteria.folders_first);
            menu_btn_icon.set_icon_name(match criteria.order {
                FileSortOrder::Ascending => "view-sort-ascending-symbolic",
                FileSortOrder::Descending => "view-sort-descending-symbolic",
            });
            syncing.set(false);
        });
    }

    /// Access the underlying [`MenuButton`] widget.
    pub fn widget(&self) -> &MenuButton {
        &self.menu_button
    }
}
