//! Mount a Grapheme UI glyph into an rndr world.

use std::collections::HashMap;

use grapheme_runtime::{AppSnapshot, StoreCommit, ViewAction};
use grapheme_sdk::{GraphemeEngine, GraphemeStoreSession};
use rndr_core::{
    App, Entity, Layout, Schedule, Size, World, set_parent, take_action_intents_matching,
};
use rndr_ui::controls::host_intent;
use serde_json::Value as JsonValue;

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

/// A Grapheme UI glyph mounted under an rndr parent.
///
/// Use this for both shapes in model A:
/// - **Full app:** the window's root content is a surface.
/// - **Embed:** an rndr component hosts the surface as a child.
pub struct GraphemeSurface {
    session: GraphemeStoreSession,
    glyph: String,
    /// Host-owned container under the caller's parent.
    root: Entity,
    projection: Projection,
    last: Option<AppSnapshot>,
}

impl GraphemeSurface {
    /// Compile `source`, mount its UI glyph (or `glyph` if given), and project
    /// the first snapshot under `parent`.
    pub fn mount(
        world: &mut World,
        parent: Entity,
        source: &str,
        glyph: Option<&str>,
    ) -> Result<Self, GraphemeUiError> {
        let engine = GraphemeEngine::builder().build();
        let mut session = engine.store_session(source)?;
        let snapshot = session.mount(glyph)?;
        let root = world.spawn((Layout {
            width: Size::Fill,
            height: Size::Hug,
            ..Layout::column()
        },));
        set_parent(world, root, parent);
        let mut surface = Self {
            session,
            glyph: snapshot.glyph.clone(),
            root,
            projection: Projection::new(),
            last: None,
        };
        surface.project_snapshot(world, snapshot)?;
        Ok(surface)
    }

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
    pub fn session(&mut self) -> &mut GraphemeStoreSession {
        &mut self.session
    }

    /// Re-evaluate the glyph and reconcile rndr entities with View IR.
    pub fn sync(&mut self, world: &mut World) -> Result<&AppSnapshot, GraphemeUiError> {
        let snapshot = self.session.app_snapshot()?;
        self.project_snapshot(world, snapshot)?;
        Ok(self.last.as_ref().expect("just projected"))
    }

    /// Projected nodes currently in the world (sorted by key).
    pub fn projected_nodes(&self, world: &World) -> Vec<ProjectedNode> {
        project::projected_nodes(&self.projection, world)
    }

    /// Invoke a Grapheme action id (same path a button click uses), then sync.
    pub fn invoke_action(
        &mut self,
        world: &mut World,
        id: &str,
    ) -> Result<StoreCommit, GraphemeUiError> {
        let commit = self.session.invoke_action(id).map_err(map_session_error)?;
        self.sync(world)?;
        Ok(commit)
    }

    /// Drain rndr action intents owned by this surface.
    ///
    /// Each successful invoke is synced before the next id runs. A later id
    /// can go stale when an earlier invoke rebinds that action (its epoch
    /// moved). Syncing first keeps the widget tree and the command map on the
    /// store, so a click after this error uses the new id. The same outcome
    /// is stored as [`DrainStatus`] for hosts that drain from an Update system.
    pub fn drain_actions(
        &mut self,
        world: &mut World,
    ) -> Result<Vec<StoreCommit>, GraphemeUiError> {
        let owned: HashMap<Entity, ()> = self
            .projection
            .entities
            .values()
            .map(|&e| (e, ()))
            .collect();
        let requests = take_action_intents_matching(world, |w, intent| {
            owned.contains_key(&intent.entity) && host_intent(w, intent).is_some()
        });
        let mut ids = Vec::new();
        for request in &requests {
            if let Some(id) = project::resolve_command(world, &self.projection, request) {
                ids.push(id);
            }
        }
        let mut commits = Vec::new();
        for id in ids {
            match self.session.invoke_action(&id) {
                Ok(commit) => {
                    commits.push(commit);
                    if let Err(err) = self.sync(world) {
                        return record_drain(world, commits, Some(err));
                    }
                }
                Err(err) => {
                    return record_drain(world, commits, Some(map_session_error(err)));
                }
            }
        }
        record_drain(world, commits, None)
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
