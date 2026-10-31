//! Error types for the desktop user interface.

use std::error::Error as StdError;

use thiserror::Error;

/// Root error type for UI operations and GTK bindings.
///
/// Every variant names the operation that failed and keeps the underlying cause reachable through
/// [`StdError::source`], so a caller can log the chain instead of a flattened sentence.
#[derive(Debug, Error)]
pub enum UiError {
    /// Failed to initialize GTK or the GUI subsystem.
    #[error("failed to initialize the GUI subsystem: {message}")]
    Initialization {
        /// What was being initialized.
        message: String,
        /// Underlying cause, when the failure came from another layer.
        #[source]
        source: Option<Box<dyn StdError + Send + Sync>>,
    },

    /// A parameter was rejected before any backend call was made.
    #[error("invalid input: {message}")]
    InvalidInput {
        /// Why the input was rejected.
        message: &'static str,
    },

    /// A controller operation failed.
    #[error("UI controller error: {operation}: {source}")]
    Controller {
        /// Operation that failed, named so a log line identifies the call.
        operation: &'static str,
        /// Underlying coordinator or backend error.
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
}

impl UiError {
    /// Wraps a backend error, naming the operation that failed.
    pub(crate) fn controller<E>(operation: &'static str, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::Controller {
            operation,
            source: Box::new(source),
        }
    }

    /// Builds an initialization failure whose cause is another error.
    pub(crate) fn initialization_with_source<E>(message: impl Into<String>, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::Initialization {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn controller_error_keeps_the_source_and_the_operation() {
        let cause = std::io::Error::other("pipe closed");
        let err = UiError::controller("refresh listing", cause);

        assert_eq!(
            err.to_string(),
            "UI controller error: refresh listing: pipe closed"
        );
        assert_eq!(
            err.source().expect("source is preserved").to_string(),
            "pipe closed"
        );
    }

    #[test]
    fn initialization_error_without_a_cause_has_no_source() {
        let err = UiError::Initialization {
            message: "display unavailable".to_string(),
            source: None,
        };
        assert_eq!(
            err.to_string(),
            "failed to initialize the GUI subsystem: display unavailable"
        );
        assert!(err.source().is_none());
    }
}
