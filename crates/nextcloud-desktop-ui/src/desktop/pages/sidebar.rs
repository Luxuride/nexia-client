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
        icon_box.set_valign(Align::Center);
        icon_box.set_halign(Align::Center);

        let initial_theme = backend.cached_theming();
        let initial_logo = backend.cached_theme_logo();

        // 1. Initial logo rendering: if cached on disk, render crisp high-res image immediately without placeholder!
        if let Some(ref logo_path) = initial_logo {
            let scale = 2;
            let target_w = 56 * scale;
            let target_h = 40 * scale;
            let texture = if let Ok(pixbuf) =
                gdk_pixbuf::Pixbuf::from_file_at_scale(logo_path, target_w, target_h, true)
            {
                Some(gtk4::gdk::Texture::for_pixbuf(&pixbuf))
            } else {
                let file = gtk4::gio::File::for_path(logo_path);
                gtk4::gdk::Texture::from_file(&file).ok()
            };

            if let Some(texture) = texture {
                let picture = gtk4::Picture::for_paintable(&texture);
                picture.set_can_shrink(true);
                picture.set_keep_aspect_ratio(true);
                picture.set_size_request(56, 40);
                icon_box.append(&picture);
            }
        } else {
            let icon = Image::from_icon_name("folder-remote-symbolic");
            icon.set_pixel_size(28);
            icon_box.append(&icon);
        }

        let initial_title = initial_theme
            .as_ref()
            .and_then(|t| t.name.clone())
            .unwrap_or_default();
        let title_label = Label::new(Some(&initial_title));
        title_label.add_css_class("nextcloud-server-title");
        title_label.set_halign(Align::Start);
        title_label.set_hexpand(true);

        top_row.append(&icon_box);
        top_row.append(&title_label);

        let initial_slogan = initial_theme
            .as_ref()
            .and_then(|t| t.slogan.clone())
            .unwrap_or_default();
        let slogan_label = Label::new(Some(&initial_slogan));
        slogan_label.add_css_class("nextcloud-server-slogan");
        slogan_label.set_halign(Align::Start);
        slogan_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        // Apply theme color immediately if known
        if let Some(ref theme) = initial_theme
            && let Some(ref color) = theme.color
        {
            apply_server_theme_color(color, theme.color_text.as_deref());
        }

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

        // Asynchronously populate server title, slogan, branding color, and logo from server theming
        let b = backend.clone();
        let title_c = title_label.clone();
        let slogan_c = slogan_label.clone();
        let icon_box_c = icon_box.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Some(theme) = b.server_theming().await {
                if let Some(name) = theme.name() {
                    title_c.set_text(name);
                }
                if let Some(slogan) = theme.slogan() {
                    slogan_c.set_text(slogan);
                }
                if let Some(color_hex) = theme.color() {
                    apply_server_theme_color(color_hex, theme.color_text());
                }
            }
            if let Some(logo_path) = b.fetch_theme_logo().await {
                let scale = icon_box_c.scale_factor().max(1);
                // Render at high resolution (2x minimum) for sharp subpixel anti-aliasing
                let target_w = 56 * scale.max(2);
                let target_h = 40 * scale.max(2);

                let texture = if let Ok(pixbuf) =
                    gdk_pixbuf::Pixbuf::from_file_at_scale(&logo_path, target_w, target_h, true)
                {
                    Some(gtk4::gdk::Texture::for_pixbuf(&pixbuf))
                } else {
                    let file = gtk4::gio::File::for_path(&logo_path);
                    gtk4::gdk::Texture::from_file(&file).ok()
                };

                if let Some(texture) = texture {
                    let picture = gtk4::Picture::for_paintable(&texture);
                    picture.set_can_shrink(true);
                    picture.set_keep_aspect_ratio(true);
                    picture.set_size_request(56, 40);
                    while let Some(child) = icon_box_c.first_child() {
                        icon_box_c.remove(&child);
                    }
                    icon_box_c.append(&picture);
                }
            }
        });

        // Event-driven online and sync status updates for the banner badge
        let mut event_rx = backend.subscribe_events();
        let dot_rx = status_dot.clone();
        let lbl_rx = status_label.clone();
        glib::MainContext::default().spawn_local(async move {
            while let Ok(event) = event_rx.recv().await {
                match event {
                    nextcloud_desktop_core::events::DesktopEvent::SyncStarted { .. } => {
                        dot_rx.remove_css_class("status-dot-connected");
                        dot_rx.remove_css_class("status-dot-offline");
                        dot_rx.add_css_class("status-dot-syncing");
                        lbl_rx.set_text("Syncing…");
                    }
                    nextcloud_desktop_core::events::DesktopEvent::SyncCompleted { .. } => {
                        dot_rx.remove_css_class("status-dot-syncing");
                        dot_rx.remove_css_class("status-dot-offline");
                        dot_rx.add_css_class("status-dot-connected");
                        lbl_rx.set_text("Online");
                    }
                    nextcloud_desktop_core::events::DesktopEvent::Notification {
                        level: nextcloud_desktop_core::events::NotificationLevel::Error,
                        ..
                    } => {
                        dot_rx.remove_css_class("status-dot-syncing");
                        dot_rx.remove_css_class("status-dot-connected");
                        dot_rx.add_css_class("status-dot-offline");
                        lbl_rx.set_text("Offline");
                    }
                    _ => {}
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

/// Dynamically updates CSS colors according to the server's brand identity.
pub fn apply_server_theme_color(color_hex: &str, text_color: Option<&str>) {
    let clean_hex = color_hex.trim();
    if !clean_hex.starts_with('#') || clean_hex.len() != 7 {
        return;
    }

    let r = u8::from_str_radix(&clean_hex[1..3], 16).unwrap_or(0);
    let g = u8::from_str_radix(&clean_hex[3..5], 16).unwrap_or(130);
    let b = u8::from_str_radix(&clean_hex[5..7], 16).unwrap_or(201);

    // Darker variant for smooth gradient bottom right
    let dark_r = (r as f32 * 0.70) as u8;
    let dark_g = (g as f32 * 0.70) as u8;
    let dark_b = (b as f32 * 0.70) as u8;
    let darker_hex = format!("#{dark_r:02x}{dark_g:02x}{dark_b:02x}");

    let text_hex = text_color.unwrap_or("#ffffff");

    let css = format!(
        "
        @define-color accent_color {clean_hex};
        @define-color accent_bg_color {clean_hex};

        .nextcloud-server-banner {{
            background: linear-gradient(135deg, {clean_hex} 0%, {darker_hex} 100%);
            color: {text_hex};
        }}
        .nextcloud-server-title {{
            color: {text_hex};
        }}
        .nextcloud-server-slogan {{
            color: alpha({text_hex}, 0.82);
        }}
        .nextcloud-status-badge {{
            color: alpha({text_hex}, 0.90);
        }}
        .status-dot-connected {{
            background-color: #46ba61;
            border-radius: 50%;
            min-width: 8px;
            min-height: 8px;
        }}
        .status-dot-syncing {{
            background-color: #3584e4;
            border-radius: 50%;
            min-width: 8px;
            min-height: 8px;
        }}
        .status-dot-offline {{
            background-color: #e01b24;
            border-radius: 50%;
            min-width: 8px;
            min-height: 8px;
        }}
        "
    );

    let provider = gtk4::CssProvider::new();
    provider.load_from_data(&css);
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}
