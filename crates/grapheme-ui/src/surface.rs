//! Mount a Grapheme UI glyph into an rndr world.

use std::collections::HashMap;

use std::path::Path;

use grapheme_runtime::{AppSnapshot, StoreCommit, ViewAction};
use grapheme_sdk::{GraphemeEngine, GraphemeSdkError, GraphemeStoreSession};

use crate::bridge::{SessionBridge, WorkerEvent};
use crate::engine::SurfaceBuilder;
use crate::load::{Diagnostic, format_diagnostic};
use rndr_core::{
    App, Color, Entity, Label, Layout, Schedule, Semantics, Size, TextChanged, World, despawn,
    events, set_parent, take_action_intents_matching,
};
use rndr_ui::controls::host_intent;
use serde_json::{Value as JsonValue, json};

use crate::error::GraphemeUiError;
use crate::project::{self, Projection};

/// Marker + identity for a projected View IR node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphemeNode {
    /// Stable View IR node key (RFC-0007 I.1 / I.3).
    pub key: String,
    /// Semantic role from View IR (`button`, `text`, `group`, …).
    pub role: Option<String>,
}

impl rndr_core::Component for GraphemeNode {}

/// One projected node, for tests and accessibility checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedNode {
    /// View IR key.
    pub key: String,
    /// View IR role, if any.
    pub role: Option<String>,
    /// rndr entity currently backing the node.
    pub entity: Entity,
}

/// View IR key of the dev-loop error overlay. Not part of the program's tree.
pub const ERROR_OVERLAY_KEY: &str = "__grapheme_error_overlay";

/// A Grapheme UI glyph mounted under an rndr parent.
///
/// Use this for both shapes in model A:
/// - **Full app:** the window's root content is a surface.
/// - **Embed:** an rndr component hosts the surface as a child.
pub struct GraphemeSurface {
    /// Store session, on a worker thread. The UI thread sends actions and
    /// applies the snapshots that come back.
    bridge: SessionBridge,
    glyph: String,
    /// Host-owned container under the caller's parent.
    root: Entity,
    projection: Projection,
    last: Option<AppSnapshot>,
    /// Catalog document used to compile this surface, including reloads.
    catalog: String,
    /// Banner shown over the last good tree when a reload fails to compile.
    overlay: Option<Entity>,
}

impl GraphemeSurface {
    /// Compile `source`, mount its UI glyph (or `glyph` if given), and project
    /// the first snapshot under `parent`.
    ///
    /// Uses a default engine. Pass a policy, host, interceptor, or module
    /// registry through [`Self::builder`].
    pub fn mount(
        world: &mut World,
        parent: Entity,
        source: &str,
        glyph: Option<&str>,
    ) -> Result<Self, GraphemeUiError> {
        Self::builder().mount(world, parent, source, glyph)
    }

    /// Start a [`SurfaceBuilder`] for host engine configuration.
    pub fn builder() -> SurfaceBuilder {
        SurfaceBuilder::new()
    }
}

impl SurfaceBuilder {
    /// Compile `source` with this engine configuration and project the glyph.
    pub fn mount(
        self,
        world: &mut World,
        parent: Entity,
        source: &str,
        glyph: Option<&str>,
    ) -> Result<GraphemeSurface, GraphemeUiError> {
        let catalog = self.catalog_document();
        let engine = self.engine()?;
        let mut session = engine.store_session(source)?;
        let snapshot = session.mount(glyph)?;
        let bridge = SessionBridge::spawn(session)?;
        let root = world.spawn((Layout {
            width: Size::Fill,
            height: Size::Hug,
            ..Layout::column()
        },));
        set_parent(world, root, parent);
        let mut surface = GraphemeSurface {
            bridge,
            glyph: snapshot.glyph.clone(),
            root,
            projection: Projection::new(),
            last: None,
            catalog,
            overlay: None,
        };
        surface.project_snapshot(world, snapshot)?;
        Ok(surface)
    }
}

impl GraphemeSurface {
    /// Surface container entity (child of the host parent).
    pub fn root(&self) -> Entity {
        self.root
    }

    /// Mounted UI glyph name.
    pub fn glyph(&self) -> &str {
        &self.glyph
    }

    /// Latest app snapshot, if synced at least once.
    pub fn snapshot(&self) -> Option<&AppSnapshot> {
        self.last.as_ref()
    }

    /// Store session for headless `invoke` / `query` alongside the surface.
    ///
    /// The session normally lives on the surface's worker thread. This checks
    /// it out onto the caller, waiting until any in-flight invoke finishes.
    /// The next [`Self::drain_actions`], [`Self::invoke_action`], or
    /// [`Self::sync`] sends it back.
    pub fn session(&mut self) -> &mut GraphemeStoreSession {
        self.bridge
            .checkout()
            .expect("grapheme session thread stopped")
    }

    /// Recompile `source` and migrate the running session (RFC-0007 A.9).
    ///
    /// A compile failure leaves the last good tree in place and shows an
    /// in-window overlay with `file`, line, column and the diagnostic.
    /// The next successful save clears the overlay. `file` is the path named
    /// on the overlay; it is not read.
    pub fn reload_source(
        &mut self,
        world: &mut World,
        source: &str,
        file: Option<&Path>,
    ) -> Result<(), GraphemeUiError> {
        let engine = GraphemeEngine::builder()
            .element_catalog(&self.catalog)?
            .build();
        match engine.compile_artifact(source) {
            Ok(artifact) => {
                self.session().reload(artifact)?;
                self.clear_overlay(world);
                if let Err(err) = self.sync(world) {
                    self.show_overlay(
                        world,
                        &Diagnostic {
                            message: err.to_string(),
                            file: file.map(|path| path.display().to_string()),
                            line: None,
                            column: None,
                        },
                    );
                }
                Ok(())
            }
            Err(err) => {
                let diagnostic = diagnostic_from_sdk(&err, file);
                self.show_overlay(world, &diagnostic);
                Ok(())
            }
        }
    }

    fn show_overlay(&mut self, world: &mut World, diagnostic: &Diagnostic) {
        self.clear_overlay(world);
        let text = format_diagnostic(diagnostic);
        let entity = world.spawn((
            Layout {
                width: Size::Fill,
                height: Size::Hug,
                padding: 12.0,
                ..Layout::column()
            },
            Color::rgb(0.45, 0.12, 0.14),
            Label::new(text.clone()),
            Semantics {
                name: Some(text),
                description: String::new(),
                readonly: true,
            },
            GraphemeNode {
                key: ERROR_OVERLAY_KEY.to_string(),
                role: Some("text".into()),
            },
        ));
        set_parent(world, entity, self.root);
        self.projection
            .entities
            .insert(ERROR_OVERLAY_KEY.to_string(), entity);
        self.overlay = Some(entity);
    }

    fn clear_overlay(&mut self, world: &mut World) {
        self.projection.entities.remove(ERROR_OVERLAY_KEY);
        if let Some(entity) = self.overlay.take()
            && world.is_alive(entity)
        {
            despawn(world, entity);
        }
    }

    /// Re-evaluate the glyph and reconcile rndr entities with View IR.
    ///
    /// While a task is in flight this returns the snapshot already projected
    /// for the latest commit, so a caller does not block on the capability.
    pub fn sync(&mut self, world: &mut World) -> Result<&AppSnapshot, GraphemeUiError> {
        if self.bridge.inflight() {
            return Ok(self.last.as_ref().expect("mounted before sync"));
        }
        let snapshot = self.bridge.current_snapshot()?;
        self.project_snapshot(world, snapshot)?;
        Ok(self.last.as_ref().expect("just projected"))
    }

    /// Projected nodes currently in the world (sorted by key).
    pub fn projected_nodes(&self, world: &World) -> Vec<ProjectedNode> {
        project::projected_nodes(&self.projection, world)
    }

    /// Invoke a Grapheme action id (same path a button click uses), then sync.
    ///
    /// Passes no payload. An action that requires one fails the same way a
    /// headless session does. The call blocks until the action finishes,
    /// including a task capability. Each task `apply` is projected onto
    /// `world` before the task continues. The frame loop should use
    /// [`Self::drain_actions`] instead, which returns while a capability runs
    /// so the window can paint.
    pub fn invoke_action(
        &mut self,
        world: &mut World,
        id: &str,
    ) -> Result<StoreCommit, GraphemeUiError> {
        self.finish_queued(world)?;
        self.bridge.enqueue(id.to_string(), None);
        let mut commits = self.finish_queued(world)?;
        commits
            .pop()
            .ok_or_else(|| GraphemeUiError::Usage(format!("invoke produced no commit for '{id}'")))
    }

    /// Drain rndr action intents owned by this surface.
    ///
    /// `TextChanged` events on projected text fields run first, each with
    /// payload `{ "value": <text> }`. Button command intents follow, with no
    /// payload. Each successful invoke is synced before the next id runs. A later id
    /// can go stale when an earlier invoke rebinds that action (its epoch
    /// moved). Syncing first keeps the widget tree and the command map on the
    /// store, so a click after this error uses the new id. The same outcome
    /// is stored as [`DrainStatus`] for hosts that drain from an Update system.
    ///
    /// Invokes run on the surface's session thread, one at a time. A click
    /// that arrives while a task is running is queued and starts only after
    /// that task finishes (it is not dropped and it does not run beside it).
    /// The queued value is the action id resolved when the intent was drained.
    /// If an earlier invoke rebinds that id, the queued invoke fails with
    /// [`GraphemeUiError::StaleAction`] and [`DrainStatus`] records the error
    /// the same way two intents in one frame do.
    ///
    /// A task `apply` is projected and this call returns before the task
    /// continues, so the frame can paint that commit (the `Loading…` state)
    /// while a later capability is still running. The next Update acknowledges
    /// the step and the task proceeds. A mutation, which commits once, still
    /// finishes inside this call.
    pub fn drain_actions(
        &mut self,
        world: &mut World,
    ) -> Result<Vec<StoreCommit>, GraphemeUiError> {
        self.bridge.ensure_checked_in()?;
        self.bridge.prepare_frame();
        for (id, payload) in self.collect_jobs(world) {
            self.bridge.enqueue(id, payload);
        }
        let mut commits = Vec::new();
        let outcome = loop {
            if let Err(err) = self.bridge.dispatch() {
                break Err(err);
            }
            if !self.bridge.has_work() {
                break Ok(());
            }
            let Some(event) = self.bridge.poll_frame_event()? else {
                break Ok(());
            };
            match event {
                WorkerEvent::Step(snapshot) => {
                    if let Err(err) = self.project_snapshot(world, snapshot) {
                        break Err(err);
                    }
                    self.bridge.defer_ack();
                    break Ok(());
                }
                WorkerEvent::Done { result, snapshot } => {
                    if let Some(snapshot) = snapshot
                        && let Err(err) = self.project_snapshot(world, snapshot)
                    {
                        break Err(err);
                    }
                    match result {
                        Ok(commit) => commits.push(commit),
                        Err(err) => break Err(map_session_error(err)),
                    }
                }
            }
        };
        record_drain(world, commits, outcome.err())
    }

    fn collect_jobs(&self, world: &mut World) -> Vec<(String, Option<JsonValue>)> {
        let owned: HashMap<Entity, ()> = self
            .projection
            .entities
            .values()
            .map(|&e| (e, ()))
            .collect();
        let requests = take_action_intents_matching(world, |w, intent| {
            owned.contains_key(&intent.entity) && host_intent(w, intent).is_some()
        });
        let mut jobs = Vec::new();
        for event in events::<TextChanged>(world) {
            if let Some(id) = self.projection.text_changes.get(&event.entity) {
                jobs.push((id.clone(), Some(json!({ "value": event.text }))));
            }
        }
        for request in &requests {
            if let Some(id) = project::resolve_command(world, &self.projection, request) {
                jobs.push((id, None));
            }
        }
        jobs
    }

    /// Run queued and in-flight invokes to completion, acknowledging task
    /// steps immediately. This is the synchronous [`Self::invoke_action`] path.
    fn finish_queued(&mut self, world: &mut World) -> Result<Vec<StoreCommit>, GraphemeUiError> {
        self.bridge.ensure_checked_in()?;
        self.bridge.ack_now();
        let mut commits = Vec::new();
        loop {
            self.bridge.dispatch()?;
            if !self.bridge.has_work() {
                break;
            }
            match self.bridge.recv_worker_event()? {
                WorkerEvent::Step(snapshot) => {
                    self.project_snapshot(world, snapshot)?;
                    self.bridge.release_worker();
                }
                WorkerEvent::Done { result, snapshot } => {
                    if let Some(snapshot) = snapshot {
                        self.project_snapshot(world, snapshot)?;
                    }
                    match result {
                        Ok(commit) => commits.push(commit),
                        Err(err) => return Err(map_session_error(err)),
                    }
                }
            }
        }
        Ok(commits)
    }

    /// Current action list from the last snapshot.
    pub fn actions(&self) -> &[ViewAction] {
        self.last
            .as_ref()
            .map(|s| s.actions.as_slice())
            .unwrap_or(&[])
    }

    /// Evaluated View IR root from the last snapshot.
    pub fn ir_root(&self) -> Option<&JsonValue> {
        self.last.as_ref().map(|s| &s.ir["root"])
    }

    fn project_snapshot(
        &mut self,
        world: &mut World,
        snapshot: AppSnapshot,
    ) -> Result<(), GraphemeUiError> {
        let root_ir = snapshot
            .ir
            .get("root")
            .ok_or_else(|| GraphemeUiError::Unsupported("app snapshot IR missing root".into()))?;
        project::project_tree(
            world,
            self.root,
            root_ir,
            &snapshot.actions,
            &mut self.projection,
        )?;
        self.last = Some(snapshot);
        Ok(())
    }
}

/// Install [`rndr_ui::UiPlugin`] on an [`rndr_core::App`] (idempotent).
pub fn install(app: &mut rndr_core::App) {
    project::ensure_ui_plugin(app);
}

/// Outcome of the latest [`GraphemeSurface::drain_actions`] call.
///
/// Hosts read this after Update. [`drain_installed`] writes it so a failed
/// drain is visible without parsing stderr.
#[derive(Clone, Debug, Default)]
pub struct DrainStatus {
    /// Commits applied by that drain, in order. Each one was synced.
    pub commits: Vec<StoreCommit>,
    /// Set when a queued action failed after the commits above were synced.
    pub error: Option<String>,
}

/// One driven surface, stored as a UI-thread local so an Update system can
/// take it out, drain intents, and put it back.
///
/// One surface per world. [`GraphemeSurface::drive`] refuses a second one.
struct InstalledSurface(Option<GraphemeSurface>);

/// Puts a taken surface back, including when the caller panics.
struct RestoreDriven<'a> {
    world: &'a mut World,
    surface: Option<GraphemeSurface>,
}

impl Drop for RestoreDriven<'_> {
    fn drop(&mut self) {
        if let Some(surface) = self.surface.take() {
            restore_driven(self.world, surface);
        }
    }
}

impl GraphemeSurface {
    /// Park this surface on `app` and drain its actions on every Update,
    /// after rndr has turned a pointer or keyboard activation into a host intent.
    ///
    /// One surface per world. A second call returns [`GraphemeUiError::Usage`]
    /// and drops the surface passed in; the one already parked stays.
    ///
    /// # Errors
    ///
    /// [`GraphemeUiError::Usage`] when a surface is already parked on `app`.
    pub fn drive(self, app: &mut App) -> Result<(), GraphemeUiError> {
        if app.world().has_local::<InstalledSurface>() {
            return Err(GraphemeUiError::Usage(
                "one GraphemeSurface per world; a surface is already parked".into(),
            ));
        }
        app.world_mut().insert_local(InstalledSurface(Some(self)));
        app.world_mut().insert_resource(DrainStatus::default());
        app.add_system(Schedule::Update, drain_installed);
        Ok(())
    }
}

/// Update system installed by [`GraphemeSurface::drive`].
///
/// No-op when the surface is not parked. A failure is stored on
/// [`DrainStatus`] and printed. The surface stays parked either way.
pub fn drain_installed(world: &mut World) {
    let Some(surface) = take_driven(world) else {
        return;
    };
    let mut guard = RestoreDriven {
        world,
        surface: Some(surface),
    };
    let RestoreDriven { world, surface } = &mut guard;
    let surface = surface.as_mut().expect("driven surface");
    if let Err(err) = surface.drain_actions(world) {
        eprintln!("grapheme-ui: {err}");
    }
}

/// Run `f` with the surface parked by [`GraphemeSurface::drive`].
///
/// The surface is not in the world for the duration of `f`, so `f` may
/// borrow `world` exclusively. A drop guard puts the surface back if `f`
/// panics. Do not call [`GraphemeSurface::drive`] or [`drain_installed`]
/// from `f`.
pub fn with_driven_surface<R>(
    world: &mut World,
    f: impl FnOnce(&mut GraphemeSurface, &mut World) -> R,
) -> Result<R, GraphemeUiError> {
    let surface = take_driven(world).ok_or_else(|| {
        GraphemeUiError::Usage(
            "no driven GraphemeSurface; call GraphemeSurface::drive first".into(),
        )
    })?;
    let mut guard = RestoreDriven {
        world,
        surface: Some(surface),
    };
    let result = {
        let RestoreDriven { world, surface } = &mut guard;
        f(surface.as_mut().expect("driven surface"), world)
    };
    Ok(result)
}

fn take_driven(world: &mut World) -> Option<GraphemeSurface> {
    world
        .local_mut::<InstalledSurface>()
        .and_then(|slot| slot.0.take())
}

fn restore_driven(world: &mut World, surface: GraphemeSurface) {
    match world.local_mut::<InstalledSurface>() {
        Some(slot) => slot.0 = Some(surface),
        None => world.insert_local(InstalledSurface(Some(surface))),
    }
}

fn diagnostic_from_sdk(err: &GraphemeSdkError, path: Option<&Path>) -> Diagnostic {
    let span = err.diagnostics().into_iter().find_map(|diag| diag.span);
    Diagnostic {
        message: err.to_string(),
        file: path.map(|path| path.display().to_string()),
        line: span.as_ref().map(|span| span.line),
        column: span.map(|span| span.column),
    }
}

fn map_session_error(err: grapheme_sdk::GraphemeSdkError) -> GraphemeUiError {
    let msg = err.to_string();
    if msg.contains("stale_action") {
        GraphemeUiError::StaleAction(msg)
    } else {
        GraphemeUiError::Session(err)
    }
}

fn record_drain(
    world: &mut World,
    commits: Vec<StoreCommit>,
    error: Option<GraphemeUiError>,
) -> Result<Vec<StoreCommit>, GraphemeUiError> {
    world.insert_resource(DrainStatus {
        commits: commits.clone(),
        error: error.as_ref().map(ToString::to_string),
    });
    match error {
        Some(err) => Err(err),
        None => Ok(commits),
    }
}
