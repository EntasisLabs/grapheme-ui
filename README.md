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
- **rndr** never depends on Grapheme or this crate. rndr apps depend on grapheme-ui; rndr itself does not.
- **Host apps** that want Grapheme UI add `grapheme-ui` to their `Cargo.toml` (opt-in at the app, not inside rndr).
- This crate depends on the published **grapheme 0.9.0** family (`grapheme-sdk`, `grapheme-runtime`) and **rndr 0.5.0** (`rndr_core`, `rndr_ui`). Default tests stay headless: no window and no GPU.

A later convenience re-export on the rndr facade (`features = ["grapheme"]`) is model **B** and is deliberately deferred.

## Two host shapes, one API

| Shape | Who owns the window | How you use this crate |
| --- | --- | --- |
| Full Grapheme app | your runner (via rndr) | `GraphemeSurface::mount` on the window root |
| rndr app with Grapheme inside | the rndr host | `GraphemeSurface::mount` under a host-owned parent entity |

Both call the same surface. Embedding is not “script rndr frames with Grapheme”; it is mounting a compiled UI glyph into a host slot and driving it through the app session (`mount` / `sync` / `invoke_action` / `drain_actions`).

`GraphemeSurface::builder` passes a policy guard, host factory, capability interceptor, or module-registry edit into that session. `const` field seeds are not on the public SDK (`GraphemeEngine::store_session` always uses `StoreSession::new`), so the builder does not accept them.

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

## Desktop counter

The `desktop` feature opens a real rndr window (`rndr` + `rndr_render`). Default tests stay headless and do not compile the GPU runner. A display is required to launch the window; CI does not start it.

```sh
cargo run -p counter-desktop --features desktop
```

Clicks take rndr's normal pointer path: the button emits `HostIntent::Command`, and an Update system calls `drain_actions`, which maps the token back to a Grapheme action id. The headless test `drain_click_matches_headless_session` drives that path with a synthetic pointer (no window) and checks the store against a pure Grapheme session.

Manual checklist when a display is available:

1. The window title is `GraphemeUI — Counter`.
2. The label reads `Count: 0 (doubled: 0)`, with buttons Increment and Add 5.
3. Click Increment. The label becomes `Count: 1 (doubled: 2)`.
4. Click Increment again, then Add 5. The label becomes `Count: 7 (doubled: 14)`.
5. Close the window. The process exits.

## Accessibility

After layout, every projected button is an `AccessRole::Button` in `rndr_core::access_tree`, every projected text node is an `AccessRole::Label`, and every projected text field is an `AccessRole::TextInput` whose name is the `label` prop. The role comes from the rndr widget (`Button`, `Label`, `TextField`), and `GraphemeNode.role` repeats the View IR role on the same entity (`button`, `text`, `textbox`). `cargo test -p grapheme-ui --test access` checks both, including after a click changes the label.

`Column` and `Row` keep `GraphemeNode.role = "group"` and also carry `CollectionAccess` with `AccessRole::Group`. That marker does not make the group focusable, so it is not a tab stop. `rndr_core::access_tree` is unconditional. Publishing that tree to the operating system needs the `accesskit` feature on `rndr_render`, which the desktop feature enables. This check does not open a window.

`expose:` actions (key `expose/Increment`) are session entry points, not widgets. Every `click` action key is in the projected set.

## Embed

An rndr app can mount a Grapheme glyph in a slot it owns. The host keeps its own widgets; guest clicks are drained by the surface and never require the host to know action id strings.

```sh
cargo run -p embed
cargo test -p embed
```

`cargo run -p embed --features desktop` opens the same shell in a window (needs a display). rndr apps depend on grapheme-ui; rndr itself does not.

## Catalog (v1)

Projects View IR primitives `Column`, `Row`, `Text`, `Button`, and `TextField` onto rndr layouts, labels, `rndr_ui::controls::Button`, and `rndr_core::TextField`. A `TextField` `change` is drained with payload `{ "value": "…" }`. Unknown primitives are rejected. The compiler’s built-in catalog stays the source of signatures until GraphemeUI publishes them as loadable modules.

## Status

Vertical slice: headless mount of `CounterApp`, projected node keys/roles, surface invoke parity with the headless Grapheme session, a desktop runner that drains real button activations, an access-tree check that buttons and labels keep rndr's roles, and an embed demo where an rndr host mounts the guest. The rndr facade re-export (model B) stays deferred: it would be an optional feature in the rndr repo, and rndr would depend only on this crate.
