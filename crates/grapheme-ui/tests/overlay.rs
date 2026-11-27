//! A broken save keeps the last good UI and shows a positioned overlay.

use std::path::Path;

use grapheme_ui::ERROR_OVERLAY_KEY;
use grapheme_ui::testing::Harness;

const COUNTER: &str = include_str!("../../../examples/counter/counter.gr");

#[test]
fn broken_save_overlays_the_last_good_ui_and_a_good_save_clears_it() {
    let mut ui = Harness::mount(COUNTER, Some("CounterApp")).unwrap();
    ui.click_labeled("Increment").unwrap();
    ui.assert_ir_str("/children/0/props/content", "Count: 1 (doubled: 2)");

    ui.reload_source("state S { n: Int = \n", Some(Path::new("app.gr")))
        .unwrap();
    ui.assert_ir_str("/children/0/props/content", "Count: 1 (doubled: 2)");
    let overlay = ui
        .access()
        .unwrap()
        .into_iter()
        .find(|entry| entry.key == ERROR_OVERLAY_KEY)
        .expect("error overlay");
    assert!(
        overlay.name.contains("app.gr") && overlay.name.contains(":1:"),
        "overlay should name the file, line and column: {}",
        overlay.name
    );
    assert!(
        overlay.name.to_lowercase().contains("parse") || overlay.name.contains("error"),
        "{}",
        overlay.name
    );

    ui.reload_source(COUNTER, Some(Path::new("app.gr")))
        .unwrap();
    assert!(
        ui.access()
            .unwrap()
            .iter()
            .all(|entry| entry.key != ERROR_OVERLAY_KEY),
        "good save clears the overlay"
    );
    ui.assert_ir_str("/children/0/props/content", "Count: 1 (doubled: 2)");
}
