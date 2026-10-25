//! Navigation sidebar component for switching between top-level application pages.

use gtk4::{
    Align, Box as GtkBox, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    ScrolledWindow, SelectionMode, Separator,
};
use libadwaita::prelude::*;
use std::rc::Rc;

use crate::model::NavigationPage;
use crate::widget::{FileExplorerBackend, ViewComponent};

/// Left-hand navigation sidebar listing top-level application views.
#[derive(Clone)]
pub struct NavigationSidebar {
    container: GtkBox,
    list_box: ListBox,
}

impl NavigationSidebar {
    /// Creates the navigation sidebar populated with branding banner and [`NavigationPage::all`].
    pub fn new<B: FileExplorerBackend>(
        backend: &B,
        on_page_selected: Rc<dyn Fn(NavigationPage)>,
    ) -> Self {
        let container = GtkBox::new(Orientation::Vertical, 0);
        container.set_width_request(210);
        container.set_vexpand(true);

        // Nextcloud Server Brand Banner Card
        let banner = GtkBox::new(Orientation::Vertical, 4);
        banner.add_css_class("nextcloud-server-banner");

        let top_row = GtkBox::new(Orientation::Horizontal, 8);
        top_row.set_valign(Align::Center);

        let icon_box = GtkBox::new(Orientation::Horizontal, 0);
        icon_box.add_css_class("nextcloud-brand-icon");
        let icon = Image::from_icon_name("weather-overcast-symbolic");
        icon.set_pixel_size(18);
        icon_box.append(&icon);

        let title_label = Label::new(Some("Nextcloud"));
        title_label.add_css_class("nextcloud-server-title");
        title_label.set_halign(Align::Start);
        title_label.set_hexpand(true);

        top_row.append(&icon_box);
        top_row.append(&title_label);

        let slogan_label = Label::new(Some("A safe home for all your data"));
        slogan_label.add_css_class("nextcloud-server-slogan");
        slogan_label.set_halign(Align::Start);
        slogan_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        let badge_row = GtkBox::new(Orientation::Horizontal, 6);
        badge_row.set_margin_top(4);
        let status_dot = GtkBox::new(Orientation::Horizontal, 0);
        status_dot.add_css_class("status-dot-connected");
        status_dot.set_valign(Align::Center);
        let status_label = Label::new(Some("Online"));
        status_label.add_css_class("nextcloud-status-badge");

        badge_row.append(&status_dot);
        badge_row.append(&status_label);

        banner.append(&top_row);
        banner.append(&slogan_label);
        banner.append(&badge_row);

        container.append(&banner);
        container.append(&Separator::new(Orientation::Horizontal));

        // Asynchronously populate server title and slogan from server theming
        let b = backend.clone();
        let title_c = title_label.clone();
        let slogan_c = slogan_label.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Some(theme) = b.server_theming().await {
                if let Some(name) = theme.name() {
                    title_c.set_text(name);
                }
                if let Some(slogan) = theme.slogan() {
                    slogan_c.set_text(slogan);
                }
            }
        });

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
        scroll.set_vexpand(true);
        scroll.set_child(Some(&list_box));

        container.append(&scroll);

        Self {
            container,
            list_box,
        }
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
    type RootWidget = GtkBox;

    fn root_widget(&self) -> &Self::RootWidget {
        &self.container
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
