//! Layout, theme roles, interaction states, Spacer and Stack.

#[path = "support/mod.rs"]
mod support;

use grapheme_ui::testing::Harness;

fn near(actual: [f32; 4], expected: [f32; 4]) {
    for (got, want) in actual.iter().zip(expected) {
        assert!(
            (got - want).abs() < 0.02,
            "channel {got} != {want} in {actual:?} vs {expected:?}"
        );
    }
}

const STATES: &str = r#"
state S { n: Int = 0 }
mutation Go on S { apply { n: $n + 1 } }
view V on S {
  Column {
    style { width: fill, height: fill, padding: 16, gap: 8, fill: bg }
    go: Button(label: "Go") {
      on click: Go
      style {
        fill: accent
        hover { fill: hover }
        pressed { fill: pressed }
        focused { fill: selection }
      }
    }
    stop: Button(label: "Stop", enabled: false) {
      on click: Go
      style { fill: accent, disabled { fill: disabled } }
    }
  }
}
glyph App on S { V }
"#;

#[test]
fn spacer_and_stack_lay_out() {
    let mut ui = support::settings();
    let label = ui
        .rects()
        .unwrap()
        .into_iter()
        .find(|r| r.key.ends_with("/label"))
        .unwrap();
    let gap = ui
        .rects()
        .unwrap()
        .into_iter()
        .find(|r| r.key.ends_with("/gap"))
        .unwrap();
    let save = ui
        .rects()
        .unwrap()
        .into_iter()
        .find(|r| r.key.ends_with("/save"))
        .unwrap();
    assert!(
        gap.x >= label.x + label.w - 1.0,
        "spacer {gap:?} label {label:?}"
    );
    assert!(
        save.x >= gap.x + gap.w - 1.0,
        "save {save:?} spacer {gap:?}"
    );
    assert!(
        save.x > label.x + label.w + 24.0,
        "spacer did not push Save"
    );

    let plate = ui
        .rects()
        .unwrap()
        .into_iter()
        .find(|r| r.key.ends_with("/plate"))
        .unwrap();
    let caption = ui
        .rects()
        .unwrap()
        .into_iter()
        .find(|r| r.key.ends_with("/caption"))
        .unwrap();
    assert!(plate.w > 100.0 && plate.h > 20.0, "{plate:?}");
    assert!(
        (plate.x - caption.x).abs() < 2.0,
        "stack x {plate:?} {caption:?}"
    );
    assert!(
        (plate.y - caption.y).abs() < 2.0,
        "stack y {plate:?} {caption:?}"
    );
    assert!(
        (plate.w - caption.w).abs() < 2.0,
        "stack w {plate:?} {caption:?}"
    );
    assert!(
        (plate.h - caption.h).abs() < 2.0,
        "stack h {plate:?} {caption:?}"
    );
}

#[test]
fn theme_follows_the_os_and_can_be_forced() {
    let mut ui = support::settings();
    let light_bg = Harness::theme_rgba("bg", false).unwrap();
    let dark_bg = Harness::theme_rgba("bg", true).unwrap();
    let light_ink = Harness::theme_rgba("text_secondary", false).unwrap();
    let dark_ink = Harness::theme_rgba("text_secondary", true).unwrap();
    assert_ne!(light_bg, dark_bg);

    near(ui.fill_rgba("SettingsView").unwrap(), light_bg);
    near(ui.ink_rgba("/hint").unwrap(), light_ink);

    ui.os_dark(Some(true)).unwrap();
    near(ui.fill_rgba("SettingsView").unwrap(), dark_bg);
    near(ui.ink_rgba("/hint").unwrap(), dark_ink);

    ui.force_theme(false).unwrap();
    near(ui.fill_rgba("SettingsView").unwrap(), light_bg);
    near(ui.ink_rgba("/hint").unwrap(), light_ink);
}

#[test]
fn button_interaction_states_recolor_the_face() {
    let mut ui = Harness::mount(STATES, Some("App")).unwrap();
    ui.force_theme(false).unwrap();
    let accent = Harness::theme_rgba("accent", false).unwrap();
    let hover = Harness::theme_rgba("hover", false).unwrap();
    let pressed = Harness::theme_rgba("pressed", false).unwrap();
    let selection = Harness::theme_rgba("selection", false).unwrap();
    let disabled = Harness::theme_rgba("disabled", false).unwrap();

    near(ui.fill_rgba("/go").unwrap(), accent);
    ui.hover("/go").unwrap();
    near(ui.fill_rgba("/go").unwrap(), hover);
    ui.hold("/go").unwrap();
    near(ui.fill_rgba("/go").unwrap(), pressed);

    let mut ui = Harness::mount(STATES, Some("App")).unwrap();
    ui.force_theme(false).unwrap();
    ui.focus_node("/go").unwrap();
    near(ui.fill_rgba("/go").unwrap(), selection);
    near(ui.fill_rgba("/stop").unwrap(), disabled);
}

#[test]
fn a_role_in_a_size_slot_fails_to_mount() {
    let source = r#"
state S { n: Int = 0 }
view V on S { Text("a") { style { width: bg } } }
glyph App on S { V }
"#;
    let err = match Harness::mount(source, Some("App")) {
        Ok(_) => panic!("width: bg should not mount"),
        Err(err) => err.to_string(),
    };
    assert!(err.contains("theme role 'bg'"), "{err}");
}
