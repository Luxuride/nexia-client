//! Android UI bindings and UniFFI interface definitions.
//!
//! # Platform Architecture & Compatibility
//!
//! The **Desktop app is the source of truth** for all Nextcloud client features,
//! view models, sync state machines, and business logic. The Android implementation
//! is designed to match the desktop application's functionality over UniFFI scaffolding,
//! exposing idiomatic foreign function interfaces for Kotlin, Android NDK, and Jetpack Compose.
//!
//! > **Status: TODO / In Progress**
//! >
//! > The Android version is currently a work-in-progress (TODO). The interface structures
//! > defined here mirror the desktop contracts and will be expanded to full parity.

use crate::controller::DesktopUiController;
use crate::model::{FileViewItem, NavigationPage};
use std::sync::Arc;

/// Android bridge controller matching [`DesktopUiController`] functionality.
///
/// **Status: TODO** - Android version is matching the functionality of the desktop app.
#[derive(Clone)]
pub struct AndroidUiBridge {
    _controller: Arc<DesktopUiController>,
}

impl AndroidUiBridge {
    /// Constructs a new Android UI bridge wrapping the desktop controller.
    pub fn new(controller: DesktopUiController) -> Self {
        Self {
            _controller: Arc::new(controller),
        }
    }

    /// Fetches the list of items for the given remote path.
    ///
    /// **Status: TODO** - Android implementation matching desktop file navigation.
    pub async fn fetch_items(&self, _path: &str) -> Vec<FileViewItem> {
        // TODO: Android version matching desktop controller functionality
        Vec::new()
    }

    /// Navigates to a target page in the Android mobile interface.
    ///
    /// **Status: TODO** - Android implementation matching desktop page navigation.
    pub fn navigate_page(&self, _page: NavigationPage) {
        // TODO: Android version matching desktop navigation
    }
}
