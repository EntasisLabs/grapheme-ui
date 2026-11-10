//! Headless vertical slice: mount CounterApp into an rndr world, invoke the
//! Increment action through the surface (same path a native click uses), and
//! print the resulting revision. No window — model A embed surface.
//!
//! ```text
//! cargo run -p counter
//! ```

use grapheme_ui::{install, GraphemeSurface};
use rndr_core::{App, Layout};

fn main() {
    let source = include_str!("../counter.gr");
    let mut app = App::new();
    install(&mut app);

    let parent = app.world_mut().spawn((Layout::column(),));
    let mut surface =
        GraphemeSurface::mount(app.world_mut(), parent, source, None).expect("mount CounterApp");

    let before = surface.snapshot().expect("synced").clone();
    println!(
        "mounted {} @ revision {} with {} actions",
        before.glyph,
        before.revision,
        before.actions.len()
    );
    for node in surface.projected_nodes(app.world()) {
        println!("  node {} role={:?}", node.key, node.role);
    }

    let increment = before
        .actions
        .iter()
        .find(|a| a.key.ends_with("/increment"))
        .expect("increment action")
        .id
        .clone();

    let commit = surface
        .invoke_action(app.world_mut(), &increment)
        .expect("invoke Increment");
    let after = surface.snapshot().expect("synced after click");
    println!(
        "after click: revision {} diff={}",
        commit.revision,
        commit
            .committed
            .as_ref()
            .map(|r| serde_json::to_string(&r.diff).unwrap_or_else(|_| "{}".into()))
            .unwrap_or_else(|| "{}".into())
    );
    println!(
        "label now: {}",
        after.ir["root"]["children"][0]["props"]["content"]
    );
}
