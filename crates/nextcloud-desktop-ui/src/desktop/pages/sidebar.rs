//! Navigation sidebar component for switching between top-level application pages.

use gtk4::{
    Align, Box as GtkBox, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    ScrolledWindow, SelectionMode, Separator,
};
use libadwaita::prelude::*;
use nextcloud_desktop_core::events::{DesktopEvent, NotificationLevel};
use std::cell::RefCell;
use std::path::Path;
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
        container.set_size_request(260, -1);
        container.set_width_request(260);
        container.set_vexpand(true);
        container.set_hexpand(false);
        container.set_halign(Align::Start);
        container.add_css_class("navigation-sidebar-fixed");

        // Nextcloud Server Brand Banner Card
        let banner = GtkBox::new(Orientation::Vertical, 4);
        banner.add_css_class("nextcloud-server-banner");
        banner.set_hexpand(false);
        banner.set_halign(Align::Fill);

        let top_row = GtkBox::new(Orientation::Horizontal, 8);
        top_row.set_valign(Align::Center);

        let icon_box = GtkBox::new(Orientation::Horizontal, 0);
        icon_box.add_css_class("nextcloud-brand-icon");
        icon_box.set_valign(Align::Center);
        icon_box.set_halign(Align::Center);

        let initial_theme = backend.cached_theming();

        set_brand_logo(&icon_box, backend.cached_theme_logo().as_deref(), 2);

        let initial_title = initial_theme
            .as_ref()
            .and_then(|t| t.name.clone())
            .unwrap_or_default();
        let title_label = Label::new(Some(&initial_title));
        title_label.add_css_class("nextcloud-server-title");
        title_label.set_halign(Align::Start);
        title_label.set_hexpand(true);
        title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

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
        match initial_theme {
            Some(ref theme) => match theme.color.as_deref() {
                Some(color) => apply_server_theme_color(color, theme.color_text.as_deref()),
                None => clear_server_theme_color(),
            },
            None => clear_server_theme_color(),
        }

        let badge_row = GtkBox::new(Orientation::Horizontal, 6);
        badge_row.set_margin_top(4);
        let status_dot = GtkBox::new(Orientation::Horizontal, 0);
        status_dot.add_css_class("status-dot-connected");
        status_dot.set_valign(Align::Center);
        let status_label = Label::new(Some("Online"));
        status_label.add_css_class("nextcloud-status-badge");
        status_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        badge_row.append(&status_dot);
        badge_row.append(&status_label);

        banner.append(&top_row);
        banner.append(&slogan_label);
        banner.append(&badge_row);

        container.append(&banner);
        container.append(&Separator::new(Orientation::Horizontal));

        // Populate server title, slogan, branding color, and logo, and repopulate them on
        // every account switch so the banner never keeps another account's identity.
        let b = backend.clone();
        let title_c = title_label.clone();
        let slogan_c = slogan_label.clone();
        let icon_box_c = icon_box.clone();
        let dot_rx = status_dot.clone();
        let lbl_rx = status_label.clone();
        let mut event_rx = backend.subscribe_events();
        glib::MainContext::default().spawn_local(async move {
            refresh_brand_banner(&b, &title_c, &slogan_c, &icon_box_c).await;
            while let Ok(event) = event_rx.recv().await {
                match event {
                    DesktopEvent::AccountSwitched { .. } => {
                        set_status(&dot_rx, &lbl_rx, StatusKind::Connected);
                        refresh_brand_banner(&b, &title_c, &slogan_c, &icon_box_c).await;
                    }
                    DesktopEvent::AccountSignedOut { .. } => {
                        refresh_brand_banner(&b, &title_c, &slogan_c, &icon_box_c).await;
                    }
                    DesktopEvent::SyncStarted { .. } => {
                        set_status(&dot_rx, &lbl_rx, StatusKind::Syncing);
                    }
                    DesktopEvent::SyncCompleted { .. } => {
                        set_status(&dot_rx, &lbl_rx, StatusKind::Connected);
                    }
                    DesktopEvent::Notification {
                        level: NotificationLevel::Error,
                        ..
                    } => {
                        set_status(&dot_rx, &lbl_rx, StatusKind::Offline);
                    }
                    _ => {}
                }
            }
        });

        let list_box = ListBox::new();
        list_box.set_selection_mode(SelectionMode::Single);
        list_box.add_css_class("navigation-sidebar");
        list_box.set_hexpand(false);
        list_box.set_halign(Align::Fill);

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
        scroll.set_vscrollbar_policy(PolicyType::Automatic);
        scroll.set_vexpand(true);
        scroll.set_hexpand(false);
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
    label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    label.set_max_width_chars(16);

    hbox.append(&icon);
    hbox.append(&label);
    row.set_child(Some(&hbox));
    row
}

#[derive(Clone, Copy)]
enum StatusKind {
    Connected,
    Syncing,
    Offline,
}

fn set_status(dot: &GtkBox, label: &Label, kind: StatusKind) {
    for class in [
        "status-dot-connected",
        "status-dot-syncing",
        "status-dot-offline",
    ] {
        dot.remove_css_class(class);
    }
    let (class, text) = match kind {
        StatusKind::Connected => ("status-dot-connected", "Online"),
        StatusKind::Syncing => ("status-dot-syncing", "Syncing…"),
        StatusKind::Offline => ("status-dot-offline", "Offline"),
    };
    dot.add_css_class(class);
    label.set_text(text);
}

/// Re-reads branding for the active account and updates the banner title, slogan, color and logo.
async fn refresh_brand_banner<B: FileExplorerBackend>(
    backend: &B,
    title: &Label,
    slogan: &Label,
    icon_box: &GtkBox,
) {
    let has_account = backend.active_account_id().await.is_some();
    let theme = if has_account {
        backend.server_theming().await
    } else {
        None
    };
    let title_text = match (has_account, theme.as_ref().and_then(|t| t.name.clone())) {
        (false, _) => NO_ACCOUNT_TITLE.to_string(),
        (true, Some(name)) => name,
        (true, None) => "Nextcloud".to_string(),
    };
    title.set_text(&title_text);
    slogan.set_text(
        theme
            .as_ref()
            .and_then(|t| t.slogan.clone())
            .unwrap_or_default()
            .as_str(),
    );
    match theme.as_ref().and_then(|t| t.color.as_deref()) {
        Some(color) => {
            apply_server_theme_color(color, theme.as_ref().and_then(|t| t.color_text.as_deref()))
        }
        None => clear_server_theme_color(),
    }
    let logo = if has_account {
        backend.fetch_theme_logo().await
    } else {
        None
    };
    set_brand_logo(icon_box, logo.as_deref(), icon_box.scale_factor().max(2));
}

/// Banner title shown while no account profile exists.
const NO_ACCOUNT_TITLE: &str = "No account";

/// Renders `logo_path` into `icon_box`, falling back to a neutral folder icon when absent.
fn set_brand_logo(icon_box: &GtkBox, logo_path: Option<&Path>, scale: i32) {
    while let Some(child) = icon_box.first_child() {
        icon_box.remove(&child);
    }

    let texture = logo_path.and_then(|path| {
        // Render at high resolution for sharp subpixel anti-aliasing
        let target_w = 56 * scale.max(1);
        let target_h = 40 * scale.max(1);
        match gdk_pixbuf::Pixbuf::from_file_at_scale(path, target_w, target_h, true) {
            Ok(pixbuf) => Some(gtk4::gdk::Texture::for_pixbuf(&pixbuf)),
            Err(_) => {
                let file = gtk4::gio::File::for_path(path);
                gtk4::gdk::Texture::from_file(&file).ok()
            }
        }
    });

    match texture {
        Some(texture) => {
            let picture = gtk4::Picture::for_paintable(&texture);
            picture.set_can_shrink(true);
            picture.set_keep_aspect_ratio(true);
            picture.set_size_request(56, 40);
            icon_box.append(&picture);
        }
        None => {
            let icon = Image::from_icon_name("folder-remote-symbolic");
            icon.set_pixel_size(28);
            icon_box.append(&icon);
        }
    }
}

thread_local! {
    /// One provider is reused for every server theme so reloading it replaces the previous
    /// server's rules instead of stacking a second provider with the same priority.
    static THEME_PROVIDER: RefCell<Option<gtk4::CssProvider>> = const { RefCell::new(None) };
}

/// Dynamically updates CSS colors according to the server's brand identity.
pub fn apply_server_theme_color(color_hex: &str, text_color: Option<&str>) {
    let Some(clean_hex) = normalized_hex(color_hex) else {
        return;
    };
    load_theme_css(&server_theme_css(&clean_hex, text_color));
}

/// Returns `hex` trimmed and lowercased when it is a `#rrggbb` color, otherwise `None`.
fn normalized_hex(hex: &str) -> Option<String> {
    let clean = hex.trim();
    if clean.len() != 7 || !clean.starts_with('#') {
        return None;
    }
    if !clean[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(clean.to_ascii_lowercase())
}

/// Restores the built-in Nextcloud branding by dropping any server theme overrides.
pub fn clear_server_theme_color() {
    load_theme_css("");
}

fn load_theme_css(css: &str) {
    THEME_PROVIDER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let provider = gtk4::CssProvider::new();
            if let Some(display) = gtk4::gdk::Display::default() {
                gtk4::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk4::STYLE_PROVIDER_PRIORITY_USER,
                );
            }
            *slot = Some(provider);
        }
        if let Some(provider) = slot.as_ref() {
            provider.load_from_data(css);
        }
    });
}

fn server_theme_css(clean_hex: &str, text_color: Option<&str>) -> String {
    let r = u8::from_str_radix(&clean_hex[1..3], 16).unwrap_or(0);
    let g = u8::from_str_radix(&clean_hex[3..5], 16).unwrap_or(130);
    let b = u8::from_str_radix(&clean_hex[5..7], 16).unwrap_or(201);

    // Darker variant for smooth gradient bottom right
    let dark_r = (r as f32 * 0.70) as u8;
    let dark_g = (g as f32 * 0.70) as u8;
    let dark_b = (b as f32 * 0.70) as u8;
    let darker_hex = format!("#{dark_r:02x}{dark_g:02x}{dark_b:02x}");

    let text_hex = text_color.unwrap_or("#ffffff");

    format!(
        "
        @define-color accent_color {clean_hex};
        @define-color accent_bg_color {clean_hex};

        .navigation-sidebar-fixed {{
            min-width: 260px !important;
            max-width: 260px !important;
            width: 260px !important;
        }}
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
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_theme_css_embeds_brand_color_and_text_color() {
        let css = server_theme_css("#ff0000", Some("#101010"));
        assert!(css.contains("@define-color accent_color #ff0000;"));
        assert!(css.contains("linear-gradient(135deg, #ff0000 0%, #b20000 100%)"));
        assert!(css.contains("color: #101010;"));
        assert!(css.contains("alpha(#101010, 0.82)"));
    }

    #[test]
    fn server_theme_css_defaults_text_color_to_white() {
        let css = server_theme_css("#0082c9", None);
        assert!(css.contains("color: #ffffff;"));
        assert!(css.contains("alpha(#ffffff, 0.90)"));
    }

    #[test]
    fn normalized_hex_accepts_only_rrggbb_colors() {
        assert_eq!(normalized_hex("  #FF0000 ").as_deref(), Some("#ff0000"));
        assert_eq!(normalized_hex("0082c9"), None);
        assert_eq!(normalized_hex("#0082c"), None);
        assert_eq!(normalized_hex("#0082c9ff"), None);
        assert_eq!(normalized_hex("#zzzzzz"), None);
    }
}
