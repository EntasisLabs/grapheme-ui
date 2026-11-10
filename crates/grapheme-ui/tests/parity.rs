//! RFC-0007 vertical slice (headless): native-path invoke and pure headless
//! invoke produce the same revisions; projected node keys/roles match View IR.

use grapheme_sdk::GraphemeEngine;
use grapheme_ui::{GraphemeSurface, install};
use rndr_core::{App, Layout};
use serde_json::json;

const SOURCE: &str = include_str!("../../../examples/counter/counter.gr");

fn action_id(snapshot: &grapheme_runtime::AppSnapshot, suffix: &str) -> String {
    snapshot
        .actions
        .iter()
        .find(|a| a.key.ends_with(suffix))
        .unwrap_or_else(|| panic!("missing action ending in {suffix}: {:?}", snapshot.actions))
        .id
        .clone()
}

#[test]
fn projected_keys_and_roles_match_view_ir() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout::column(),));
    let surface =
        GraphemeSurface::mount(app.world_mut(), parent, SOURCE, Some("CounterApp")).unwrap();

    let ir = surface.ir_root().unwrap();
    let projected = surface.projected_nodes(app.world());
    let keys: Vec<_> = projected.iter().map(|n| n.key.as_str()).collect();

    assert!(keys.contains(&"CounterView"), "{keys:?}");
    assert!(keys.contains(&"CounterView/value"), "{keys:?}");
    assert!(keys.contains(&"CounterView/actions"), "{keys:?}");
    assert!(keys.contains(&"CounterView/actions/increment"), "{keys:?}");
    assert!(keys.contains(&"CounterView/actions/add_five"), "{keys:?}");

    let increment = projected
        .iter()
        .find(|n| n.key == "CounterView/actions/increment")
        .unwrap();
    assert_eq!(increment.role.as_deref(), Some("button"));
    assert_eq!(ir["key"], "CounterView");
    assert_eq!(ir["role"], "group");
    assert_eq!(ir["children"][1]["children"][0]["role"], "button");
}

#[test]
fn surface_invoke_matches_headless_session() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout::column(),));
    let mut surface = GraphemeSurface::mount(app.world_mut(), parent, SOURCE, None).unwrap();
    let id = action_id(surface.snapshot().unwrap(), "/increment");

    let commit = surface.invoke_action(app.world_mut(), &id).unwrap();
    assert_eq!(commit.revision, 1);
    assert_eq!(
        commit.committed.as_ref().unwrap().diff["count"],
        json!([0, 1])
    );

    // Pure headless twin: same source, same action id sequence.
    let mut headless = GraphemeEngine::builder()
        .build()
        .store_session(SOURCE)
        .unwrap();
    let snap = headless.mount(None).unwrap();
    let headless_id = action_id(&snap, "/increment");
    // Epochs start at 1 on both sides for the same binding.
    assert_eq!(headless_id, id);
    let headless_commit = headless.invoke_action(&headless_id).unwrap();
    assert_eq!(headless_commit.revision, commit.revision);
    assert_eq!(
        headless_commit.committed.unwrap().diff,
        commit.committed.unwrap().diff
    );
    assert_eq!(
        surface.session().snapshot("Counter").unwrap(),
        headless.snapshot("Counter").unwrap()
    );
}

#[test]
fn expose_action_works_through_the_surface_session() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout::column(),));
    let mut surface = GraphemeSurface::mount(app.world_mut(), parent, SOURCE, None).unwrap();
    assert!(surface.actions().iter().any(|a| a.id == "expose:Increment"));
    let commit = surface
        .invoke_action(app.world_mut(), "expose:Increment")
        .unwrap();
    assert_eq!(commit.revision, 1);
    assert_eq!(surface.snapshot().unwrap().revision, 1);
}
