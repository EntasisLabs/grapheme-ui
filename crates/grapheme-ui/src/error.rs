//! Errors from mounting or projecting a Grapheme surface.

use thiserror::Error;

/// Failure while mounting, syncing, or dispatching actions on a surface.
#[derive(Debug, Error)]
pub enum GraphemeUiError {
    /// Grapheme compile / session error.
    #[error(transparent)]
    Session(#[from] grapheme_sdk::GraphemeSdkError),
    /// rndr UI mount / props error.
    #[error("rndr ui error: {0:?}")]
    Ui(rndr_ui::UiError),
    /// View IR shape this version of GraphemeUI does not project yet.
    #[error("{0}")]
    Unsupported(String),
    /// An action id is no longer in the current list (binding epoch moved).
    #[error("{0}")]
    StaleAction(String),
    /// Programmer misuse of the surface API.
    #[error("{0}")]
    Usage(String),
}

impl From<rndr_ui::UiError> for GraphemeUiError {
    fn from(value: rndr_ui::UiError) -> Self {
        Self::Ui(value)
    }
}
