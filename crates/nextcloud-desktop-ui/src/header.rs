//! Static top-level window header bar component.

use libadwaita as adw;
use std::rc::Rc;

use crate::account::AccountSwitcher;
use crate::widget::{FileExplorerBackend, ViewComponent};

/// Top-level window header bar whose structure remains constant across all navigation pages.
#[derive(Clone)]
pub struct MainHeaderBar {
    header_bar: adw::HeaderBar,
    window_title: adw::WindowTitle,
}

impl MainHeaderBar {
    /// Constructs the main application header bar wired to the backend and refresh callback.
    pub fn new<B: FileExplorerBackend>(
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) -> Self {
        let header_bar = adw::HeaderBar::new();
        let window_title = adw::WindowTitle::new("Nextcloud", "Cloud Files");
        header_bar.set_title_widget(Some(&window_title));

        let account_switcher = AccountSwitcher::new(backend, toast_overlay, on_refresh);
        header_bar.pack_start(account_switcher.widget());

        Self {
            header_bar,
            window_title,
        }
    }

    /// Updates the subtitle displayed in the center [`adw::WindowTitle`] without altering the header bar layout.
    pub fn set_page_subtitle(&self, subtitle: &str) {
        self.window_title.set_subtitle(subtitle);
    }
}

impl ViewComponent for MainHeaderBar {
    type RootWidget = adw::HeaderBar;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.header_bar
    }
}
