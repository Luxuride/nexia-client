//! Nautilus-style detailed list view with multi-selection, hover suggestions, and inline actions.

use gtk4::{
    Align, Box as GtkBox, Button, EventControllerKey, Image, Label, ListBox, ListBoxRow,
    Orientation, PolicyType, ScrolledWindow, SelectionMode, Separator, gdk, pango,
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
use crate::files::rubberband::{RubberbandOverlay, strip_listbox_linear_drag};
use crate::files::selection::SelectionTracker;
use crate::model::{FileViewItem, ResourceIconCategory, ZoomLevel};
use crate::widget::{FileExplorerBackend, ItemCollectionView, ViewComponent};

/// Detailed columnar list view representing the active virtual directory.
#[derive(Clone)]
pub struct FileListView {
    container: GtkBox,
    list_box: ListBox,
    items: Rc<RefCell<Vec<FileViewItem>>>,
    selection_tracker: SelectionTracker,
}

impl FileListView {
    /// Creates a new columnar list view wired to `backend` and callbacks.
    pub fn new<B: FileExplorerBackend>(
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        selection_tracker: SelectionTracker,
        on_refresh: Rc<dyn Fn()>,
    ) -> Self {
        let container = GtkBox::new(Orientation::Vertical, 0);
        container.set_vexpand(true);
        container.set_hexpand(true);

        // Column header bar matching Nautilus list view layout
        let col_header = GtkBox::new(Orientation::Horizontal, 12);
        col_header.set_margin_top(8);
        col_header.set_margin_bottom(6);
        col_header.set_margin_start(16);
        col_header.set_margin_end(16);

        let col_name = Label::new(Some("Name"));
        col_name.set_hexpand(true);
        col_name.set_halign(Align::Start);
        col_name.add_css_class("caption-heading");
        col_name.add_css_class("dim-label");

        let col_type = Label::new(Some("Type"));
        col_type.set_size_request(120, -1);
        col_type.set_halign(Align::Start);
        col_type.add_css_class("caption-heading");
        col_type.add_css_class("dim-label");

        let col_size = Label::new(Some("Size"));
        col_size.set_size_request(90, -1);
        col_size.set_halign(Align::End);
        col_size.add_css_class("caption-heading");
        col_size.add_css_class("dim-label");

        let col_actions = Label::new(Some("Actions"));
        col_actions.set_size_request(110, -1);
        col_actions.set_halign(Align::End);
        col_actions.add_css_class("caption-heading");
        col_actions.add_css_class("dim-label");

        col_header.append(&col_name);
        col_header.append(&col_type);
        col_header.append(&col_size);
        col_header.append(&col_actions);

        container.append(&col_header);
        container.append(&Separator::new(Orientation::Horizontal));

        let list_box = ListBox::new();
        list_box.set_can_focus(false);
        list_box.set_focus_on_click(false);
        list_box.set_selection_mode(SelectionMode::Multiple);
        list_box.set_activate_on_single_click(false);
        list_box.add_css_class("file-list-view");
        strip_listbox_linear_drag(&list_box);

        let items = Rc::new(RefCell::new(Vec::new()));

        // Wire selection changes
        let items_for_sel = items.clone();
        let tracker_for_sel = selection_tracker.clone();
        list_box.connect_selected_rows_changed(move |lb| {
            let selected_rows = lb.selected_rows();
            let all = items_for_sel.borrow();
            let selected: Vec<FileViewItem> = selected_rows
                .into_iter()
                .filter_map(|r| all.get(r.index() as usize).cloned())
                .collect();
            tracker_for_sel.set_selected(selected);
        });

        // Wire double-click activation EXACTLY ONCE here in constructor
        let b_act = backend.clone();
        let toast_act = toast_overlay.clone();
        let cb_act = on_refresh.clone();
        let items_for_act = items.clone();
        list_box.connect_row_activated(move |_, row| {
            let idx = row.index() as usize;
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
        scroll.set_child(Some(&list_box));

        // Wire visual rubberband selection marquee and empty-space deselection
        let rubberband = RubberbandOverlay::new(&scroll);
        rubberband.attach_list_box(&list_box, items.clone(), selection_tracker.clone());

        // Wire canvas context menu for right clicking empty space
        let overlay_for_hit = rubberband.clone();
        let is_hit = Rc::new(move |x: f64, y: f64| {
            overlay_for_hit
                .root_widget()
                .pick(x, y, gtk4::PickFlags::DEFAULT)
                .and_then(|w| w.ancestor(ListBoxRow::static_type()))
                .is_some()
        });
        CanvasContextMenu::attach(
            rubberband.root_widget(),
            backend,
            toast_overlay,
            on_refresh.clone(),
            is_hit,
        );

        container.append(rubberband.root_widget());

        // Wire F2 key shortcut to present Nautilus RenameDialog
        let key_controller = EventControllerKey::new();
        let tracker_key = selection_tracker.clone();
        let b_key = backend.clone();
        let toast_key = toast_overlay.clone();
        let cb_key = on_refresh;
        let container_for_key = container.clone();
        key_controller.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::F2 {
                let selected = tracker_key.selected();
                if let Some(first) = selected.first() {
                    RenameDialog::present(
                        Some(&container_for_key),
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
        container.add_controller(key_controller);

        Self {
            container,
            list_box,
            items,
            selection_tracker,
        }
    }

    /// Returns a reference to the active selection tracker.
    pub fn selection_tracker(&self) -> &SelectionTracker {
        &self.selection_tracker
    }

    /// Removes all rendered children from the list and clears active selection.
    pub fn clear(&self) {
        self.list_box.unselect_all();
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }
        self.items.borrow_mut().clear();
        self.selection_tracker.clear();
    }
}

impl ViewComponent for FileListView {
    type RootWidget = GtkBox;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.container
    }
}

impl<B: FileExplorerBackend> ItemCollectionView<B> for FileListView {
    fn clear(&self) {
        self.list_box.unselect_all();
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
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
            let row = FileListRow::build(item, backend, toast_overlay, zoom, on_refresh.clone());
            self.list_box.append(row.root_widget());
        }
        self.list_box.unselect_all();
        self.selection_tracker.clear();
    }
}

/// A single row within the [`FileListView`].
pub struct FileListRow {
    row: ListBoxRow,
}

impl FileListRow {
    /// Builds a row widget for `item` with hover suggestions and action buttons.
    pub fn build<B: FileExplorerBackend>(
        item: &FileViewItem,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        zoom: ZoomLevel,
        on_refresh: Rc<dyn Fn()>,
    ) -> Self {
        let row = ListBoxRow::new();
        row.set_focusable(false);
        row.set_focus_on_click(false);
        let hbox = GtkBox::new(Orientation::Horizontal, 12);
        hbox.set_margin_top(6);
        hbox.set_margin_bottom(6);
        hbox.set_margin_start(16);
        hbox.set_margin_end(16);

        // Hover suggestion tooltip displaying full item metadata
        let tooltip = format!(
            "{}\nType: {}\nSize: {}\nPath: {}",
            item.name,
            item.type_label(),
            item.size_formatted,
            item.path,
        );
        hbox.set_tooltip_text(Some(&tooltip));

        // 1. Icon & Name
        let icon_size = zoom.list_icon_size();
        let icon_box = GtkBox::new(Orientation::Horizontal, 0);
        icon_box.set_size_request(icon_size, icon_size);
        icon_box.set_halign(Align::Center);
        icon_box.set_valign(Align::Center);

        let icon = Image::from_icon_name(item.nautilus_icon_name());
        icon.set_pixel_size(icon_size);
        icon_box.append(&icon);

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
            let backend_clone = backend.clone();
            let path_clone = item.path.clone();
            let icon_box_clone = icon_box.clone();
            let thumb_dim = icon_size as u32;

            glib::MainContext::default().spawn_local(async move {
                if let Some(thumb_path) = backend_clone.get_thumbnail(&path_clone, thumb_dim).await
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

        let name = Label::new(Some(&item.name));
        name.set_hexpand(true);
        name.set_halign(Align::Start);
        name.set_ellipsize(pango::EllipsizeMode::End);

        // 2. Type column
        let type_lbl = Label::new(Some(item.type_label()));
        type_lbl.set_size_request(120, -1);
        type_lbl.set_halign(Align::Start);
        type_lbl.add_css_class("dim-label");

        // 3. Size column
        let size_lbl = Label::new(Some(&item.size_formatted));
        size_lbl.set_size_request(90, -1);
        size_lbl.set_halign(Align::End);
        size_lbl.add_css_class("dim-label");

        // 4. Action buttons
        let actions = GtkBox::new(Orientation::Horizontal, 4);
        actions.set_size_request(110, -1);
        actions.set_halign(Align::End);

        let open_icon = if item.is_directory {
            "go-next-symbolic"
        } else {
            "document-open-symbolic"
        };
        let open_btn = Button::from_icon_name(open_icon);
        open_btn.add_css_class("flat");
        open_btn.set_focusable(false);
        open_btn.set_tooltip_text(Some(if item.is_directory {
            "Open Folder"
        } else {
            "Open File"
        }));

        {
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_refresh.clone();
            let item_clone = item.clone();
            open_btn.connect_clicked(move |_| {
                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();
                let item = item_clone.clone();
                glib::MainContext::default().spawn_local(async move {
                    if item.is_directory {
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
            });
        }

        // Rename button
        let rename_btn = Button::from_icon_name("document-edit-symbolic");
        rename_btn.add_css_class("flat");
        rename_btn.set_focusable(false);
        rename_btn.set_tooltip_text(Some("Rename (F2)"));

        {
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_refresh.clone();
            let item_clone = item.clone();
            rename_btn.connect_clicked(move |btn| {
                RenameDialog::present(Some(btn), &item_clone, &b, &toast, cb.clone());
            });
        }

        // Delete button
        let del_btn = Button::from_icon_name("user-trash-symbolic");
        del_btn.add_css_class("flat");
        del_btn.set_focusable(false);
        del_btn.set_tooltip_text(Some("Delete"));

        {
            let b = backend.clone();
            let toast = toast_overlay.clone();
            let cb = on_refresh.clone();
            let path = item.path.clone();
            let name = item.name.clone();
            del_btn.connect_clicked(move |_| {
                let b = b.clone();
                let toast = toast.clone();
                let cb = cb.clone();
                let p = path.clone();
                let n = name.clone();
                glib::MainContext::default().spawn_local(async move {
                    match b.delete_item(&p).await {
                        Ok(_) => {
                            toast.add_toast(adw::Toast::new(&format!("Deleted “{n}”")));
                            cb();
                        }
                        Err(err) => toast.add_toast(adw::Toast::new(&err.to_string())),
                    }
                });
            });
        }

        actions.append(&open_btn);
        actions.append(&rename_btn);
        actions.append(&del_btn);

        hbox.append(&icon);
        hbox.append(&name);
        hbox.append(&type_lbl);
        hbox.append(&size_lbl);
        hbox.append(&actions);

        ItemContextMenu::attach(&hbox, item, backend, toast_overlay, on_refresh.clone());
        FileDragDrop::attach_source(&hbox, item);
        FileDragDrop::attach_target(&hbox, item, backend, toast_overlay, on_refresh);

        row.set_child(Some(&hbox));
        Self { row }
    }
}

impl ViewComponent for FileListRow {
    type RootWidget = ListBoxRow;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.row
    }
}
