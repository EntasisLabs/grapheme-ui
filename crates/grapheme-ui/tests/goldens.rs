//! View IR goldens for counter and feed-view.
//!
//! These files are JSON and run on every OS. PNG goldens live in the same
//! directory and are compared only when the `capture` feature is on.

#[path = "support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/goldens")
        .join(name)
}

#[test]
fn counter_initial_ir() {
    support::counter_initial()
        .assert_ir_golden(golden("counter-initial.json"))
        .unwrap();
}

#[test]
fn counter_after_click_ir() {
    support::counter_after_click()
        .assert_ir_golden(golden("counter-after-click.json"))
        .unwrap();
}

#[test]
fn counter_after_typing_ir() {
    support::counter_after_typing()
        .assert_ir_golden(golden("counter-after-typing.json"))
        .unwrap();
}

#[test]
fn feed_initial_ir() {
    support::feed_initial()
        .assert_ir_golden(golden("feed-initial.json"))
        .unwrap();
}

#[test]
fn feed_after_click_ir() {
    support::feed_after_click()
        .assert_ir_golden(golden("feed-after-click.json"))
        .unwrap();
}

#[test]
fn feed_after_typing_ir() {
    support::feed_after_typing()
        .assert_ir_golden(golden("feed-after-typing.json"))
        .unwrap();
}

#[cfg(feature = "capture")]
mod png {
    use super::golden;

    #[test]
    fn counter_initial_png() {
        super::support::counter_initial()
            .assert_png_golden(golden("counter-initial.png"))
            .unwrap();
    }

    #[test]
    fn counter_after_click_png() {
        super::support::counter_after_click()
            .assert_png_golden(golden("counter-after-click.png"))
            .unwrap();
    }

    #[test]
    fn counter_after_typing_png() {
        super::support::counter_after_typing()
            .assert_png_golden(golden("counter-after-typing.png"))
            .unwrap();
    }

    #[test]
    fn feed_initial_png() {
        super::support::feed_initial()
            .assert_png_golden(golden("feed-initial.png"))
            .unwrap();
    }

    #[test]
    fn feed_after_click_png() {
        super::support::feed_after_click()
            .assert_png_golden(golden("feed-after-click.png"))
            .unwrap();
    }

    #[test]
    fn feed_after_typing_png() {
        super::support::feed_after_typing()
            .assert_png_golden(golden("feed-after-typing.png"))
            .unwrap();
    }
}
