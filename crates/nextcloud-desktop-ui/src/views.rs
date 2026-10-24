//! GTK 4 and Libadwaita HIG-compliant sidebar and preference page builders.

#[cfg(feature = "gtk")]
use gtk4::{Align, Box as GtkBox, Button, Image, Label, ListBoxRow, Orientation};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;

#[cfg(feature = "gtk")]
use crate::controller::DesktopUiController;

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
        .title("Transfers and Hydration Activity")
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
    let safe_cache = glib::markup_escape_text(&cache_dir_display);
    let cache_row = adw::ActionRow::builder()
        .title("Hydration Cache Directory")
        .subtitle(safe_cache.as_str())
        .build();
    cache_row.add_prefix(&Image::from_icon_name("drive-harddisk-symbolic"));
    storage_group.add(&cache_row);

    page.add(&account_group);
    page.add(&storage_group);
    page
}
