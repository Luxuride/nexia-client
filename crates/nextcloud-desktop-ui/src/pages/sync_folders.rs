//! Two-way local folder synchronization page component.

use gtk4::{Align, Button, Image};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::widget::{FileExplorerBackend, ViewComponent};

/// Preferences page configuring local folder synchronization pairings.
#[derive(Clone)]
pub struct SyncFoldersPage {
    page: adw::PreferencesPage,
}

impl SyncFoldersPage {
    /// Creates a new synchronization folders page bound to `backend`.
    pub fn new<B: FileExplorerBackend>(backend: &B) -> Self {
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

        let backend_clone = backend.clone();
        run_sync_btn.connect_clicked(move |_| {
            let b = backend_clone.clone();
            glib::MainContext::default().spawn_local(async move {
                let _ = b.trigger_sync().await;
            });
        });

        sync_row.add_suffix(&run_sync_btn);
        group.add(&sync_row);
        page.add(&group);

        Self { page }
    }
}

impl ViewComponent for SyncFoldersPage {
    type RootWidget = adw::PreferencesPage;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.page
    }
}
