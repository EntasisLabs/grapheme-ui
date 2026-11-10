//! Project evaluated View IR nodes onto rndr entities.

use std::collections::{HashMap, HashSet};

use rndr_core::{
    AccessRole, Axis, CollectionAccess, Entity, Label, Layout, Size, World, despawn, set_parent,
};
use rndr_ui::controls::{Button, ButtonProps, HostIntent, host_intent};
use rndr_ui::{UiId, UiPlugin};
use serde_json::Value as JsonValue;

use crate::error::GraphemeUiError;
use crate::surface::{GraphemeNode, ProjectedNode};

/// Bookkeeping for one projected tree under a surface root.
pub(crate) struct Projection {
    /// Node key → entity.
    pub entities: HashMap<String, Entity>,
    /// Node key → stable UiId (unique under its parent for Button::mount).
    ui_ids: HashMap<String, u64>,
    next_ui: u64,
    /// HostIntent::Command token → Grapheme action id for this sync.
    pub commands: HashMap<u64, String>,
    next_cmd: u64,
}

impl Projection {
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            ui_ids: HashMap::new(),
            next_ui: 1,
            commands: HashMap::new(),
            next_cmd: 1,
        }
    }

    fn ui_id_for(&mut self, key: &str) -> UiId {
        let id = *self.ui_ids.entry(key.to_string()).or_insert_with(|| {
            let id = self.next_ui;
            self.next_ui += 1;
            id
        });
        UiId(id)
    }

    fn command_for(&mut self, action_id: &str) -> u64 {
        let cmd = self.next_cmd;
        self.next_cmd += 1;
        self.commands.insert(cmd, action_id.to_string());
        cmd
    }
}

/// Ensure `UiPlugin` is installed once on the app world.
pub fn ensure_ui_plugin(app: &mut rndr_core::App) {
    app.add_plugin(UiPlugin);
}

/// Project `root` IR under `parent`. Returns the projected root entity.
pub(crate) fn project_tree(
    world: &mut World,
    parent: Entity,
    root: &JsonValue,
    actions: &[grapheme_runtime::ViewAction],
    proj: &mut Projection,
) -> Result<Entity, GraphemeUiError> {
    proj.commands.clear();
    proj.next_cmd = 1;
    let mut seen = HashSet::new();
    let entity = project_node(world, parent, root, actions, proj, &mut seen)?;
    // Drop entities whose keys disappeared this revision.
    let stale: Vec<_> = proj
        .entities
        .keys()
        .filter(|k| !seen.contains(k.as_str()))
        .cloned()
        .collect();
    for key in stale {
        if let Some(entity) = proj.entities.remove(&key) {
            // Cascade despawn is a no-op for dead ids (parent may have taken them).
            if world.is_alive(entity) {
                despawn(world, entity);
            }
        }
        proj.ui_ids.remove(&key);
    }
    Ok(entity)
}

fn project_node(
    world: &mut World,
    parent: Entity,
    node: &JsonValue,
    actions: &[grapheme_runtime::ViewAction],
    proj: &mut Projection,
    seen: &mut HashSet<String>,
) -> Result<Entity, GraphemeUiError> {
    let key = node
        .get("key")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| GraphemeUiError::Unsupported("View IR node missing key".into()))?
        .to_string();
    seen.insert(key.clone());
    let role = node
        .get("role")
        .and_then(JsonValue::as_str)
        .map(str::to_string);

    if let Some(prim) = node.get("$prim").and_then(JsonValue::as_str) {
        match prim {
            "Column" | "Row" => {
                project_group(world, parent, &key, role, prim, node, actions, proj, seen)
            }
            "Text" => project_text(world, parent, &key, role, node, proj),
            "Button" => project_button(world, parent, &key, role, node, actions, proj),
            other => Err(GraphemeUiError::Unsupported(format!(
                "primitive '{other}' is not in the GraphemeUI v1 catalog"
            ))),
        }
    } else if node.get("$view").is_some() {
        // Inlined view reference: the evaluated tree already has $prim on the
        // same node (see StoreSession). If only $view remains, treat children.
        if node.get("$prim").is_some() {
            unreachable!("handled above");
        }
        // Fallback: project first child or empty group.
        project_group(
            world, parent, &key, role, "Column", node, actions, proj, seen,
        )
    } else {
        Err(GraphemeUiError::Unsupported(format!(
            "node '{key}' has neither $prim nor $view"
        )))
    }
}

#[allow(clippy::too_many_arguments)]
fn project_group(
    world: &mut World,
    parent: Entity,
    key: &str,
    role: Option<String>,
    prim: &str,
    node: &JsonValue,
    actions: &[grapheme_runtime::ViewAction],
    proj: &mut Projection,
    seen: &mut HashSet<String>,
) -> Result<Entity, GraphemeUiError> {
    let gap = node
        .pointer("/props/gap")
        .and_then(JsonValue::as_f64)
        .unwrap_or(0.0) as f32;
    let axis = if prim == "Row" {
        Axis::Row
    } else {
        Axis::Column
    };
    let entity = if let Some(&existing) = proj.entities.get(key) {
        if let Some(layout) = world.get_mut::<Layout>(existing) {
            layout.axis = axis;
            layout.gap = gap;
        }
        existing
    } else {
        let layout = Layout {
            axis,
            gap,
            padding: 0.0,
            width: Size::Hug,
            height: Size::Hug,
            min_width: 0.0,
            min_height: 0.0,
        };
        let e = world.spawn((layout,));
        set_parent(world, e, parent);
        proj.entities.insert(key.to_string(), e);
        e
    };
    world.insert(
        entity,
        GraphemeNode {
            key: key.to_string(),
            role: role.or_else(|| Some("group".into())),
        },
    );
    // Layout groups are not focusable. CollectionAccess publishes
    // AccessRole::Group without adding a tab stop.
    world.insert(
        entity,
        CollectionAccess {
            role: AccessRole::Group,
            expanded: None,
            level: None,
            parent: None,
        },
    );
    if let Some(children) = node.get("children").and_then(JsonValue::as_array) {
        for child in children {
            project_node(world, entity, child, actions, proj, seen)?;
        }
    }
    Ok(entity)
}

fn project_text(
    world: &mut World,
    parent: Entity,
    key: &str,
    role: Option<String>,
    node: &JsonValue,
    proj: &mut Projection,
) -> Result<Entity, GraphemeUiError> {
    let content = node
        .pointer("/props/content")
        .map(json_to_label)
        .unwrap_or_default();
    let entity = if let Some(&existing) = proj.entities.get(key) {
        if let Some(label) = world.get_mut::<Label>(existing) {
            label.text = content.clone();
        }
        existing
    } else {
        let e = world.spawn((Label::new(content),));
        set_parent(world, e, parent);
        proj.entities.insert(key.to_string(), e);
        e
    };
    world.insert(
        entity,
        GraphemeNode {
            key: key.to_string(),
            role: role.or_else(|| Some("text".into())),
        },
    );
    Ok(entity)
}

fn project_button(
    world: &mut World,
    parent: Entity,
    key: &str,
    role: Option<String>,
    node: &JsonValue,
    actions: &[grapheme_runtime::ViewAction],
    proj: &mut Projection,
) -> Result<Entity, GraphemeUiError> {
    let label = node
        .pointer("/props/label")
        .and_then(JsonValue::as_str)
        .unwrap_or("Button")
        .to_string();
    let enabled = node
        .pointer("/props/enabled")
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);
    let action_id = node
        .pointer("/events/click/id")
        .and_then(JsonValue::as_str)
        .or_else(|| {
            actions
                .iter()
                .find(|a| a.key == key && a.event == "click")
                .map(|a| a.id.as_str())
        })
        .ok_or_else(|| {
            GraphemeUiError::Unsupported(format!(
                "button '{key}' has no click action id in IR or action list"
            ))
        })?;
    let cmd = proj.command_for(action_id);
    let mut props = ButtonProps::new(label, HostIntent::Command(cmd));
    props.enabled = enabled;
    let ui_id = proj.ui_id_for(key);
    let handle = Button::mount(world, parent, ui_id, props)?;
    proj.entities.insert(key.to_string(), handle.root);
    world.insert(
        handle.root,
        GraphemeNode {
            key: key.to_string(),
            role: role.or_else(|| Some("button".into())),
        },
    );
    Ok(handle.root)
}

fn json_to_label(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Null => String::new(),
        other => other.to_string(),
    }
}

/// Resolve a drained rndr action intent to a Grapheme action id, if this
/// projection owns the command token.
pub(crate) fn resolve_command(
    world: &World,
    proj: &Projection,
    request: &rndr_core::ActionIntent,
) -> Option<String> {
    let intent = host_intent(world, request)?;
    match intent {
        HostIntent::Command(cmd) => proj.commands.get(&cmd).cloned(),
        HostIntent::RevealSource(_) => None,
    }
}

/// Snapshot of projected nodes for tests / accessibility checks.
pub(crate) fn projected_nodes(proj: &Projection, world: &World) -> Vec<ProjectedNode> {
    let mut out: Vec<_> = proj
        .entities
        .iter()
        .filter_map(|(key, &entity)| {
            let node = world.get::<GraphemeNode>(entity)?;
            Some(ProjectedNode {
                key: key.clone(),
                role: node.role.clone(),
                entity,
            })
        })
        .collect();
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}
