//! Drag-and-drop support for file and folder items in the Cloud Files browser.

#[cfg(feature = "gtk")]
use gtk4::{gdk, glib, prelude::*};
#[cfg(feature = "gtk")]
use libadwaita as adw;
#[cfg(feature = "gtk")]
use std::rc::Rc;

#[cfg(feature = "gtk")]
use crate::model::FileViewItem;
#[cfg(feature = "gtk")]
use crate::widget::FileExplorerBackend;

/// Coordinates file drag sources and directory drop targets.
#[cfg(feature = "gtk")]
pub struct FileDragDrop;

#[cfg(feature = "gtk")]
impl FileDragDrop {
    /// Attaches a drag source to `widget` carrying `item`'s virtual path and displaying its icon.
    pub fn attach_source(widget: &impl IsA<gtk4::Widget>, item: &FileViewItem) {
        let drag_source = gtk4::DragSource::new();
        drag_source.set_actions(gdk::DragAction::MOVE);

        let path_clone = item.path.clone();
        drag_source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(&path_clone.to_value()))
        });

        let icon_name = item.nautilus_icon_name().to_string();
        drag_source.connect_drag_begin(move |source, _| {
            if let Some(display) = gdk::Display::default() {
                let theme = gtk4::IconTheme::for_display(&display);
                let paintable = theme.lookup_icon(
                    &icon_name,
                    &[],
                    48,
                    1,
                    gtk4::TextDirection::None,
                    gtk4::IconLookupFlags::empty(),
                );
                source.set_icon(Some(&paintable), 24, 24);
            }
        });

        widget.add_controller(drag_source);
    }

    /// Attaches a drop target to `widget` if `target_item` is a directory, enabling drop-to-move.
    pub fn attach_target<B: FileExplorerBackend>(
        widget: &impl IsA<gtk4::Widget>,
        target_item: &FileViewItem,
        backend: &B,
        toast_overlay: &adw::ToastOverlay,
        on_refresh: Rc<dyn Fn()>,
    ) {
        if !target_item.is_directory {
            return;
        }

        let drop_target = gtk4::DropTarget::new(glib::types::Type::STRING, gdk::DragAction::MOVE);

        let widget_enter = widget.clone().upcast::<gtk4::Widget>();
        drop_target.connect_enter(move |_, _, _| {
            widget_enter.add_css_class("drop-hover");
            gdk::DragAction::MOVE
        });

        let widget_leave = widget.clone().upcast::<gtk4::Widget>();
        drop_target.connect_leave(move |_| {
            widget_leave.remove_css_class("drop-hover");
        });

        let widget_drop = widget.clone().upcast::<gtk4::Widget>();
        let b = backend.clone();
        let toast = toast_overlay.clone();
        let cb = on_refresh;
        let dest_path = target_item.path.clone();
        let target_name = target_item.name.clone();

        drop_target.connect_drop(move |_, value, _, _| {
            widget_drop.remove_css_class("drop-hover");
            if let Ok(src_path) = value.get::<String>() {
                if src_path == dest_path {
                    return false;
                }
                let b_move = b.clone();
                let toast_move = toast.clone();
                let cb_move = cb.clone();
                let dest = dest_path.clone();
                let name = target_name.clone();
                glib::MainContext::default().spawn_local(async move {
                    match b_move.move_item(&src_path, &dest).await {
                        Ok(_) => {
                            toast_move
                                .add_toast(adw::Toast::new(&format!("Moved item into “{}”", name)));
                            cb_move();
                        }
                        Err(err) => {
                            toast_move.add_toast(adw::Toast::new(&err.to_string()));
                        }
                    }
                });
                true
            } else {
                false
            }
        });

        widget.add_controller(drop_target);
    }
}
