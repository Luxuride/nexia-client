//! Error types for the desktop user interface.

use thiserror::Error;

/// Root error type for UI operations and GTK bindings.
#[derive(Debug, Error)]
pub enum UiError {
    /// Failed to initialize GTK or GUI subsystem.
    #[error("failed to initialize GUI subsystem: {0}")]
    Initialization(String),

    /// GLib event loop or dispatcher channel failure.
    #[error("GLib channel dispatch error: {0}")]
    Dispatch(String),

    /// Controller or backend coordination failure.
    #[error("UI controller error: {0}")]
    Controller(String),
}
