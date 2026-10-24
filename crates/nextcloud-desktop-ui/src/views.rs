//! GTK 4 and Libadwaita HIG-compliant page builders and row widgets.

#[cfg(feature = "gtk")]
use gtk4::{
    Align, Box as GtkBox, Button, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    ScrolledWindow, SelectionMode, Stack,
};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;

#[cfg(feature = "gtk")]
use crate::controller::DesktopUiController;
#[cfg(feature = "gtk")]
use crate::model::FileSyncStatus;

/// Widgets composing the Virtual Cloud Files browser view.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct CloudFilesView {
    /// Root container added to the content stack.
    pub root: GtkBox,
    /// Up navigation button.
    pub up_btn: Button,
    /// Current virtual directory path label.
    pub path_label: Label,
    /// Stack switching between empty status page and boxed file list.
    pub view_stack: Stack,
    /// Boxed list containing file and folder rows.
    pub file_list: ListBox,
}

/// Builds a GNOME HIG navigation sidebar row with a symbolic icon and title label.
#[cfg(feature = "gtk")]
pub fn build_sidebar_row(icon_name: &str, title: &str) -> ListBoxRow {
    let row = ListBoxRow::new();
    let hbox = GtkBox::new(Orientation::Horizontal, 12);
    hbox.set_margin_top(8);
    hbox.set_margin_bottom(8);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);

    let icon = Image::from_icon_name(icon_name);
    let label = Label::new(Some(title));
    label.set_halign(Align::Start);
    label.set_hexpand(true);

    hbox.append(&icon);
    hbox.append(&label);
    row.set_child(Some(&hbox));
    row
}

/// Builds the Cloud Files (Virtual Drive) browser view following GNOME HIG spacing.
#[cfg(feature = "gtk")]
pub fn build_cloud_files_view(controller: &DesktopUiController) -> CloudFilesView {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);

    // Path bar inside a horizontal clamp
    let path_clamp = adw::Clamp::new();
    path_clamp.set_maximum_size(860);
    path_clamp.set_tightening_threshold(600);
    path_clamp.set_margin_top(18);
    path_clamp.set_margin_bottom(6);
    path_clamp.set_margin_start(24);
    path_clamp.set_margin_end(24);

    let path_bar = GtkBox::new(Orientation::Horizontal, 8);
    let up_btn = Button::from_icon_name("go-up-symbolic");
    up_btn.set_tooltip_text(Some("Parent Folder"));
    up_btn.add_css_class("flat");

    let path_label = Label::new(Some("/"));
    path_label.set_hexpand(true);
    path_label.set_halign(Align::Start);
    path_label.add_css_class("heading");

    path_bar.append(&up_btn);
    path_bar.append(&path_label);
    path_clamp.set_child(Some(&path_bar));
    root.append(&path_clamp);

    // View stack: empty state vs boxed list
    let view_stack = Stack::new();
    view_stack.set_vexpand(true);
    view_stack.set_hexpand(true);

    let empty_status = adw::StatusPage::builder()
        .icon_name("folder-remote-symbolic")
        .title("Virtual Drive Ready")
        .description(
            "Refresh the virtual drive to browse your Nextcloud files on-demand without consuming local disk space.",
        )
        .vexpand(true)
        .build();

    let refresh_empty_btn = Button::with_label("Refresh Virtual Drive");
    refresh_empty_btn.add_css_class("suggested-action");
    refresh_empty_btn.add_css_class("pill");
    refresh_empty_btn.set_halign(Align::Center);
    empty_status.set_child(Some(&refresh_empty_btn));

    let file_list = ListBox::new();
    file_list.set_selection_mode(SelectionMode::None);
    file_list.add_css_class("boxed-list");

    let list_clamp = adw::Clamp::new();
    list_clamp.set_maximum_size(860);
    list_clamp.set_tightening_threshold(600);
    list_clamp.set_margin_top(12);
    list_clamp.set_margin_bottom(24);
    list_clamp.set_margin_start(24);
    list_clamp.set_margin_end(24);
    list_clamp.set_child(Some(&file_list));

    let scrolled = ScrolledWindow::new();
    scrolled.set_hscrollbar_policy(PolicyType::Never);
    scrolled.set_vexpand(true);
    scrolled.set_child(Some(&list_clamp));

    view_stack.add_named(&empty_status, Some("empty"));
    view_stack.add_named(&scrolled, Some("list"));
    view_stack.set_visible_child_name("empty");

    let ctrl = controller.clone();
    let fl = file_list.clone();
    let pl = path_label.clone();
    let vs = view_stack.clone();
    refresh_empty_btn.connect_clicked(move |_| {
        let c = ctrl.clone();
        let fl = fl.clone();
        let pl = pl.clone();
        let vs = vs.clone();
        glib::MainContext::default().spawn_local(async move {
            let _ = c.refresh_virtual_tree().await;
            populate_file_list(&c, &fl, &pl, &vs).await;
        });
    });

    root.append(&view_stack);

    CloudFilesView {
        root,
        up_btn,
        path_label,
        view_stack,
        file_list,
    }
}

/// Populates the virtual file browser list using `adw::ActionRow` items.
#[cfg(feature = "gtk")]
pub async fn populate_file_list(
    controller: &DesktopUiController,
    list_box: &ListBox,
    path_label: &Label,
    view_stack: &Stack,
) {
    let cur_path = controller.current_path().await;
    path_label.set_text(&cur_path);

    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let Ok(items) = controller.list_current_items().await else {
        view_stack.set_visible_child_name("empty");
        return;
    };

    if items.is_empty() {
        view_stack.set_visible_child_name("empty");
        return;
    }

    view_stack.set_visible_child_name("list");

    for item in items {
        let subtitle = if item.is_directory {
            "Folder".to_string()
        } else {
            format!("{} · {}", item.size_formatted, item.status.display_label())
        };

        let row = adw::ActionRow::builder()
            .title(&item.name)
            .subtitle(&subtitle)
            .build();

        let item_icon_name = if item.is_directory {
            "folder-symbolic"
        } else {
            "text-x-generic-symbolic"
        };
        let prefix_icon = Image::from_icon_name(item_icon_name);
        row.add_prefix(&prefix_icon);

        let status_icon = Image::from_icon_name(item.status.icon_name());
        status_icon.add_css_class(item.status.css_class());
        status_icon.set_tooltip_text(Some(item.status.display_label()));
        row.add_suffix(&status_icon);

        if item.is_directory {
            let open_btn = Button::from_icon_name("go-next-symbolic");
            open_btn.add_css_class("flat");
            open_btn.set_valign(Align::Center);
            open_btn.set_tooltip_text(Some("Open Folder"));

            let ctrl = controller.clone();
            let folder_name = item.name.clone();
            let fl = list_box.clone();
            let pl = path_label.clone();
            let vs = view_stack.clone();
            open_btn.connect_clicked(move |_| {
                let c = ctrl.clone();
                let name = folder_name.clone();
                let fl = fl.clone();
                let pl = pl.clone();
                let vs = vs.clone();
                glib::MainContext::default().spawn_local(async move {
                    c.navigate_into(&name).await;
                    populate_file_list(&c, &fl, &pl, &vs).await;
                });
            });
            row.add_suffix(&open_btn);
        } else if item.status == FileSyncStatus::CloudPlaceholder {
            let hydrate_btn = Button::from_icon_name("folder-download-symbolic");
            hydrate_btn.add_css_class("flat");
            hydrate_btn.set_valign(Align::Center);
            hydrate_btn.set_tooltip_text(Some("Download for Offline Access"));

            let ctrl = controller.clone();
            let target_path = item.path.clone();
            let fl = list_box.clone();
            let pl = path_label.clone();
            let vs = view_stack.clone();
            hydrate_btn.connect_clicked(move |_| {
                let c = ctrl.clone();
                let p = target_path.clone();
                let fl = fl.clone();
                let pl = pl.clone();
                let vs = vs.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _ = c.hydrate_file(&p).await;
                    populate_file_list(&c, &fl, &pl, &vs).await;
                });
            });
            row.add_suffix(&hydrate_btn);
        } else {
            let dehydrate_btn = Button::from_icon_name("user-trash-symbolic");
            dehydrate_btn.add_css_class("flat");
            dehydrate_btn.set_valign(Align::Center);
            dehydrate_btn.set_tooltip_text(Some("Free Local Space (Keep on Cloud)"));

            let ctrl = controller.clone();
            let target_path = item.path.clone();
            let fl = list_box.clone();
            let pl = path_label.clone();
            let vs = view_stack.clone();
            dehydrate_btn.connect_clicked(move |_| {
                let c = ctrl.clone();
                let p = target_path.clone();
                let fl = fl.clone();
                let pl = pl.clone();
                let vs = vs.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _ = c.dehydrate_file(&p).await;
                    populate_file_list(&c, &fl, &pl, &vs).await;
                });
            });
            row.add_suffix(&dehydrate_btn);
        }

        list_box.append(&row);
    }
}

/// Builds the Local Sync Folders page using `adw::PreferencesPage`.
#[cfg(feature = "gtk")]
pub fn build_sync_folders_page(controller: &DesktopUiController) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();

    let group = adw::PreferencesGroup::builder()
        .title("Local Folder Synchronization")
        .description(
            "Two-way synchronization keeps selected local folders reconciled with remote Nextcloud WebDAV directories.",
        )
        .build();

    let sync_row = adw::ActionRow::builder()
        .title("Bidirectional Folder Sync")
        .subtitle("Reconciles local modifications, remote ETag updates, and sync journal state")
        .build();
    sync_row.add_prefix(&Image::from_icon_name("emblem-synchronizing-symbolic"));

    let run_sync_btn = Button::with_label("Sync Now");
    run_sync_btn.set_valign(Align::Center);
    run_sync_btn.add_css_class("suggested-action");

    let ctrl = controller.clone();
    run_sync_btn.connect_clicked(move |_| {
        let c = ctrl.clone();
        glib::MainContext::default().spawn_local(async move {
            let _ = c.trigger_sync().await;
        });
    });

    sync_row.add_suffix(&run_sync_btn);
    group.add(&sync_row);
    page.add(&group);
    page
}

/// Builds the Transfers & Activity page using `adw::PreferencesPage`.
#[cfg(feature = "gtk")]
pub fn build_transfers_page() -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();

    let group = adw::PreferencesGroup::builder()
        .title("Transfers & Hydration Activity")
        .description("Recent uploads, downloads, and on-demand virtual file hydration events.")
        .build();

    let hydration_row = adw::ActionRow::builder()
        .title("On-Demand Hydration Engine")
        .subtitle("Files are fetched via WebDAV GET only when opened or explicitly requested")
        .build();
    hydration_row.add_prefix(&Image::from_icon_name("network-transmit-receive-symbolic"));

    let badge = Image::from_icon_name("emblem-ok-symbolic");
    badge.add_css_class("success");
    hydration_row.add_suffix(&badge);

    group.add(&hydration_row);
    page.add(&group);
    page
}

/// Builds the Settings page using `adw::PreferencesPage`.
#[cfg(feature = "gtk")]
pub fn build_settings_page(controller: &DesktopUiController) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();

    let account_group = adw::PreferencesGroup::builder()
        .title("Nextcloud Account")
        .description("Connection profile and credential security settings.")
        .build();

    let security_row = adw::ActionRow::builder()
        .title("Credential Redaction")
        .subtitle("App tokens and passwords are automatically redacted from logs and diagnostics")
        .build();
    security_row.add_prefix(&Image::from_icon_name("network-server-symbolic"));
    account_group.add(&security_row);

    let storage_group = adw::PreferencesGroup::builder()
        .title("Virtual File System Cache")
        .description("Local disk storage used for hydrated files.")
        .build();

    let cache_dir_display = controller
        .coordinator()
        .vfs()
        .cache_dir()
        .display()
        .to_string();
    let cache_row = adw::ActionRow::builder()
        .title("Hydration Cache Directory")
        .subtitle(&cache_dir_display)
        .build();
    cache_row.add_prefix(&Image::from_icon_name("drive-harddisk-symbolic"));
    storage_group.add(&cache_row);

    page.add(&account_group);
    page.add(&storage_group);
    page
}
