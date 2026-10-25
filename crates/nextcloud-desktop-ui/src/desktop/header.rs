//! Static top-level window header bar component.

use gtk4::prelude::*;
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
        let initial_title = backend
            .cached_theming()
            .and_then(|t| t.name)
            .unwrap_or_else(|| "Nextcloud".to_string());
        let window_title = adw::WindowTitle::new(&initial_title, "Cloud Files");
        header_bar.set_title_widget(Some(&window_title));

        let account_switcher = AccountSwitcher::new(backend, toast_overlay, on_refresh.clone());
        header_bar.pack_start(account_switcher.widget());

        // Top-right window header "..." menu (opens the same canvas context menu)
        let more_btn = gtk4::MenuButton::new();
        more_btn.set_icon_name("view-more-symbolic");
        more_btn.set_tooltip_text(Some("Cloud Files actions and settings"));
        let menu_popover = crate::files::canvas_menu::CanvasContextMenu::create_menu_popover(
            &more_btn,
            backend,
            toast_overlay,
            on_refresh,
        );
        more_btn.set_popover(Some(&menu_popover));
        header_bar.pack_end(&more_btn);

        // Asynchronously populate server title from server theming if available
        let b = backend.clone();
        let wt = window_title.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Some(theme) = b.server_theming().await
                && let Some(name) = theme.name()
            {
                wt.set_title(name);
            }
        });

        Self {
            header_bar,
            window_title,
        }
    }

    /// Updates the server title displayed in the center [`adw::WindowTitle`].
    pub fn set_server_name(&self, name: &str) {
        self.window_title.set_title(name);
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
