//! Modal dialog for visually browsing and choosing a remote Nextcloud directory.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Button, Image, Label, Orientation, ProgressBar, ScrolledWindow};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::widget::FileExplorerBackend;

/// Modal subwindow enabling visual browsing and selection of a remote directory on Nextcloud.
pub struct RemoteDirectoryBrowserDialog;

impl RemoteDirectoryBrowserDialog {
    /// Presents the remote directory browser dialog.
    pub fn present<B: FileExplorerBackend>(
        parent: Option<&impl IsA<gtk4::Widget>>,
        backend: &B,
        initial_path: &str,
        on_selected: Rc<dyn Fn(String)>,
    ) {
        let root_window = parent
            .and_then(|w| w.root())
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let window = adw::Window::builder()
            .title("Choose Remote Folder")
            .default_width(460)
            .default_height(520)
            .modal(true)
            .build();

        if let Some(ref win) = root_window {
            window.set_transient_for(Some(win));
        }

        let header_bar = adw::HeaderBar::new();
        header_bar.set_show_end_title_buttons(true);
        header_bar.set_show_start_title_buttons(false);

        let current_path = Rc::new(RefCell::new({
            let clean = format!("/{}", initial_path.trim_matches('/'));
            if clean == "//" {
                "/".to_string()
            } else {
                clean
            }
        }));

        let content_box = gtk4::Box::new(Orientation::Vertical, 12);
        content_box.set_margin_top(12);
        content_box.set_margin_bottom(12);
        content_box.set_margin_start(16);
        content_box.set_margin_end(16);

        // Path navigation bar
        let path_bar = gtk4::Box::new(Orientation::Horizontal, 8);
        path_bar.set_hexpand(true);

        let up_btn = Button::from_icon_name("go-up-symbolic");
        up_btn.set_tooltip_text(Some("Go to parent directory"));
        up_btn.add_css_class("flat");

        let path_label = Label::new(Some(current_path.borrow().as_str()));
        path_label.set_hexpand(true);
        path_label.set_halign(Align::Start);
        path_label.add_css_class("title-4");

        path_bar.append(&up_btn);
        path_bar.append(&path_label);
        content_box.append(&path_bar);

        let pbar = ProgressBar::new();
        pbar.set_size_request(-1, 3);
        pbar.set_fraction(1.0);
        content_box.append(&pbar);

        // Folders list group
        let folders_group = adw::PreferencesGroup::builder()
            .title("Remote Folders")
            .description("Select a folder or click to navigate into subdirectories.")
            .build();

        let tracked_rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));

        let scroll = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .min_content_height(260)
            .vexpand(true)
            .child(&folders_group)
            .build();
        content_box.append(&scroll);

        // Action buttons
        let actions_box = gtk4::Box::new(Orientation::Horizontal, 10);
        actions_box.set_halign(Align::End);
        actions_box.set_margin_top(8);

        let cancel_btn = Button::with_label("Cancel");
        let win_cancel = window.clone();
        cancel_btn.connect_clicked(move |_| {
            win_cancel.close();
        });

        let select_btn = Button::with_label("Select Current Folder");
        select_btn.add_css_class("suggested-action");

        let curr_for_sel = current_path.clone();
        let win_select = window.clone();
        let cb_select = on_selected;
        select_btn.connect_clicked(move |_| {
            let chosen = curr_for_sel.borrow().clone();
            cb_select(chosen);
            win_select.close();
        });

        actions_box.append(&cancel_btn);
        actions_box.append(&select_btn);
        content_box.append(&actions_box);

        // Up button wire
        let curr_up = current_path.clone();
        let b_up = backend.clone();
        let lbl_up = path_label.clone();
        let pb_up = pbar.clone();
        let grp_up = folders_group.clone();
        let tr_up = tracked_rows.clone();

        up_btn.connect_clicked(move |_| {
            let current = curr_up.borrow().clone();
            if current != "/" {
                let parent = std::path::Path::new(&current)
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("/");
                let clean_parent = if parent.is_empty() { "/" } else { parent };
                *curr_up.borrow_mut() = clean_parent.to_string();
                load_remote_folders(&b_up, &curr_up, &lbl_up, &pb_up, &grp_up, &tr_up);
            }
        });

        // Initial load
        load_remote_folders(
            &backend.clone(),
            &current_path,
            &path_label,
            &pbar,
            &folders_group,
            &tracked_rows,
        );

        let main_box = gtk4::Box::new(Orientation::Vertical, 0);
        main_box.append(&header_bar);
        main_box.append(&content_box);
        window.set_content(Some(&main_box));
        window.present();
    }
}

fn load_remote_folders<B: FileExplorerBackend>(
    backend: &B,
    current_path: &Rc<RefCell<String>>,
    path_label: &Label,
    progress_bar: &ProgressBar,
    folders_group: &adw::PreferencesGroup,
    tracked_rows: &Rc<RefCell<Vec<adw::ActionRow>>>,
) {
    let path = current_path.borrow().clone();
    path_label.set_text(&path);
    progress_bar.set_fraction(0.3);

    let b = backend.clone();
    let curr = current_path.clone();
    let lbl = path_label.clone();
    let pb = progress_bar.clone();
    let grp = folders_group.clone();
    let tr = tracked_rows.clone();

    glib::MainContext::default().spawn_local(async move {
        for r in tr.borrow_mut().drain(..) {
            grp.remove(&r);
        }

        match b.list_remote_directory_folders(&path).await {
            Ok(folders) => {
                pb.set_fraction(1.0);
                if folders.is_empty() {
                    let empty_row = adw::ActionRow::builder()
                        .title("No Subfolders")
                        .subtitle("This directory contains no subfolders")
                        .build();
                    empty_row.add_prefix(&Image::from_icon_name("folder-open-symbolic"));
                    grp.add(&empty_row);
                    tr.borrow_mut().push(empty_row);
                } else {
                    for folder_path in folders {
                        let folder_name = std::path::Path::new(&folder_path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&folder_path)
                            .to_string();

                        let row = adw::ActionRow::builder()
                            .title(&folder_name)
                            .subtitle(&folder_path)
                            .activatable(true)
                            .build();
                        row.add_prefix(&Image::from_icon_name("folder-symbolic"));
                        row.add_suffix(&Image::from_icon_name("go-next-symbolic"));

                        let b_click = b.clone();
                        let curr_click = curr.clone();
                        let lbl_click = lbl.clone();
                        let pb_click = pb.clone();
                        let grp_click = grp.clone();
                        let tr_click = tr.clone();
                        let target = folder_path.clone();

                        row.connect_activated(move |_| {
                            *curr_click.borrow_mut() = target.clone();
                            load_remote_folders(
                                &b_click,
                                &curr_click,
                                &lbl_click,
                                &pb_click,
                                &grp_click,
                                &tr_click,
                            );
                        });

                        grp.add(&row);
                        tr.borrow_mut().push(row);
                    }
                }
            }
            Err(e) => {
                pb.set_fraction(1.0);
                let err_row = adw::ActionRow::builder()
                    .title("Failed to Load Directory")
                    .subtitle(e.to_string())
                    .build();
                err_row.add_prefix(&Image::from_icon_name("dialog-warning-symbolic"));
                grp.add(&err_row);
                tr.borrow_mut().push(err_row);
            }
        }
    });
}
