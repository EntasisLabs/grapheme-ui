//! Map a renderer-free `style` object onto rndr layout and theme tokens.
//!
//! The compiler (RFC-0007 C.5) decides what a bare identifier means and stores
//! the choice: `"fill"`, a length, `{ "$role": "bg" }`, `"center"`. This module
//! is the only place those names become rndr types. It does not parse Grapheme.

use rndr_core::{
    Absolute, Align, App, Color, Entity, Focus, Input, Items, Justify, Label, Layout, MaxSize,
    Padding, Parent, Schedule, Size, TextStyle, World, hit_test, interaction_pressed,
    system_appearance,
};
use rndr_ui::{Appearance, Token, UiTheme};
use serde_json::Value as JsonValue;

use crate::error::GraphemeUiError;

/// OS appearance is ignored while this resource is present.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ForcedAppearance(pub Appearance);

/// Resolved colour, either a theme token or an authored sRGB value.
#[derive(Clone, Copy, Debug)]
pub(crate) enum StyleColor {
    Token(Token),
    Explicit(Color),
}

impl StyleColor {
    fn resolve(self, theme: &UiTheme) -> Color {
        match self {
            StyleColor::Token(token) => theme.color(token),
            StyleColor::Explicit(color) => color,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Edges {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl Edges {
    fn uniform(self) -> Option<f32> {
        (self.top == self.right && self.top == self.bottom && self.top == self.left)
            .then_some(self.top)
    }
}

/// Paint fields an interaction state may replace.
#[derive(Clone, Copy, Debug, Default)]
struct Paint {
    fill: Option<StyleColor>,
    ink: Option<StyleColor>,
    font_size: Option<f32>,
    weight: Option<u16>,
}

impl Paint {
    fn over(self, rest: Paint) -> Paint {
        Paint {
            fill: self.fill.or(rest.fill),
            ink: self.ink.or(rest.ink),
            font_size: self.font_size.or(rest.font_size),
            weight: self.weight.or(rest.weight),
        }
    }

    fn is_empty(self) -> bool {
        self.fill.is_none()
            && self.ink.is_none()
            && self.font_size.is_none()
            && self.weight.is_none()
    }
}

/// One element's style after the compiler has checked it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Style {
    width: Option<Size>,
    height: Option<Size>,
    min_width: Option<f32>,
    min_height: Option<f32>,
    max_width: Option<f32>,
    max_height: Option<f32>,
    padding: Option<Edges>,
    gap: Option<f32>,
    align: Option<Items>,
    justify: Option<Justify>,
    paint: Paint,
    hover: Option<Paint>,
    pressed: Option<Paint>,
    focused: Option<Paint>,
    disabled: Option<Paint>,
}

impl Style {
    fn paints_button(&self) -> bool {
        !self.paint.is_empty()
            || self.hover.is_some()
            || self.pressed.is_some()
            || self.focused.is_some()
            || self.disabled.is_some()
    }
}

/// Fill kept as a role so a theme change can recolor it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ThemedFill(StyleColor);
impl rndr_core::Component for ThemedFill {}

/// Label colour kept as a role so a theme change can recolor it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ThemedInk(StyleColor);
impl rndr_core::Component for ThemedInk {}

/// Button whose face and label follow interaction-state blocks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ButtonStyle {
    face: Entity,
    label: Entity,
    enabled: bool,
    rest: Paint,
    hover: Option<Paint>,
    pressed: Option<Paint>,
    focused: Option<Paint>,
    disabled: Option<Paint>,
}
impl rndr_core::Component for ButtonStyle {}

struct StyleSystemsInstalled;

/// Register theme following and authored-style application. Idempotent.
pub(crate) fn install(app: &mut App) {
    if app.world().has_resource::<StyleSystemsInstalled>() {
        return;
    }
    app.world_mut().insert_resource(StyleSystemsInstalled);
    app.add_system_front(Schedule::Layout, follow_appearance);
    app.add_system(Schedule::Layout, apply_authored_style);
}

/// Read `props.style`. Missing style is the default, which changes nothing.
pub(crate) fn read(node: &JsonValue) -> Result<Style, GraphemeUiError> {
    let Some(value) = node.pointer("/props/style") else {
        return Ok(Style::default());
    };
    if value.is_null() {
        return Ok(Style::default());
    }
    parse_style(value)
}

/// Patch `entity`'s layout with the box fields of `style`.
///
/// Fields the style omits stay as the widget set them on this sync.
pub(crate) fn sync_box(world: &mut World, entity: Entity, style: &Style) {
    if let Some(layout) = world.get_mut::<Layout>(entity) {
        if let Some(gap) = style.gap {
            layout.gap = gap;
        }
        if let Some(width) = style.width {
            layout.width = width;
        }
        if let Some(height) = style.height {
            layout.height = height;
        }
        if let Some(min_width) = style.min_width {
            layout.min_width = min_width;
        }
        if let Some(min_height) = style.min_height {
            layout.min_height = min_height;
        }
        if let Some(padding) = style.padding
            && padding.uniform().is_some()
        {
            layout.padding = padding.top;
        } else if style.padding.is_some() {
            layout.padding = 0.0;
        }
    }
    match style.padding {
        Some(padding) if padding.uniform().is_none() => {
            world.insert(
                entity,
                Padding {
                    top: padding.top,
                    right: padding.right,
                    bottom: padding.bottom,
                    left: padding.left,
                },
            );
        }
        Some(_) | None => {
            if world.has::<Padding>(entity) {
                world.remove::<Padding>(entity);
            }
        }
    }
    match (style.justify, style.align) {
        (None, None) => {
            if world.has::<Align>(entity) {
                world.remove::<Align>(entity);
            }
        }
        (justify, items) => {
            world.insert(
                entity,
                Align {
                    justify: justify.unwrap_or_default(),
                    items: items.unwrap_or_default(),
                },
            );
        }
    }
    match (style.max_width, style.max_height) {
        (None, None) => {
            if world.has::<MaxSize>(entity) {
                world.remove::<MaxSize>(entity);
            }
        }
        (width, height) => {
            world.insert(
                entity,
                MaxSize {
                    width: width.unwrap_or(f32::INFINITY),
                    height: height.unwrap_or(f32::INFINITY),
                },
            );
        }
    }
}

/// Remember fill and text colour so a later theme change can recolor them.
pub(crate) fn sync_paint(world: &mut World, entity: Entity, style: &Style) {
    remember(world, entity, style.paint.fill, true);
    remember(world, entity, style.paint.ink, false);
    if style.paint.font_size.is_some() || style.paint.weight.is_some() {
        let mut text = world.get::<TextStyle>(entity).cloned().unwrap_or_default();
        if let Some(size) = style.paint.font_size {
            text.size = size;
        }
        if let Some(weight) = style.paint.weight {
            text.weight = weight;
        }
        world.insert(entity, text);
    }
}

fn remember(world: &mut World, entity: Entity, color: Option<StyleColor>, fill: bool) {
    let Some(color) = color else {
        if fill {
            if world.has::<ThemedFill>(entity) {
                world.remove::<ThemedFill>(entity);
                world.remove::<Color>(entity);
            }
        } else if world.has::<ThemedInk>(entity) {
            world.remove::<ThemedInk>(entity);
            if let Some(label) = world.get_mut::<Label>(entity) {
                label.color = None;
            }
        }
        return;
    };
    let theme = world.resource::<UiTheme>().cloned().unwrap_or_default();
    let resolved = color.resolve(&theme);
    if fill {
        world.insert(entity, ThemedFill(color));
        world.insert(entity, resolved);
    } else {
        world.insert(entity, ThemedInk(color));
        if let Some(label) = world.get_mut::<Label>(entity) {
            label.color = Some(resolved);
        }
    }
}

/// Record interaction paints for a button. `face` is the control's filled child.
pub(crate) fn sync_button(
    world: &mut World,
    root: Entity,
    face: Entity,
    label: Entity,
    enabled: bool,
    style: &Style,
) {
    sync_box(world, root, style);
    if !style.paints_button() {
        if world.has::<ButtonStyle>(root) {
            world.remove::<ButtonStyle>(root);
        }
        return;
    }
    world.insert(
        root,
        ButtonStyle {
            face,
            label,
            enabled,
            rest: style.paint,
            hover: style.hover,
            pressed: style.pressed,
            focused: style.focused,
            disabled: style.disabled,
        },
    );
}

/// Fill currently painted for `entity`: a button's face, or the entity itself.
pub(crate) fn fill_of(world: &World, entity: Entity) -> Option<Color> {
    world
        .get::<ButtonStyle>(entity)
        .and_then(|style| world.get::<Color>(style.face).copied())
        .or_else(|| world.get::<Color>(entity).copied())
}

/// Label colour for `entity`: a button's label, or the entity's own label.
pub(crate) fn ink_of(world: &World, entity: Entity) -> Option<Color> {
    if let Some(style) = world.get::<ButtonStyle>(entity) {
        return world
            .get::<Label>(style.label)
            .and_then(|label| label.color);
    }
    world.get::<Label>(entity).and_then(|label| label.color)
}

/// The button face rndr paints, which is the label's parent.
pub(crate) fn button_face(world: &World, label: Entity) -> Option<Entity> {
    world.get::<Parent>(label).map(|parent| parent.entity)
}

/// Main axis of `parent`, used so a Spacer fills the stack it sits in.
pub(crate) fn parent_axis(world: &World, parent: Entity) -> rndr_core::Axis {
    world
        .get::<Layout>(parent)
        .map(|layout| layout.axis)
        .unwrap_or(rndr_core::Axis::Column)
}

/// Pin every child after the first to the stack's box. The first stays in
/// flow so a hugging stack still has a size.
pub(crate) fn pin_overlay(world: &mut World, children: &[Entity]) {
    if let Some(first) = children.first()
        && world.has::<Absolute>(*first)
    {
        world.remove::<Absolute>(*first);
    }
    for child in children.iter().skip(1) {
        world.insert(*child, Absolute::fill(0.0));
    }
}

fn follow_appearance(world: &mut World) {
    let mut theme = world.resource::<UiTheme>().cloned().unwrap_or_default();
    if let Some(forced) = world.resource::<ForcedAppearance>().copied() {
        theme.appearance = forced.0;
    } else {
        theme = theme.follow_system(&system_appearance(world));
    }
    let changed = world
        .resource::<UiTheme>()
        .is_none_or(|current| current.appearance != theme.appearance);
    if changed {
        world.insert_resource(theme);
    }
}

fn apply_authored_style(world: &mut World) {
    let theme = world.resource::<UiTheme>().cloned().unwrap_or_default();
    for entity in world.entities_with::<ThemedFill>() {
        let Some(ThemedFill(color)) = world.get::<ThemedFill>(entity).copied() else {
            continue;
        };
        world.insert(entity, color.resolve(&theme));
    }
    for entity in world.entities_with::<ThemedInk>() {
        let Some(ThemedInk(color)) = world.get::<ThemedInk>(entity).copied() else {
            continue;
        };
        if let Some(label) = world.get_mut::<Label>(entity) {
            label.color = Some(color.resolve(&theme));
        }
    }
    // After themed fills, so a button state wins over the resting colour.
    apply_buttons(world, &theme);
}

fn ancestor_button(world: &World, mut entity: Entity) -> Option<Entity> {
    loop {
        if world.has::<ButtonStyle>(entity) {
            return Some(entity);
        }
        entity = world.get::<Parent>(entity)?.entity;
    }
}

fn apply_buttons(world: &mut World, theme: &UiTheme) {
    let focused = world.resource::<Focus>().and_then(|focus| focus.entity);
    let pointer = world
        .resource::<Input>()
        .map(|input| input.pointer.position);
    let hovered = pointer
        .and_then(|at| hit_test(world, at[0], at[1]))
        .and_then(|hit| ancestor_button(world, hit));
    let roots = world.entities_with::<ButtonStyle>();
    for root in roots {
        let Some(style) = world.get::<ButtonStyle>(root).copied() else {
            continue;
        };
        let state = if !style.enabled {
            style.disabled
        } else if interaction_pressed(world, root) {
            style.pressed
        } else if hovered == Some(root) {
            style.hover
        } else if focused == Some(root) {
            style.focused
        } else {
            None
        };
        let paint = state.unwrap_or_default().over(style.rest);
        if let Some(fill) = paint.fill {
            world.insert(style.face, fill.resolve(theme));
        }
        if let Some(ink) = paint.ink
            && let Some(label) = world.get_mut::<Label>(style.label)
        {
            label.color = Some(ink.resolve(theme));
        }
        if paint.font_size.is_some() || paint.weight.is_some() {
            let mut text = world
                .get::<TextStyle>(style.label)
                .cloned()
                .unwrap_or_default();
            if let Some(size) = paint.font_size {
                text.size = size;
            }
            if let Some(weight) = paint.weight {
                text.weight = weight;
            }
            world.insert(style.label, text);
        }
    }
}

fn parse_style(value: &JsonValue) -> Result<Style, GraphemeUiError> {
    let obj = value
        .as_object()
        .ok_or_else(|| GraphemeUiError::Unsupported("style must be an object".to_string()))?;
    let mut style = Style::default();
    for (key, value) in obj {
        match key.as_str() {
            "width" => style.width = Some(parse_size("width", value)?),
            "height" => style.height = Some(parse_size("height", value)?),
            "min" => {
                let (width, height) = parse_edges_box("min", value)?;
                style.min_width = width;
                style.min_height = height;
            }
            "max" => {
                let (width, height) = parse_edges_box("max", value)?;
                style.max_width = width;
                style.max_height = height;
            }
            "padding" => style.padding = Some(parse_padding(value)?),
            "gap" => style.gap = Some(parse_length("gap", value)?),
            "align" => style.align = Some(parse_align(value)?),
            "justify" => style.justify = Some(parse_justify(value)?),
            "fill" => style.paint.fill = Some(parse_color("fill", value)?),
            "color" => style.paint.ink = Some(parse_color("color", value)?),
            "size" => style.paint.font_size = Some(parse_length("size", value)?),
            "weight" => style.paint.weight = Some(parse_weight(value)?),
            "hover" => style.hover = Some(parse_state("hover", value)?),
            "pressed" => style.pressed = Some(parse_state("pressed", value)?),
            "focused" => style.focused = Some(parse_state("focused", value)?),
            "disabled" => style.disabled = Some(parse_state("disabled", value)?),
            other => {
                return Err(GraphemeUiError::Unsupported(format!(
                    "style property '{other}' is not projected"
                )));
            }
        }
    }
    Ok(style)
}

fn parse_state(name: &str, value: &JsonValue) -> Result<Paint, GraphemeUiError> {
    let obj = value.as_object().ok_or_else(|| {
        GraphemeUiError::Unsupported(format!("style state '{name}' must be an object"))
    })?;
    let mut paint = Paint::default();
    for (key, value) in obj {
        match key.as_str() {
            "fill" => paint.fill = Some(parse_color("fill", value)?),
            "color" => paint.ink = Some(parse_color("color", value)?),
            "size" => paint.font_size = Some(parse_length("size", value)?),
            "weight" => paint.weight = Some(parse_weight(value)?),
            // Layout inside a state is legal Grapheme. v1 restyles paint only.
            "width" | "height" | "min" | "max" | "padding" | "gap" | "align" | "justify" => {}
            other => {
                return Err(GraphemeUiError::Unsupported(format!(
                    "style state '{name}' has no property '{other}'"
                )));
            }
        }
    }
    if paint.is_empty() {
        return Err(GraphemeUiError::Unsupported(format!(
            "style state '{name}' sets no paint"
        )));
    }
    Ok(paint)
}

fn parse_size(prop: &str, value: &JsonValue) -> Result<Size, GraphemeUiError> {
    match value {
        JsonValue::String(text) if text == "fill" => Ok(Size::Fill),
        JsonValue::String(text) if text == "hug" => Ok(Size::Hug),
        JsonValue::Number(_) => Ok(Size::Px(parse_length(prop, value)?)),
        _ => Err(GraphemeUiError::Unsupported(format!(
            "style property '{prop}' expects fill, hug, or a length, found {value}"
        ))),
    }
}

fn parse_edges_box(
    prop: &str,
    value: &JsonValue,
) -> Result<(Option<f32>, Option<f32>), GraphemeUiError> {
    let obj = value.as_object().ok_or_else(|| {
        GraphemeUiError::Unsupported(format!(
            "style property '{prop}' expects {{ width, height }}"
        ))
    })?;
    let mut width = None;
    let mut height = None;
    for (key, value) in obj {
        match key.as_str() {
            "width" => width = Some(parse_length("width", value)?),
            "height" => height = Some(parse_length("height", value)?),
            other => {
                return Err(GraphemeUiError::Unsupported(format!(
                    "style property '{prop}' has no axis '{other}'"
                )));
            }
        }
    }
    if width.is_none() && height.is_none() {
        return Err(GraphemeUiError::Unsupported(format!(
            "style property '{prop}' needs width, height, or both"
        )));
    }
    Ok((width, height))
}

fn parse_padding(value: &JsonValue) -> Result<Edges, GraphemeUiError> {
    if let Some(length) = value.as_f64() {
        let length = finite("padding", length)?;
        return Ok(Edges {
            top: length,
            right: length,
            bottom: length,
            left: length,
        });
    }
    let obj = value.as_object().ok_or_else(|| {
        GraphemeUiError::Unsupported(
            "style property 'padding' expects a length or edge insets".into(),
        )
    })?;
    let mut edges = Edges {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };
    for (key, value) in obj {
        let length = parse_length(key, value)?;
        match key.as_str() {
            "top" => edges.top = length,
            "right" => edges.right = length,
            "bottom" => edges.bottom = length,
            "left" => edges.left = length,
            other => {
                return Err(GraphemeUiError::Unsupported(format!(
                    "style padding has no edge '{other}'"
                )));
            }
        }
    }
    Ok(edges)
}

fn parse_align(value: &JsonValue) -> Result<Items, GraphemeUiError> {
    let name = value.as_str().unwrap_or("");
    match name {
        "start" => Ok(Items::Start),
        "center" => Ok(Items::Center),
        "end" => Ok(Items::End),
        "stretch" => Ok(Items::Stretch),
        _ => Err(GraphemeUiError::Unsupported(format!(
            "style property 'align' expects start, center, end, or stretch, found {value}"
        ))),
    }
}

fn parse_justify(value: &JsonValue) -> Result<Justify, GraphemeUiError> {
    let name = value.as_str().unwrap_or("");
    match name {
        "start" => Ok(Justify::Start),
        "center" => Ok(Justify::Center),
        "end" => Ok(Justify::End),
        "space_between" => Ok(Justify::SpaceBetween),
        "space_around" => Ok(Justify::SpaceAround),
        "space_evenly" => Ok(Justify::SpaceEvenly),
        _ => Err(GraphemeUiError::Unsupported(format!(
            "style property 'justify' expects start, center, end, space_between, space_around, or space_evenly, found {value}"
        ))),
    }
}

fn parse_color(prop: &str, value: &JsonValue) -> Result<StyleColor, GraphemeUiError> {
    if let Some(role) = value.get("$role").and_then(JsonValue::as_str) {
        return role_color(prop, role);
    }
    if let Some(text) = value.as_str() {
        if let Some(color) = parse_hex(text) {
            return Ok(StyleColor::Explicit(color));
        }
        if let Ok(color) = role_color(prop, text) {
            return Ok(color);
        }
    }
    Err(GraphemeUiError::Unsupported(format!(
        "style property '{prop}' expects a theme role or #rrggbb color, found {value}"
    )))
}

pub(crate) fn theme_channels(role: &str, dark: bool) -> Result<[f32; 4], GraphemeUiError> {
    let theme = UiTheme {
        appearance: if dark {
            Appearance::Dark
        } else {
            Appearance::Light
        },
        ..UiTheme::default()
    };
    let color = role_color("theme", role)?.resolve(&theme);
    Ok([color.r, color.g, color.b, color.a])
}

fn role_color(prop: &str, role: &str) -> Result<StyleColor, GraphemeUiError> {
    let token = match role {
        "bg" => Token::Canvas,
        "text" => Token::Text,
        "text_secondary" => Token::Secondary,
        "accent" => Token::Accent,
        "separator" => Token::SubtleStroke,
        "selection" => Token::Selected,
        "control_off" => Token::Content,
        "hover" => Token::ControlHover,
        "press" | "pressed" => Token::ControlPressed,
        "disabled" => Token::Disabled,
        "transparent" => return Ok(StyleColor::Explicit(Color::rgba(0.0, 0.0, 0.0, 0.0))),
        _ => {
            return Err(GraphemeUiError::Unsupported(format!(
                "style property '{prop}' has no theme role '{role}'"
            )));
        }
    };
    Ok(StyleColor::Token(token))
}

fn parse_hex(text: &str) -> Option<Color> {
    let hex = text.strip_prefix('#')?;
    let byte = |slice: &str| u8::from_str_radix(slice, 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 | 4 => {
            let chars: Vec<char> = hex.chars().collect();
            let channel = |index: usize| {
                let ch = chars.get(index)?;
                let value = ch.to_digit(16)?;
                Some((value * 17) as u8)
            };
            (
                channel(0)?,
                channel(1)?,
                channel(2)?,
                if chars.len() == 4 { channel(3)? } else { 255 },
            )
        }
        6 | 8 => (
            byte(&hex[0..2])?,
            byte(&hex[2..4])?,
            byte(&hex[4..6])?,
            if hex.len() == 8 {
                byte(&hex[6..8])?
            } else {
                255
            },
        ),
        _ => return None,
    };
    Some(Color::rgba(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ))
}

fn parse_weight(value: &JsonValue) -> Result<u16, GraphemeUiError> {
    match value {
        JsonValue::String(text) => match text.as_str() {
            "regular" => Ok(400),
            "medium" => Ok(500),
            "semibold" => Ok(600),
            "bold" => Ok(700),
            _ => Err(GraphemeUiError::Unsupported(format!(
                "style property 'weight' expects regular, medium, semibold, bold, or 100..=900, found {text}"
            ))),
        },
        JsonValue::Number(number) => {
            let int = number.as_u64().ok_or_else(|| {
                GraphemeUiError::Unsupported(format!(
                    "style property 'weight' expects an integer from 100 to 900, found {number}"
                ))
            })?;
            if (100..=900).contains(&int) {
                Ok(int as u16)
            } else {
                Err(GraphemeUiError::Unsupported(format!(
                    "style property 'weight' expects an integer from 100 to 900, found {int}"
                )))
            }
        }
        _ => Err(GraphemeUiError::Unsupported(format!(
            "style property 'weight' expects a font weight, found {value}"
        ))),
    }
}

fn parse_length(prop: &str, value: &JsonValue) -> Result<f32, GraphemeUiError> {
    let number = value.as_f64().ok_or_else(|| {
        GraphemeUiError::Unsupported(format!(
            "style property '{prop}' expects a length in px, found {value}"
        ))
    })?;
    finite(prop, number)
}

fn finite(prop: &str, number: f64) -> Result<f32, GraphemeUiError> {
    if !number.is_finite() || number < 0.0 {
        return Err(GraphemeUiError::Unsupported(format!(
            "style property '{prop}' expects a length >= 0, found {number}"
        )));
    }
    Ok(number as f32)
}
