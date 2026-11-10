//! Projected keys and roles match View IR, and rndr's headless access tree
//! reports button and label roles for those entities.
//!
//! `Column` / `Row` stay `GraphemeNode.role = "group"`. rndr flattens layout
//! containers that have no access role, so those groups are not
//! `AccessRole::Group`. A focusable group would add a tab stop the glyph did
//! not declare. Platform AccessKit is an `rndr_render` feature; `access_tree`
//! is the headless tree that feature publishes, and this test does not enable
//! it or open a window.
//!
//! `expose:` actions (`key` like `expose/Increment`) are session entry points,
//! not projected widgets. Every `click` action key is in the projected set.

use std::collections::HashSet;

use grapheme_ui::{GraphemeSurface, install};
use rndr_core::{AccessRole, App, Layout, Size, World, access_id, access_tree};
use serde_json::Value as JsonValue;

const SOURCE: &str = include_str!("../../../examples/counter/counter.gr");

struct IrNode {
    key: String,
    role: String,
    text: Option<String>,
}

fn collect(node: &JsonValue, out: &mut Vec<IrNode>) {
    let key = node["key"]
        .as_str()
        .unwrap_or_else(|| panic!("IR node missing key: {node}"))
        .to_string();
    let prim = node["$prim"].as_str();
    let role = node["role"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| match prim {
            Some("Button") => "button".into(),
            Some("Text") => "text".into(),
            _ => "group".into(),
        });
    let text = match role.as_str() {
        "button" => node["props"]["label"].as_str().map(str::to_string),
        "text" => node["props"]["content"].as_str().map(str::to_string),
        _ => None,
    };
    out.push(IrNode { key, role, text });
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect(child, out);
        }
    }
}

fn assert_identity(surface: &GraphemeSurface, world: &World) {
    let mut ir_nodes = Vec::new();
    collect(surface.ir_root().expect("ir"), &mut ir_nodes);
    assert!(!ir_nodes.is_empty());

    let projected = surface.projected_nodes(world);
    let keys: HashSet<&str> = projected.iter().map(|n| n.key.as_str()).collect();

    for node in &ir_nodes {
        assert!(
            keys.contains(node.key.as_str()),
            "IR key {} is not projected: {keys:?}",
            node.key
        );
        let projected_node = projected.iter().find(|n| n.key == node.key).unwrap();
        assert_eq!(
            projected_node.role.as_deref(),
            Some(node.role.as_str()),
            "role for {}",
            node.key
        );
    }

    let mut saw_click = false;
    for action in surface.actions() {
        if action.event == "click" {
            saw_click = true;
            assert!(
                keys.contains(action.key.as_str()),
                "click action {} key {} is not projected",
                action.id,
                action.key
            );
        } else if action.id.starts_with("expose:") {
            assert!(
                !keys.contains(action.key.as_str()),
                "expose action {} was projected as a widget",
                action.id
            );
        }
    }
    assert!(saw_click, "snapshot has no click actions");

    let tree = access_tree(world);
    for node in &projected {
        let role = node.role.as_deref().unwrap_or("");
        let id = access_id(node.entity);
        let access = tree.nodes.iter().find(|n| n.id == id);
        match role {
            "button" => {
                let access = access.unwrap_or_else(|| {
                    panic!(
                        "button {} ({:?}) has no access node; GraphemeNode.role={role:?}",
                        node.key, node.entity
                    )
                });
                assert_eq!(
                    access.role,
                    AccessRole::Button,
                    "button {} access role",
                    node.key
                );
                let ir = ir_nodes.iter().find(|n| n.key == node.key).unwrap();
                assert_eq!(access.name, ir.text.clone().unwrap_or_default());
            }
            "text" => {
                let access =
                    access.unwrap_or_else(|| panic!("text {} has no access node", node.key));
                assert_eq!(
                    access.role,
                    AccessRole::Label,
                    "text {} access role",
                    node.key
                );
                let ir = ir_nodes.iter().find(|n| n.key == node.key).unwrap();
                assert_eq!(access.name, ir.text.clone().unwrap_or_default());
            }
            "group" => {
                if let Some(access) = access {
                    assert_eq!(
                        access.role,
                        AccessRole::Group,
                        "group {} got access role {:?}",
                        node.key,
                        access.role
                    );
                }
            }
            other => panic!("unexpected projected role {other} on {}", node.key),
        }
    }
}

#[test]
fn access_roles_match_projected_buttons_and_labels() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout {
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let mut surface =
        GraphemeSurface::mount(app.world_mut(), parent, SOURCE, Some("CounterApp")).unwrap();
    app.update();
    assert_identity(&surface, app.world());

    let increment = surface
        .actions()
        .iter()
        .find(|a| a.key.ends_with("/increment"))
        .unwrap()
        .id
        .clone();
    surface.invoke_action(app.world_mut(), &increment).unwrap();
    app.update();
    assert_identity(&surface, app.world());
    assert_eq!(
        surface.ir_root().unwrap()["children"][0]["props"]["content"],
        "Count: 1 (doubled: 2)"
    );
}
