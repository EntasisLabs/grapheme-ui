//! Pointer click → rndr action intent → `drain_actions` matches a headless
//! session that invoked the same action ids. No window and no GPU.

use grapheme_sdk::GraphemeEngine;
use grapheme_ui::{GraphemeSurface, install, with_driven_surface};
use rndr_core::{App, Entity, Input, Layout, PointerButton, Rect, Size};

const SOURCE: &str = include_str!("../../../examples/counter/counter.gr");

fn action_id(snapshot: &grapheme_runtime::AppSnapshot, suffix: &str) -> String {
    snapshot
        .actions
        .iter()
        .find(|a| a.key.ends_with(suffix))
        .unwrap_or_else(|| panic!("missing action {suffix}: {:?}", snapshot.actions))
        .id
        .clone()
}

fn boot() -> App {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout {
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let surface =
        GraphemeSurface::mount(app.world_mut(), parent, SOURCE, Some("CounterApp")).unwrap();
    surface.drive(&mut app);
    // Lay out once so buttons have rects before the first click.
    app.update();
    app
}

fn entity_with_key(app: &mut App, suffix: &str) -> Entity {
    with_driven_surface(app.world_mut(), |surface, world| {
        surface
            .projected_nodes(world)
            .into_iter()
            .find(|n| n.key.ends_with(suffix))
            .unwrap_or_else(|| panic!("missing projected node {suffix}"))
            .entity
    })
    .unwrap()
}

fn center(app: &App, entity: Entity) -> [f32; 2] {
    let rect = app
        .world()
        .get::<Rect>(entity)
        .copied()
        .unwrap_or_else(|| panic!("button {entity:?} has no rect"));
    assert!(
        rect.w > 0.0 && rect.h > 0.0,
        "button {entity:?} rect {rect:?} is empty"
    );
    [rect.x + rect.w * 0.5, rect.y + rect.h * 0.5]
}

/// Press then release on `at`, one frame each, so ReleaseActivation commits.
fn click(app: &mut App, at: [f32; 2]) {
    {
        let pointer = &mut app.world_mut().resource_mut::<Input>().unwrap().pointer;
        pointer.position = at;
        pointer.press_at = Some(at);
        pointer.button = PointerButton::Left;
        pointer.pressed = true;
        pointer.down = true;
        pointer.released = false;
    }
    app.update();
    {
        let pointer = &mut app.world_mut().resource_mut::<Input>().unwrap().pointer;
        pointer.position = at;
        pointer.pressed = false;
        pointer.down = false;
        pointer.released = true;
    }
    app.update();
    app.world_mut()
        .resource_mut::<Input>()
        .unwrap()
        .pointer
        .released = false;
}

fn click_key(app: &mut App, suffix: &str) {
    let entity = entity_with_key(app, suffix);
    let at = center(app, entity);
    click(app, at);
}

fn label_text(app: &mut App) -> String {
    with_driven_surface(app.world_mut(), |surface, _| {
        surface.ir_root().unwrap()["children"][0]["props"]["content"]
            .as_str()
            .unwrap()
            .to_string()
    })
    .unwrap()
}

#[test]
fn drain_click_matches_headless_session() {
    let mut app = boot();
    assert_eq!(label_text(&mut app), "Count: 0 (doubled: 0)");

    // A miss must not invoke anything.
    click(&mut app, [-10.0, -10.0]);
    with_driven_surface(app.world_mut(), |surface, _| {
        assert_eq!(surface.snapshot().unwrap().revision, 0);
    })
    .unwrap();

    click_key(&mut app, "/increment");
    assert_eq!(label_text(&mut app), "Count: 1 (doubled: 2)");
    click_key(&mut app, "/increment");
    assert_eq!(label_text(&mut app), "Count: 2 (doubled: 4)");
    click_key(&mut app, "/add_five");
    assert_eq!(label_text(&mut app), "Count: 7 (doubled: 14)");

    let (snapshot, diffs) = with_driven_surface(app.world_mut(), |surface, _| {
        let snapshot = surface.session().snapshot("Counter").unwrap();
        let diffs: Vec<_> = surface
            .session()
            .revisions()
            .iter()
            .map(|rev| rev.diff.clone())
            .collect();
        (snapshot, diffs)
    })
    .unwrap();
    assert_eq!(diffs.len(), 3, "{diffs:?}");

    let mut headless = GraphemeEngine::builder()
        .build()
        .store_session(SOURCE)
        .unwrap();
    headless.mount(Some("CounterApp")).unwrap();
    let mut headless_diffs = Vec::new();
    for suffix in ["/increment", "/increment", "/add_five"] {
        let snap = headless.app_snapshot().unwrap();
        let id = action_id(&snap, suffix);
        assert!(
            id.contains("#click@"),
            "native path and headless path share view action ids, got {id}"
        );
        let commit = headless.invoke_action(&id).unwrap();
        headless_diffs.push(commit.committed.unwrap().diff);
    }
    assert_eq!(headless_diffs, diffs);
    assert_eq!(headless.snapshot("Counter").unwrap(), snapshot);
    assert_eq!(headless.revision("Counter"), Some(3));
}
