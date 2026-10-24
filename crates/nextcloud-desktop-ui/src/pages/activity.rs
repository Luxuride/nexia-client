//! Transfers and hydration activity page component.

#[cfg(feature = "gtk")]
use gtk4::Image;
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;

#[cfg(feature = "gtk")]
use crate::widget::ViewComponent;

/// Preferences page displaying recent file transfers and virtual hydration status.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct ActivityPage {
    page: adw::PreferencesPage,
}

#[cfg(feature = "gtk")]
impl ActivityPage {
    /// Creates a new activity and transfers page.
    pub fn new() -> Self {
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

        Self { page }
    }
}

#[cfg(feature = "gtk")]
impl Default for ActivityPage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "gtk")]
impl ViewComponent for ActivityPage {
    type RootWidget = adw::PreferencesPage;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.page
    }
}
