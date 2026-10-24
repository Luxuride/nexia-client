//! Nautilus-style Cloud Files explorer components, toolbar, and views.

pub mod context_menu;
pub mod dnd;
pub mod grid_view;
pub mod list_view;
pub mod new_folder;
pub mod page;
pub mod path_bar;
pub mod rename_dialog;
pub mod rubberband;
pub mod selection;
pub mod status_bar;
pub mod toolbar;

#[cfg(feature = "gtk")]
pub use context_menu::ItemContextMenu;
#[cfg(feature = "gtk")]
pub use dnd::FileDragDrop;
#[cfg(feature = "gtk")]
pub use grid_view::{FileGridTile, FileGridView};
#[cfg(feature = "gtk")]
pub use list_view::{FileListRow, FileListView};
#[cfg(feature = "gtk")]
pub use new_folder::NewFolderPopover;
#[cfg(feature = "gtk")]
pub use page::CloudFilesPage;
#[cfg(feature = "gtk")]
pub use path_bar::PathBar;
#[cfg(feature = "gtk")]
pub use rename_dialog::RenameDialog;
#[cfg(feature = "gtk")]
pub use rubberband::RubberbandOverlay;
#[cfg(feature = "gtk")]
pub use selection::SelectionTracker;
#[cfg(feature = "gtk")]
pub use status_bar::FileStatusBar;
#[cfg(feature = "gtk")]
pub use toolbar::PathToolbar;
