//! Nautilus-style Cloud Files explorer components, toolbar, and views.

pub mod canvas_menu;
pub mod context_menu;
pub mod dnd;
pub mod grid_view;
pub mod launcher;
pub mod list_view;
pub mod new_folder;
pub mod new_item_dialog;
pub mod page;
pub mod path_bar;
pub mod properties_dialog;
pub mod rename_dialog;
pub mod rubberband;
pub mod selection;
pub mod sort_popover;
pub mod status_bar;
pub mod thumbnail_dialog;
pub mod toolbar;

pub use canvas_menu::CanvasContextMenu;
pub use context_menu::ItemContextMenu;
pub use dnd::FileDragDrop;
pub use grid_view::{FileGridTile, FileGridView};
pub use launcher::launch_file_externally;
pub use list_view::{FileListRow, FileListView};
pub use new_folder::NewFolderPopover;
pub use new_item_dialog::NewItemDialog;
pub use page::CloudFilesPage;
pub use path_bar::PathBar;
pub use properties_dialog::PropertiesDialog;
pub use rename_dialog::RenameDialog;
pub use rubberband::RubberbandOverlay;
pub use selection::SelectionTracker;
pub use sort_popover::SortPopover;
pub use status_bar::FileStatusBar;
pub use thumbnail_dialog::ThumbnailDownloadsDialog;
pub use toolbar::PathToolbar;
