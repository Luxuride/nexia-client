//! Static top-level window header bar component.

use gtk4::Button;
use libadwaita as adw;
use libadwaita::prelude::*;
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

        let refresh_btn = Button::from_icon_name("view-refresh-symbolic");
        refresh_btn.set_tooltip_text(Some("Refresh Virtual Drive"));

        let sync_btn = Button::from_icon_name("emblem-synchronizing-symbolic");
        sync_btn.set_tooltip_text(Some("Sync All Folders"));

        let account_switcher = AccountSwitcher::new(backend, toast_overlay, on_refresh.clone());

        header_bar.pack_end(account_switcher.widget());
        header_bar.pack_end(&sync_btn);
        header_bar.pack_end(&refresh_btn);

        {
            let backend_clone = backend.clone();
            let refresh_cb = on_refresh;
            refresh_btn.connect_clicked(move |_| {
                let b = backend_clone.clone();
                let cb = refresh_cb.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _ = b.refresh_virtual_tree().await;
                    cb();
                });
            });
        }

        {
            let backend_clone = backend.clone();
            sync_btn.connect_clicked(move |_| {
                let b = backend_clone.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _ = b.trigger_sync().await;
                });
            });
        }

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
