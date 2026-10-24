//! Main Cloud Files page view embedding the in-page toolbar, view stack, and status bar.

#[cfg(feature = "gtk")]
use gtk4::{Box as GtkBox, Orientation, Separator, Stack};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use std::rc::Rc;

#[cfg(feature = "gtk")]
use crate::files::grid_view::FileGridView;
#[cfg(feature = "gtk")]
use crate::files::list_view::FileListView;
#[cfg(feature = "gtk")]
use crate::files::selection::SelectionTracker;
#[cfg(feature = "gtk")]
use crate::files::status_bar::FileStatusBar;
#[cfg(feature = "gtk")]
use crate::files::toolbar::PathToolbar;
#[cfg(feature = "gtk")]
use crate::model::{FileViewItem, FileViewMode};
#[cfg(feature = "gtk")]
use crate::widget::{FileExplorerBackend, ItemCollectionView, ViewComponent};

/// Cloud Files explorer page view with an integrated in-page toolbar, icon grid, and list view.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct CloudFilesPage<B: FileExplorerBackend> {
    toast_overlay: adw::ToastOverlay,
    toolbar: PathToolbar,
    grid_view: FileGridView,
    list_view: FileListView,
    status_bar: FileStatusBar,
    view_stack: Stack,
    backend: B,
}

#[cfg(feature = "gtk")]
impl<B: FileExplorerBackend> CloudFilesPage<B> {
    /// Constructs the Cloud Files explorer page view.
    pub fn new(backend: &B, on_refresh_request: Rc<dyn Fn()>) -> Self {
        let toast_overlay = adw::ToastOverlay::new();

        let toolbar = PathToolbar::new(backend, &toast_overlay, on_refresh_request.clone());
        let status_bar = FileStatusBar::new();

        let status_bar_for_sel = status_bar.clone();
        let selection_tracker = SelectionTracker::new(Rc::new(move |items: Vec<FileViewItem>| {
            status_bar_for_sel.update_multi_selection(&items);
        }));

        let grid_view = FileGridView::new(
            backend,
            &toast_overlay,
            selection_tracker.clone(),
            on_refresh_request.clone(),
        );

        let list_view = FileListView::new(
            backend,
            &toast_overlay,
            selection_tracker,
            on_refresh_request.clone(),
        );

        let empty_page = adw::StatusPage::builder()
            .icon_name("folder-open-symbolic")
            .title("Folder is Empty")
            .description("Use the New Folder button above or add files to this directory.")
            .vexpand(true)
            .build();

        let view_stack = Stack::new();
        view_stack.set_vexpand(true);
        view_stack.set_hexpand(true);
        view_stack.add_named(grid_view.root_widget(), Some("grid"));
        view_stack.add_named(list_view.root_widget(), Some("list"));
        view_stack.add_named(&empty_page, Some("empty"));
        view_stack.set_visible_child_name("grid");

        let page_box = GtkBox::new(Orientation::Vertical, 0);
        page_box.set_vexpand(true);
        page_box.set_hexpand(true);

        // 1. In-page top panel toolbar
        page_box.append(toolbar.root_widget());
        page_box.append(&Separator::new(Orientation::Horizontal));

        // 2. Main content view stack
        page_box.append(&view_stack);
        page_box.append(&Separator::new(Orientation::Horizontal));

        // 3. Bottom status bar
        page_box.append(status_bar.root_widget());

        toast_overlay.set_child(Some(&page_box));

        Self {
            toast_overlay,
            toolbar,
            grid_view,
            list_view,
            status_bar,
            view_stack,
            backend: backend.clone(),
        }
    }

    /// Asynchronously refreshes all child views (toolbar, grid, list, and status bar).
    pub async fn refresh(&self, on_refresh_cb: Rc<dyn Fn()>) {
        self.toolbar
            .sync_state(&self.backend, on_refresh_cb.clone())
            .await;

        let items = self.backend.list_current_items().await.unwrap_or_default();
        self.status_bar.update_counts(&items);

        if items.is_empty() {
            self.view_stack.set_visible_child_name("empty");
            return;
        }

        let mode = self.backend.view_mode().await;
        match mode {
            FileViewMode::Grid => {
                self.grid_view.populate(
                    &items,
                    &self.backend,
                    &self.toast_overlay,
                    on_refresh_cb.clone(),
                );
                self.view_stack.set_visible_child_name("grid");
            }
            FileViewMode::List => {
                self.list_view.populate(
                    &items,
                    &self.backend,
                    &self.toast_overlay,
                    on_refresh_cb.clone(),
                );
                self.view_stack.set_visible_child_name("list");
            }
        }
    }
}

#[cfg(feature = "gtk")]
impl<B: FileExplorerBackend> ViewComponent for CloudFilesPage<B> {
    type RootWidget = adw::ToastOverlay;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.toast_overlay
    }
}
