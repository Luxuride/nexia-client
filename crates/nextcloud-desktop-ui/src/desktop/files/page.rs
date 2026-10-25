//! Main Cloud Files page view embedding the in-page toolbar, view stack, and status bar.

use gtk4::{Box as GtkBox, Orientation, Separator, Stack};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::files::grid_view::FileGridView;
use crate::files::list_view::FileListView;
use crate::files::selection::SelectionTracker;
use crate::files::status_bar::FileStatusBar;
use crate::files::toolbar::PathToolbar;
use crate::model::{FileViewItem, FileViewMode};
use crate::widget::{FileExplorerBackend, ItemCollectionView, ViewComponent};

/// Cloud Files explorer page view with an integrated in-page toolbar, icon grid, and list view.
#[derive(Clone)]
pub struct CloudFilesPage<B: FileExplorerBackend> {
    toast_overlay: adw::ToastOverlay,
    toolbar: PathToolbar,
    grid_view: FileGridView,
    list_view: FileListView,
    status_bar: FileStatusBar,
    view_stack: Stack,
    empty_page: adw::StatusPage,
    search_query: Rc<RefCell<Option<String>>>,
    search_generation: Rc<std::cell::Cell<u64>>,
    backend: B,
}

impl<B: FileExplorerBackend> CloudFilesPage<B> {
    /// Constructs the Cloud Files explorer page view.
    pub fn new(backend: &B, on_refresh_request: Rc<dyn Fn()>) -> Self {
        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_hexpand(true);
        toast_overlay.set_vexpand(true);

        let search_query = Rc::new(RefCell::new(None));
        let search_generation = Rc::new(std::cell::Cell::new(0));

        let sq_clone = search_query.clone();
        let gen_clone = search_generation.clone();
        let cb = on_refresh_request.clone();
        let on_search = Rc::new(move |q: Option<String>| {
            let sanitized = q.and_then(|s| {
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            });
            *sq_clone.borrow_mut() = sanitized;
            gen_clone.set(gen_clone.get() + 1);
            cb();
        });

        let toolbar = PathToolbar::new(
            backend,
            &toast_overlay,
            on_refresh_request.clone(),
            on_search,
        );
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

        // Support hardware mouse forward (Button 9) and back (Button 8) buttons
        let mouse_nav = gtk4::GestureClick::new();
        mouse_nav.set_button(0);
        mouse_nav.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let b_nav = backend.clone();
        let cb_nav = on_refresh_request.clone();
        mouse_nav.connect_pressed(move |gesture, _, _, _| {
            let btn = gesture.current_button();
            if btn == 8 {
                let b = b_nav.clone();
                let cb = cb_nav.clone();
                glib::MainContext::default().spawn_local(async move {
                    if b.go_back().await {
                        cb();
                    }
                });
            } else if btn == 9 {
                let b = b_nav.clone();
                let cb = cb_nav.clone();
                glib::MainContext::default().spawn_local(async move {
                    if b.go_forward().await {
                        cb();
                    }
                });
            }
        });
        page_box.add_controller(mouse_nav);

        toast_overlay.set_child(Some(&page_box));

        Self {
            toast_overlay,
            toolbar,
            grid_view,
            list_view,
            status_bar,
            view_stack,
            empty_page,
            search_query,
            search_generation,
            backend: backend.clone(),
        }
    }

    /// Asynchronously refreshes all child views (toolbar, grid, list, and status bar).
    pub async fn refresh(&self, on_refresh_cb: Rc<dyn Fn()>) {
        self.toolbar
            .sync_state(&self.backend, on_refresh_cb.clone())
            .await;

        let active_search = self.search_query.borrow().clone();
        let current_gen = self.search_generation.get();

        let items = if let Some(ref q) = active_search {
            let trimmed = q.trim();
            if trimmed.is_empty() {
                self.backend.list_current_items().await.unwrap_or_default()
            } else {
                let res = self.backend.search_items(trimmed).await.unwrap_or_default();
                if self.search_generation.get() != current_gen {
                    tracing::debug!(query = %trimmed, "Discarding outdated search results");
                    return;
                }
                res
            }
        } else {
            self.backend.list_current_items().await.unwrap_or_default()
        };

        if self.search_generation.get() != current_gen {
            return;
        }

        self.status_bar.update_counts(&items);

        if items.is_empty() {
            if let Some(ref q) = active_search {
                let trimmed = q.trim();
                if !trimmed.is_empty() {
                    self.empty_page.set_title("No Results Found");
                    self.empty_page
                        .set_description(Some(&format!("No files match “{trimmed}”.")));
                    self.empty_page.set_icon_name(Some("edit-find-symbolic"));
                } else {
                    self.empty_page.set_title("Folder is Empty");
                    self.empty_page.set_description(Some(
                        "Use the New Folder button above or add files to this directory.",
                    ));
                    self.empty_page.set_icon_name(Some("folder-open-symbolic"));
                }
            } else {
                self.empty_page.set_title("Folder is Empty");
                self.empty_page.set_description(Some(
                    "Use the New Folder button above or add files to this directory.",
                ));
                self.empty_page.set_icon_name(Some("folder-open-symbolic"));
            }
            self.view_stack.set_visible_child_name("empty");
            return;
        }

        let mode = self.backend.view_mode().await;
        let zoom = self.backend.zoom_level().await;
        let thumb_dim = zoom.grid_icon_size() as u32;

        let thumb_candidates: Vec<String> = items
            .iter()
            .filter(|it| {
                !it.is_directory
                    && (it.icon_category == crate::model::ResourceIconCategory::Image
                        || it
                            .mime_type
                            .as_deref()
                            .is_some_and(|m| m.starts_with("image/"))
                        || it.name.ends_with(".png")
                        || it.name.ends_with(".jpg")
                        || it.name.ends_with(".jpeg")
                        || it.name.ends_with(".webp")
                        || it.name.ends_with(".gif")
                        || it.name.ends_with(".svg")
                        || it.name.ends_with(".pdf"))
            })
            .map(|it| it.path.clone())
            .collect();

        if !thumb_candidates.is_empty() {
            self.backend
                .prefetch_thumbnails(thumb_candidates, thumb_dim);
        }

        match mode {
            FileViewMode::Grid => {
                self.grid_view.populate(
                    &items,
                    &self.backend,
                    &self.toast_overlay,
                    zoom,
                    on_refresh_cb.clone(),
                );
                self.view_stack.set_visible_child_name("grid");
            }
            FileViewMode::List => {
                self.list_view.populate(
                    &items,
                    &self.backend,
                    &self.toast_overlay,
                    zoom,
                    on_refresh_cb.clone(),
                );
                self.view_stack.set_visible_child_name("list");
            }
        }
    }
}

impl<B: FileExplorerBackend> ViewComponent for CloudFilesPage<B> {
    type RootWidget = adw::ToastOverlay;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.toast_overlay
    }
}
