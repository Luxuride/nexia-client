//! Multi-selection tracking and state management for file collections.

use std::cell::RefCell;
use std::rc::Rc;

use crate::model::FileViewItem;

/// Manages multi-selection state for file collections.
#[derive(Clone)]
pub struct SelectionTracker {
    selected_items: Rc<RefCell<Vec<FileViewItem>>>,
    on_selection_changed: Rc<dyn Fn(Vec<FileViewItem>)>,
}

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
}
