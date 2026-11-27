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
//! Headless tests do not enable it. The `capture` feature turns on
//! offscreen PNG snapshots in [`testing`] (`rndr_render::capture`).
//!
//! [`testing::Harness`] mounts a `.gr` file without a window and drives it
//! the way a click or keypress would. [`testing::Harness::settle`] is where
//! a later worker-thread session waits for the next snapshot.
//!
//! # Surfaces
//!
//! [`GraphemeSurface`] mounts a UI glyph (`glyph Name on Store { … }`) under
//! an rndr parent entity, projects View IR into `Column`/`Row`/`Text`/`Button`/
//! `TextField` widgets, and routes activations back through
//! [`grapheme_sdk::GraphemeStoreSession::invoke_action`]. Button clicks pass
//! no payload. A `TextField` `change` passes `{ "value": <text> }`.

#![deny(missing_docs)]

mod bridge;
mod catalog;
#[cfg(feature = "desktop")]
pub mod desktop;
mod engine;
mod error;
mod load;
mod project;
mod surface;
pub mod testing;
#[cfg(any(test, feature = "desktop"))]
mod watch;

pub use engine::{
    CapabilityCall, CapabilityHost, HostCallError, ModuleRegistry, PolicyGuard, SurfaceBuilder,
};
pub use error::GraphemeUiError;
pub use load::{Diagnostic, LoadError, LoadedProgram, format_diagnostic, load_path, load_str};
pub use surface::{
    DrainStatus, ERROR_OVERLAY_KEY, GraphemeNode, GraphemeSurface, ProjectedNode, drain_installed,
    install, with_driven_surface,
};
