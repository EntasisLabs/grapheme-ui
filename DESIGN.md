# Design: keeping Grapheme × rndr dependencies boring

## Decision

Ship **model A** first:

1. `grapheme` — language, stores, views, View IR, headless app session. Zero renderer deps.
2. `rndr` — application-owned world and widgets. Zero Grapheme deps.
3. `grapheme-ui` (this repo) — the only bridge. Host applications depend on it directly when they want Grapheme UI.

**Model B** (later): an optional rndr facade feature that only re-exports `grapheme-ui`, never depends on `grapheme` crates directly.

## Why not put the bridge in either monorepo?

- Graphemeci would need rndr; rndr CI would need Grapheme.
- Workspace version coupling and feature creep (“just enable grapheme on rndr”).
- Harder to keep the RFC rule: *Grapheme does not depend on any renderer*.

## Embed vs full app

Same `GraphemeSurface` API:

- Full app: surface is the window content.
- Embed: an rndr component owns a parent entity; the surface mounts under it.

This matches rndr’s existing host-slot pattern (markdown slots, structured embeds): the host owns the mount; the guest has no authority over parent history or IO.

Not in scope: per-frame Grapheme scripting of rndr (RFC-0007 non-goal).

## Action routing

1. View IR action ids (node key + event + binding epoch) come from Grapheme’s session.
2. Each `Button` gets `HostIntent::Command(token)` for the current id.
3. Host drains `take_action_intents`, maps tokens back to action ids, calls `invoke_action`.
4. Stale epochs fail the same way headless sessions do (`stale_action`).

Native click and headless invoke therefore share one codepath.

The store session lives on a worker thread the surface owns. The UI thread sends action ids and payloads and applies the snapshots that come back. `drain_actions` runs from Update and does not wait inside a capability, so a task can paint each `apply` before it finishes. The runtime hook is `StoreSession::set_commit_observer` (no renderer types; see EntasisLabs/grapheme#50). Publish that grapheme change before this crate. Until it is on crates.io, grapheme-ui pins the commit by git.

A click that arrives while a task is running is queued and runs after that task, in order. It is not dropped and it does not run in parallel. The queued value is the action id from when the intent was drained. If an earlier invoke rebinds it, the queued invoke fails with `stale_action` and `DrainStatus` records that the same way two intents in one frame do.

## Catalog ownership

RFC-0007: the primitive catalog is owned by GraphemeUI. `catalog/v1.json` is that document (`$catalog` is `grapheme-ui/v1`). Every compile in `load` and `engine` passes it to `GraphemeEngine::builder().element_catalog`. The compiler checks element names, props, prop types, events and payload params. It does not render.

`project` dispatches `$prim` through a table keyed by the catalog element name (`Column`, `Row`, `Text`, `Button`, `TextField`). `Divider` is in that table and not in `v1.json`: a test adds it to a catalog document, and it compiles, projects and shows up in `access_tree` with no change to the Grapheme crates. Until the catalog hook is on crates.io, this repo's grapheme dependencies point at the grapheme pull request branch.
