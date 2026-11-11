//! `grapheme-ui run <file.gr> [--glyph NAME]`
//!
//! Compiles a Grapheme UI glyph and opens it in an rndr window titled with
//! the glyph name. Compile failures print diagnostics and exit non-zero.
//! The window itself needs the `desktop` feature and a display.
//!
//! ```text
//! cargo run -p grapheme-ui-cli --features desktop -- run examples/feed-view.gr
//! ```

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use grapheme_ui::{LoadError, LoadedProgram, format_diagnostic, load_path};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(message)) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
        Err(CliError::Load(error)) => {
            if error.diagnostics.is_empty() {
                eprintln!("error: {error}");
            } else {
                for diagnostic in &error.diagnostics {
                    eprintln!("error: {}", format_diagnostic(diagnostic));
                }
            }
            ExitCode::from(1)
        }
        Err(CliError::Message(message)) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Load(LoadError),
    Message(String),
}

#[derive(Debug)]
struct RunArgs {
    file: PathBuf,
    glyph: Option<String>,
}

fn run(args: Vec<String>) -> Result<(), CliError> {
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Err(CliError::Usage(usage()));
    }
    if args[0] != "run" {
        return Err(CliError::Usage(format!(
            "unknown command '{}'\n{}",
            args[0],
            usage()
        )));
    }
    let parsed = parse_run_args(&args[1..])?;
    let loaded = load_path(&parsed.file, parsed.glyph.as_deref()).map_err(CliError::Load)?;
    launch(loaded)
}

fn parse_run_args(args: &[String]) -> Result<RunArgs, CliError> {
    let mut file = None;
    let mut glyph = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--glyph" => {
                let name = args.get(index + 1).ok_or_else(|| {
                    CliError::Usage("--glyph requires a glyph name\n".to_string() + &usage())
                })?;
                if name.starts_with('-') {
                    return Err(CliError::Usage(
                        "--glyph requires a glyph name\n".to_string() + &usage(),
                    ));
                }
                glyph = Some(name.clone());
                index += 2;
            }
            other if other.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown flag '{other}'\n{}",
                    usage()
                )));
            }
            other => {
                if file.is_some() {
                    return Err(CliError::Usage(format!(
                        "unexpected argument '{other}'\n{}",
                        usage()
                    )));
                }
                file = Some(PathBuf::from(other));
                index += 1;
            }
        }
    }
    let file = file.ok_or_else(|| CliError::Usage(usage()))?;
    Ok(RunArgs { file, glyph })
}

fn usage() -> String {
    "\
usage:
  grapheme-ui run <file.gr> [--glyph NAME]

Compiles the file and opens its UI glyph in a window titled with the glyph
name. With no --glyph, the file must contain exactly one. Compile errors
print a message and a line and column when the compiler reports them.

The window needs a display build:
  cargo run -p grapheme-ui-cli --features desktop -- run <file.gr>"
        .to_string()
}

#[cfg(not(feature = "desktop"))]
fn launch(_loaded: LoadedProgram) -> Result<(), CliError> {
    Err(CliError::Message(
        "grapheme-ui run needs the desktop feature to open a window. \
         Rebuild with: cargo run -p grapheme-ui-cli --features desktop -- run <file.gr>"
            .into(),
    ))
}

#[cfg(feature = "desktop")]
fn launch(loaded: LoadedProgram) -> Result<(), CliError> {
    grapheme_ui::desktop::run_program(&loaded).map_err(|err| CliError::Message(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_run_args_accepts_glyph_before_or_after_the_file() {
        let args = vec![
            "examples/feed-view.gr".into(),
            "--glyph".into(),
            "FeedApp".into(),
        ];
        let parsed = parse_run_args(&args).unwrap();
        assert_eq!(parsed.file, PathBuf::from("examples/feed-view.gr"));
        assert_eq!(parsed.glyph.as_deref(), Some("FeedApp"));

        let args = vec!["--glyph".into(), "FeedApp".into(), "app.gr".into()];
        let parsed = parse_run_args(&args).unwrap();
        assert_eq!(parsed.file, PathBuf::from("app.gr"));
        assert_eq!(parsed.glyph.as_deref(), Some("FeedApp"));
    }

    #[test]
    fn missing_glyph_name_is_usage() {
        let err = parse_run_args(&["--glyph".into()]).unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }
}
