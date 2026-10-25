//! Nautilus-style icon grid view using GTK 4 `FlowBox` with multi-selection, hover suggestions, and F2 renaming.

use gtk4::{
    Align, Box as GtkBox, EventControllerKey, FlowBox, FlowBoxChild, Image, Justification, Label,
    Orientation, Overlay, PolicyType, ScrolledWindow, SelectionMode, gdk, pango,
};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::files::canvas_menu::CanvasContextMenu;
use crate::files::context_menu::ItemContextMenu;
use crate::files::dnd::FileDragDrop;
use crate::files::launcher::launch_file_externally;
use crate::files::rename_dialog::RenameDialog;
use crate::files::rubberband::{RubberbandOverlay, strip_flowbox_linear_drag};
use crate::files::selection::SelectionTracker;
use crate::model::{FileViewItem, ResourceIconCategory, ZoomLevel};
use crate::widget::{FileExplorerBackend, ItemCollectionView, ViewComponent};

/// Icon grid representation of the active virtual directory.
#[derive(Clone)]
pub struct FileGridView {
    overlay: RubberbandOverlay,
    flow_box: FlowBox,
    items: Rc<RefCell<Vec<FileViewItem>>>,
    selection_tracker: SelectionTracker,
}

impl FileGridView {
    /// Creates a new icon grid view wired to `backend` and callbacks.
    pub fn new<B: FileExplorerBackend>(
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        selection_tracker: SelectionTracker,
        on_refresh: Rc<dyn Fn()>,
    ) -> Self {
        let flow_box = FlowBox::new();
        flow_box.set_can_focus(false);
        flow_box.set_focus_on_click(false);
        flow_box.set_selection_mode(SelectionMode::Multiple);
        flow_box.set_activate_on_single_click(false);
        flow_box.set_homogeneous(true);
        flow_box.set_max_children_per_line(100);
        flow_box.set_min_children_per_line(1);
        flow_box.set_column_spacing(12);
        flow_box.set_row_spacing(12);
        flow_box.set_margin_top(16);
        flow_box.set_margin_bottom(16);
        flow_box.set_margin_start(16);
        flow_box.set_margin_end(16);
        flow_box.set_halign(Align::Fill);
        flow_box.set_valign(Align::Start);
        flow_box.set_hexpand(true);
        strip_flowbox_linear_drag(&flow_box);

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
                                launch_file_externally(&path);
                                toast
                                    .add_toast(adw::Toast::new(
                                        &format!("Opened “{}”", item.name,),
                                    ));
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

        // Wire visual rubberband selection marquee and empty-space deselection
        let overlay = RubberbandOverlay::new(&scroll);
        overlay.attach_flow_box(&flow_box, items.clone(), selection_tracker.clone());

        // Wire canvas context menu for right clicking empty space
        let overlay_for_hit = overlay.clone();
        let is_hit = Rc::new(move |x: f64, y: f64| {
            overlay_for_hit
                .root_widget()
                .pick(x, y, gtk4::PickFlags::DEFAULT)
                .and_then(|w| w.ancestor(FlowBoxChild::static_type()))
                .is_some()
        });
        CanvasContextMenu::attach(
            overlay.root_widget(),
            backend,
            toast_overlay,
            on_refresh.clone(),
            is_hit,
        );

        // Wire F2 key shortcut to present Nautilus RenameDialog
        let key_controller = EventControllerKey::new();
        let tracker_key = selection_tracker.clone();
        let b_key = backend.clone();
        let toast_key = toast_overlay.clone();
        let cb_key = on_refresh;
        let overlay_for_key = overlay.clone();
        key_controller.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::F2 {
                let selected = tracker_key.selected();
                if let Some(first) = selected.first() {
                    RenameDialog::present(
                        Some(overlay_for_key.root_widget()),
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
        overlay.root_widget().add_controller(key_controller);

        Self {
            overlay,
            flow_box,
            items,
            selection_tracker,
        }
    }

    /// Returns a reference to the active selection tracker.
    pub fn selection_tracker(&self) -> &SelectionTracker {
        &self.selection_tracker
    }

    /// Removes all rendered children from the grid and resets selection.
    pub fn clear(&self) {
        self.flow_box.unselect_all();
        while let Some(child) = self.flow_box.first_child() {
            self.flow_box.remove(&child);
        }
        self.items.borrow_mut().clear();
        self.selection_tracker.clear();
    }
}

impl ViewComponent for FileGridView {
    type RootWidget = Overlay;

    fn root_widget(&self) -> &Self::RootWidget {
        self.overlay.root_widget()
    }
}

impl<B: FileExplorerBackend> ItemCollectionView<B> for FileGridView {
    fn clear(&self) {
        self.flow_box.unselect_all();
        while let Some(child) = self.flow_box.first_child() {
            self.flow_box.remove(&child);
        }
        self.items.borrow_mut().clear();
        self.selection_tracker.clear();
    }

    fn populate(
        &self,
        items: &[FileViewItem],
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        zoom: ZoomLevel,
        on_refresh: Rc<dyn Fn()>,
    ) {
        self.clear();
        *self.items.borrow_mut() = items.to_vec();

        for item in items {
            let tile = FileGridTile::build(item, backend, toast_overlay, zoom, on_refresh.clone());
            self.flow_box.insert(tile.root_widget(), -1);
        }
        self.flow_box.unselect_all();
        self.selection_tracker.clear();
    }
}

/// A single icon tile within the [`FileGridView`].
pub struct FileGridTile {
    child: FlowBoxChild,
}

impl FileGridTile {
    /// Builds a tile widget for `item` with hover suggestions and context menu.
    pub fn build<B: FileExplorerBackend>(
        item: &FileViewItem,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        zoom: ZoomLevel,
        on_refresh: Rc<dyn Fn()>,
    ) -> Self {
        let child = FlowBoxChild::new();
        child.set_focusable(false);
        child.set_focus_on_click(false);
        child.set_hexpand(true);
        let card = GtkBox::new(Orientation::Vertical, 4);
        let item_width = zoom.grid_item_width();
        let item_height = (item_width as f32 * 1.1) as i32;
        card.set_size_request(item_width, item_height);
        card.set_halign(Align::Fill);
        card.set_hexpand(true);
        card.set_valign(Align::Start);
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

        let icon_size = zoom.grid_icon_size();
        let icon_box = GtkBox::new(Orientation::Vertical, 0);
        icon_box.set_size_request(icon_size, icon_size);
        icon_box.set_halign(Align::Center);
        icon_box.set_valign(Align::Center);

        let icon = Image::from_icon_name(item.nautilus_icon_name());
        icon.set_pixel_size(icon_size);
        icon.set_halign(Align::Center);
        icon_box.append(&icon);

        // Check for thumbnail support
        let is_thumbnail_candidate = !item.is_directory
            && (item.icon_category == ResourceIconCategory::Image
                || item
                    .mime_type
                    .as_deref()
                    .is_some_and(|m| m.starts_with("image/"))
                || item.name.ends_with(".png")
                || item.name.ends_with(".jpg")
                || item.name.ends_with(".jpeg")
                || item.name.ends_with(".webp")
                || item.name.ends_with(".gif")
                || item.name.ends_with(".svg")
                || item.name.ends_with(".pdf"));

        if is_thumbnail_candidate {
            let thumb_dim = icon_size as u32;

            // 1. Immediately provide cached thumbnail (target or 64px placeholder) if already on disk!
            if let Some(cached_path) = backend.cached_thumbnail(&item.path, Some(thumb_dim)) {
                let file = gtk4::gio::File::for_path(&cached_path);
                if let Ok(texture) = gdk::Texture::from_file(&file) {
                    let picture = gtk4::Picture::for_paintable(&texture);
                    picture.set_can_shrink(true);
                    picture.set_keep_aspect_ratio(true);
                    picture.set_size_request(icon_size, icon_size);
                    while let Some(first) = icon_box.first_child() {
                        icon_box.remove(&first);
                    }
                    icon_box.append(&picture);
                }
            }

            // 2. Asynchronously fetch if target size is not yet on disk
            let has_target_cached = backend
                .cached_thumbnail(&item.path, Some(thumb_dim))
                .is_some_and(|p| p.to_string_lossy().contains(&format!("_{thumb_dim}.")));

            if !has_target_cached {
                let backend_clone = backend.clone();
                let path_clone = item.path.clone();
                let icon_box_clone = icon_box.clone();

                glib::MainContext::default().spawn_local(async move {
                    if let Some(thumb_path) =
                        backend_clone.get_thumbnail(&path_clone, thumb_dim).await
                    {
                        let file = gtk4::gio::File::for_path(&thumb_path);
                        if let Ok(texture) = gdk::Texture::from_file(&file) {
                            let picture = gtk4::Picture::for_paintable(&texture);
                            picture.set_can_shrink(true);
                            picture.set_keep_aspect_ratio(true);
                            picture.set_size_request(icon_size, icon_size);
                            while let Some(first) = icon_box_clone.first_child() {
                                icon_box_clone.remove(&first);
                            }
                            icon_box_clone.append(&picture);
                        }
                    }
                });
            }
        }

        let name = Label::new(Some(&item.name));
        name.set_wrap(true);
        name.set_wrap_mode(pango::WrapMode::WordChar);
        name.set_justify(Justification::Center);
        name.set_halign(Align::Fill);
        name.set_hexpand(true);
        name.set_max_width_chars(match zoom {
            ZoomLevel::Small => 10,
            ZoomLevel::Medium => 14,
            ZoomLevel::Large => 18,
            ZoomLevel::ExtraLarge => 24,
        });
        name.set_lines(2);
        name.set_ellipsize(pango::EllipsizeMode::End);

        let sub = Label::new(Some(&item.size_formatted));
        sub.add_css_class("caption");
        sub.add_css_class("dim-label");

        let icon_overlay = gtk4::Overlay::new();
        icon_overlay.set_child(Some(&icon_box));

        let sync_emblem = Image::from_icon_name(item.status.icon_name());
        sync_emblem.set_pixel_size((icon_size / 3).clamp(14, 20));
        sync_emblem.set_halign(Align::End);
        sync_emblem.set_valign(Align::End);
        sync_emblem.set_tooltip_text(Some(item.status.display_label()));
        sync_emblem.add_css_class(item.status.css_class());
        icon_overlay.add_overlay(&sync_emblem);

        card.append(&icon_overlay);
        card.append(&name);
        card.append(&sub);

        ItemContextMenu::attach(&card, item, backend, toast_overlay, on_refresh.clone());
        FileDragDrop::attach_source(&card, item);
        FileDragDrop::attach_target(&card, item, backend, toast_overlay, on_refresh);

        child.set_child(Some(&card));
        Self { child }
    }
}

impl ViewComponent for FileGridTile {
    type RootWidget = FlowBoxChild;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.child
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flowbox_stretches_and_allocates_dynamically() {
        crate::suppress_gtk_logs();
        if gtk4::init().is_err() {
            return;
        }
        let flow_box = FlowBox::new();
        flow_box.set_homogeneous(true);
        flow_box.set_max_children_per_line(100);
        flow_box.set_min_children_per_line(1);
        flow_box.set_column_spacing(12);
        flow_box.set_row_spacing(12);
        flow_box.set_halign(Align::Fill);
        flow_box.set_valign(Align::Start);
        flow_box.set_hexpand(true);

        let child1 = FlowBoxChild::new();
        child1.set_hexpand(true);
        let card = GtkBox::new(Orientation::Vertical, 4);
        card.set_size_request(100, 110);
        card.set_halign(Align::Fill);
        card.set_hexpand(true);

        let label = Label::new(Some(
            "A very long filename for testing stretch behavior.png",
        ));
        label.set_wrap(true);
        label.set_wrap_mode(pango::WrapMode::WordChar);
        label.set_justify(Justification::Center);
        label.set_lines(2);
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.set_max_width_chars(14);
        label.set_hexpand(true);
        card.append(&label);

        child1.set_child(Some(&card));
        flow_box.insert(&child1, -1);

        // When allocated 1200px wide, FlowBox stretches to 1200px and child expands
        flow_box.size_allocate(&gtk4::Allocation::new(0, 0, 1200, 800), -1);
        assert_eq!(flow_box.allocation().width(), 1200);
        assert!(child1.allocation().width() >= 100);
    }
}
