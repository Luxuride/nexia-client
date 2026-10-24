//! Nautilus-style icon grid view using GTK 4 `FlowBox` with multi-selection, hover suggestions, and F2 renaming.

#[cfg(feature = "gtk")]
use gtk4::{
    Align, Box as GtkBox, EventControllerKey, FlowBox, FlowBoxChild, Image, Justification, Label,
    Orientation, PolicyType, ScrolledWindow, SelectionMode, gdk, pango,
};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use std::cell::RefCell;
#[cfg(feature = "gtk")]
use std::rc::Rc;

#[cfg(feature = "gtk")]
use crate::files::context_menu::ItemContextMenu;
#[cfg(feature = "gtk")]
use crate::files::rename_dialog::RenameDialog;
#[cfg(feature = "gtk")]
use crate::files::selection::SelectionTracker;
#[cfg(feature = "gtk")]
use crate::model::FileViewItem;
#[cfg(feature = "gtk")]
use crate::widget::{FileExplorerBackend, ItemCollectionView, ViewComponent};

/// Icon grid representation of the active virtual directory.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct FileGridView {
    scroll: ScrolledWindow,
    flow_box: FlowBox,
    items: Rc<RefCell<Vec<FileViewItem>>>,
    selection_tracker: SelectionTracker,
}

#[cfg(feature = "gtk")]
impl FileGridView {
    /// Creates a new icon grid view wired to `backend` and callbacks.
    pub fn new<B: FileExplorerBackend>(
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        selection_tracker: SelectionTracker,
        on_refresh: Rc<dyn Fn()>,
    ) -> Self {
        let flow_box = FlowBox::new();
        flow_box.set_selection_mode(SelectionMode::Multiple);
        flow_box.set_activate_on_single_click(false);
        flow_box.set_homogeneous(true);
        flow_box.set_max_children_per_line(8);
        flow_box.set_min_children_per_line(2);
        flow_box.set_column_spacing(12);
        flow_box.set_row_spacing(12);
        flow_box.set_margin_top(16);
        flow_box.set_margin_bottom(16);
        flow_box.set_margin_start(16);
        flow_box.set_margin_end(16);
        flow_box.set_halign(Align::Start);
        flow_box.set_valign(Align::Start);

        let items = Rc::new(RefCell::new(Vec::new()));

        // Wire selection change (Ctrl+click, Shift+click, drag-select)
        let items_for_sel = items.clone();
        let tracker_for_sel = selection_tracker.clone();
        flow_box.connect_selected_children_changed(move |fb| {
            let selected_children = fb.selected_children();
            let all = items_for_sel.borrow();
            let selected: Vec<FileViewItem> = selected_children
                .into_iter()
                .filter_map(|c| all.get(c.index() as usize).cloned())
                .collect();
            tracker_for_sel.set_selected(selected);
        });

        // Wire double-click activation EXACTLY ONCE here in constructor
        let b_act = backend.clone();
        let toast_act = toast_overlay.clone();
        let cb_act = on_refresh.clone();
        let items_for_act = items.clone();
        flow_box.connect_child_activated(move |_, child| {
            let idx = child.index() as usize;
            if let Some(item) = items_for_act.borrow().get(idx).cloned() {
                let b = b_act.clone();
                let toast = toast_act.clone();
                let cb = cb_act.clone();
                glib::MainContext::default().spawn_local(async move {
                    if item.is_directory {
                        // Use canonical absolute path to guarantee idempotent navigation
                        b.set_current_path(&item.path).await;
                        cb();
                    } else {
                        match b.open_file(&item.path).await {
                            Ok(path) => {
                                toast.add_toast(adw::Toast::new(&format!(
                                    "Opened “{}” ({})",
                                    item.name,
                                    path.display()
                                )));
                            }
                            Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                        }
                    }
                });
            }
        });

        let scroll = ScrolledWindow::new();
        scroll.set_hscrollbar_policy(PolicyType::Never);
        scroll.set_vexpand(true);
        scroll.set_hexpand(true);
        scroll.set_child(Some(&flow_box));

        // Wire empty canvas deselection and drag selection
        selection_tracker.attach_flow_box_interactions(&scroll, &flow_box, items.clone());

        // Wire F2 key shortcut to present Nautilus RenameDialog
        let key_controller = EventControllerKey::new();
        let tracker_key = selection_tracker.clone();
        let b_key = backend.clone();
        let toast_key = toast_overlay.clone();
        let cb_key = on_refresh;
        let scroll_for_key = scroll.clone();
        key_controller.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::F2 {
                let selected = tracker_key.selected();
                if let Some(first) = selected.first() {
                    RenameDialog::present(
                        Some(&scroll_for_key),
                        first,
                        &b_key,
                        &toast_key,
                        cb_key.clone(),
                    );
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        scroll.add_controller(key_controller);

        Self {
            scroll,
            flow_box,
            items,
            selection_tracker,
        }
    }

    /// Returns a reference to the active selection tracker.
    pub fn selection_tracker(&self) -> &SelectionTracker {
        &self.selection_tracker
    }

    /// Removes all rendered children from the grid.
    pub fn clear(&self) {
        while let Some(child) = self.flow_box.first_child() {
            self.flow_box.remove(&child);
        }
        self.items.borrow_mut().clear();
    }
}

#[cfg(feature = "gtk")]
impl ViewComponent for FileGridView {
    type RootWidget = ScrolledWindow;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.scroll
    }
}

#[cfg(feature = "gtk")]
impl<B: FileExplorerBackend> ItemCollectionView<B> for FileGridView {
    fn clear(&self) {
        while let Some(child) = self.flow_box.first_child() {
            self.flow_box.remove(&child);
        }
        self.items.borrow_mut().clear();
    }

    fn populate(
        &self,
        items: &[FileViewItem],
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) {
        self.clear();
        *self.items.borrow_mut() = items.to_vec();

        for item in items {
            let tile = FileGridTile::build(item, backend, toast_overlay, on_refresh.clone());
            self.flow_box.insert(tile.root_widget(), -1);
        }
    }
}

/// A single icon tile within the [`FileGridView`].
#[cfg(feature = "gtk")]
pub struct FileGridTile {
    child: FlowBoxChild,
}

#[cfg(feature = "gtk")]
impl FileGridTile {
    /// Builds a tile widget for `item` with hover suggestions and context menu.
    pub fn build<B: FileExplorerBackend>(
        item: &FileViewItem,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) -> Self {
        let child = FlowBoxChild::new();
        let card = GtkBox::new(Orientation::Vertical, 4);
        card.set_size_request(114, 108);
        card.set_halign(Align::Center);
        card.set_valign(Align::Center);
        card.add_css_class("file-tile");

        // Hover suggestion tooltip displaying full item metadata
        let tooltip = format!(
            "{}\nType: {}\nSize: {}\nPath: {}",
            item.name,
            item.type_label(),
            item.size_formatted,
            item.path,
        );
        card.set_tooltip_text(Some(&tooltip));

        let icon = Image::from_icon_name(item.nautilus_icon_name());
        icon.set_pixel_size(48);
        icon.set_halign(Align::Center);

        let name = Label::new(Some(&item.name));
        name.set_wrap(true);
        name.set_wrap_mode(pango::WrapMode::WordChar);
        name.set_justify(Justification::Center);
        name.set_max_width_chars(14);
        name.set_lines(2);
        name.set_ellipsize(pango::EllipsizeMode::End);

        let sub = Label::new(Some(&item.size_formatted));
        sub.add_css_class("caption");
        sub.add_css_class("dim-label");

        card.append(&icon);
        card.append(&name);
        card.append(&sub);

        ItemContextMenu::attach(&card, item, backend, toast_overlay, on_refresh);

        child.set_child(Some(&card));
        Self { child }
    }
}

#[cfg(feature = "gtk")]
impl ViewComponent for FileGridTile {
    type RootWidget = FlowBoxChild;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.child
    }
}
