//! Bottom status bar for the Cloud Files view.

#[cfg(feature = "gtk")]
use gtk4::{Align, Box as GtkBox, Label, Orientation};
#[cfg(feature = "gtk")]
use libadwaita::prelude::*;

#[cfg(feature = "gtk")]
use crate::model::FileViewItem;
#[cfg(feature = "gtk")]
use crate::widget::ViewComponent;

/// Bottom status bar displaying file/folder counts and active selection details.
#[cfg(feature = "gtk")]
#[derive(Clone)]
pub struct FileStatusBar {
    container: GtkBox,
    summary_label: Label,
    selection_label: Label,
}

#[cfg(feature = "gtk")]
impl FileStatusBar {
    /// Creates a new status bar.
    pub fn new() -> Self {
        let container = GtkBox::new(Orientation::Horizontal, 12);
        container.set_margin_top(6);
        container.set_margin_bottom(6);
        container.set_margin_start(16);
        container.set_margin_end(16);

        let summary_label = Label::new(Some("Ready"));
        summary_label.set_halign(Align::Start);
        summary_label.set_hexpand(true);
        summary_label.add_css_class("caption");
        summary_label.add_css_class("dim-label");

        let selection_label = Label::new(Some("Double-click to open · Right-click for actions"));
        selection_label.set_halign(Align::End);
        selection_label.add_css_class("caption");
        selection_label.add_css_class("dim-label");

        container.append(&summary_label);
        container.append(&selection_label);

        Self {
            container,
            summary_label,
            selection_label,
        }
    }

    /// Updates the summary count label for the given list of directory items.
    pub fn update_counts(&self, items: &[FileViewItem]) {
        let folders = items.iter().filter(|i| i.is_directory).count();
        let files = items.len().saturating_sub(folders);
        self.summary_label.set_text(&format!(
            "{} items ({} folders, {} files)",
            items.len(),
            folders,
            files
        ));
    }

    /// Updates the selection label when an item is selected or deselected.
    pub fn update_selection(&self, item: Option<&FileViewItem>) {
        if let Some(i) = item {
            self.selection_label
                .set_text(&format!("“{}” ({})", i.name, i.size_formatted));
        } else {
            self.selection_label
                .set_text("Double-click to open · Right-click for actions");
        }
    }

    /// Updates the selection label with multi-selection counts and total size.
    pub fn update_multi_selection(&self, items: &[FileViewItem]) {
        match items.len() {
            0 => {
                self.selection_label
                    .set_text("Double-click to open · Right-click for actions");
            }
            1 => {
                let i = &items[0];
                self.selection_label
                    .set_text(&format!("“{}” ({})", i.name, i.size_formatted));
            }
            n => {
                let total_bytes: u64 = items.iter().map(|i| i.size_bytes).sum();
                self.selection_label.set_text(&format!(
                    "{} items selected ({})",
                    n,
                    FileViewItem::format_bytes(total_bytes)
                ));
            }
        }
    }
}

#[cfg(feature = "gtk")]
impl Default for FileStatusBar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "gtk")]
impl ViewComponent for FileStatusBar {
    type RootWidget = GtkBox;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.container
    }
}
