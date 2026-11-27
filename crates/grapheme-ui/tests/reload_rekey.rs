//! A reload that changes which element sits at a key replaces that widget.
//!
//! View IR keys are positional (`Only/1`), so inserting an element shifts the
//! elements after it onto keys that used to hold a different kind of widget.

use grapheme_ui::testing::Harness;

const BEFORE: &str = r#"state S { n: Int = 0 }
mutation Inc on S { apply { n: $n + 1 } }
view Only on S { Column { Text("c {$n}") Button(label: "Inc") { on click: Inc } } }
glyph OnlyApp on S { Only }
"#;

fn access(ui: &mut Harness) -> Vec<(String, String)> {
    ui.access()
        .unwrap()
        .into_iter()
        .map(|entry| (entry.key, entry.name))
        .collect()
}

#[test]
fn inserting_an_element_on_reload_does_not_leave_the_old_widget_behind() {
    let after = BEFORE.replace(r#"Text("c {$n}")"#, r#"Text("c {$n}") Spacer()"#);
    let mut ui = Harness::mount(BEFORE, Some("OnlyApp")).unwrap();
    ui.reload_source(&after, None).unwrap();
    ui.settle().unwrap();

    let mut fresh = Harness::mount(&after, Some("OnlyApp")).unwrap();
    assert_eq!(access(&mut ui), access(&mut fresh));
    let buttons = access(&mut ui)
        .into_iter()
        .filter(|(_, name)| name == "Inc")
        .count();
    assert_eq!(buttons, 1, "one Inc button after the reload");

    ui.click_labeled("Inc").unwrap();
    ui.assert_ir_str("/children/0/props/content", "c 1");
}

#[test]
fn removing_an_element_on_reload_shifts_widgets_back() {
    let with_spacer = BEFORE.replace(r#"Text("c {$n}")"#, r#"Text("c {$n}") Spacer()"#);
    let mut ui = Harness::mount(&with_spacer, Some("OnlyApp")).unwrap();
    ui.reload_source(BEFORE, None).unwrap();
    ui.settle().unwrap();

    let mut fresh = Harness::mount(BEFORE, Some("OnlyApp")).unwrap();
    assert_eq!(access(&mut ui), access(&mut fresh));
    ui.click_labeled("Inc").unwrap();
    ui.assert_ir_str("/children/0/props/content", "c 1");
}
