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
        let initial_title = backend
            .cached_theming()
            .and_then(|t| t.name)
            .unwrap_or_else(|| "Nexia".to_string());
        let window_title = adw::WindowTitle::new(&initial_title, "Cloud Files");
        header_bar.set_title_widget(Some(&window_title));

        let account_switcher = AccountSwitcher::new(backend, toast_overlay, on_refresh);
        header_bar.pack_start(account_switcher.widget());

        // Populate the server title from server theming, and refresh it whenever the active
        // account changes so the bar never shows the previous account's server.
        let b = backend.clone();
        let wt = window_title.clone();
        let mut event_rx = backend.subscribe_events();
        glib::MainContext::default().spawn_local(async move {
            refresh_server_title(&b, &wt).await;
            while let Ok(event) = event_rx.recv().await {
                if matches!(
                    event,
                    nexia_core::events::DesktopEvent::AccountSwitched { .. }
                        | nexia_core::events::DesktopEvent::AccountSignedOut { .. }
                ) {
                    refresh_server_title(&b, &wt).await;
                }
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

async fn refresh_server_title<B: FileExplorerBackend>(
    backend: &B,
    window_title: &adw::WindowTitle,
) {
    let name = backend
        .server_theming()
        .await
        .and_then(|t| t.name)
        .unwrap_or_else(|| "Nexia".to_string());
    window_title.set_title(&name);
}

impl ViewComponent for MainHeaderBar {
    type RootWidget = adw::HeaderBar;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.header_bar
    }
}
