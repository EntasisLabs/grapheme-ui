# Changelog

All notable changes to grapheme-ui are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
