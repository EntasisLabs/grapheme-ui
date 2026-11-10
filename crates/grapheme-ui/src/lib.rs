//! GraphemeUI — the only crate that knows both Grapheme and rndr.
//!
//! # Dependency model (A)
//!
//! ```text
//! grapheme  (language, View IR, headless session)  — no UI deps
//!    ▲
//! grapheme-ui  ──►  rndr_core / rndr_ui            — this crate
//!    ▲
//! host app  (full Grapheme runner OR rndr app that opts in)
//! ```
//!
//! - Grapheme never depends on this crate or on rndr.
//! - rndr never depends on Grapheme or this crate.
//! - Host applications that want Grapheme UI add `grapheme-ui` directly
//!   (model A). A later optional rndr facade re-export is model B.
//!
//! The `desktop` feature adds the rndr window loop (`desktop::run`).
//! Headless tests do not enable it.
//!
//! # Surfaces
//!
//! [`GraphemeSurface`] mounts a UI glyph (`glyph Name on Store { … }`) under
//! an rndr parent entity, projects View IR into `Column`/`Row`/`Text`/`Button`
//! widgets, and routes button activations back through
//! [`grapheme_runtime::StoreSession::invoke_action`].

#![deny(missing_docs)]

#[cfg(feature = "desktop")]
pub mod desktop;
mod error;
mod project;
mod surface;

pub use error::GraphemeUiError;
pub use surface::{
    GraphemeNode, GraphemeSurface, ProjectedNode, drain_installed, install, with_driven_surface,
};
