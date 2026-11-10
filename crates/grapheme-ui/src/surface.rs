//! Mount a Grapheme UI glyph into an rndr world.

use std::collections::HashMap;

use grapheme_runtime::{AppSnapshot, StoreCommit, ViewAction};
use grapheme_sdk::{GraphemeEngine, GraphemeStoreSession};
use rndr_core::{Entity, Layout, Size, World, set_parent, take_action_intents_matching};
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
        let commit = self.session.invoke_action(id).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("stale_action") {
                GraphemeUiError::StaleAction(msg)
            } else {
                GraphemeUiError::Session(e)
            }
        })?;
        self.sync(world)?;
        Ok(commit)
    }

    /// Drain rndr action intents owned by this surface, invoke matching
    /// Grapheme actions, and sync once at the end.
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
        let mut commits = Vec::new();
        let mut ids = Vec::new();
        for request in &requests {
            if let Some(id) = project::resolve_command(world, &self.projection, request) {
                ids.push(id);
            }
        }
        for id in ids {
            let commit = self.session.invoke_action(&id).map_err(|e| {
                let msg = e.to_string();
                if msg.contains("stale_action") {
                    GraphemeUiError::StaleAction(msg)
                } else {
                    GraphemeUiError::Session(e)
                }
            })?;
            commits.push(commit);
        }
        if !commits.is_empty() {
            self.sync(world)?;
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
