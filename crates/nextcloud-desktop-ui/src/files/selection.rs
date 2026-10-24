//! Multi-selection tracking, empty-canvas deselection, and rubberband drag selection.

#[cfg(feature = "gtk")]
use gtk4::graphene::Rect;
#[cfg(feature = "gtk")]
use gtk4::{FlowBox, GestureClick, GestureDrag, ListBox, ScrolledWindow};
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use std::cell::RefCell;
#[cfg(feature = "gtk")]
use std::rc::Rc;

#[cfg(feature = "gtk")]
use crate::model::FileViewItem;

/// Manages multi-selection state, empty-canvas deselection, and drag selection for file collections.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct SelectionTracker {
    selected_items: Rc<RefCell<Vec<FileViewItem>>>,
    on_selection_changed: Rc<dyn Fn(Vec<FileViewItem>)>,
}

#[cfg(feature = "gtk")]
impl SelectionTracker {
    /// Creates a new selection tracker notifying `on_selection_changed` on any change.
    pub fn new(on_selection_changed: Rc<dyn Fn(Vec<FileViewItem>)>) -> Self {
        Self {
            selected_items: Rc::new(RefCell::new(Vec::new())),
            on_selection_changed,
        }
    }

    /// Returns the currently selected items.
    pub fn selected(&self) -> Vec<FileViewItem> {
        self.selected_items.borrow().clone()
    }

    /// Sets the current list of selected items and triggers notification.
    pub fn set_selected(&self, items: Vec<FileViewItem>) {
        *self.selected_items.borrow_mut() = items.clone();
        (self.on_selection_changed)(items);
    }

    /// Clears the selection.
    pub fn clear(&self) {
        self.set_selected(Vec::new());
    }

    /// Attaches empty-space click deselection and rubberband drag selection to a `FlowBox` within a `ScrolledWindow`.
    pub fn attach_flow_box_interactions(
        &self,
        scroll: &ScrolledWindow,
        flow_box: &FlowBox,
        items_ref: Rc<RefCell<Vec<FileViewItem>>>,
    ) {
        // 1. Unselect when clicking on empty canvas outside any child
        let click_tracker = self.clone();
        let fb_for_click = flow_box.clone();
        let click_gesture = GestureClick::new();
        click_gesture.set_button(1); // Primary click
        click_gesture.connect_pressed(move |_, _, x, y| {
            let hit_child = fb_for_click.child_at_pos(x as i32, y as i32);
            if hit_child.is_none() {
                fb_for_click.unselect_all();
                click_tracker.clear();
            }
        });
        scroll.add_controller(click_gesture);

        // 2. Rubberband drag selection across tiles
        let drag_tracker = self.clone();
        let fb_for_drag = flow_box.clone();
        let items_for_drag = items_ref;
        let drag_gesture = GestureDrag::new();

        let start_pos: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));
        let start_pos_begin = start_pos.clone();
        let start_pos_update = start_pos;

        let fb_drag_begin = fb_for_drag.clone();
        let tracker_drag_begin = drag_tracker.clone();
        drag_gesture.connect_drag_begin(move |gesture, start_x, start_y| {
            if fb_drag_begin
                .child_at_pos(start_x as i32, start_y as i32)
                .is_none()
            {
                *start_pos_begin.borrow_mut() = Some((start_x, start_y));
                let current_event = gesture.current_event();
                let is_modifier = current_event
                    .map(|e| {
                        e.modifier_state()
                            .contains(gtk4::gdk::ModifierType::CONTROL_MASK)
                    })
                    .unwrap_or(false);
                if !is_modifier {
                    fb_drag_begin.unselect_all();
                    tracker_drag_begin.clear();
                }
            } else {
                *start_pos_begin.borrow_mut() = None;
            }
        });

        let fb_drag_update = fb_for_drag.clone();
        drag_gesture.connect_drag_update(move |_, offset_x, offset_y| {
            if let Some((start_x, start_y)) = *start_pos_update.borrow() {
                let min_x = (start_x.min(start_x + offset_x)) as f32;
                let max_x = (start_x.max(start_x + offset_x)) as f32;
                let min_y = (start_y.min(start_y + offset_y)) as f32;
                let max_y = (start_y.max(start_y + offset_y)) as f32;

                let drag_rect = Rect::new(min_x, min_y, max_x - min_x, max_y - min_y);

                let mut idx = 0;
                while let Some(child) = fb_drag_update.child_at_index(idx) {
                    if let Some(child_bounds) = child.compute_bounds(&fb_drag_update)
                        && drag_rect.intersection(&child_bounds).is_some()
                    {
                        fb_drag_update.select_child(&child);
                    }
                    idx += 1;
                }
            }
        });

        drag_gesture.connect_drag_end(move |_, _, _| {
            let selected_children = fb_for_drag.selected_children();
            let all_items = items_for_drag.borrow();
            let mut selected = Vec::new();
            for child in selected_children {
                let idx = child.index() as usize;
                if let Some(item) = all_items.get(idx) {
                    selected.push(item.clone());
                }
            }
            drag_tracker.set_selected(selected);
        });

        scroll.add_controller(drag_gesture);
    }

    /// Attaches empty-space click deselection to a `ListBox` within a `ScrolledWindow`.
    pub fn attach_list_box_interactions(&self, scroll: &ScrolledWindow, list_box: &ListBox) {
        let tracker = self.clone();
        let lb = list_box.clone();
        let click_gesture = GestureClick::new();
        click_gesture.set_button(1);
        click_gesture.connect_pressed(move |_, _, _, y| {
            if lb.row_at_y(y as i32).is_none() {
                lb.unselect_all();
                tracker.clear();
            }
        });
        scroll.add_controller(click_gesture);
    }
}
