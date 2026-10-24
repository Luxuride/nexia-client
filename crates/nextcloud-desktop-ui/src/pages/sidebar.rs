//! Navigation sidebar component for switching between top-level application pages.

use gtk4::{
    Align, Box as GtkBox, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    ScrolledWindow, SelectionMode,
};
use libadwaita::prelude::*;
use std::rc::Rc;

use crate::model::NavigationPage;
use crate::widget::ViewComponent;

/// Left-hand navigation sidebar listing top-level application views.
#[derive(Clone)]
pub struct NavigationSidebar {
    scroll: ScrolledWindow,
    list_box: ListBox,
}

impl NavigationSidebar {
    /// Creates the navigation sidebar populated with [`NavigationPage::all`].
    pub fn new(on_page_selected: Rc<dyn Fn(NavigationPage)>) -> Self {
        let list_box = ListBox::new();
        list_box.set_selection_mode(SelectionMode::Single);
        list_box.add_css_class("navigation-sidebar");

        let mut first_row: Option<ListBoxRow> = None;
        for page in NavigationPage::all() {
            let row = build_sidebar_row(page.icon_name(), page.title());
            if first_row.is_none() {
                first_row = Some(row.clone());
            }
            list_box.append(&row);
        }

        if let Some(ref first) = first_row {
            list_box.select_row(Some(first));
        }

        list_box.connect_row_selected(move |_, row| {
            if let Some(r) = row
                && let Some(page) = NavigationPage::from_index(r.index())
            {
                on_page_selected(page);
            }
        });

        let scroll = ScrolledWindow::new();
        scroll.set_hscrollbar_policy(PolicyType::Never);
        scroll.set_width_request(210);
        scroll.set_vexpand(true);
        scroll.set_child(Some(&list_box));

        Self { scroll, list_box }
    }

    /// Selects the sidebar row corresponding to `page`.
    pub fn select_page(&self, page: NavigationPage) {
        let index = match page {
            NavigationPage::CloudFiles => 0,
            NavigationPage::SyncFolders => 1,
            NavigationPage::Transfers => 2,
            NavigationPage::Settings => 3,
        };
        if let Some(row) = self.list_box.row_at_index(index) {
            self.list_box.select_row(Some(&row));
        }
    }
}

impl ViewComponent for NavigationSidebar {
    type RootWidget = ScrolledWindow;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.scroll
    }
}

fn build_sidebar_row(icon_name: &str, title: &str) -> ListBoxRow {
    let row = ListBoxRow::new();
    let hbox = GtkBox::new(Orientation::Horizontal, 12);
    hbox.set_margin_top(8);
    hbox.set_margin_bottom(8);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);

    let icon = Image::from_icon_name(icon_name);
    let label = Label::new(Some(title));
    label.set_halign(Align::Start);
    label.set_hexpand(true);

    hbox.append(&icon);
    hbox.append(&label);
    row.set_child(Some(&hbox));
    row
}
