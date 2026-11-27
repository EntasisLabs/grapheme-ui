//! Shared mounts for the harness and golden tests.

use grapheme_ui::testing::{Harness, Key};

const COUNTER: &str = include_str!("../../../../examples/counter/counter.gr");
const FEED: &str = include_str!("../../../../examples/feed-view.gr");

pub fn counter_initial() -> Harness {
    Harness::mount(COUNTER, Some("CounterApp")).expect("mount counter")
}

pub fn counter_after_click() -> Harness {
    let mut ui = counter_initial();
    ui.click_labeled("Increment").expect("click Increment");
    ui
}

pub fn counter_after_typing() -> Harness {
    let mut ui = counter_initial();
    ui.press(Key::Tab).expect("focus the first button");
    ui.press(Key::Tab).expect("focus Add 5");
    ui.press(Key::Enter).expect("activate Add 5");
    ui
}

pub fn feed_initial() -> Harness {
    Harness::mount(FEED, Some("FeedApp")).expect("mount feed")
}

pub fn feed_after_click() -> Harness {
    let mut ui = feed_initial();
    ui.click_labeled("Load feed").expect("click Load feed");
    ui
}

pub fn feed_after_typing() -> Harness {
    let mut ui = feed_initial();
    ui.type_into("/search", "Hello").expect("type into search");
    ui
}
