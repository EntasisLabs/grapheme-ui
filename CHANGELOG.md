# Changelog

All notable changes to grapheme-ui are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Reload no longer reuses a widget whose element changed.** View IR keys are positional, so a reload that inserted an element (say a `Spacer` before a `Button`) moved the button onto a new key and left the old button at the old key, showing two buttons. The projection now records which catalog element sits at each key; when that changes, it despawns the old widget and its subtree and mounts the new one with a fresh UiId.
- **Dropping a surface while a task step is in flight no longer hangs.** If the worker had sent a step the UI had not received yet, `SessionBridge`'s drop sent no ack and then joined a worker that was waiting for one. Drop now closes the ack channel, so the worker finishes the task and exits.
- **`Harness::settle` waits for a running store action.** Two equal frames while the worker was between task steps counted as settled, so a slow machine could assert on the `pending` view (seen as a hang, then a golden mismatch, in `feed_after_click_ir` on macOS). Frames while an action is running or queued no longer count, with a 10 s cap.

## [0.3.0] - 2026-11-27

Depends on **grapheme 0.11.0** (`grapheme-sdk`, `grapheme-runtime`) and **rndr 0.6.0**. Hosts that link grapheme 0.10 must move to 0.11 with this release.

### Added

- **Layout and style.** View `style { }` (width, height, min, max, padding, gap, align, justify, fill, text color, font size, weight) projects onto rndr `Layout` and rndr_ui theme tokens. Light and dark follow the OS appearance; tests can force either. Button `hover`, `pressed`, `focused` and `disabled` blocks restyle that button. Catalog primitives `Spacer` and `Stack` (overlay) ship in `catalog/v1.json`. (#17)
- **Reload in place.** `GraphemeSurface::reload_source` recompiles a program into the running session through grapheme's `StoreSession::reload` (RFC-0007 A.9): plain fields keep their values, transient fields reset, and a running task is cancelled. A program that does not compile leaves the last good tree up and shows an in-window overlay (`ERROR_OVERLAY_KEY`) with the file, line, column and message; the next good reload clears it. `testing::Harness::reload_source` does the same headlessly. (#18)
- **`desktop::dev_file`.** Opens a `.gr` file and reloads it in place on save; the window wakes within one file-watch event. (#18)

### Changed

- **Diagnostics come from grapheme's source spans.** `load` reads line and column from grapheme 0.11's structured diagnostics instead of parsing them out of the error text. (#18)
- **grapheme 0.11.0 from crates.io.** Was 0.10.0.

## [0.2.0] - 2026-11-27

Depends on **grapheme 0.10.0** (`grapheme-sdk`, `grapheme-runtime`) and **rndr 0.6.0**. Hosts that link grapheme 0.9 must move to 0.10 with this release.

### Added

- **Element catalog `catalog/v1.json` (`grapheme-ui/v1`), bundled in the crate.** grapheme-ui owns the element catalog: Column, Row, Text, Button and TextField. Every compile in `load` and in the surface engine passes it to `GraphemeEngine::builder().element_catalog`. Projection dispatches on the element name through a table, so a host can add an element (catalog entry plus projector) without a grapheme change. Unknown elements and props name `grapheme-ui/v1` with a line and column.
- **Test harness `grapheme_ui::testing::Harness`.** Mounts a `.gr` file headlessly, clicks, presses keys, types or invokes actions, and reads the View IR, the access tree and laid-out rects. `settle` runs frames until the View IR stops changing. View IR JSON goldens run on every OS; PNG goldens (feature `capture`, via `rndr_render::capture`) run on Linux with lavapipe with a per-channel tolerance of 16. `UPDATE_GOLDENS=1` rewrites them.

### Changed

- **Store actions run off the UI thread.** `GraphemeSurface` owns a worker thread for the store session. A task doing IO no longer freezes the window, and each task `apply` is painted before the task continues (via grapheme's `StoreSession::set_commit_observer`). A click that arrives while a task runs is queued and runs after it, in order; if an earlier invoke rebinds its action id, it fails with `stale_action`. `GraphemeSurface`'s public signatures are unchanged.
- **rndr 0.6.0.** The workspace moves from rndr 0.5.0 to 0.6.0. No call sites changed.
- **grapheme 0.10.0 from crates.io.** Was 0.9.0.

## [0.1.0] - 2026-11-11

First release: the bridge between the Grapheme View IR / headless store session and rndr, with `desktop::run_program` and `desktop::run_file` behind the `desktop` feature.
