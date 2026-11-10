//! Host engine configuration reaches the session the surface drives.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use grapheme_ui::{CapabilityHost, GraphemeSurface, HostCallError, PolicyGuard, install};
use rndr_core::{App, Layout};
use serde_json::{Value as JsonValue, json};

const COUNTER: &str = r#"
state S { n: Int = 0 }
mutation Inc on S { apply { n: $n + 1 } }
view V on S { Text("{$n}") }
glyph App on S { expose Inc V }
"#;

const HTTP: &str = r#"
state S { text: String = "" }
task Load on S {
  http.get(url: "https://example.com/feed")
  apply { text: "ok" }
}
view V on S { Text($text) }
glyph App on S { expose Load V }
"#;

const VALUE: &str = r#"
state S { text: String = "" }
task Load on S {
  core.value(value: "original")
  apply { text: $current }
}
view V on S { Text($text) }
glyph App on S { expose Load V }
"#;

struct RejectHost;

impl CapabilityHost for RejectHost {
    fn call(&mut self, _call: &grapheme_ui::CapabilityCall) -> Result<JsonValue, HostCallError> {
        Err(HostCallError::Fatal("host-factory-used".into()))
    }
}

fn mount(
    app: &mut App,
    source: &str,
    builder: grapheme_ui::SurfaceBuilder,
) -> grapheme_ui::GraphemeSurface {
    install(app);
    let parent = app.world_mut().spawn((Layout::column(),));
    builder
        .mount(app.world_mut(), parent, source, Some("App"))
        .unwrap()
}

#[test]
fn policy_guard_rejects_a_domain_outside_the_allow_list() {
    let mut app = App::new();
    let mut surface = mount(
        &mut app,
        HTTP,
        GraphemeSurface::builder().policy_guard(PolicyGuard {
            allowed_http_domains: vec!["nowhere.test".into()],
            ..PolicyGuard::default()
        }),
    );
    let err = surface
        .invoke_action(app.world_mut(), "expose:Load")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("not allowed") && err.contains("example.com"),
        "{err}"
    );
}

#[test]
fn capability_interceptor_supplies_the_task_current_value() {
    let mut app = App::new();
    let mut surface = mount(
        &mut app,
        VALUE,
        GraphemeSurface::builder().capability_interceptor(|call| {
            if call.op == "value" {
                Some(Ok(json!("intercepted")))
            } else {
                None
            }
        }),
    );
    surface
        .invoke_action(app.world_mut(), "expose:Load")
        .unwrap();
    assert_eq!(
        surface.session().snapshot("S").unwrap()["text"],
        json!("intercepted")
    );
}

#[test]
fn host_factory_dispatches_capability_calls() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let mut app = App::new();
    let mut surface = mount(
        &mut app,
        COUNTER,
        GraphemeSurface::builder().host_factory(move || {
            seen.fetch_add(1, Ordering::SeqCst);
            Box::new(RejectHost)
        }),
    );
    let err = surface
        .invoke_action(app.world_mut(), "expose:Inc")
        .unwrap_err()
        .to_string();
    assert!(err.contains("host-factory-used"), "{err}");
    assert!(calls.load(Ordering::SeqCst) >= 1);
}

#[test]
fn configure_module_registry_runs_before_the_session_starts() {
    let ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&ran);
    let mut app = App::new();
    let surface = mount(
        &mut app,
        COUNTER,
        GraphemeSurface::builder().configure_module_registry(move |registry| {
            assert!(registry.has_module("core"));
            flag.store(true, Ordering::SeqCst);
        }),
    );
    assert!(ran.load(Ordering::SeqCst));
    assert_eq!(surface.glyph(), "App");
}
