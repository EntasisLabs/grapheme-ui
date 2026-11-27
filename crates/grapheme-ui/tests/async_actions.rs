//! A blocking task capability must not stop Update, and the pending commit is
//! painted before ready. A second click while the task runs is queued.

use std::sync::{Arc, Condvar, Mutex};

use grapheme_ui::{GraphemeSurface, install};
use rndr_core::{App, Entity, Input, Layout, PointerButton, Rect, Schedule, Size};

const SOURCE: &str = r#"
state S {
  status: String = "idle"
  n: Int = 0
}

task Go on S {
  apply { status: "pending", n: $n }
  core.value(value: 1)
  apply { status: "ready", n: $n + 1 }
}

view V on S {
  Column {
    label: Text("{$status}:{$n}")
    go: Button(label: "Go") { on click: Go }
  }
}

glyph App on S {
  V
}
"#;

struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Gate {
    fn new() -> Self {
        Self {
            open: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.cv.wait(open).unwrap();
        }
    }

    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }
}

struct OpenOnDrop(Arc<Gate>);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.open();
    }
}

#[derive(Default)]
struct FrameCount(u64);

fn count_frames(world: &mut rndr_core::World) {
    world.resource_mut::<FrameCount>().unwrap().0 += 1;
}

fn boot(gate: Arc<Gate>, log: Arc<Mutex<Vec<&'static str>>>) -> App {
    let mut app = App::new();
    install(&mut app);
    let parent = app.world_mut().spawn((Layout {
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let gate_host = Arc::clone(&gate);
    let log_host = Arc::clone(&log);
    let surface = GraphemeSurface::builder()
        .capability_interceptor(move |call| {
            if call.op != "value" {
                return None;
            }
            log_host.lock().unwrap().push("enter");
            gate_host.wait();
            log_host.lock().unwrap().push("leave");
            Some(Ok(serde_json::json!(1)))
        })
        .mount(app.world_mut(), parent, SOURCE, Some("App"))
        .unwrap();
    surface.drive(&mut app).unwrap();
    app.world_mut().insert_resource(FrameCount::default());
    app.add_system(Schedule::Update, count_frames);
    app.update();
    app
}

fn label(app: &mut App) -> String {
    grapheme_ui::with_driven_surface(app.world_mut(), |surface, _| {
        surface.ir_root().unwrap()["children"][0]["props"]["content"]
            .as_str()
            .unwrap()
            .to_string()
    })
    .unwrap()
}

fn entity_with_key(app: &mut App, suffix: &str) -> Entity {
    grapheme_ui::with_driven_surface(app.world_mut(), |surface, world| {
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
    let rect = app.world().get::<Rect>(entity).copied().unwrap();
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

fn frames(app: &App) -> u64 {
    app.world().resource::<FrameCount>().unwrap().0
}

#[test]
fn blocking_task_keeps_frames_ticking_and_queues_a_second_click() {
    let gate = Arc::new(Gate::new());
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut app = boot(Arc::clone(&gate), Arc::clone(&log));
    // Drop opens the gate before the surface joins the session thread.
    let _open = OpenOnDrop(Arc::clone(&gate));

    assert_eq!(label(&mut app), "idle:0");
    let go = entity_with_key(&mut app, "/go");
    let at = center(&app, go);
    click(&mut app, at);
    assert_eq!(
        label(&mut app),
        "pending:0",
        "the pending apply is painted before the capability returns"
    );

    let before = frames(&app);
    for _ in 0..4 {
        app.update();
    }
    assert!(
        frames(&app) >= before + 4,
        "Update kept running while the capability blocked: {} -> {}",
        before,
        frames(&app)
    );
    assert_eq!(label(&mut app), "pending:0");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["enter"],
        "the capability is in progress and has not returned"
    );

    let go = entity_with_key(&mut app, "/go");
    let at = center(&app, go);
    click(&mut app, at);
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["enter"],
        "a second click is queued; it does not enter the capability yet"
    );
    assert_eq!(label(&mut app), "pending:0");

    gate.open();
    let mut saw_ready = false;
    for _ in 0..40 {
        app.update();
        let text = label(&mut app);
        if text.starts_with("ready") {
            saw_ready = true;
        }
        if text == "ready:2" && saw_ready {
            break;
        }
    }
    assert_eq!(label(&mut app), "ready:2");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["enter", "leave", "enter", "leave"],
        "queued click runs after the first task, not beside it"
    );
}
