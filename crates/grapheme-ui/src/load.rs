//! Load a `.gr` file, compile it, and choose the UI glyph to mount.
//!
//! Compile and mount failures come back as [`Diagnostic`]s. This module does
//! not panic on a bad program.

use std::fs;
use std::path::Path;

use grapheme_sdk::{GraphemeEngine, GraphemeSdkError};

/// One compile or load failure.
///
/// `line` and `column` are set when the compiler message carries a position
/// (parse errors do). Type errors from the current SDK do not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Compiler or I/O message, unchanged.
    pub message: String,
    /// Path the host asked to load, when there was one.
    pub file: Option<String>,
    /// 1-based line, when the message includes one.
    pub line: Option<u32>,
    /// 1-based column, when the message includes one.
    pub column: Option<u32>,
}

/// A program that compiled and resolved to one UI glyph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedProgram {
    /// Source that compiled.
    pub source: String,
    /// Path passed to [`load_path`], if any.
    pub path: Option<std::path::PathBuf>,
    /// Glyph [`crate::GraphemeSurface::mount`] should open.
    pub glyph: String,
}

/// Compile or glyph-selection failure. Display prints one line per diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadError {
    /// One or more diagnostics. Empty only if a caller builds it that way.
    pub diagnostics: Vec<Diagnostic>,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{}", format_diagnostic(diagnostic))?;
        }
        Ok(())
    }
}

impl std::error::Error for LoadError {}

/// Read `path` and [`load_str`] it.
pub fn load_path(path: &Path, glyph: Option<&str>) -> Result<LoadedProgram, LoadError> {
    let source = fs::read_to_string(path).map_err(|err| {
        LoadError::one(Diagnostic {
            message: format!("failed to read {}: {err}", path.display()),
            file: Some(path.display().to_string()),
            line: None,
            column: None,
        })
    })?;
    let mut loaded = load_str(&source, Some(path), glyph)?;
    loaded.path = Some(path.to_path_buf());
    Ok(loaded)
}

/// Compile `source`, then mount the only UI glyph or `glyph` when named.
///
/// `path` is recorded on diagnostics and is not read. `Ok` returns the glyph
/// name the session actually mounted.
pub fn load_str(
    source: &str,
    path: Option<&Path>,
    glyph: Option<&str>,
) -> Result<LoadedProgram, LoadError> {
    let engine = GraphemeEngine::builder().build();
    let mut session = engine
        .store_session(source)
        .map_err(|err| LoadError::one(from_sdk(&err, path)))?;
    let snapshot = session
        .mount(glyph)
        .map_err(|err| LoadError::one(from_sdk(&err, path)))?;
    Ok(LoadedProgram {
        source: source.to_string(),
        path: path.map(Path::to_path_buf),
        glyph: snapshot.glyph,
    })
}

/// `file:line:col: message` when a position is known, otherwise the message.
pub fn format_diagnostic(diagnostic: &Diagnostic) -> String {
    match (&diagnostic.file, diagnostic.line, diagnostic.column) {
        (Some(file), Some(line), Some(column)) => {
            format!("{file}:{line}:{column}: {}", diagnostic.message)
        }
        (Some(file), Some(line), None) => format!("{file}:{line}: {}", diagnostic.message),
        (Some(file), None, None) => format!("{file}: {}", diagnostic.message),
        (None, Some(line), Some(column)) => {
            format!("{line}:{column}: {}", diagnostic.message)
        }
        _ => diagnostic.message.clone(),
    }
}

impl LoadError {
    fn one(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }
}

fn from_sdk(err: &GraphemeSdkError, path: Option<&Path>) -> Diagnostic {
    let message = err.to_string();
    let (line, column) = line_col(&message);
    Diagnostic {
        message,
        file: path.map(|path| path.display().to_string()),
        line,
        column,
    }
}

/// Pull `line:column` out of a compiler message.
///
/// pest prints `--> 1:5` or `--> file.gr:1:5`. Type errors have neither.
fn line_col(message: &str) -> (Option<u32>, Option<u32>) {
    let Some(rest) = message.split("-->").nth(1) else {
        return (None, None);
    };
    let token = rest
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches(':');
    let mut parts: Vec<&str> = token.rsplitn(3, ':').collect();
    parts.reverse();
    if parts.len() < 2 {
        return (None, None);
    }
    let column = parts.pop().and_then(|text| text.parse().ok());
    let line = parts.pop().and_then(|text| text.parse().ok());
    match (line, column) {
        (Some(line), Some(column)) => (Some(line), Some(column)),
        _ => (None, None),
    }
}
