//! Account and application settings page component.

#[cfg(feature = "gtk")]
use gtk4::Image;
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;

#[cfg(feature = "gtk")]
use crate::widget::{FileExplorerBackend, ViewComponent};

/// Preferences page displaying Nextcloud account security and local VFS cache configuration.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct SettingsPage {
    page: adw::PreferencesPage,
}

#[cfg(feature = "gtk")]
impl SettingsPage {
    /// Creates a new settings page bound to `backend`.
    pub fn new<B: FileExplorerBackend>(backend: &B) -> Self {
        let page = adw::PreferencesPage::new();

        let account_group = adw::PreferencesGroup::builder()
            .title("Nextcloud Account")
            .description("Connection profile and credential security settings.")
            .build();

        let security_row = adw::ActionRow::builder()
            .title("Credential Redaction")
            .subtitle(
                "App tokens and passwords are automatically redacted from logs and diagnostics",
            )
            .build();
        security_row.add_prefix(&Image::from_icon_name("network-server-symbolic"));
        account_group.add(&security_row);

        let storage_group = adw::PreferencesGroup::builder()
            .title("Virtual File System Cache")
            .description("Local disk storage used for hydrated files.")
            .build();

        let cache_dir_display = backend.cache_dir_display();
        let safe_cache = glib::markup_escape_text(&cache_dir_display);
        let cache_row = adw::ActionRow::builder()
            .title("Hydration Cache Directory")
            .subtitle(safe_cache.as_str())
            .build();
        cache_row.add_prefix(&Image::from_icon_name("drive-harddisk-symbolic"));
        storage_group.add(&cache_row);

        page.add(&account_group);
        page.add(&storage_group);

        Self { page }
    }
}

#[cfg(feature = "gtk")]
impl ViewComponent for SettingsPage {
    type RootWidget = adw::PreferencesPage;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.page
    }
}
