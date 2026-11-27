//! A primitive that exists only in grapheme-ui: the compiler learns `Divider`
//! from a catalog document, and this crate projects it into the access tree.

use grapheme_ui::{GraphemeSurface, install};
use rndr_core::{App, Layout, Size, access_id, access_tree};
use serde_json::{Value as JsonValue, json};

const V1: &str = include_str!("../../../catalog/v1.json");

const SOURCE: &str = r#"
state S { n: Int = 0 }
mutation Nop on S { apply { n: $n } }
view V on S {
  Column {
    rule: Divider(label: "Section")
  }
}
glyph App on S { V }
"#;

fn catalog_with_divider() -> String {
    let mut doc: JsonValue = serde_json::from_str(V1).expect("v1 catalog");
    doc["elements"]
        .as_array_mut()
        .expect("elements")
        .push(json!({
            "name": "Divider",
            "role": "separator",
            "content": false,
            "children": false,
            "props": [{ "name": "label", "type": "String" }],
            "events": []
        }));
    doc.to_string()
}

#[test]
fn divider_compiles_projects_and_appears_in_the_access_tree() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout {
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let surface = GraphemeSurface::builder()
        .element_catalog(catalog_with_divider())
        .mount(app.world_mut(), parent, SOURCE, Some("App"))
        .expect("Divider compiles against the extended catalog");
    app.update();

    let ir = surface.ir_root().expect("ir");
    let divider = &ir["children"][0];
    assert_eq!(divider["$prim"], "Divider");
    assert_eq!(divider["role"], "separator");
    assert_eq!(divider["props"]["label"], "Section");

    let projected = surface.projected_nodes(app.world());
    let node = projected
        .iter()
        .find(|node| node.key.ends_with("/rule"))
        .expect("Divider is projected");
    assert_eq!(node.role.as_deref(), Some("separator"));

    let tree = access_tree(app.world());
    let access = tree
        .nodes
        .iter()
        .find(|access| access.id == access_id(node.entity))
        .expect("Divider appears in the access tree");
    assert_eq!(access.name, "Section");
}
