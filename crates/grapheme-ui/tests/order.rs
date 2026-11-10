//! Child order follows the evaluated View IR.
//!
//! `set_parent` appends a newly created child and does not move one that is
//! already parented. An `if` branch that turns on in front of a sibling, and
//! a `for` list whose keys change order, must still paint in source order.

use grapheme_ui::{GraphemeSurface, install};
use rndr_core::{App, Entity, Label, Layout, Rect, Size, World, children_of};

const ORDER: &str = r#"
state S {
  show: Bool = false
  items: [String] = ["b", "a"]
}

mutation Show on S {
  apply { show: true }
}

mutation Sort on S {
  apply { items: ["a", "b"] }
}

view OrderView on S {
  Column {
    if $show {
      Text("I should be first")
    }
    Text("I am last")
    for $item in $items key $item {
      Text($item)
    }
  }
}

glyph OrderApp on S {
  expose Show, Sort
  OrderView
}
"#;

fn labels(world: &World, parent: Entity) -> Vec<String> {
    children_of(world, parent)
        .into_iter()
        .map(|entity| {
            world
                .get::<Label>(entity)
                .map(|label| label.text.clone())
                .unwrap_or_else(|| format!("<no label {entity:?}>"))
        })
        .collect()
}

fn column(surface: &GraphemeSurface, world: &World) -> Entity {
    surface
        .projected_nodes(world)
        .into_iter()
        .find(|node| node.key == "OrderView")
        .expect("column")
        .entity
}

#[test]
fn if_branch_and_for_list_follow_source_order() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout::column(),));
    let mut surface =
        GraphemeSurface::mount(app.world_mut(), parent, ORDER, Some("OrderApp")).unwrap();

    assert_eq!(
        labels(app.world(), column(&surface, app.world())),
        ["I am last", "b", "a"]
    );

    surface
        .invoke_action(app.world_mut(), "expose:Show")
        .unwrap();
    assert_eq!(
        labels(app.world(), column(&surface, app.world())),
        ["I should be first", "I am last", "b", "a"]
    );

    surface
        .invoke_action(app.world_mut(), "expose:Sort")
        .unwrap();
    assert_eq!(
        labels(app.world(), column(&surface, app.world())),
        ["I should be first", "I am last", "a", "b"]
    );
}

const COUNTER: &str = include_str!("../../../examples/counter/counter.gr");

#[test]
fn counter_label_and_buttons_keep_a_single_line() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout {
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let surface =
        GraphemeSurface::mount(app.world_mut(), parent, COUNTER, Some("CounterApp")).unwrap();
    app.update();
    let world = app.world();
    for node in surface.projected_nodes(world) {
        let Some(role) = node.role.as_deref() else {
            continue;
        };
        if role != "button" && role != "text" {
            continue;
        }
        let rect = world.get::<Rect>(node.entity).copied().unwrap_or_default();
        let text = first_label(world, node.entity).unwrap_or_default();
        assert!(
            rect.w >= text.chars().count() as f32 * 8.0,
            "{role} {text:?} is narrower than its text: {rect:?}"
        );
        assert!(
            rect.h > 8.0 && rect.h < 80.0,
            "{role} {text:?} is not one line: {rect:?}"
        );
    }
}

const FEED: &str = include_str!("../../../examples/feed-view.gr");

fn first_label(world: &World, entity: Entity) -> Option<String> {
    if let Some(label) = world.get::<Label>(entity) {
        return Some(label.text.clone());
    }
    children_of(world, entity)
        .into_iter()
        .find_map(|child| first_label(world, child))
}

fn feed_labels(surface: &GraphemeSurface, world: &World) -> Vec<String> {
    let column = surface
        .projected_nodes(world)
        .into_iter()
        .find(|node| node.key == "FeedView")
        .expect("feed column")
        .entity;
    children_of(world, column)
        .into_iter()
        .map(|entity| {
            first_label(world, entity).unwrap_or_else(|| format!("<no label {entity:?}>"))
        })
        .collect()
}

#[test]
fn feed_view_mounts_and_lists_posts_in_source_order() {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout::column(),));
    let mut surface =
        GraphemeSurface::mount(app.world_mut(), parent, FEED, Some("FeedApp")).unwrap();
    let roles: Vec<_> = surface
        .projected_nodes(app.world())
        .into_iter()
        .map(|node| (node.key, node.role))
        .collect();
    assert!(
        roles
            .iter()
            .any(|(key, role)| key.ends_with("/search") && role.as_deref() == Some("textbox")),
        "{roles:?}"
    );
    assert_eq!(
        feed_labels(&surface, app.world()),
        ["Feed (0 likes)", "", "Load feed"]
    );
    app.update();
    let world = app.world();
    for node in surface.projected_nodes(world) {
        if node.role.as_deref() != Some("button") {
            continue;
        }
        let rect = world.get::<Rect>(node.entity).copied().unwrap_or_default();
        let text = first_label(world, node.entity).unwrap_or_default();
        assert!(
            rect.w >= text.chars().count() as f32 * 7.0 && rect.h < 80.0,
            "feed button {text:?} {rect:?}"
        );
    }

    surface
        .invoke_action(app.world_mut(), "expose:LoadFeed")
        .unwrap();
    assert_eq!(
        feed_labels(&surface, app.world()),
        ["Feed (0 likes)", "", "Hello Grapheme", "Store tasks land"]
    );
    assert_eq!(surface.snapshot().unwrap().revision, 2);
}
