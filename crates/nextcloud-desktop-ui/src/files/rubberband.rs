//! Visual rubberband selection overlay for icon grid and columnar list views.

#[cfg(feature = "gtk")]
use gtk4::gdk;
#[cfg(feature = "gtk")]
use gtk4::graphene::{Point, Rect};
#[cfg(feature = "gtk")]
use gtk4::{DrawingArea, FlowBox, GestureClick, GestureDrag, ListBox, Overlay, ScrolledWindow};
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;
#[cfg(feature = "gtk")]
use std::cell::RefCell;
#[cfg(feature = "gtk")]
use std::rc::Rc;

#[cfg(feature = "gtk")]
use crate::files::selection::SelectionTracker;
#[cfg(feature = "gtk")]
use crate::model::FileViewItem;
#[cfg(feature = "gtk")]
use crate::widget::ViewComponent;

/// Active rubberband rectangular region `(x, y, width, height)`.
#[cfg(feature = "gtk")]
pub type RubberbandBounds = Rc<RefCell<Option<(f64, f64, f64, f64)>>>;

/// Overlay wrapper that visually paints a translucent selection marquee during rubberband drag.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct RubberbandOverlay {
    overlay: Overlay,
    drawing_area: DrawingArea,
    active_rect: RubberbandBounds,
}

#[cfg(feature = "gtk")]
impl RubberbandOverlay {
    /// Creates a new rubberband overlay wrapping the given `child` scroll window.
    pub fn new(child: &ScrolledWindow) -> Self {
        let overlay = Overlay::new();
        overlay.set_child(Some(child));

        let drawing_area = DrawingArea::new();
        drawing_area.set_can_target(false);
        drawing_area.set_hexpand(true);
        drawing_area.set_vexpand(true);

        let active_rect: RubberbandBounds = Rc::new(RefCell::new(None));
        let rect_draw = active_rect.clone();

        drawing_area.set_draw_func(move |_, cr, _, _| {
            if let Some((x, y, w, h)) = *rect_draw.borrow()
                && w > 0.0
                && h > 0.0
            {
                // Translucent accent blue fill
                cr.set_source_rgba(0.208, 0.518, 0.894, 0.22);
                cr.rectangle(x, y, w, h);
                let _ = cr.fill_preserve();
                // Sharp accent border
                cr.set_source_rgba(0.208, 0.518, 0.894, 0.85);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            }
        });

        overlay.add_overlay(&drawing_area);

        Self {
            overlay,
            drawing_area,
            active_rect,
        }
    }

    /// Attaches rubberband drag selection and canvas click deselection to `flow_box`.
    pub fn attach_flow_box(
        &self,
        flow_box: &FlowBox,
        items_ref: Rc<RefCell<Vec<FileViewItem>>>,
        tracker: SelectionTracker,
    ) {
        let overlay = self.overlay.clone();
        let drawing_area = self.drawing_area.clone();
        let active_rect = self.active_rect.clone();

        // 1. Click on empty space: unselect all
        let click_gesture = GestureClick::new();
        click_gesture.set_button(1);
        let fb_click = flow_box.clone();
        let overlay_click = overlay.clone();
        let tracker_click = tracker.clone();
        click_gesture.connect_pressed(move |_, _, x, y| {
            let pt = Point::new(x as f32, y as f32);
            let mut hit = false;
            let mut idx = 0;
            while let Some(child) = fb_click.child_at_index(idx) {
                if let Some(bounds) = child.compute_bounds(&overlay_click)
                    && bounds.contains_point(&pt)
                {
                    hit = true;
                    break;
                }
                idx += 1;
            }
            if !hit {
                fb_click.unselect_all();
                tracker_click.clear();
            }
        });
        overlay.add_controller(click_gesture);

        // 2. Drag selection marquee
        let drag_gesture = GestureDrag::new();
        let fb_drag = flow_box.clone();
        let overlay_drag = overlay.clone();
        let start_pos: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));

        let start_pos_begin = start_pos.clone();
        let fb_begin = fb_drag.clone();
        let overlay_begin = overlay_drag.clone();
        let tracker_begin = tracker.clone();
        drag_gesture.connect_drag_begin(move |gesture, start_x, start_y| {
            let pt = Point::new(start_x as f32, start_y as f32);
            let mut hit = false;
            let mut idx = 0;
            while let Some(child) = fb_begin.child_at_index(idx) {
                if let Some(bounds) = child.compute_bounds(&overlay_begin)
                    && bounds.contains_point(&pt)
                {
                    hit = true;
                    break;
                }
                idx += 1;
            }

            if !hit {
                *start_pos_begin.borrow_mut() = Some((start_x, start_y));
                let is_ctrl = gesture
                    .current_event()
                    .map(|e| e.modifier_state().contains(gdk::ModifierType::CONTROL_MASK))
                    .unwrap_or(false);
                if !is_ctrl {
                    fb_begin.unselect_all();
                    tracker_begin.clear();
                }
            } else {
                *start_pos_begin.borrow_mut() = None;
            }
        });

        let start_pos_update = start_pos.clone();
        let fb_update = fb_drag.clone();
        let overlay_update = overlay_drag.clone();
        let rect_update = active_rect.clone();
        let da_update = drawing_area.clone();
        drag_gesture.connect_drag_update(move |gesture, offset_x, offset_y| {
            if let Some((start_x, start_y)) = *start_pos_update.borrow() {
                let min_x = start_x.min(start_x + offset_x);
                let max_x = start_x.max(start_x + offset_x);
                let min_y = start_y.min(start_y + offset_y);
                let max_y = start_y.max(start_y + offset_y);
                let w = max_x - min_x;
                let h = max_y - min_y;

                *rect_update.borrow_mut() = Some((min_x, min_y, w, h));
                da_update.queue_draw();

                let drag_rect = Rect::new(min_x as f32, min_y as f32, w as f32, h as f32);
                let is_ctrl = gesture
                    .current_event()
                    .map(|e| e.modifier_state().contains(gdk::ModifierType::CONTROL_MASK))
                    .unwrap_or(false);

                let mut idx = 0;
                while let Some(child) = fb_update.child_at_index(idx) {
                    if let Some(bounds) = child.compute_bounds(&overlay_update) {
                        if drag_rect.intersection(&bounds).is_some() {
                            fb_update.select_child(&child);
                        } else if !is_ctrl {
                            fb_update.unselect_child(&child);
                        }
                    }
                    idx += 1;
                }
            }
        });

        let start_pos_end = start_pos;
        let fb_end = fb_drag;
        let rect_end = active_rect;
        let da_end = drawing_area;
        let items_end = items_ref;
        let tracker_end = tracker;
        drag_gesture.connect_drag_end(move |_, _, _| {
            *start_pos_end.borrow_mut() = None;
            *rect_end.borrow_mut() = None;
            da_end.queue_draw();

            let selected_children = fb_end.selected_children();
            let all = items_end.borrow();
            let selected: Vec<FileViewItem> = selected_children
                .into_iter()
                .filter_map(|c| all.get(c.index() as usize).cloned())
                .collect();
            tracker_end.set_selected(selected);
        });

        overlay.add_controller(drag_gesture);
    }

    /// Attaches rubberband drag selection and canvas click deselection to `list_box`.
    pub fn attach_list_box(
        &self,
        list_box: &ListBox,
        items_ref: Rc<RefCell<Vec<FileViewItem>>>,
        tracker: SelectionTracker,
    ) {
        let overlay = self.overlay.clone();
        let drawing_area = self.drawing_area.clone();
        let active_rect = self.active_rect.clone();

        // 1. Click on empty space: unselect all
        let click_gesture = GestureClick::new();
        click_gesture.set_button(1);
        let lb_click = list_box.clone();
        let overlay_click = overlay.clone();
        let tracker_click = tracker.clone();
        click_gesture.connect_pressed(move |_, _, x, y| {
            let pt = Point::new(x as f32, y as f32);
            let mut hit = false;
            let mut idx = 0;
            while let Some(row) = lb_click.row_at_index(idx) {
                if let Some(bounds) = row.compute_bounds(&overlay_click)
                    && bounds.contains_point(&pt)
                {
                    hit = true;
                    break;
                }
                idx += 1;
            }
            if !hit {
                lb_click.unselect_all();
                tracker_click.clear();
            }
        });
        overlay.add_controller(click_gesture);

        // 2. Drag selection marquee
        let drag_gesture = GestureDrag::new();
        let lb_drag = list_box.clone();
        let overlay_drag = overlay.clone();
        let start_pos: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));

        let start_pos_begin = start_pos.clone();
        let lb_begin = lb_drag.clone();
        let overlay_begin = overlay_drag.clone();
        let tracker_begin = tracker.clone();
        drag_gesture.connect_drag_begin(move |gesture, start_x, start_y| {
            let pt = Point::new(start_x as f32, start_y as f32);
            let mut hit = false;
            let mut idx = 0;
            while let Some(row) = lb_begin.row_at_index(idx) {
                if let Some(bounds) = row.compute_bounds(&overlay_begin)
                    && bounds.contains_point(&pt)
                {
                    hit = true;
                    break;
                }
                idx += 1;
            }

            if !hit {
                *start_pos_begin.borrow_mut() = Some((start_x, start_y));
                let is_ctrl = gesture
                    .current_event()
                    .map(|e| e.modifier_state().contains(gdk::ModifierType::CONTROL_MASK))
                    .unwrap_or(false);
                if !is_ctrl {
                    lb_begin.unselect_all();
                    tracker_begin.clear();
                }
            } else {
                *start_pos_begin.borrow_mut() = None;
            }
        });

        let start_pos_update = start_pos.clone();
        let lb_update = lb_drag.clone();
        let overlay_update = overlay_drag.clone();
        let rect_update = active_rect.clone();
        let da_update = drawing_area.clone();
        drag_gesture.connect_drag_update(move |gesture, offset_x, offset_y| {
            if let Some((start_x, start_y)) = *start_pos_update.borrow() {
                let min_x = start_x.min(start_x + offset_x);
                let max_x = start_x.max(start_x + offset_x);
                let min_y = start_y.min(start_y + offset_y);
                let max_y = start_y.max(start_y + offset_y);
                let w = max_x - min_x;
                let h = max_y - min_y;

                *rect_update.borrow_mut() = Some((min_x, min_y, w, h));
                da_update.queue_draw();

                let drag_rect = Rect::new(min_x as f32, min_y as f32, w as f32, h as f32);
                let is_ctrl = gesture
                    .current_event()
                    .map(|e| e.modifier_state().contains(gdk::ModifierType::CONTROL_MASK))
                    .unwrap_or(false);

                let mut idx = 0;
                while let Some(row) = lb_update.row_at_index(idx) {
                    if let Some(bounds) = row.compute_bounds(&overlay_update) {
                        if drag_rect.intersection(&bounds).is_some() {
                            lb_update.select_row(Some(&row));
                        } else if !is_ctrl {
                            lb_update.unselect_row(&row);
                        }
                    }
                    idx += 1;
                }
            }
        });

        let start_pos_end = start_pos;
        let lb_end = lb_drag;
        let rect_end = active_rect;
        let da_end = drawing_area;
        let items_end = items_ref;
        let tracker_end = tracker;
        drag_gesture.connect_drag_end(move |_, _, _| {
            *start_pos_end.borrow_mut() = None;
            *rect_end.borrow_mut() = None;
            da_end.queue_draw();

            let selected_rows = lb_end.selected_rows();
            let all = items_end.borrow();
            let selected: Vec<FileViewItem> = selected_rows
                .into_iter()
                .filter_map(|r| all.get(r.index() as usize).cloned())
                .collect();
            tracker_end.set_selected(selected);
        });

        overlay.add_controller(drag_gesture);
    }
}

#[cfg(feature = "gtk")]
impl ViewComponent for RubberbandOverlay {
    type RootWidget = Overlay;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.overlay
    }
}
