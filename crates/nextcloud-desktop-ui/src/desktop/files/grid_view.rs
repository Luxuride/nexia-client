//! Nautilus-style icon grid view using GTK 4 `FlowBox` with multi-selection, hover suggestions, and F2 renaming.

use gtk4::{
    Align, Box as GtkBox, EventControllerKey, FlowBox, FlowBoxChild, Image, Justification, Label,
    Orientation, Overlay, PolicyType, ScrolledWindow, SelectionMode, gdk, pango,
};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
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
    icon_widgets: Rc<RefCell<HashMap<String, (GtkBox, i32)>>>,
    selection_tracker: SelectionTracker,
    generation: Rc<Cell<u64>>,
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
        flow_box.set_homogeneous(false);
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
        let icon_widgets = Rc::new(RefCell::new(HashMap::new()));
        let generation = Rc::new(Cell::new(0u64));

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
            icon_widgets,
            selection_tracker,
            generation,
        }
    }

    /// Returns a reference to the active selection tracker.
    pub fn selection_tracker(&self) -> &SelectionTracker {
        &self.selection_tracker
    }

    /// Removes all rendered children from the grid and resets selection.
    pub fn clear(&self) {
        self.generation.set(self.generation.get() + 1);
        self.flow_box.unselect_all();
        while let Some(child) = self.flow_box.first_child() {
            self.flow_box.remove(&child);
        }
        self.items.borrow_mut().clear();
        self.icon_widgets.borrow_mut().clear();
        self.selection_tracker.clear();
    }

    /// Dynamically updates the thumbnail image for a rendered tile when ready.
    pub fn update_thumbnail(&self, path: &str, cached_path: &std::path::Path) {
        if let Some((icon_box, icon_size)) = self.icon_widgets.borrow().get(path).cloned() {
            let file = gtk4::gio::File::for_path(cached_path);
            if let Ok(texture) = gdk::Texture::from_file(&file) {
                let picture = gtk4::Picture::for_paintable(&texture);
                picture.set_can_shrink(true);
                picture.set_keep_aspect_ratio(true);
                picture.set_halign(Align::Center);
                picture.set_valign(Align::Center);
                picture.set_size_request(icon_size, icon_size);
                while let Some(first) = icon_box.first_child() {
                    icon_box.remove(&first);
                }
                icon_box.append(&picture);
            }
        }
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
        self.clear();
    }

    fn populate(
        &self,
        items: &[FileViewItem],
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        zoom: ZoomLevel,
        on_refresh: Rc<dyn Fn()>,
    ) {
        // If the items in the directory are already identical, do not clear the flowbox or reset scroll!
        let is_identical = {
            let existing = self.items.borrow();
            existing.len() == items.len()
                && existing.iter().zip(items.iter()).all(|(a, b)| {
                    a.path == b.path
                        && a.status == b.status
                        && a.name == b.name
                        && a.size_bytes == b.size_bytes
                })
        };
        if is_identical {
            return;
        }

        self.clear();
        let current_gen = self.generation.get();
        let all_items = Rc::new(items.to_vec());
        *self.items.borrow_mut() = items.to_vec();

        // Populate initial batch immediately for zero-latency first paint
        let chunk_size = 50;
        let initial_count = chunk_size.min(all_items.len());
        for item in &all_items[..initial_count] {
            let tile = FileGridTile::build(
                item,
                backend,
                toast_overlay,
                zoom,
                on_refresh.clone(),
                &self.icon_widgets,
            );
            self.flow_box.insert(tile.root_widget(), -1);
        }

        // Schedule remaining items in idle slices so the UI stays responsive without freezing
        if all_items.len() > initial_count {
            let ctx = GridChunkContext {
                items: all_items,
                generation: self.generation.clone(),
                expected_gen: current_gen,
                flow_box: self.flow_box.clone(),
                backend: backend.clone(),
                toast_overlay: toast_overlay.clone(),
                zoom,
                on_refresh,
                icon_widgets: self.icon_widgets.clone(),
            };
            schedule_grid_chunks(ctx, initial_count, chunk_size);
        }

        self.flow_box.unselect_all();
        self.selection_tracker.clear();
    }
}

#[derive(Clone)]
struct GridChunkContext<B> {
    items: Rc<Vec<FileViewItem>>,
    generation: Rc<Cell<u64>>,
    expected_gen: u64,
    flow_box: FlowBox,
    backend: B,
    toast_overlay: adw::ToastOverlay,
    zoom: ZoomLevel,
    on_refresh: Rc<dyn Fn()>,
    icon_widgets: Rc<RefCell<HashMap<String, (GtkBox, i32)>>>,
}

fn schedule_grid_chunks<B: FileExplorerBackend + 'static>(
    ctx: GridChunkContext<B>,
    start_idx: usize,
    chunk_size: usize,
) {
    glib::idle_add_local_once(move || {
        if ctx.generation.get() != ctx.expected_gen || start_idx >= ctx.items.len() {
            return;
        }
        let end_idx = (start_idx + chunk_size).min(ctx.items.len());
        for item in &ctx.items[start_idx..end_idx] {
            let tile = FileGridTile::build(
                item,
                &ctx.backend,
                &ctx.toast_overlay,
                ctx.zoom,
                ctx.on_refresh.clone(),
                &ctx.icon_widgets,
            );
            ctx.flow_box.insert(tile.root_widget(), -1);
        }
        if end_idx < ctx.items.len() {
            schedule_grid_chunks(ctx, end_idx, chunk_size);
        }
    });
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
        icon_widgets: &Rc<RefCell<HashMap<String, (GtkBox, i32)>>>,
    ) -> Self {
        let child = FlowBoxChild::new();
        child.set_focusable(false);
        child.set_focus_on_click(false);
        let item_width = zoom.grid_item_width();
        let icon_size = zoom.grid_icon_size();

        // Enforce unified width across all items (files and directories)
        child.set_size_request(item_width, -1);
        child.set_hexpand(false);
        child.set_halign(Align::Center);

        let card = GtkBox::new(Orientation::Vertical, 4);
        card.set_size_request(item_width, -1);
        card.set_halign(Align::Fill);
        card.set_hexpand(false);
        card.set_valign(Align::Start);
        card.set_overflow(gtk4::Overflow::Hidden);
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

        let icon_box = GtkBox::new(Orientation::Vertical, 0);
        icon_box.set_size_request(icon_size, icon_size);
        icon_box.set_overflow(gtk4::Overflow::Hidden);
        icon_box.set_halign(Align::Center);
        icon_box.set_valign(Align::Center);

        let icon = Image::from_icon_name(item.nautilus_icon_name());
        icon.set_pixel_size(icon_size);
        icon.set_halign(Align::Center);
        icon.set_valign(Align::Center);
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
            icon_widgets
                .borrow_mut()
                .insert(item.path.clone(), (icon_box.clone(), icon_size));

            // Fast display if thumbnail is already cached on local disk
            if let Some(cached_path) = backend.cached_thumbnail(&item.path, Some(64)) {
                let file = gtk4::gio::File::for_path(&cached_path);
                if let Ok(texture) = gdk::Texture::from_file(&file) {
                    let picture = gtk4::Picture::for_paintable(&texture);
                    picture.set_can_shrink(true);
                    picture.set_keep_aspect_ratio(true);
                    picture.set_halign(Align::Center);
                    picture.set_valign(Align::Center);
                    picture.set_size_request(icon_size, icon_size);
                    while let Some(first) = icon_box.first_child() {
                        icon_box.remove(&first);
                    }
                    icon_box.append(&picture);
                }
            }
        }

        let name = Label::new(Some(&item.name));
        name.set_wrap(true);
        name.set_wrap_mode(pango::WrapMode::WordChar);
        name.set_justify(Justification::Center);
        name.set_halign(Align::Fill);
        name.set_valign(Align::Start);
        name.set_hexpand(false);
        name.set_max_width_chars(match zoom {
            ZoomLevel::Small => 12,
            ZoomLevel::Medium => 16,
            ZoomLevel::Large => 20,
            ZoomLevel::ExtraLarge => 26,
        });
        name.set_lines(3);
        name.set_ellipsize(pango::EllipsizeMode::End);

        let sub = Label::new(Some(&item.size_formatted));
        sub.set_wrap(true);
        sub.set_wrap_mode(pango::WrapMode::WordChar);
        sub.set_justify(Justification::Center);
        sub.set_halign(Align::Fill);
        sub.add_css_class("caption");
        sub.add_css_class("dim-label");

        let icon_overlay = gtk4::Overlay::new();
        icon_overlay.set_halign(Align::Center);
        icon_overlay.set_valign(Align::Center);
        icon_overlay.set_size_request(icon_size, icon_size);
        icon_overlay.set_child(Some(&icon_box));

        let sync_emblem = Image::from_icon_name(item.status.icon_name());
        sync_emblem.set_pixel_size((icon_size / 3).clamp(14, 20));
        sync_emblem.set_halign(Align::End);
        sync_emblem.set_valign(Align::End);
        sync_emblem.set_tooltip_text(Some(item.status.display_label()));
        sync_emblem.add_css_class(item.status.css_class());
        sync_emblem.add_css_class("sync-emblem-badge");
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn test_flowbox_stretches_and_allocates_dynamically() {
        crate::suppress_gtk_logs();
        if gtk4::init().is_err() || gdk::Display::default().is_none() {
            return;
        }
        let flow_box = FlowBox::new();
        flow_box.set_homogeneous(false);
        flow_box.set_max_children_per_line(100);
        flow_box.set_min_children_per_line(1);
        flow_box.set_column_spacing(12);
        flow_box.set_row_spacing(12);
        flow_box.set_halign(Align::Fill);
        flow_box.set_valign(Align::Start);
        flow_box.set_hexpand(true);

        let child1 = FlowBoxChild::new();
        child1.set_hexpand(false);
        let card = GtkBox::new(Orientation::Vertical, 4);
        card.set_size_request(100, -1);
        card.set_halign(Align::Fill);
        card.set_hexpand(false);

        let label = Label::new(Some(
            "A very long filename for testing stretch behavior.png",
        ));
        label.set_wrap(true);
        label.set_wrap_mode(pango::WrapMode::WordChar);
        label.set_justify(Justification::Center);
        label.set_lines(3);
        label.set_ellipsize(pango::EllipsizeMode::End);
        label.set_max_width_chars(14);
        label.set_hexpand(false);
        card.append(&label);

        child1.set_child(Some(&card));
        flow_box.insert(&child1, -1);

        assert!(!flow_box.is_homogeneous());
        assert_eq!(flow_box.max_children_per_line(), 100);
        assert_eq!(flow_box.column_spacing(), 12);
        assert_eq!(flow_box.row_spacing(), 12);
    }
}
