//! Headless by default. With `--features desktop`, opens a window instead.
//!
//! ```text
//! cargo run -p embed
//! cargo run -p embed --features desktop
//! ```
//!
//! The desktop build needs a display. `cargo test -p embed` does not open one.

fn main() {
    let mut app = embed::boot();
    #[cfg(feature = "desktop")]
    {
        use rndr_core::{LogicalSize, WindowConfig};
        app.world_mut().insert_resource(WindowConfig {
            title: "rndr host + Grapheme guest".into(),
            size: LogicalSize::new(480.0, 360.0),
            min_size: Some(LogicalSize::new(320.0, 240.0)),
            ..WindowConfig::default()
        });
        grapheme_ui::desktop::run(app);
    }
    #[cfg(not(feature = "desktop"))]
    embed::run_headless(&mut app);
}
