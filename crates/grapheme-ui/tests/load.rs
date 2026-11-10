//! Loader returns a glyph name or structured diagnostics. It does not panic.

use std::path::Path;

use grapheme_ui::{load_path, load_str};

const FEED: &str = include_str!("../../../examples/feed-view.gr");

#[test]
fn load_str_picks_the_only_glyph() {
    let loaded = load_str(FEED, Some(Path::new("examples/feed-view.gr")), None).unwrap();
    assert_eq!(loaded.glyph, "FeedApp");
    assert!(loaded.source.contains("TextField"));
}

#[test]
fn load_str_selects_a_glyph_by_name() {
    let loaded = load_str(FEED, None, Some("FeedApp")).unwrap();
    assert_eq!(loaded.glyph, "FeedApp");
    let missing =
        load_str(FEED, Some(Path::new("examples/feed-view.gr")), Some("Nope")).unwrap_err();
    let diagnostic = &missing.diagnostics[0];
    assert!(
        diagnostic.message.contains("Nope"),
        "{}",
        diagnostic.message
    );
    assert_eq!(diagnostic.file.as_deref(), Some("examples/feed-view.gr"));
    assert!(diagnostic.line.is_none());
}

#[test]
fn parse_error_keeps_line_and_column() {
    let err = load_str("state S { n: Int = \n", Some(Path::new("bad.gr")), None).unwrap_err();
    let diagnostic = &err.diagnostics[0];
    assert!(
        diagnostic.message.to_lowercase().contains("parse"),
        "{}",
        diagnostic.message
    );
    assert_eq!(diagnostic.file.as_deref(), Some("bad.gr"));
    assert!(
        diagnostic.line.is_some() && diagnostic.column.is_some(),
        "expected a position in {diagnostic:?}"
    );
    let rendered = err.to_string();
    assert!(
        rendered.starts_with("bad.gr:"),
        "formatted diagnostic should lead with the file: {rendered}"
    );
}

#[test]
fn type_error_has_no_position() {
    // TextField `change` supplies String. The SDK reports the mismatch as a
    // type error without a source position.
    let err = load_str(
        r#"
state S { n: Int = 0 }
mutation Set($value: Int) on S { apply { n: $value } }
view V on S {
  TextField(label: "Q") { on change: Set }
}
glyph App on S { V }
"#,
        Some(Path::new("typed.gr")),
        None,
    )
    .unwrap_err();
    let diagnostic = &err.diagnostics[0];
    assert!(
        diagnostic.message.to_lowercase().contains("type"),
        "{}",
        diagnostic.message
    );
    assert!(
        diagnostic.line.is_none() && diagnostic.column.is_none(),
        "type errors from the SDK have no position: {diagnostic:?}"
    );
}

#[test]
fn program_without_a_glyph_is_a_diagnostic() {
    let source = "state S { n: Int = 0 }\nmutation Nop on S { apply { n: $n } }\n";
    let err = load_str(source, None, None).unwrap_err();
    assert!(
        err.diagnostics[0].message.contains("no UI glyph"),
        "{}",
        err.diagnostics[0].message
    );
}

#[test]
fn load_path_reads_feed_view_and_reports_a_missing_file() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/feed-view.gr");
    let loaded = load_path(&path, None).unwrap();
    assert_eq!(loaded.glyph, "FeedApp");
    assert_eq!(loaded.path.as_deref(), Some(path.as_path()));

    let missing = load_path(Path::new("examples/no-such-file.gr"), None).unwrap_err();
    let diagnostic = &missing.diagnostics[0];
    assert!(
        diagnostic.message.contains("no-such-file.gr"),
        "{}",
        diagnostic.message
    );
    assert!(diagnostic.line.is_none());
}
