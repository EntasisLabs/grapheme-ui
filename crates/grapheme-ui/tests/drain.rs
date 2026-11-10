//! Pointer click → rndr action intent → `drain_actions` matches a headless
//! session that invoked the same action ids. No window and no GPU.

use std::panic::{AssertUnwindSafe, catch_unwind};

use grapheme_sdk::GraphemeEngine;
use grapheme_ui::{DrainStatus, GraphemeSurface, GraphemeUiError, install, with_driven_surface};
use rndr_core::{
    ActivationResult, App, Entity, Input, Layout, PointerButton, Rect, Size, try_activate,
};

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
    surface.drive(&mut app).unwrap();
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

/// `Add(by: $count)` rebinds when count changes, so a second intent queued
/// in the same frame carries the pre-invoke epoch.
const REBIND: &str = r#"
state Counter {
  count: Int = 1
}

mutation Add($by: Int) on Counter {
  apply { count: $count + $by }
}

view CounterView on Counter {
  Column(gap: 8) {
    value: Text("{$count}")
    add: Button(label: "Add") { on click: Add(by: $count) }
  }
}

glyph CounterApp on Counter {
  CounterView
}
"#;

#[test]
fn two_clicks_in_one_frame_sync_before_a_stale_epoch() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout {
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let mut surface =
        GraphemeSurface::mount(app.world_mut(), parent, REBIND, Some("CounterApp")).unwrap();
    app.update();
    let button = surface
        .projected_nodes(app.world())
        .into_iter()
        .find(|node| node.role.as_deref() == Some("button"))
        .unwrap()
        .entity;
    assert_eq!(
        try_activate(app.world_mut(), button),
        ActivationResult::Accepted
    );
    assert_eq!(
        try_activate(app.world_mut(), button),
        ActivationResult::Accepted
    );

    let err = surface.drain_actions(app.world_mut()).unwrap_err();
    assert!(matches!(err, GraphemeUiError::StaleAction(_)), "{err}");
    let status = app.world().resource::<DrainStatus>().cloned().unwrap();
    assert_eq!(status.commits.len(), 1, "{status:?}");
    assert!(status.error.is_some(), "{status:?}");
    assert_eq!(surface.session().snapshot("Counter").unwrap()["count"], 2);
    assert_eq!(
        surface.ir_root().unwrap()["children"][0]["props"]["content"],
        "2"
    );

    assert_eq!(
        try_activate(app.world_mut(), button),
        ActivationResult::Accepted
    );
    let commits = surface.drain_actions(app.world_mut()).unwrap();
    assert_eq!(commits.len(), 1, "a later click uses the rebound action id");
    assert!(
        app.world()
            .resource::<DrainStatus>()
            .unwrap()
            .error
            .is_none()
    );
    assert_eq!(surface.session().snapshot("Counter").unwrap()["count"], 4);
}

#[test]
fn drive_rejects_a_second_surface() {
    let mut app = boot();
    let parent = app.world_mut().spawn((Layout::column(),));
    let surface =
        GraphemeSurface::mount(app.world_mut(), parent, SOURCE, Some("CounterApp")).unwrap();
    let err = surface.drive(&mut app).unwrap_err();
    assert!(matches!(err, GraphemeUiError::Usage(_)), "{err}");
    with_driven_surface(app.world_mut(), |surface, _| {
        assert_eq!(surface.glyph(), "CounterApp");
    })
    .unwrap();
}

#[test]
fn with_driven_surface_restores_the_surface_after_panic() {
    let mut app = boot();
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        with_driven_surface(app.world_mut(), |_surface, _world| panic!("boom")).ok();
    }));
    assert!(panicked.is_err());
    with_driven_surface(app.world_mut(), |surface, _| {
        assert_eq!(surface.glyph(), "CounterApp");
    })
    .unwrap();
}
