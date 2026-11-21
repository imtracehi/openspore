//! The button-prompt bar: while the controller is the device in use, the
//! current screen's controls are listed as glyph + action (for example
//! "(A) Select  (B) Back"). It is rebuilt when the screen or its controls
//! change and removed as soon as the mouse or keyboard is used.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::glyphs::{self, Glyph};
use super::{ControllerContext, InputMode, StickUse};

/// One prompt: one or more glyphs and what they do.
#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    pub glyphs: Vec<Glyph>,
    pub label: &'static str,
}

/// Where the bar sits on screen (chosen per screen so it clears the HUD).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Anchor {
    #[default]
    BottomRight,
    /// Bottom right, lifted over a HUD strip along the bottom edge.
    AboveHud,
    TopRight,
}

/// How far `Anchor::AboveHud` lifts the bar, in pixels at 1080 lines.
const HUD_LIFT: f32 = 88.0;

/// Glyph height in pixels at 1080 lines.
const GLYPH: f32 = 26.0;
const LABEL: f32 = 17.0;
/// Most hints in one row of the bar.
const ROW: usize = 9;

#[derive(Component)]
struct PromptBar {
    signature: String,
}

/// The stick cursor, drawn only in scripted captures (the system cursor is
/// not part of an offscreen render).
#[derive(Component)]
struct CaptureCursor;

pub struct PromptsPlugin;

impl Plugin for PromptsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, (update_bar, capture_cursor));
    }
}

/// The hints of a screen: its labelled bindings (bindings sharing a label
/// share a hint, e.g. LT / RT "Zoom"), then its sticks. The extra bindings
/// are listed only with `all` (while View is held).
pub fn hints(context: &ControllerContext, all: bool) -> Vec<Hint> {
    let mut out: Vec<Hint> = Vec::new();
    for binding in &context.bindings {
        let Some(label) = binding.label else { continue };
        if binding.extra && !all {
            continue;
        }
        if let Some(hint) = out.iter_mut().find(|h| h.label == label) {
            if !hint.glyphs.contains(&Glyph::Button(binding.pad)) {
                hint.glyphs.push(Glyph::Button(binding.pad));
            }
        } else {
            out.push(Hint {
                glyphs: vec![Glyph::Button(binding.pad)],
                label,
            });
        }
    }
    for (glyph, usage, label) in [
        (Glyph::LeftStick, context.left, context.left_label),
        (Glyph::RightStick, context.right, context.right_label),
    ] {
        if usage == StickUse::None {
            continue;
        }
        if let Some(label) = label {
            out.push(Hint { glyphs: vec![glyph], label });
        }
    }
    out
}

fn update_bar(
    mut commands: Commands,
    mode: Res<InputMode>,
    pad: Res<super::PadState>,
    context: Res<ControllerContext>,
    bars: Query<(Entity, &PromptBar)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<Entity, With<crate::GameCamera>>,
) {
    let list = hints(&context, pad.pressed(super::Pad::View));
    let wanted = mode.gamepad() && !list.is_empty();
    let scale = windows
        .iter()
        .next()
        .map(|w| (w.height() / 1080.0).clamp(0.6, 2.0))
        .unwrap_or(1.0);
    let signature = format!("{:?}|{:?}|{scale:.2}", context.anchor, list);
    let mut current = false;
    for (entity, bar) in &bars {
        if wanted && bar.signature == signature && !current {
            current = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if !wanted || current {
        return;
    }
    let margin = Val::Px(12.0 * scale);
    // Explicit rows (no flex wrapping): at most ROW hints per row.
    let mut node = Node {
        position_type: PositionType::Absolute,
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::FlexEnd,
        row_gap: Val::Px(6.0 * scale),
        padding: UiRect::axes(Val::Px(12.0 * scale), Val::Px(6.0 * scale)),
        border_radius: BorderRadius::all(Val::Px(16.0 * scale)),
        ..default()
    };
    match context.anchor {
        Anchor::BottomRight => {
            node.bottom = margin;
            node.right = margin;
        }
        Anchor::AboveHud => {
            node.bottom = Val::Px(HUD_LIFT * scale);
            node.right = margin;
        }
        Anchor::TopRight => {
            node.top = margin;
            node.right = margin;
        }
    }
    let mut bar = commands.spawn((
        PromptBar { signature },
        node,
        BackgroundColor(Color::srgba(0.0, 0.02, 0.05, 0.55)),
        GlobalZIndex(i32::MAX - 16),
        Pickable::IGNORE,
    ));
    // On the game camera, like the other HUD roots.
    if let Some(camera) = cameras.iter().next() {
        bar.insert(UiTargetCamera(camera));
    }
    let rows = list.len().div_ceil(ROW);
    let per_row = list.len().div_ceil(rows.max(1));
    bar.with_children(|column| {
        for chunk in list.chunks(per_row.max(1)) {
            column
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::NoWrap,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(16.0 * scale),
                    ..default()
                })
                .with_children(|row| {
                    for hint in chunk {
                        row.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(5.0 * scale),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|item| {
                            for glyph in &hint.glyphs {
                                glyphs::spawn(item, *glyph, GLYPH * scale);
                            }
                            item.spawn((
                                Text::new(hint.label),
                                TextFont {
                                    font_size: bevy::text::FontSize::Px(LABEL * scale),
                                    ..default()
                                },
                                TextColor(Color::srgb(0.96, 0.97, 0.98)),
                                TextShadow::default(),
                                TextLayout::no_wrap(),
                                Node {
                                    margin: UiRect::left(Val::Px(2.0 * scale)),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                            ));
                        });
                    }
                });
        }
    });
}

fn capture_cursor(
    mut commands: Commands,
    script: Option<Res<super::script::ScriptPad>>,
    mode: Res<InputMode>,
    context: Res<ControllerContext>,
    windows: Query<&Window>,
    cameras: Query<Entity, With<crate::GameCamera>>,
    mut markers: Query<(Entity, &mut Node), With<CaptureCursor>>,
) {
    let uses_cursor = context.left == StickUse::Cursor || context.right == StickUse::Cursor;
    let at = windows.iter().find_map(|w| w.cursor_position());
    let (Some(_), true, true, Some(at)) = (script, mode.gamepad(), uses_cursor, at) else {
        for (entity, _) in &markers {
            commands.entity(entity).despawn();
        }
        return;
    };
    const SIZE: f32 = 22.0;
    if let Some((_, mut node)) = markers.iter_mut().next() {
        node.left = Val::Px(at.x - SIZE * 0.5);
        node.top = Val::Px(at.y - SIZE * 0.5);
        return;
    }
    let mut marker = commands.spawn((
        CaptureCursor,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x - SIZE * 0.5),
            top: Val::Px(at.y - SIZE * 0.5),
            width: Val::Px(SIZE),
            height: Val::Px(SIZE),
            border: UiRect::all(Val::Px(3.0)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BorderColor::all(Color::srgb(1.0, 0.95, 0.2)),
        GlobalZIndex(i32::MAX - 8),
        Pickable::IGNORE,
    ));
    if let Some(camera) = cameras.iter().next() {
        marker.insert(UiTargetCamera(camera));
    }
}
