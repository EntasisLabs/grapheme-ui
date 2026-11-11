//! Native window loop. Enable the crate feature `desktop`.
//!
//! Headless tests never call [`run`]. Opening a window needs a display.

use std::fmt;
use std::path::Path;

use rndr_core::{App, Layout, LogicalSize, Size, WindowConfig};

use crate::{GraphemeSurface, GraphemeUiError, LoadError, LoadedProgram, install, load_path};

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

/// Open a loaded program's glyph in one full-window slot and block until the
/// window closes.
///
/// This is the whole `grapheme-ui run` host: a window titled with the glyph
/// name, a padded column filling it, and the glyph mounted under that column.
/// Hosts such as `grapheme run` call this instead of shelling out.
pub fn run_program(loaded: &LoadedProgram) -> Result<(), GraphemeUiError> {
    let mut app = App::new();
    install(&mut app);
    app.world_mut().insert_resource(WindowConfig {
        title: loaded.glyph.clone(),
        size: LogicalSize::new(520.0, 640.0),
        min_size: Some(LogicalSize::new(320.0, 240.0)),
        ..WindowConfig::default()
    });
    let parent = app.world_mut().spawn((Layout {
        padding: 24.0,
        gap: 8.0,
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let surface =
        GraphemeSurface::mount(app.world_mut(), parent, &loaded.source, Some(&loaded.glyph))?;
    surface.drive(&mut app)?;
    run(app);
    Ok(())
}

/// Failure from [`run_file`]: the file did not load, or the surface did not mount.
#[derive(Debug)]
pub enum RunFileError {
    /// Read, compile, or glyph-selection failure, with diagnostics.
    Load(LoadError),
    /// The program loaded but the surface failed to mount or drive.
    Surface(GraphemeUiError),
}

impl fmt::Display for RunFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(error) => write!(f, "{error}"),
            Self::Surface(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RunFileError {}

/// [`load_path`] then [`run_program`]: open `path` in a window and block until
/// it closes. With no `glyph`, the file must contain exactly one UI glyph.
pub fn run_file(path: &Path, glyph: Option<&str>) -> Result<(), RunFileError> {
    let loaded = load_path(path, glyph).map_err(RunFileError::Load)?;
    run_program(&loaded).map_err(RunFileError::Surface)
}
