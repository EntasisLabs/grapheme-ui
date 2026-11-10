//! Windowed counter (Milestone 1).
//!
//! Opens a native window, mounts `CounterApp`, and drains button clicks
//! through [`grapheme_ui::drain_installed`] — the same `invoke_action` path
//! the headless surface uses. Without the `desktop` feature this binary
//! refuses to start, so default CI stays GPU-free.
//!
//! ```text
//! cargo run -p counter-desktop --features desktop
//! ```
//!
//! # Manual checklist (needs a display)
//!
//! 1. The window title is `GraphemeUI — Counter`.
//! 2. The label reads `Count: 0 (doubled: 0)`, with buttons **Increment** and **Add 5**.
//! 3. Click **Increment**. The label becomes `Count: 1 (doubled: 2)`.
//! 4. Click **Increment** again. The label becomes `Count: 2 (doubled: 4)`.
//! 5. Click **Add 5**. The label becomes `Count: 7 (doubled: 14)`.
//! 6. Close the window. The process exits.
//!
//! A headless stand-in for steps 3–5 is `drain_click_matches_headless_session`
//! in `grapheme-ui`. `cargo test` does not open the window.

#[cfg(not(feature = "desktop"))]
fn main() {
    eprintln!(
        "counter-desktop needs a display. Rebuild with: cargo run -p counter-desktop --features desktop"
    );
    std::process::exit(2);
}

#[cfg(feature = "desktop")]
fn main() {
    use grapheme_ui::{GraphemeSurface, install};
    use rndr_core::{App, Layout, LogicalSize, Size, WindowConfig};

    let source = include_str!("../../counter/counter.gr");
    let mut app = App::new();
    install(&mut app);
    app.world_mut().insert_resource(WindowConfig {
        title: "GraphemeUI — Counter".into(),
        size: LogicalSize::new(420.0, 280.0),
        min_size: Some(LogicalSize::new(320.0, 200.0)),
        ..WindowConfig::default()
    });

    let parent = app.world_mut().spawn((Layout {
        padding: 24.0,
        gap: 8.0,
        width: Size::Fill,
        height: Size::Fill,
        ..Layout::column()
    },));
    let surface = GraphemeSurface::mount(app.world_mut(), parent, source, Some("CounterApp"))
        .expect("mount CounterApp");
    surface.drive(&mut app).expect("park CounterApp");
    grapheme_ui::desktop::run(app);
}
