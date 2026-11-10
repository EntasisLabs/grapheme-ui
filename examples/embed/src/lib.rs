//! An rndr shell that mounts a Grapheme counter in a child slot.
//!
//! The host owns the window, the title, and its own button. The guest is a
//! [`grapheme_ui::GraphemeSurface`] under that shell. Guest clicks are applied
//! by [`grapheme_ui::drain_installed`], which maps rndr command tokens back to
//! Grapheme actions. This host never names an action id.
//!
//! rndr apps depend on grapheme-ui; rndr itself does not.

use grapheme_ui::{GraphemeSurface, install, with_driven_surface};
use rndr_core::{
    AccessRole, App, Entity, Input, Label, Layout, PointerButton, Rect, Schedule, Size, World,
    access_id, access_tree, set_parent, take_action_intents_matching,
};
use rndr_ui::UiId;
use rndr_ui::controls::{Button, ButtonProps, HostIntent};

const SOURCE: &str = include_str!("../../counter/counter.gr");

struct HostState {
    button: Entity,
    pings: u64,
}

/// Host shell plus a driven Grapheme guest. No window is opened.
pub fn boot() -> App {
    let mut app = App::new();
    install(&mut app);

    let shell = app.world_mut().spawn((Layout {
        padding: 16.0,
        gap: 8.0,
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let title = app.world_mut().spawn((Label::new("Host shell"),));
    set_parent(app.world_mut(), title, shell);

    // Guest buttons also use HostIntent::Command, and the first one is
    // Command(1) as well. Tokens are not globally unique. drain_actions keeps
    // a guest click by entity, and drain_host keeps a host click the same way.
    let host_button = Button::mount(
        app.world_mut(),
        shell,
        UiId(1),
        ButtonProps::new("Host ping", HostIntent::Command(1)),
    )
    .expect("mount host button")
    .root;
    app.world_mut().insert_resource(HostState {
        button: host_button,
        pings: 0,
    });

    let guest_slot = app.world_mut().spawn((Layout {
        gap: 8.0,
        width: Size::Fill,
        height: Size::Hug,
        ..Layout::column()
    },));
    set_parent(app.world_mut(), guest_slot, shell);
    let caption = app.world_mut().spawn((Label::new("Grapheme guest"),));
    set_parent(app.world_mut(), caption, guest_slot);

    let surface = GraphemeSurface::mount(app.world_mut(), guest_slot, SOURCE, Some("CounterApp"))
        .expect("mount CounterApp");
    surface.drive(&mut app).expect("park the guest surface");
    app.add_system(Schedule::Update, drain_host);
    app
}

/// How many times the host button has been activated.
pub fn host_pings(app: &App) -> u64 {
    app.world()
        .resource::<HostState>()
        .map(|s| s.pings)
        .unwrap_or(0)
}

/// Guest store revision. Does not invoke anything.
///
/// `None` when the guest surface is not parked. A missing surface is not
/// revision 0.
pub fn guest_revision(app: &mut App) -> Option<u64> {
    with_driven_surface(app.world_mut(), |surface, _| {
        surface.snapshot().map(|s| s.revision)
    })
    .ok()
    .flatten()
}

/// Guest value label from the projected view, not from an action id.
pub fn guest_label(app: &mut App) -> String {
    with_driven_surface(app.world_mut(), |surface, _| {
        surface.ir_root().unwrap()["children"][0]["props"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string()
    })
    .unwrap_or_default()
}

/// Activate the host button through the pointer path.
pub fn click_host(app: &mut App) {
    let button = app.world().resource::<HostState>().unwrap().button;
    let at = center(app, button);
    click(app, at);
}

/// Activate the guest button whose visible name is `label`.
///
/// The name is the button's access label (`Increment`, `Add 5`), not a
/// Grapheme action id. Dispatch is still [`grapheme_ui::drain_installed`].
pub fn click_guest(app: &mut App, label: &str) {
    let entity = guest_button(app, label);
    let at = center(app, entity);
    click(app, at);
}

/// Scripted headless run: one host ping, then Increment, then Add 5.
pub fn run_headless(app: &mut App) {
    app.update();
    click_host(app);
    click_guest(app, "Increment");
    click_guest(app, "Add 5");
    println!("host pings: {}", host_pings(app));
    println!(
        "guest revision: {}",
        guest_revision(app).expect("guest surface")
    );
    println!("guest label: {}", guest_label(app));
}

fn drain_host(world: &mut World) {
    let Some(button) = world.resource::<HostState>().map(|state| state.button) else {
        return;
    };
    let intents = take_action_intents_matching(world, |world, intent| {
        intent.entity == button && rndr_ui::controls::host_intent(world, intent).is_some()
    });
    if intents.is_empty() {
        return;
    }
    if let Some(state) = world.resource_mut::<HostState>() {
        state.pings += intents.len() as u64;
    }
}

fn guest_button(app: &mut App, label: &str) -> Entity {
    with_driven_surface(app.world_mut(), |surface, world| {
        let tree = access_tree(world);
        surface
            .projected_nodes(world)
            .into_iter()
            .find(|node| {
                node.role.as_deref() == Some("button")
                    && tree.nodes.iter().any(|access| {
                        access.id == access_id(node.entity)
                            && access.role == AccessRole::Button
                            && access.name == label
                    })
            })
            .unwrap_or_else(|| panic!("no guest button labeled {label}"))
            .entity
    })
    .unwrap()
}

fn center(app: &App, entity: Entity) -> [f32; 2] {
    let rect = app
        .world()
        .get::<Rect>(entity)
        .copied()
        .unwrap_or_else(|| panic!("entity {entity:?} has no rect"));
    assert!(rect.w > 0.0 && rect.h > 0.0, "empty rect {rect:?}");
    [rect.x + rect.w * 0.5, rect.y + rect.h * 0.5]
}

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
