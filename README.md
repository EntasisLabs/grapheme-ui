# grapheme-ui

Bridge from [Grapheme](https://github.com/EntasisLabs/grapheme) View IR to [rndr](https://github.com/EntasisLabs/rndr).

GraphemeUI is to Grapheme what SwiftUI is to Swift: the layer that turns a verified UI glyph into a native tree. It is also the **only** crate that depends on both Grapheme and rndr.

## Dependency model (A)

```text
grapheme  ─────────────────────────  no UI / renderer deps
   ▲
grapheme-ui  ──►  rndr_core / rndr_ui
   ▲
host application
```

- **Grapheme** never depends on this crate or on rndr.
- **rndr** never depends on Grapheme or this crate.
- **Host apps** that want Grapheme UI add `grapheme-ui` to their `Cargo.toml` (opt-in at the app, not inside rndr).
- This crate depends on the published **grapheme 0.8.0** family (`grapheme-sdk`, `grapheme-runtime`) and **rndr 0.5.0** (`rndr_core`, `rndr_ui`). Default tests stay headless: no window and no GPU.

A later convenience re-export on the rndr facade (`features = ["grapheme"]`) is model **B** and is deliberately deferred.

## Two host shapes, one API

| Shape | Who owns the window | How you use this crate |
| --- | --- | --- |
| Full Grapheme app | your runner (via rndr) | `GraphemeSurface::mount` on the window root |
| rndr app with Grapheme inside | the rndr host | `GraphemeSurface::mount` under a host-owned parent entity |

Both call the same surface. Embedding is not “script rndr frames with Grapheme”; it is mounting a compiled UI glyph into a host slot and driving it through the app session (`mount` / `sync` / `invoke_action` / `drain_actions`).

## Quick start (headless)

```rust
use grapheme_ui::{install, GraphemeSurface};
use rndr_core::{App, Layout};

let mut app = App::new();
install(&mut app);
let parent = app.world_mut().spawn((Layout::column(),));
let mut surface = GraphemeSurface::mount(app.world_mut(), parent, source, None)?;
let id = surface.actions().iter().find(|a| a.key.ends_with("/increment")).unwrap().id.clone();
surface.invoke_action(app.world_mut(), &id)?;
```

Try the counter example:

```sh
cargo run -p counter
cargo test -p grapheme-ui
```

## Catalog (v1)

Projects View IR primitives `Column`, `Row`, `Text`, and `Button` onto rndr layouts / labels / `rndr_ui::controls::Button`. Unknown primitives are rejected. The compiler’s built-in catalog stays the source of signatures until GraphemeUI publishes them as loadable modules.

## Status

Vertical slice: headless mount of `CounterApp`, projected node keys/roles, and surface invoke parity with the headless Grapheme session. Windowed desktop runner and rndr facade re-export (B) are next.
