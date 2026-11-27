//! Headless UI harness: mount a `.gr` file, drive it, and read the screen.
//!
//! The harness does not open a window. Pointer clicks, key presses, and
//! [`Harness::invoke`] go through the same parked surface a desktop host
//! drives. [`Harness::settle`] runs frames until the View IR stops changing.
//! The session applies those actions on this thread today. When the session
//! moves to a worker, `settle` is the place that waits for the published
//! snapshot; callers stay the same.
//!
//! ```
//! use grapheme_ui::testing::Harness;
//!
//! let source = include_str!("../../../examples/counter/counter.gr");
//! let mut ui = Harness::mount(source, Some("CounterApp")).unwrap();
//! ui.assert_ir_str("/children/0/props/content", "Count: 0 (doubled: 0)");
//! ui.click_labeled("Increment").unwrap();
//! ui.assert_ir_str("/children/0/props/content", "Count: 1 (doubled: 2)");
//! ```

use std::path::Path;

use serde_json::Value as JsonValue;

use crate::{GraphemeSurface, GraphemeUiError, install, with_driven_surface};
use rndr_core::{
    AccessRole, App, Entity, Focus, Input, KeyEvent, Layout, Modifiers, NamedKey, PointerButton,
    Rect, Size, TextField, Viewport, access_id, access_tree, platform_of,
};

/// Logical width of the harness viewport, in points.
pub const VIEWPORT_WIDTH: f32 = 480.0;
/// Logical height of the harness viewport, in points.
pub const VIEWPORT_HEIGHT: f32 = 360.0;

/// Per-channel absolute difference allowed when a PNG golden is compared.
///
/// Antialiasing on lavapipe can move a coverage byte without moving layout.
/// A channel past this value fails the golden. `GOLDEN_PIXEL_TOLERANCE`
/// overrides it for one run.
pub const PNG_CHANNEL_TOLERANCE: u8 = 16;

/// How long [`Harness::settle`] waits for the View IR to stop changing.
///
/// Two identical frames count as settled. The default of 8 frames is one
/// step for the synchronous session and spare frames for a worker that
/// publishes the snapshot later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settle {
    /// Maximum `App::update` calls, including the frame that must repeat.
    pub max_frames: u32,
}

impl Default for Settle {
    fn default() -> Self {
        Self { max_frames: 8 }
    }
}

/// A named key or one insertable character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Activates the focused button.
    Enter,
    /// Moves focus to the next focusable control.
    Tab,
    /// Press then release. A focused button activates on the release.
    Space,
    /// Deletes one character in the focused text field.
    Backspace,
    /// One insertable character, delivered on [`Input::chars`].
    Character(char),
}

/// One projected node in the access tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessEntry {
    /// View IR key.
    pub key: String,
    /// View IR role (`button`, `text`, `group`, `textbox`).
    pub view_role: Option<String>,
    /// rndr access role (`button`, `label`, `text_input`, `group`).
    /// Absent when the entity is not in the access tree.
    pub access_role: Option<String>,
    /// Accessible name. Empty when the node has none.
    pub name: String,
    /// Accessible value. Empty except on text inputs.
    pub value: String,
    /// The access node can take keyboard focus.
    pub focusable: bool,
}

/// Laid-out rectangle of one projected node, in logical points.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeRect {
    /// View IR key.
    pub key: String,
    /// Left edge, viewport origin.
    pub x: f32,
    /// Top edge, viewport origin.
    pub y: f32,
    /// Width. Zero when the entity has no rect yet.
    pub w: f32,
    /// Height. Zero when the entity has no rect yet.
    pub h: f32,
}

/// View IR, access tree, and rects for one settled frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Evaluated View IR root.
    pub ir: JsonValue,
    /// Projected nodes joined to the access tree, sorted by key.
    pub access: Vec<AccessEntry>,
    /// Projected nodes' layout rects, sorted by key.
    pub rects: Vec<NodeRect>,
}

/// A mounted `.gr` glyph in a headless rndr world.
pub struct Harness {
    app: App,
    /// Fill parent the glyph is mounted under. PNG capture frames this node,
    /// so a shift inside the viewport shows up in the pixels.
    #[cfg_attr(not(feature = "capture"), allow(dead_code))]
    parent: Entity,
    #[cfg(feature = "capture")]
    exporter: Option<rndr_render::Exporter>,
}

impl Harness {
    /// Mount `source` at [`VIEWPORT_WIDTH`] by [`VIEWPORT_HEIGHT`] and settle.
    ///
    /// `glyph` selects one UI glyph. `None` requires the file to contain one.
    pub fn mount(source: &str, glyph: Option<&str>) -> Result<Self, GraphemeUiError> {
        let mut viewport = Viewport::new(VIEWPORT_WIDTH, VIEWPORT_HEIGHT);
        viewport.scale = 1.0;
        Self::mount_in(source, glyph, viewport)
    }

    /// Mount `source` into an explicit viewport and settle.
    ///
    /// Scale should stay at 1 for PNG goldens. A 2× scale changes the pixel
    /// grid even when the layout in points is unchanged.
    pub fn mount_in(
        source: &str,
        glyph: Option<&str>,
        viewport: Viewport,
    ) -> Result<Self, GraphemeUiError> {
        let mut app = App::new();
        install(&mut app);
        app.world_mut().insert_resource(viewport);
        let parent = app.world_mut().spawn((Layout {
            padding: 16.0,
            gap: 8.0,
            width: Size::Fill,
            height: Size::Fill,
            ..Layout::column()
        },));
        let surface = GraphemeSurface::mount(app.world_mut(), parent, source, glyph)?;
        surface.drive(&mut app)?;
        let mut harness = Self {
            app,
            parent,
            #[cfg(feature = "capture")]
            exporter: None,
        };
        harness.settle()?;
        Ok(harness)
    }

    /// Run frames until the View IR repeats, or the frame budget runs out.
    ///
    /// This is the wait a later async session plugs into. Today the store
    /// session runs on this thread, so the second frame matches the first
    /// unless an action is still in flight.
    pub fn settle(&mut self) -> Result<(), GraphemeUiError> {
        self.settle_with(Settle::default())
    }

    /// [`Harness::settle`] with an explicit frame budget.
    pub fn settle_with(&mut self, settle: Settle) -> Result<(), GraphemeUiError> {
        let frames = settle.max_frames.max(1);
        let mut previous = None;
        for _ in 0..frames {
            self.app.update();
            let current = canonical_ir(&self.ir_now()?);
            if previous.as_ref() == Some(&current) {
                return Ok(());
            }
            previous = Some(current);
        }
        Err(GraphemeUiError::Usage(format!(
            "view IR still changing after {frames} frames"
        )))
    }

    /// Borrow the parked surface.
    ///
    /// The surface is out of the world for `f` and is put back afterwards,
    /// including when `f` panics. Do not call [`GraphemeSurface::drive`] or
    /// [`crate::drain_installed`] from `f`. A later worker-thread session can
    /// wait for the snapshot inside this method; `f` still sees a settled surface.
    pub fn with_surface<R>(
        &mut self,
        f: impl FnOnce(&mut GraphemeSurface, &mut rndr_core::World) -> R,
    ) -> Result<R, GraphemeUiError> {
        with_driven_surface(self.app.world_mut(), f)
    }

    fn with_surface_try<T>(
        &mut self,
        f: impl FnOnce(&mut GraphemeSurface, &mut rndr_core::World) -> Result<T, GraphemeUiError>,
    ) -> Result<T, GraphemeUiError> {
        self.with_surface(f).and_then(std::convert::identity)
    }

    /// Invoke an action by id (`expose:Increment`) or by key suffix
    /// (`/increment`), then settle.
    pub fn invoke(&mut self, action: &str) -> Result<(), GraphemeUiError> {
        self.with_surface_try(|surface, world| {
            let id = resolve_action(surface, action)?;
            surface.invoke_action(world, &id)?;
            Ok(())
        })?;
        self.settle()
    }

    /// Press and release the left button at the center of the node whose key
    /// matches `key_suffix`, then settle.
    pub fn click(&mut self, key_suffix: &str) -> Result<(), GraphemeUiError> {
        let entity = self.entity(key_suffix)?;
        self.click_entity(entity)
    }

    /// Click the button whose View IR `label` prop equals `label`, then settle.
    pub fn click_labeled(&mut self, label: &str) -> Result<(), GraphemeUiError> {
        let key = self.button_key(label)?;
        self.click(&key)
    }

    /// Press and release at a point in viewport space, then settle.
    pub fn click_at(&mut self, at: [f32; 2]) -> Result<(), GraphemeUiError> {
        self.pointer_down(at, true)?;
        self.app.update();
        self.pointer_down(at, false)?;
        self.app.update();
        self.clear_input();
        self.settle()
    }

    /// Deliver `key` and settle.
    ///
    /// [`Key::Character`] and [`Key::Backspace`] use the text-field channel
    /// (`Input::chars` / the legacy key, with an empty key queue). Tab, Enter,
    /// and Space go through the key queue a focused button reads. Space is a
    /// press frame and a release frame.
    pub fn press(&mut self, key: Key) -> Result<(), GraphemeUiError> {
        match key {
            Key::Character(ch) => self.insert_chars(&ch.to_string())?,
            Key::Backspace => self.legacy_key(rndr_core::Key::Backspace)?,
            Key::Space => {
                let event = named_event(NamedKey::Space);
                self.queue_key(event.clone())?;
                self.app.update();
                self.clear_input();
                self.queue_key(event.released())?;
                self.app.update();
                self.clear_input();
            }
            Key::Enter => self.queue_named(NamedKey::Enter)?,
            Key::Tab => self.queue_named(NamedKey::Tab)?,
        }
        self.settle()
    }

    /// Insert `text` into the focused text field, one character per frame.
    ///
    /// The field has to already be focused ([`Harness::click`] it, or use
    /// [`Harness::type_into`]).
    pub fn type_text(&mut self, text: &str) -> Result<(), GraphemeUiError> {
        if !self.focused_is_field() {
            return Err(GraphemeUiError::Usage(
                "type_text needs a focused text field; click it first".into(),
            ));
        }
        for ch in text.chars() {
            self.insert_chars(&ch.to_string())?;
        }
        self.settle()
    }

    /// Click the node matching `key_suffix`, then type `text` into it.
    pub fn type_into(&mut self, key_suffix: &str, text: &str) -> Result<(), GraphemeUiError> {
        let entity = self.entity(key_suffix)?;
        self.click_entity(entity)?;
        self.app.world_mut().insert_resource(Focus {
            entity: Some(entity),
        });
        self.type_text(text)
    }

    /// Settled View IR root.
    pub fn ir(&mut self) -> Result<JsonValue, GraphemeUiError> {
        self.settle()?;
        self.ir_now()
    }

    /// Settled access entries for projected nodes, sorted by key.
    pub fn access(&mut self) -> Result<Vec<AccessEntry>, GraphemeUiError> {
        self.settle()?;
        self.access_now()
    }

    /// Settled layout rects for projected nodes, sorted by key.
    pub fn rects(&mut self) -> Result<Vec<NodeRect>, GraphemeUiError> {
        self.settle()?;
        self.rects_now()
    }

    /// Settled View IR, access tree, and rects.
    pub fn snapshot(&mut self) -> Result<Snapshot, GraphemeUiError> {
        self.settle()?;
        Ok(Snapshot {
            ir: self.ir_now()?,
            access: self.access_now()?,
            rects: self.rects_now()?,
        })
    }

    /// Panic when the settled IR at `pointer` is not `expected`.
    ///
    /// `pointer` is a JSON pointer (`/children/0/props/content`).
    pub fn assert_ir_eq(&mut self, pointer: &str, expected: &JsonValue) {
        let ir = self.ir().unwrap_or_else(|err| panic!("view IR: {err}"));
        let actual = ir.pointer(pointer).unwrap_or_else(|| {
            panic!("view IR has no {pointer}: {ir}");
        });
        assert_eq!(actual, expected, "view IR {pointer}");
    }

    /// [`Harness::assert_ir_eq`] for a string value.
    pub fn assert_ir_str(&mut self, pointer: &str, expected: &str) {
        self.assert_ir_eq(pointer, &JsonValue::String(expected.to_string()));
    }

    /// Panic when the projected node matching `key_suffix` has a different
    /// access role. `role` is `button`, `label`, `text_input`, or `group`.
    pub fn assert_access_role(&mut self, key_suffix: &str, role: &str) {
        let entry = self
            .access_entry(key_suffix)
            .unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(
            entry.access_role.as_deref(),
            Some(role),
            "{} access role (name {:?})",
            entry.key,
            entry.name
        );
    }

    /// Panic when `pred` rejects the settled rect of `key_suffix`.
    pub fn assert_rect(&mut self, key_suffix: &str, pred: impl FnOnce(&NodeRect) -> bool) {
        let rect = self
            .rect_entry(key_suffix)
            .unwrap_or_else(|err| panic!("{err}"));
        assert!(pred(&rect), "{} rect {rect:?}", rect.key);
    }

    /// Compare the settled View IR to `path`, or write it when
    /// `UPDATE_GOLDENS=1`.
    ///
    /// Object keys are sorted so the file does not depend on map order.
    /// The comparison is exact: IR goldens run on every operating system.
    pub fn assert_ir_golden(&mut self, path: impl AsRef<Path>) -> Result<(), GraphemeUiError> {
        let text = canonical_ir(&self.ir()?);
        let path = path.as_ref();
        if update_goldens() {
            write_golden(path, text.as_bytes())?;
            return Ok(());
        }
        let existing = read_golden(path)?;
        let expected = String::from_utf8(existing)
            .map_err(|_| GraphemeUiError::Usage(format!("{} is not UTF-8", path.display())))?;
        if expected == text {
            return Ok(());
        }
        Err(GraphemeUiError::Usage(text_mismatch(
            path, &expected, &text,
        )))
    }

    /// Adapter name from the offscreen exporter, once a device exists.
    #[cfg(feature = "capture")]
    pub fn adapter_name(&mut self) -> Result<String, GraphemeUiError> {
        let exporter = self.exporter_mut()?;
        Ok(exporter.adapter_name().to_string())
    }

    /// Capture the viewport to a PNG via `rndr_render::capture`.
    ///
    /// The harness attests [`rndr_render::observe::ObservationContent::Public`]
    /// for this synthetic world before the capture. The image is the parent's
    /// visible box, which is the viewport, so content that moves still lines
    /// up with the previous golden.
    #[cfg(feature = "capture")]
    pub fn capture_png(&mut self) -> Result<Vec<u8>, GraphemeUiError> {
        self.settle()?;
        self.app
            .world_mut()
            .insert_resource(rndr_render::observe::ObservationContent::Public);
        let target = rndr_render::capture::Target::Entity(self.parent);
        let mut last = None;
        for _ in 0..3 {
            let mut exporter = self.take_exporter()?;
            let captured = rndr_render::capture::capture_isolated(
                &mut self.app,
                &mut exporter,
                target,
                |_, _| None,
            );
            self.exporter = Some(exporter);
            match captured {
                Ok(captured) => {
                    if !has_coverage(&captured.image.rgba) {
                        return Err(GraphemeUiError::Usage(
                            "offscreen capture is fully transparent".into(),
                        ));
                    }
                    return rndr_render::encode_png(&captured.image).map_err(|err| {
                        GraphemeUiError::Usage(format!("png encode failed: {err}"))
                    });
                }
                Err(rndr_render::capture::Error::Changed) => {
                    last = Some("post_paint_mutation".to_string());
                }
                Err(err) => {
                    return Err(GraphemeUiError::Usage(format!(
                        "offscreen capture failed: {err}"
                    )));
                }
            }
        }
        Err(GraphemeUiError::Usage(format!(
            "offscreen capture did not settle ({})",
            last.unwrap_or_else(|| "unknown".into())
        )))
    }

    /// Compare a viewport PNG to `path`, or write it when `UPDATE_GOLDENS=1`.
    ///
    /// Channels may differ by [`pixel_tolerance`]. Dimensions must match.
    /// Updating refuses a hardware adapter unless `GOLDEN_ALLOW_ANY_ADAPTER=1`,
    /// so a local GPU cannot replace the lavapipe golden. `GOLDEN_REQUIRE_SOFTWARE=1`
    /// also rejects a compare that did not run on lavapipe or llvmpipe.
    #[cfg(feature = "capture")]
    pub fn assert_png_golden(&mut self, path: impl AsRef<Path>) -> Result<(), GraphemeUiError> {
        let name = self.adapter_name()?;
        if env_flag("GOLDEN_REQUIRE_SOFTWARE") && !is_software_adapter(&name) {
            return Err(GraphemeUiError::Usage(format!(
                "PNG goldens need lavapipe or llvmpipe, adapter is {name}"
            )));
        }
        if update_goldens() && !is_software_adapter(&name) && !env_flag("GOLDEN_ALLOW_ANY_ADAPTER")
        {
            return Err(GraphemeUiError::Usage(format!(
                "refusing to update PNG goldens on adapter {name}; use lavapipe or llvmpipe, or set GOLDEN_ALLOW_ANY_ADAPTER=1"
            )));
        }
        let png = self.capture_png()?;
        let path = path.as_ref();
        if update_goldens() {
            write_golden(path, &png)?;
            return Ok(());
        }
        let expected = read_golden(path)?;
        let (ew, eh, e_rgba) = decode_rgba(&expected)?;
        let (aw, ah, a_rgba) = decode_rgba(&png)?;
        if ew != aw || eh != ah {
            return Err(GraphemeUiError::Usage(format!(
                "{} is {ew}x{eh}, capture is {aw}x{ah}",
                path.display()
            )));
        }
        let tolerance = pixel_tolerance();
        rgba_within(&e_rgba, &a_rgba, ew, eh, tolerance)
            .map_err(|message| GraphemeUiError::Usage(format!("{}: {message}", path.display())))
    }

    fn ir_now(&mut self) -> Result<JsonValue, GraphemeUiError> {
        self.with_surface_try(|surface, _| {
            surface
                .ir_root()
                .cloned()
                .ok_or_else(|| GraphemeUiError::Usage("surface has no view IR".into()))
        })
    }

    fn access_now(&mut self) -> Result<Vec<AccessEntry>, GraphemeUiError> {
        self.with_surface_try(|surface, world| {
            let tree = access_tree(world);
            let mut entries = Vec::new();
            for node in surface.projected_nodes(world) {
                let id = access_id(node.entity);
                let access = tree.nodes.iter().find(|entry| entry.id == id);
                entries.push(AccessEntry {
                    key: node.key,
                    view_role: node.role,
                    access_role: access.map(|entry| access_role_name(entry.role).to_string()),
                    name: access.map(|entry| entry.name.clone()).unwrap_or_default(),
                    value: access.map(|entry| entry.value.clone()).unwrap_or_default(),
                    focusable: access.is_some_and(|entry| entry.focusable),
                });
            }
            entries.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(entries)
        })
    }

    fn rects_now(&mut self) -> Result<Vec<NodeRect>, GraphemeUiError> {
        self.with_surface_try(|surface, world| {
            let mut rects = Vec::new();
            for node in surface.projected_nodes(world) {
                let rect = world.get::<Rect>(node.entity).copied().unwrap_or(Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                });
                rects.push(NodeRect {
                    key: node.key,
                    x: rect.x,
                    y: rect.y,
                    w: rect.w,
                    h: rect.h,
                });
            }
            rects.sort_by(|a, b| a.key.cmp(&b.key));
            Ok(rects)
        })
    }

    fn entity(&mut self, key_suffix: &str) -> Result<Entity, GraphemeUiError> {
        self.with_surface_try(|surface, world| {
            let matches: Vec<_> = surface
                .projected_nodes(world)
                .into_iter()
                .filter(|node| key_matches(&node.key, key_suffix))
                .map(|node| (node.key, node.entity))
                .collect();
            match matches.len() {
                1 => Ok(matches[0].1),
                0 => Err(GraphemeUiError::Usage(format!(
                    "no projected node matching {key_suffix}"
                ))),
                _ => Err(GraphemeUiError::Usage(format!(
                    "projected node {key_suffix} is ambiguous: {}",
                    matches
                        .iter()
                        .map(|(key, _)| key.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))),
            }
        })
    }

    fn button_key(&mut self, label: &str) -> Result<String, GraphemeUiError> {
        let ir = self.ir_now()?;
        let mut found = Vec::new();
        collect_buttons(&ir, &mut found);
        let matches: Vec<_> = found
            .iter()
            .filter(|(_, button)| button == label)
            .map(|(key, _)| key.clone())
            .collect();
        match matches.len() {
            1 => Ok(matches[0].clone()),
            0 => Err(GraphemeUiError::Usage(format!(
                "no button labeled {label:?}; buttons: {found:?}"
            ))),
            _ => Err(GraphemeUiError::Usage(format!(
                "button labeled {label:?} is ambiguous: {}",
                matches.join(", ")
            ))),
        }
    }

    fn click_entity(&mut self, entity: Entity) -> Result<(), GraphemeUiError> {
        let rect = self.rect_of(entity)?;
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return Err(GraphemeUiError::Usage(format!(
                "node has an empty rect {rect:?}"
            )));
        }
        let at = [rect.x + rect.w * 0.5, rect.y + rect.h * 0.5];
        self.click_at(at)
    }

    fn rect_of(&self, entity: Entity) -> Result<Rect, GraphemeUiError> {
        self.app
            .world()
            .get::<Rect>(entity)
            .copied()
            .ok_or_else(|| GraphemeUiError::Usage(format!("entity {entity:?} has no rect")))
    }

    fn access_entry(&mut self, key_suffix: &str) -> Result<AccessEntry, GraphemeUiError> {
        let entries = self.access()?;
        let matches: Vec<_> = entries
            .into_iter()
            .filter(|entry| key_matches(&entry.key, key_suffix))
            .collect();
        match matches.len() {
            1 => Ok(matches.into_iter().next().unwrap()),
            0 => Err(GraphemeUiError::Usage(format!(
                "no access entry matching {key_suffix}"
            ))),
            _ => Err(GraphemeUiError::Usage(format!(
                "access entry {key_suffix} is ambiguous"
            ))),
        }
    }

    fn rect_entry(&mut self, key_suffix: &str) -> Result<NodeRect, GraphemeUiError> {
        let rects = self.rects()?;
        let matches: Vec<_> = rects
            .into_iter()
            .filter(|rect| key_matches(&rect.key, key_suffix))
            .collect();
        match matches.len() {
            1 => Ok(matches.into_iter().next().unwrap()),
            0 => Err(GraphemeUiError::Usage(format!(
                "no rect matching {key_suffix}"
            ))),
            _ => Err(GraphemeUiError::Usage(format!(
                "rect {key_suffix} is ambiguous"
            ))),
        }
    }

    fn pointer_down(&mut self, at: [f32; 2], pressed: bool) -> Result<(), GraphemeUiError> {
        let pointer = &mut self
            .app
            .world_mut()
            .resource_mut::<Input>()
            .ok_or_else(|| GraphemeUiError::Usage("world has no Input resource".into()))?
            .pointer;
        pointer.position = at;
        pointer.button = PointerButton::Left;
        if pressed {
            pointer.press_at = Some(at);
            pointer.pressed = true;
            pointer.down = true;
            pointer.released = false;
        } else {
            pointer.pressed = false;
            pointer.down = false;
            pointer.released = true;
        }
        Ok(())
    }

    fn queue_named(&mut self, key: NamedKey) -> Result<(), GraphemeUiError> {
        self.queue_key(named_event(key))?;
        self.app.update();
        self.clear_input();
        Ok(())
    }

    fn queue_key(&mut self, event: KeyEvent) -> Result<(), GraphemeUiError> {
        let platform = platform_of(self.app.world());
        let input = self.input_mut()?;
        input.keys.clear();
        input.key = None;
        input.chars.clear();
        input.push_key(event, platform);
        Ok(())
    }

    fn insert_chars(&mut self, text: &str) -> Result<(), GraphemeUiError> {
        {
            let input = self.input_mut()?;
            input.keys.clear();
            input.key = None;
            input.chars = text.to_string();
        }
        self.app.update();
        self.clear_input();
        Ok(())
    }

    fn legacy_key(&mut self, key: rndr_core::Key) -> Result<(), GraphemeUiError> {
        {
            let input = self.input_mut()?;
            input.keys.clear();
            input.chars.clear();
            input.key = Some(key);
        }
        self.app.update();
        self.clear_input();
        Ok(())
    }

    fn focused_is_field(&self) -> bool {
        self.app
            .world()
            .resource::<Focus>()
            .and_then(|focus| focus.entity)
            .is_some_and(|entity| self.app.world().has::<TextField>(entity))
    }

    fn input_mut(&mut self) -> Result<&mut Input, GraphemeUiError> {
        self.app
            .world_mut()
            .resource_mut::<Input>()
            .ok_or_else(|| GraphemeUiError::Usage("world has no Input resource".into()))
    }

    fn clear_input(&mut self) {
        let Some(input) = self.app.world_mut().resource_mut::<Input>() else {
            return;
        };
        input.pointer.pressed = false;
        input.pointer.down = false;
        input.pointer.released = false;
        input.pointer.press_at = None;
        input.keys.clear();
        input.key = None;
        input.chars.clear();
        input.ime_preedit.clear();
        input.ime_commit.clear();
    }

    #[cfg(feature = "capture")]
    fn exporter_mut(&mut self) -> Result<&mut rndr_render::Exporter, GraphemeUiError> {
        if self.exporter.is_none() {
            self.exporter = Some(open_exporter()?);
        }
        Ok(self.exporter.as_mut().expect("exporter was just inserted"))
    }

    #[cfg(feature = "capture")]
    fn take_exporter(&mut self) -> Result<rndr_render::Exporter, GraphemeUiError> {
        if self.exporter.is_none() {
            self.exporter = Some(open_exporter()?);
        }
        Ok(self.exporter.take().expect("exporter was just inserted"))
    }
}

#[cfg(feature = "capture")]
fn open_exporter() -> Result<rndr_render::Exporter, GraphemeUiError> {
    rndr_render::Exporter::new().map_err(|err| {
        GraphemeUiError::Usage(format!(
            "offscreen adapter unavailable ({err}). On Linux install mesa-vulkan-drivers and point VK_DRIVER_FILES at the lavapipe ICD"
        ))
    })
}

fn named_event(key: NamedKey) -> KeyEvent {
    KeyEvent::named(key, Modifiers::new())
}

fn resolve_action(surface: &GraphemeSurface, action: &str) -> Result<String, GraphemeUiError> {
    let actions = surface.actions();
    if let Some(found) = actions.iter().find(|entry| entry.id == action) {
        return Ok(found.id.clone());
    }
    let matches: Vec<_> = actions
        .iter()
        .filter(|entry| key_matches(&entry.key, action))
        .collect();
    match matches.len() {
        1 => Ok(matches[0].id.clone()),
        0 => Err(GraphemeUiError::Usage(format!(
            "no action matching {action}"
        ))),
        _ => Err(GraphemeUiError::Usage(format!(
            "action {action} is ambiguous: {}",
            matches
                .iter()
                .map(|entry| entry.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn key_matches(key: &str, suffix: &str) -> bool {
    if key == suffix || (suffix.starts_with('/') && key.ends_with(suffix)) {
        return true;
    }
    let Some(rest) = key.strip_suffix(suffix) else {
        return false;
    };
    rest.is_empty() || rest.ends_with('/')
}

fn collect_buttons(node: &JsonValue, out: &mut Vec<(String, String)>) {
    let role = node.get("role").and_then(JsonValue::as_str);
    let label = node
        .pointer("/props/label")
        .and_then(JsonValue::as_str)
        .unwrap_or("");
    if role == Some("button")
        && !label.is_empty()
        && let Some(key) = node.get("key").and_then(JsonValue::as_str)
    {
        out.push((key.to_string(), label.to_string()));
    }
    if let Some(children) = node.get("children").and_then(JsonValue::as_array) {
        for child in children {
            collect_buttons(child, out);
        }
    }
}

fn access_role_name(role: AccessRole) -> &'static str {
    match role {
        AccessRole::Window => "window",
        AccessRole::Group => "group",
        AccessRole::Button => "button",
        AccessRole::Label => "label",
        AccessRole::TextInput => "text_input",
        AccessRole::List => "list",
        AccessRole::ListItem => "list_item",
        AccessRole::TabList => "tab_list",
        AccessRole::Tab => "tab",
        AccessRole::Disclosure => "disclosure",
        AccessRole::Tree => "tree",
        AccessRole::TreeItem => "tree_item",
        AccessRole::Table => "table",
        AccessRole::Row => "row",
        AccessRole::ColumnHeader => "column_header",
        AccessRole::Cell => "cell",
        AccessRole::CheckBox => "check_box",
        AccessRole::ComboBox => "combo_box",
        AccessRole::ListBox => "list_box",
        AccessRole::Option => "option",
        AccessRole::ScrollBar => "scroll_bar",
        AccessRole::Code => "code",
        AccessRole::Tooltip => "tooltip",
        AccessRole::Image => "image",
    }
}

fn sort_json(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(map) => {
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            let mut sorted = serde_json::Map::new();
            for key in keys {
                sorted.insert(key.clone(), sort_json(&map[&key]));
            }
            JsonValue::Object(sorted)
        }
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(sort_json).collect()),
        other => other.clone(),
    }
}

fn canonical_ir(value: &JsonValue) -> String {
    let mut text = serde_json::to_string_pretty(&sort_json(value)).expect("view IR is JSON");
    text.push('\n');
    text
}

fn update_goldens() -> bool {
    matches!(
        std::env::var("UPDATE_GOLDENS").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

#[cfg(feature = "capture")]
fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

fn write_golden(path: &Path, bytes: &[u8]) -> Result<(), GraphemeUiError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| GraphemeUiError::Usage(format!("create {}: {err}", parent.display())))?;
    }
    std::fs::write(path, bytes)
        .map_err(|err| GraphemeUiError::Usage(format!("write {}: {err}", path.display())))?;
    eprintln!("updated golden {}", path.display());
    Ok(())
}

fn read_golden(path: &Path) -> Result<Vec<u8>, GraphemeUiError> {
    std::fs::read(path).map_err(|err| {
        GraphemeUiError::Usage(format!(
            "missing golden {} ({err}). Run with UPDATE_GOLDENS=1 to write it.",
            path.display()
        ))
    })
}

fn text_mismatch(path: &Path, expected: &str, actual: &str) -> String {
    for (index, (left, right)) in expected.lines().zip(actual.lines()).enumerate() {
        if left != right {
            return format!(
                "golden mismatch {}\nline {}: expected {left}\nline {}: actual   {right}",
                path.display(),
                index + 1,
                index + 1
            );
        }
    }
    format!(
        "golden mismatch {}: {} bytes vs {} bytes",
        path.display(),
        expected.len(),
        actual.len()
    )
}

/// Tolerance for one PNG compare. `GOLDEN_PIXEL_TOLERANCE` overrides
/// [`PNG_CHANNEL_TOLERANCE`] when it parses as a `u8`.
pub fn pixel_tolerance() -> u8 {
    std::env::var("GOLDEN_PIXEL_TOLERANCE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(PNG_CHANNEL_TOLERANCE)
}

#[cfg_attr(not(any(test, feature = "capture")), allow(dead_code))]
pub(crate) fn rgba_within(
    expected: &[u8],
    actual: &[u8],
    width: u32,
    height: u32,
    tolerance: u8,
) -> Result<(), String> {
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| format!("png size {width}x{height} overflows"))?;
    if expected.len() != pixels || actual.len() != pixels {
        return Err(format!(
            "rgba length {} and {} , expected {pixels} for {width}x{height}",
            expected.len(),
            actual.len()
        ));
    }
    let mut over = 0usize;
    let mut max_delta = 0u8;
    let mut first = None;
    for (index, (left, right)) in expected.iter().zip(actual.iter()).enumerate() {
        let delta = left.abs_diff(*right);
        if delta > max_delta {
            max_delta = delta;
        }
        if delta > tolerance {
            over += 1;
            if first.is_none() {
                let pixel = index / 4;
                let channel = index % 4;
                first = Some((
                    pixel as u32 % width,
                    pixel as u32 / width,
                    channel,
                    *left,
                    *right,
                ));
            }
        }
    }
    if over == 0 {
        return Ok(());
    }
    Err(format!(
        "{over} channels exceed tolerance {tolerance} (max delta {max_delta}, first {first:?})"
    ))
}

#[cfg(feature = "capture")]
fn is_software_adapter(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("llvmpipe") || name.contains("lavapipe")
}

#[cfg(feature = "capture")]
fn has_coverage(rgba: &[u8]) -> bool {
    rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0)
}

#[cfg(feature = "capture")]
fn decode_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), GraphemeUiError> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|err| GraphemeUiError::Usage(format!("png decode failed: {err}")))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| GraphemeUiError::Usage("png is too large to decode".into()))?;
    let mut buffer = vec![0; size];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|err| GraphemeUiError::Usage(format!("png frame failed: {err}")))?;
    buffer.truncate(info.buffer_size());
    if info.bit_depth != png::BitDepth::Eight {
        return Err(GraphemeUiError::Usage(format!(
            "png bit depth {:?}",
            info.bit_depth
        )));
    }
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        other => {
            return Err(GraphemeUiError::Usage(format!(
                "png color type {other:?} is not RGBA"
            )));
        }
    };
    let row = (info.width as usize)
        .checked_mul(channels)
        .ok_or_else(|| GraphemeUiError::Usage("png row overflows".into()))?;
    let mut packed = Vec::with_capacity((info.width as usize) * (info.height as usize) * channels);
    if info.line_size == row {
        packed.extend_from_slice(&buffer[..info.height as usize * row]);
    } else {
        for y in 0..info.height as usize {
            let start = y * info.line_size;
            packed.extend_from_slice(&buffer[start..start + row]);
        }
    }
    let rgba = if channels == 4 {
        packed
    } else {
        let mut expanded = Vec::with_capacity(packed.len() / 3 * 4);
        for chunk in packed.as_chunks::<3>().0 {
            expanded.extend_from_slice(chunk);
            expanded.push(255);
        }
        expanded
    };
    Ok((info.width, info.height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_ir_sorts_object_keys() {
        let value = json!({"b": 1, "a": {"d": true, "c": [2, {"z": 1, "y": 0}]}});
        let text = canonical_ir(&value);
        assert!(text.find("\"a\"").unwrap() < text.find("\"b\"").unwrap());
        assert!(text.find("\"c\"").unwrap() < text.find("\"d\"").unwrap());
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn pixel_tolerance_rejects_a_moved_edge() {
        let expected = [0, 0, 0, 255, 255, 255, 255, 255];
        let actual = [255, 255, 255, 255, 0, 0, 0, 255];
        let err = rgba_within(&expected, &actual, 2, 1, 16).unwrap_err();
        assert!(err.contains("exceed tolerance"), "{err}");
    }

    #[test]
    fn pixel_tolerance_allows_antialiasing() {
        let expected = [10, 20, 30, 255];
        let actual = [18, 12, 40, 250];
        rgba_within(&expected, &actual, 1, 1, 16).unwrap();
    }

    #[test]
    fn key_suffix_respects_path_boundaries() {
        assert!(key_matches("CounterView/actions/increment", "increment"));
        assert!(key_matches("CounterView/actions/increment", "/increment"));
        assert!(!key_matches("CounterView/actions/increment", "actions"));
        assert!(key_matches("CounterView/actions", "actions"));
        assert!(!key_matches("CounterView", "View"));
    }
}
