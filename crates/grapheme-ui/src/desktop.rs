//! Native window loop. Enable the crate feature `desktop`.
//!
//! Headless tests never call [`run`]. Opening a window needs a display.

use rndr_core::App;

/// Extra window, for a host that outgrows the single [`run`] loop.
///
/// Re-exported so the `desktop` feature's direct `rndr_render` dependency is
/// the winit/wgpu crate, not only a transitive edge through the `rndr` facade.
pub use rndr_render::Session;

/// Open the window described by [`rndr_core::WindowConfig`] and block until it closes.
///
/// Drain Grapheme actions with [`crate::GraphemeSurface::drive`] before calling
/// this, so a click is applied on the Update of the frame that activated the
/// button. [`crate::install`] must already have run.
pub fn run(app: App) {
    rndr::run(app);
}
