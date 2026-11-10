//! Projected keys and roles match View IR, and rndr's headless access tree
//! reports button and label roles for those entities.
//!
//! `Column` / `Row` carry `CollectionAccess { role: Group, .. }`, so
//! `access_tree` reports `AccessRole::Group` without making the group a tab
//! stop. A `TextField` is an `AccessRole::TextInput` named by its `label`
//! prop. `access_tree` is always available from `rndr_core`. Pushing that tree
//! to the OS needs the `accesskit` feature on `rndr_render`. This test does
//! not open a window.
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
    name: Option<String>,
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
            Some("TextField") => "textbox".into(),
            _ => "group".into(),
        });
    let text = match role.as_str() {
        "button" => node["props"]["label"].as_str().map(str::to_string),
        "text" => node["props"]["content"].as_str().map(str::to_string),
        "textbox" => node["props"]["value"].as_str().map(str::to_string),
        _ => None,
    };
    let name = if role == "textbox" {
        Some(
            node["props"]["label"]
                .as_str()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or("Text field")
                .to_string(),
        )
    } else {
        None
    };
    out.push(IrNode {
        key,
        role,
        text,
        name,
    });
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
                let access =
                    access.unwrap_or_else(|| panic!("group {} has no access node", node.key));
                assert_eq!(
                    access.role,
                    AccessRole::Group,
                    "group {} access role",
                    node.key
                );
                assert!(!access.focusable, "group {} became a tab stop", node.key);
            }
            "textbox" => {
                let access =
                    access.unwrap_or_else(|| panic!("textbox {} has no access node", node.key));
                assert_eq!(
                    access.role,
                    AccessRole::TextInput,
                    "textbox {} access role",
                    node.key
                );
                assert!(access.focusable, "textbox {} is not focusable", node.key);
                let ir = ir_nodes.iter().find(|n| n.key == node.key).unwrap();
                assert_eq!(access.name, ir.name.clone().unwrap_or_default());
                assert_eq!(access.value, ir.text.clone().unwrap_or_default());
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

const FEED: &str = include_str!("../../../examples/feed-view.gr");

#[test]
fn text_field_is_a_text_input() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout {
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let surface = GraphemeSurface::mount(app.world_mut(), parent, FEED, Some("FeedApp")).unwrap();
    app.update();
    assert_identity(&surface, app.world());
    let search = surface
        .projected_nodes(app.world())
        .into_iter()
        .find(|node| node.role.as_deref() == Some("textbox"))
        .expect("search field");
    assert!(search.key.ends_with("/search"), "{}", search.key);
}
