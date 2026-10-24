//! Nautilus-style (GNOME Files) file browser view with breadcrumb path bar, icon grid, list view, and context menus.

#[cfg(feature = "gtk")]
use gtk4::{
    Align, Box as GtkBox, Button, Entry, FlowBox, FlowBoxChild, GestureClick, Image, Justification,
    Label, ListBox, ListBoxRow, MenuButton, Orientation, Overlay, PolicyType, Popover,
    ScrolledWindow, SelectionMode, Separator, Stack, ToggleButton, pango,
};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use std::cell::RefCell;
#[cfg(feature = "gtk")]
use std::rc::Rc;

#[cfg(feature = "gtk")]
use crate::controller::DesktopUiController;
#[cfg(feature = "gtk")]
use crate::model::{FileSyncStatus, FileViewItem, FileViewMode};

/// Shared widgets composing the Nautilus-style Cloud Files browser and header bar controls.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct NautilusBrowser {
    /// Root content widget wrapped in an `adw::ToastOverlay`.
    pub toast_overlay: adw::ToastOverlay,
    /// Back button in the header bar.
    pub back_btn: Button,
    /// Forward button in the header bar.
    pub forward_btn: Button,
    /// Interactive breadcrumb box placed in the header bar title area.
    pub breadcrumb_box: GtkBox,
    /// Grid view toggle button in the header bar.
    pub grid_toggle: ToggleButton,
    /// List view toggle button in the header bar.
    pub list_toggle: ToggleButton,
    /// New folder menu button in the header bar.
    pub new_folder_btn: MenuButton,
    /// Stack switching between Grid (`"grid"`), List (`"list"`), and Empty (`"empty"`) states.
    pub view_stack: Stack,
    /// Nautilus icon grid (`FlowBox`).
    pub flow_box: FlowBox,
    /// Nautilus list view (`ListBox`).
    pub list_box: ListBox,
    /// Bottom status bar label showing item counts and selection details.
    pub status_label: Label,
    /// Currently rendered items in the active directory (indexed by child position).
    pub current_items: Rc<RefCell<Vec<FileViewItem>>>,
}

/// Builds the Nautilus-style Cloud Files browser and its associated header bar controls.
#[cfg(feature = "gtk")]
pub fn build_nautilus_browser(controller: &DesktopUiController) -> NautilusBrowser {
    let toast_overlay = adw::ToastOverlay::new();

    // Header bar navigation buttons (Back / Forward linked like Nautilus)
    let back_btn = Button::from_icon_name("go-previous-symbolic");
    back_btn.set_tooltip_text(Some("Back"));
    back_btn.set_sensitive(false);

    let forward_btn = Button::from_icon_name("go-next-symbolic");
    forward_btn.set_tooltip_text(Some("Forward"));
    forward_btn.set_sensitive(false);

    // Interactive Nautilus breadcrumb path bar
    let breadcrumb_box = GtkBox::new(Orientation::Horizontal, 2);
    breadcrumb_box.add_css_class("linked");
    breadcrumb_box.set_valign(Align::Center);

    // View mode switcher (Grid vs List)
    let grid_toggle = ToggleButton::new();
    grid_toggle.set_icon_name("view-grid-symbolic");
    grid_toggle.set_tooltip_text(Some("Grid View"));
    grid_toggle.set_active(true);

    let list_toggle = ToggleButton::new();
    list_toggle.set_icon_name("view-list-symbolic");
    list_toggle.set_tooltip_text(Some("List View"));
    list_toggle.set_group(Some(&grid_toggle));

    // New Folder popover button
    let new_folder_btn = MenuButton::builder()
        .icon_name("folder-new-symbolic")
        .tooltip_text("New Folder")
        .build();

    let new_folder_popover = Popover::new();
    let popover_box = GtkBox::new(Orientation::Vertical, 8);
    popover_box.set_margin_top(12);
    popover_box.set_margin_bottom(12);
    popover_box.set_margin_start(12);
    popover_box.set_margin_end(12);

    let popover_title = Label::new(Some("Folder Name"));
    popover_title.set_halign(Align::Start);
    popover_title.add_css_class("heading");

    let folder_entry = Entry::new();
    folder_entry.set_placeholder_text(Some("New Folder"));
    folder_entry.set_width_chars(22);

    let create_btn = Button::with_label("Create");
    create_btn.add_css_class("suggested-action");
    create_btn.set_halign(Align::End);

    popover_box.append(&popover_title);
    popover_box.append(&folder_entry);
    popover_box.append(&create_btn);
    new_folder_popover.set_child(Some(&popover_box));
    new_folder_btn.set_popover(Some(&new_folder_popover));

    // Main vertical layout: View Stack + Bottom Status Bar
    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.set_hexpand(true);
    root_box.set_vexpand(true);

    let view_stack = Stack::new();
    view_stack.set_hexpand(true);
    view_stack.set_vexpand(true);

    // 1. Nautilus Icon Grid View (FlowBox)
    let flow_box = FlowBox::new();
    flow_box.set_selection_mode(SelectionMode::Single);
    flow_box.set_activate_on_single_click(false); // Single-click selects, double-click opens/hydrates
    flow_box.set_homogeneous(true);
    flow_box.set_max_children_per_line(10);
    flow_box.set_min_children_per_line(2);
    flow_box.set_column_spacing(12);
    flow_box.set_row_spacing(12);
    flow_box.set_margin_top(18);
    flow_box.set_margin_bottom(18);
    flow_box.set_margin_start(18);
    flow_box.set_margin_end(18);
    flow_box.set_valign(Align::Start);

    let grid_scroll = ScrolledWindow::new();
    grid_scroll.set_hscrollbar_policy(PolicyType::Never);
    grid_scroll.set_vexpand(true);
    grid_scroll.set_child(Some(&flow_box));

    // 2. Nautilus Detailed List View
    let list_container = GtkBox::new(Orientation::Vertical, 0);
    list_container.set_vexpand(true);

    // Column Header Bar
    let col_header = GtkBox::new(Orientation::Horizontal, 12);
    col_header.set_margin_top(10);
    col_header.set_margin_bottom(8);
    col_header.set_margin_start(24);
    col_header.set_margin_end(24);

    let col_name = Label::new(Some("Name"));
    col_name.set_hexpand(true);
    col_name.set_halign(Align::Start);
    col_name.add_css_class("caption-heading");
    col_name.add_css_class("dim-label");

    let col_status = Label::new(Some("Sync Status"));
    col_status.set_width_chars(18);
    col_status.set_halign(Align::Start);
    col_status.add_css_class("caption-heading");
    col_status.add_css_class("dim-label");

    let col_size = Label::new(Some("Size"));
    col_size.set_width_chars(10);
    col_size.set_halign(Align::End);
    col_size.add_css_class("caption-heading");
    col_size.add_css_class("dim-label");

    let col_actions = Label::new(Some("Actions"));
    col_actions.set_width_chars(10);
    col_actions.set_halign(Align::End);
    col_actions.add_css_class("caption-heading");
    col_actions.add_css_class("dim-label");

    col_header.append(&col_name);
    col_header.append(&col_status);
    col_header.append(&col_size);
    col_header.append(&col_actions);
    list_container.append(&col_header);
    list_container.append(&Separator::new(Orientation::Horizontal));

    let list_box = ListBox::new();
    list_box.set_selection_mode(SelectionMode::Single);
    list_box.set_activate_on_single_click(false);
    list_box.set_margin_top(12);
    list_box.set_margin_bottom(18);
    list_box.set_margin_start(18);
    list_box.set_margin_end(18);
    list_box.add_css_class("boxed-list");

    let list_scroll = ScrolledWindow::new();
    list_scroll.set_hscrollbar_policy(PolicyType::Never);
    list_scroll.set_vexpand(true);
    list_scroll.set_child(Some(&list_box));
    list_container.append(&list_scroll);

    // 3. Empty Folder Status Page
    let empty_status = adw::StatusPage::builder()
        .icon_name("folder-open-symbolic")
        .title("Folder is Empty")
        .description("Use the New Folder button in the header bar or sync files from Nextcloud.")
        .vexpand(true)
        .build();

    view_stack.add_named(&grid_scroll, Some("grid"));
    view_stack.add_named(&list_container, Some("list"));
    view_stack.add_named(&empty_status, Some("empty"));
    view_stack.set_visible_child_name("grid");

    root_box.append(&view_stack);
    root_box.append(&Separator::new(Orientation::Horizontal));

    // Bottom Nautilus status bar
    let status_bar = GtkBox::new(Orientation::Horizontal, 12);
    status_bar.set_margin_top(6);
    status_bar.set_margin_bottom(6);
    status_bar.set_margin_start(18);
    status_bar.set_margin_end(18);

    let status_label = Label::new(Some("Ready"));
    status_label.set_halign(Align::Start);
    status_label.set_hexpand(true);
    status_label.add_css_class("caption");
    status_label.add_css_class("dim-label");

    let hint_label = Label::new(Some(
        "Double-click to open or hydrate · Right-click for actions",
    ));
    hint_label.set_halign(Align::End);
    hint_label.add_css_class("caption");
    hint_label.add_css_class("dim-label");

    status_bar.append(&status_label);
    status_bar.append(&hint_label);
    root_box.append(&status_bar);

    toast_overlay.set_child(Some(&root_box));

    let browser = NautilusBrowser {
        toast_overlay,
        back_btn,
        forward_btn,
        breadcrumb_box,
        grid_toggle,
        list_toggle,
        new_folder_btn,
        view_stack,
        flow_box,
        list_box,
        status_label,
        current_items: Rc::new(RefCell::new(Vec::new())),
    };

    wire_nautilus_interactions(
        &browser,
        controller,
        &folder_entry,
        &create_btn,
        &new_folder_popover,
    );
    browser
}

#[cfg(feature = "gtk")]
fn wire_nautilus_interactions(
    browser: &NautilusBrowser,
    controller: &DesktopUiController,
    folder_entry: &Entry,
    create_btn: &Button,
    new_folder_popover: &Popover,
) {
    // Back button
    {
        let ctrl = controller.clone();
        let b = browser.clone();
        browser.back_btn.connect_clicked(move |_| {
            let ctrl = ctrl.clone();
            let b = b.clone();
            glib::MainContext::default().spawn_local(async move {
                if ctrl.go_back().await {
                    refresh_nautilus_view(&b, &ctrl).await;
                }
            });
        });
    }

    // Forward button
    {
        let ctrl = controller.clone();
        let b = browser.clone();
        browser.forward_btn.connect_clicked(move |_| {
            let ctrl = ctrl.clone();
            let b = b.clone();
            glib::MainContext::default().spawn_local(async move {
                if ctrl.go_forward().await {
                    refresh_nautilus_view(&b, &ctrl).await;
                }
            });
        });
    }

    // Grid / List toggle
    {
        let ctrl = controller.clone();
        let b = browser.clone();
        browser.grid_toggle.connect_toggled(move |btn| {
            let mode = if btn.is_active() {
                FileViewMode::Grid
            } else {
                FileViewMode::List
            };
            let ctrl = ctrl.clone();
            let b = b.clone();
            glib::MainContext::default().spawn_local(async move {
                ctrl.set_view_mode(mode).await;
                refresh_nautilus_view(&b, &ctrl).await;
            });
        });
    }

    // Create New Folder popover submission
    {
        let ctrl = controller.clone();
        let b = browser.clone();
        let entry = folder_entry.clone();
        let pop = new_folder_popover.clone();
        create_btn.connect_clicked(move |_| {
            let name = entry.text().to_string();
            entry.set_text("");
            pop.popdown();
            let ctrl = ctrl.clone();
            let b = b.clone();
            glib::MainContext::default().spawn_local(async move {
                match ctrl.create_folder(&name).await {
                    Ok(_) => {
                        let toast = adw::Toast::new(&format!("Created folder “{name}”"));
                        b.toast_overlay.add_toast(toast);
                        refresh_nautilus_view(&b, &ctrl).await;
                    }
                    Err(err) => {
                        let toast = adw::Toast::new(&err.to_string());
                        b.toast_overlay.add_toast(toast);
                    }
                }
            });
        });
    }

    // Grid View single-click selection -> update status bar
    {
        let b = browser.clone();
        browser
            .flow_box
            .connect_selected_children_changed(move |fb| {
                if let Some(child) = fb.selected_children().first() {
                    let idx = child.index() as usize;
                    if let Some(item) = b.current_items.borrow().get(idx) {
                        b.status_label.set_text(&format!(
                            "“{}” selected — {} ({})",
                            item.name,
                            item.size_formatted,
                            item.status.display_label()
                        ));
                    }
                }
            });
    }

    // Grid View double-click / Enter activation -> open folder or hydrate file
    {
        let ctrl = controller.clone();
        let b = browser.clone();
        browser.flow_box.connect_child_activated(move |_, child| {
            let idx = child.index() as usize;
            let maybe_item = b.current_items.borrow().get(idx).cloned();
            if let Some(item) = maybe_item {
                let ctrl = ctrl.clone();
                let b = b.clone();
                glib::MainContext::default().spawn_local(async move {
                    activate_item(&b, &ctrl, &item).await;
                });
            }
        });
    }

    // List View single-click selection -> update status bar
    {
        let b = browser.clone();
        browser.list_box.connect_row_selected(move |_, maybe_row| {
            if let Some(row) = maybe_row {
                let idx = row.index() as usize;
                if let Some(item) = b.current_items.borrow().get(idx) {
                    b.status_label.set_text(&format!(
                        "“{}” selected — {} ({})",
                        item.name,
                        item.size_formatted,
                        item.status.display_label()
                    ));
                }
            }
        });
    }

    // List View double-click / Enter activation -> open folder or hydrate file
    {
        let ctrl = controller.clone();
        let b = browser.clone();
        browser.list_box.connect_row_activated(move |_, row| {
            let idx = row.index() as usize;
            let maybe_item = b.current_items.borrow().get(idx).cloned();
            if let Some(item) = maybe_item {
                let ctrl = ctrl.clone();
                let b = b.clone();
                glib::MainContext::default().spawn_local(async move {
                    activate_item(&b, &ctrl, &item).await;
                });
            }
        });
    }
}

#[cfg(feature = "gtk")]
async fn activate_item(
    browser: &NautilusBrowser,
    controller: &DesktopUiController,
    item: &FileViewItem,
) {
    if item.is_directory {
        controller.navigate_into(&item.name).await;
        refresh_nautilus_view(browser, controller).await;
    } else if item.status == FileSyncStatus::CloudPlaceholder {
        match controller.hydrate_file(&item.path).await {
            Ok(cached_path) => {
                let toast = adw::Toast::new(&format!(
                    "Hydrated “{}” ({})",
                    item.name,
                    cached_path.display()
                ));
                browser.toast_overlay.add_toast(toast);
                refresh_nautilus_view(browser, controller).await;
            }
            Err(err) => {
                browser
                    .toast_overlay
                    .add_toast(adw::Toast::new(&err.to_string()));
            }
        }
    } else {
        let toast = adw::Toast::new(&format!(
            "“{}” is available offline ({})",
            item.name, item.size_formatted
        ));
        browser.toast_overlay.add_toast(toast);
    }
}

/// Re-renders the breadcrumb path bar, navigation buttons, and active Grid/List view.
#[cfg(feature = "gtk")]
pub async fn refresh_nautilus_view(browser: &NautilusBrowser, controller: &DesktopUiController) {
    let cur_path = controller.current_path().await;
    browser
        .back_btn
        .set_sensitive(controller.can_go_back().await);
    browser
        .forward_btn
        .set_sensitive(controller.can_go_forward().await);

    // 1. Rebuild Nautilus Breadcrumb Path Bar
    while let Some(child) = browser.breadcrumb_box.first_child() {
        browser.breadcrumb_box.remove(&child);
    }

    let segments = FileViewItem::breadcrumb_segments(&cur_path);
    let last_idx = segments.len().saturating_sub(1);

    for (idx, seg) in segments.into_iter().enumerate() {
        let btn = Button::new();
        if seg.is_root {
            let btn_box = GtkBox::new(Orientation::Horizontal, 6);
            let icon = Image::from_icon_name("folder-remote-symbolic");
            let lbl = Label::new(Some(&seg.name));
            if idx == last_idx {
                lbl.add_css_class("heading");
            }
            btn_box.append(&icon);
            btn_box.append(&lbl);
            btn.set_child(Some(&btn_box));
        } else {
            let lbl = Label::new(Some(&seg.name));
            if idx == last_idx {
                lbl.add_css_class("heading");
            }
            btn.set_child(Some(&lbl));
        }

        let target = seg.full_path;
        let ctrl = controller.clone();
        let b = browser.clone();
        btn.connect_clicked(move |_| {
            let ctrl = ctrl.clone();
            let b = b.clone();
            let target = target.clone();
            glib::MainContext::default().spawn_local(async move {
                ctrl.set_current_path(&target).await;
                refresh_nautilus_view(&b, &ctrl).await;
            });
        });

        browser.breadcrumb_box.append(&btn);
    }

    // 2. Fetch directory children
    let items = controller.list_current_items().await.unwrap_or_default();
    let folder_count = items.iter().filter(|i| i.is_directory).count();
    let file_count = items.len().saturating_sub(folder_count);
    browser.status_label.set_text(&format!(
        "{} items ({} folders, {} files)",
        items.len(),
        folder_count,
        file_count
    ));

    *browser.current_items.borrow_mut() = items.clone();

    while let Some(child) = browser.flow_box.first_child() {
        browser.flow_box.remove(&child);
    }
    while let Some(child) = browser.list_box.first_child() {
        browser.list_box.remove(&child);
    }

    if items.is_empty() {
        browser.view_stack.set_visible_child_name("empty");
        return;
    }

    let mode = controller.view_mode().await;
    match mode {
        FileViewMode::Grid => browser.view_stack.set_visible_child_name("grid"),
        FileViewMode::List => browser.view_stack.set_visible_child_name("list"),
    }

    for item in &items {
        let tile = build_grid_tile(browser, controller, item);
        browser.flow_box.insert(&tile, -1);

        let row = build_list_row(browser, controller, item);
        browser.list_box.append(&row);
    }
}

#[cfg(feature = "gtk")]
fn build_grid_tile(
    browser: &NautilusBrowser,
    controller: &DesktopUiController,
    item: &FileViewItem,
) -> FlowBoxChild {
    let child = FlowBoxChild::new();
    let card = GtkBox::new(Orientation::Vertical, 6);
    card.set_width_request(116);
    card.set_margin_top(10);
    card.set_margin_bottom(10);
    card.set_margin_start(8);
    card.set_margin_end(8);
    card.set_halign(Align::Center);

    // 64px Nautilus full-color icon with status badge overlay
    let overlay = Overlay::new();
    overlay.set_halign(Align::Center);

    let main_icon = Image::from_icon_name(item.nautilus_icon_name());
    main_icon.set_pixel_size(64);
    overlay.set_child(Some(&main_icon));

    if !item.is_directory {
        let badge = Image::from_icon_name(item.status.icon_name());
        badge.set_pixel_size(16);
        badge.set_halign(Align::End);
        badge.set_valign(Align::End);
        badge.add_css_class(item.status.css_class());
        badge.set_tooltip_text(Some(item.status.display_label()));
        overlay.add_overlay(&badge);
    }

    let name_label = Label::new(Some(&item.name));
    name_label.set_wrap(true);
    name_label.set_wrap_mode(pango::WrapMode::WordChar);
    name_label.set_justify(Justification::Center);
    name_label.set_max_width_chars(14);
    name_label.set_lines(2);
    name_label.set_ellipsize(pango::EllipsizeMode::End);

    let sub_label = Label::new(Some(&item.size_formatted));
    sub_label.add_css_class("caption");
    sub_label.add_css_class("dim-label");

    card.append(&overlay);
    card.append(&name_label);
    card.append(&sub_label);

    attach_context_popover(&card, browser, controller, item);

    child.set_child(Some(&card));
    child
}

#[cfg(feature = "gtk")]
fn build_list_row(
    browser: &NautilusBrowser,
    controller: &DesktopUiController,
    item: &FileViewItem,
) -> ListBoxRow {
    let row = ListBoxRow::new();
    let hbox = GtkBox::new(Orientation::Horizontal, 12);
    hbox.set_margin_top(8);
    hbox.set_margin_bottom(8);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);

    let icon = Image::from_icon_name(item.nautilus_icon_name());
    icon.set_pixel_size(32);

    let name_label = Label::new(Some(&item.name));
    name_label.set_hexpand(true);
    name_label.set_halign(Align::Start);
    name_label.set_ellipsize(pango::EllipsizeMode::End);

    let status_box = GtkBox::new(Orientation::Horizontal, 6);
    status_box.set_width_request(140);
    let status_icon = Image::from_icon_name(item.status.icon_name());
    status_icon.add_css_class(item.status.css_class());
    let status_text = if item.is_directory {
        "Folder"
    } else {
        item.status.display_label()
    };
    let status_lbl = Label::new(Some(status_text));
    status_lbl.add_css_class("caption");
    status_lbl.add_css_class(item.status.css_class());
    status_box.append(&status_icon);
    status_box.append(&status_lbl);

    let size_lbl = Label::new(Some(&item.size_formatted));
    size_lbl.set_width_chars(10);
    size_lbl.set_halign(Align::End);
    size_lbl.add_css_class("dim-label");

    let actions_box = GtkBox::new(Orientation::Horizontal, 4);
    actions_box.set_halign(Align::End);

    if item.is_directory {
        let open_btn = Button::from_icon_name("go-next-symbolic");
        open_btn.add_css_class("flat");
        open_btn.set_tooltip_text(Some("Open Folder"));
        let ctrl = controller.clone();
        let b = browser.clone();
        let folder_name = item.name.clone();
        open_btn.connect_clicked(move |_| {
            let ctrl = ctrl.clone();
            let b = b.clone();
            let name = folder_name.clone();
            glib::MainContext::default().spawn_local(async move {
                ctrl.navigate_into(&name).await;
                refresh_nautilus_view(&b, &ctrl).await;
            });
        });
        actions_box.append(&open_btn);
    } else if item.status == FileSyncStatus::CloudPlaceholder {
        let dl_btn = Button::from_icon_name("folder-download-symbolic");
        dl_btn.add_css_class("flat");
        dl_btn.set_tooltip_text(Some("Make Available Offline (Hydrate)"));
        let ctrl = controller.clone();
        let b = browser.clone();
        let path = item.path.clone();
        let name = item.name.clone();
        dl_btn.connect_clicked(move |_| {
            let ctrl = ctrl.clone();
            let b = b.clone();
            let p = path.clone();
            let n = name.clone();
            glib::MainContext::default().spawn_local(async move {
                if ctrl.hydrate_file(&p).await.is_ok() {
                    b.toast_overlay
                        .add_toast(adw::Toast::new(&format!("Hydrated “{n}”")));
                    refresh_nautilus_view(&b, &ctrl).await;
                }
            });
        });
        actions_box.append(&dl_btn);
    } else {
        let free_btn = Button::from_icon_name("weather-overcast-symbolic");
        free_btn.add_css_class("flat");
        free_btn.set_tooltip_text(Some("Free Up Local Space (Dehydrate)"));
        let ctrl = controller.clone();
        let b = browser.clone();
        let path = item.path.clone();
        let name = item.name.clone();
        free_btn.connect_clicked(move |_| {
            let ctrl = ctrl.clone();
            let b = b.clone();
            let p = path.clone();
            let n = name.clone();
            glib::MainContext::default().spawn_local(async move {
                if ctrl.dehydrate_file(&p).await.is_ok() {
                    b.toast_overlay
                        .add_toast(adw::Toast::new(&format!("Dehydrated “{n}” to cloud")));
                    refresh_nautilus_view(&b, &ctrl).await;
                }
            });
        });
        actions_box.append(&free_btn);
    }

    hbox.append(&icon);
    hbox.append(&name_label);
    hbox.append(&status_box);
    hbox.append(&size_lbl);
    hbox.append(&actions_box);

    attach_context_popover(&hbox, browser, controller, item);

    row.set_child(Some(&hbox));
    row
}

#[cfg(feature = "gtk")]
fn attach_context_popover(
    target_widget: &GtkBox,
    browser: &NautilusBrowser,
    controller: &DesktopUiController,
    item: &FileViewItem,
) {
    let popover = Popover::new();
    popover.set_parent(target_widget);
    popover.set_has_arrow(true);

    let menu_box = GtkBox::new(Orientation::Vertical, 4);
    menu_box.set_margin_top(6);
    menu_box.set_margin_bottom(6);
    menu_box.set_margin_start(6);
    menu_box.set_margin_end(6);

    let primary_label = if item.is_directory {
        "Open Folder"
    } else if item.status == FileSyncStatus::CloudPlaceholder {
        "Make Available Offline"
    } else {
        "Free Up Local Space"
    };

    let primary_btn = Button::with_label(primary_label);
    primary_btn.add_css_class("flat");
    {
        let pop = popover.clone();
        let ctrl = controller.clone();
        let b = browser.clone();
        let item_clone = item.clone();
        primary_btn.connect_clicked(move |_| {
            pop.popdown();
            let ctrl = ctrl.clone();
            let b = b.clone();
            let item = item_clone.clone();
            glib::MainContext::default().spawn_local(async move {
                if item.is_directory {
                    ctrl.navigate_into(&item.name).await;
                } else if item.status == FileSyncStatus::CloudPlaceholder {
                    if ctrl.hydrate_file(&item.path).await.is_ok() {
                        b.toast_overlay.add_toast(adw::Toast::new(&format!(
                            "“{}” is now available offline",
                            item.name
                        )));
                    }
                } else if ctrl.dehydrate_file(&item.path).await.is_ok() {
                    b.toast_overlay.add_toast(adw::Toast::new(&format!(
                        "Freed local cache space for “{}”",
                        item.name
                    )));
                }
                refresh_nautilus_view(&b, &ctrl).await;
            });
        });
    }

    let delete_btn = Button::with_label("Delete");
    delete_btn.add_css_class("flat");
    delete_btn.add_css_class("destructive-action");
    {
        let pop = popover.clone();
        let ctrl = controller.clone();
        let b = browser.clone();
        let target_path = item.path.clone();
        let target_name = item.name.clone();
        delete_btn.connect_clicked(move |_| {
            pop.popdown();
            let ctrl = ctrl.clone();
            let b = b.clone();
            let path = target_path.clone();
            let name = target_name.clone();
            glib::MainContext::default().spawn_local(async move {
                if ctrl.delete_item(&path).await.is_ok() {
                    b.toast_overlay
                        .add_toast(adw::Toast::new(&format!("Deleted “{name}”")));
                    refresh_nautilus_view(&b, &ctrl).await;
                }
            });
        });
    }

    menu_box.append(&primary_btn);
    menu_box.append(&Separator::new(Orientation::Horizontal));
    menu_box.append(&delete_btn);
    popover.set_child(Some(&menu_box));

    let gesture = GestureClick::new();
    gesture.set_button(3); // Secondary (right) mouse button
    let pop_for_click = popover;
    gesture.connect_pressed(move |_, _, _, _| {
        pop_for_click.popup();
    });
    target_widget.add_controller(gesture);
}
