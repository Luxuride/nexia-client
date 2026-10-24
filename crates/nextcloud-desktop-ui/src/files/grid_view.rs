//! Nautilus-style icon grid view using GTK 4 `FlowBox`.

#[cfg(feature = "gtk")]
use gtk4::{
    Align, Box as GtkBox, FlowBox, FlowBoxChild, Image, Justification, Label, Orientation,
    PolicyType, ScrolledWindow, SelectionMode, pango,
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
}

#[cfg(feature = "gtk")]
impl FileGridView {
    /// Creates a new empty icon grid view.
    pub fn new(on_select: Rc<dyn Fn(Option<FileViewItem>)>) -> Self {
        let flow_box = FlowBox::new();
        flow_box.set_selection_mode(SelectionMode::Single);
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
        let items_for_sel = items.clone();
        flow_box.connect_selected_children_changed(move |fb| {
            if let Some(child) = fb.selected_children().first() {
                let idx = child.index() as usize;
                let item = items_for_sel.borrow().get(idx).cloned();
                on_select(item);
            } else {
                on_select(None);
            }
        });

        let scroll = ScrolledWindow::new();
        scroll.set_hscrollbar_policy(PolicyType::Never);
        scroll.set_vexpand(true);
        scroll.set_hexpand(true);
        scroll.set_child(Some(&flow_box));

        Self {
            scroll,
            flow_box,
            items,
        }
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

        // Wire double-click activation
        let b = backend.clone();
        let toast = toast_overlay.clone();
        let cb = on_refresh;
        let items_clone = self.items.clone();
        self.flow_box.connect_child_activated(move |_, child| {
            let idx = child.index() as usize;
            let maybe_item = items_clone.borrow().get(idx).cloned();
            if let Some(item) = maybe_item {
                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();
                glib::MainContext::default().spawn_local(async move {
                    if item.is_directory {
                        b.navigate_into(&item.name).await;
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
    }
}

/// A single icon tile within the [`FileGridView`].
#[cfg(feature = "gtk")]
pub struct FileGridTile {
    child: FlowBoxChild,
}

#[cfg(feature = "gtk")]
impl FileGridTile {
    /// Builds a tile widget for `item`.
    pub fn build<B: FileExplorerBackend>(
        item: &FileViewItem,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) -> Self {
        let child = FlowBoxChild::new();
        let card = GtkBox::new(Orientation::Vertical, 4);
        card.set_size_request(114, 108);
        card.set_margin_top(6);
        card.set_margin_bottom(6);
        card.set_margin_start(4);
        card.set_margin_end(4);
        card.set_halign(Align::Center);

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
