//! The testing harness mounts a glyph, drives pointer and keyboard input,
//! and reads View IR, the access tree, and laid-out rects. No window and no GPU.

#[path = "support/mod.rs"]
mod support;

#[test]
fn counter_click_and_keyboard_change_the_label() {
    let mut ui = support::counter_initial();
    ui.assert_ir_str("/children/0/props/content", "Count: 0 (doubled: 0)");
    ui.assert_access_role("/increment", "button");
    ui.assert_access_role("/value", "label");
    ui.assert_access_role("CounterView", "group");
    ui.assert_rect("/increment", |rect| rect.w > 0.0 && rect.h > 0.0);
    ui.assert_rect("/value", |rect| rect.w > 0.0 && rect.h > 0.0);

    let rects = ui.rects().unwrap();
    let value = rects
        .iter()
        .find(|rect| rect.key.ends_with("/value"))
        .unwrap();
    let increment = rects
        .iter()
        .find(|rect| rect.key.ends_with("/increment"))
        .unwrap();
    let add_five = rects
        .iter()
        .find(|rect| rect.key.ends_with("/add_five"))
        .unwrap();
    assert!(
        value.y < increment.y,
        "label should sit above the buttons: {value:?} {increment:?}"
    );
    assert!(
        increment.x < add_five.x,
        "Increment should sit left of Add 5: {increment:?} {add_five:?}"
    );

    let mut clicked = support::counter_after_click();
    clicked.assert_ir_str("/children/0/props/content", "Count: 1 (doubled: 2)");

    let mut typed = support::counter_after_typing();
    typed.assert_ir_str("/children/0/props/content", "Count: 5 (doubled: 10)");
}

#[test]
fn feed_click_loads_posts_and_typing_fills_the_field() {
    let mut ui = support::feed_initial();
    ui.assert_access_role("/search", "text_input");
    let search = ui
        .access()
        .unwrap()
        .into_iter()
        .find(|entry| entry.key.ends_with("/search"))
        .unwrap();
    assert_eq!(search.name, "Search");
    assert!(
        !ui.access()
            .unwrap()
            .iter()
            .find(|entry| entry.key == "FeedView")
            .unwrap()
            .focusable
    );

    let mut clicked = support::feed_after_click();
    let ir = clicked.ir().unwrap();
    let text = ir.to_string();
    assert!(
        text.contains("Hello Grapheme"),
        "loaded feed IR missing the first post: {ir}"
    );
    assert!(
        !text.contains("Load feed"),
        "Load feed should leave after the task runs: {ir}"
    );

    let mut typed = support::feed_after_typing();
    typed.assert_ir_str("/children/1/props/value", "Hello");
    let field = typed
        .access()
        .unwrap()
        .into_iter()
        .find(|entry| entry.key.ends_with("/search"))
        .unwrap();
    assert_eq!(field.value, "Hello");
}

#[test]
fn invoke_matches_a_click_and_a_second_settle_is_stable() {
    let mut clicked = support::counter_after_click();
    let mut invoked = support::counter_initial();
    invoked.invoke("/increment").unwrap();
    assert_eq!(clicked.ir().unwrap(), invoked.ir().unwrap());

    let mut ui = support::counter_initial();
    let first = ui.snapshot().unwrap();
    let second = ui.snapshot().unwrap();
    assert_eq!(first, second);
}

#[test]
fn missing_ir_golden_names_the_update_flow() {
    let mut ui = support::counter_initial();
    let path = std::env::temp_dir().join(format!(
        "grapheme-ui-missing-golden-{}-{}.json",
        std::process::id(),
        "harness"
    ));
    let _ = std::fs::remove_file(&path);
    if std::env::var("UPDATE_GOLDENS").ok().as_deref() == Some("1") {
        ui.assert_ir_golden(&path).unwrap();
        assert!(path.is_file());
        let _ = std::fs::remove_file(&path);
        return;
    }
    let err = ui.assert_ir_golden(&path).unwrap_err();
    assert!(err.to_string().contains("UPDATE_GOLDENS=1"), "{err}");
}
