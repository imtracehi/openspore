//! Controller button glyphs for the prompts, drawn with UI shapes (no
//! image files): the face buttons as coloured discs with their letter, the
//! bumpers and triggers as labelled caps, the sticks as rings, the D-pad as a
//! cross and View / Menu as their icons.

use bevy::prelude::*;

use super::Pad;

/// What one glyph shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Button(Pad),
    /// A stick being moved ("L" / "R" ring).
    LeftStick,
    RightStick,
}

impl From<Pad> for Glyph {
    fn from(pad: Pad) -> Self {
        Glyph::Button(pad)
    }
}

const WHITE: Color = Color::srgb(0.97, 0.97, 0.97);
const CAP: Color = Color::srgba(0.12, 0.13, 0.15, 0.95);
const CAP_EDGE: Color = Color::srgba(0.78, 0.80, 0.84, 0.9);

/// The face-button colours of an Xbox pad.
pub fn face_colour(pad: Pad) -> Option<Color> {
    Some(match pad {
        Pad::A => Color::srgb(0.38, 0.72, 0.20),
        Pad::B => Color::srgb(0.86, 0.22, 0.20),
        Pad::X => Color::srgb(0.13, 0.47, 0.88),
        Pad::Y => Color::srgb(0.95, 0.73, 0.10),
        _ => return None,
    })
}

/// Spawns `glyph` (`size` pixels high) as a child of `parent`.
pub fn spawn(parent: &mut ChildSpawnerCommands, glyph: Glyph, size: f32) {
    match glyph {
        Glyph::Button(pad) => match pad {
            Pad::A | Pad::B | Pad::X | Pad::Y => {
                let colour = face_colour(pad).unwrap_or(CAP);
                let letter = if pad == Pad::Y { Color::srgb(0.10, 0.08, 0.02) } else { WHITE };
                disc(parent, size, colour, Color::srgba(0.0, 0.0, 0.0, 0.45), |p| {
                    label(p, &format!("{pad:?}"), size * 0.62, letter);
                });
            }
            Pad::LB | Pad::RB => {
                // A bumper: a wide cap, rounder on the outer top corner.
                let (outer_left, outer_right) = if pad == Pad::LB { (size * 0.5, size * 0.18) } else { (size * 0.18, size * 0.5) };
                parent
                    .spawn((
                        Node {
                            width: Val::Px(size * 1.45),
                            height: Val::Px(size * 0.82),
                            border: UiRect::all(Val::Px(1.5)),
                            border_radius: BorderRadius {
                                top_left: Val::Px(outer_left).into(),
                                top_right: Val::Px(outer_right).into(),
                                bottom_left: Val::Px(size * 0.14).into(),
                                bottom_right: Val::Px(size * 0.14).into(),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(CAP),
                        BorderColor::all(CAP_EDGE),
                    ))
                    .with_children(|p| label(p, &format!("{pad:?}"), size * 0.42, WHITE));
            }
            Pad::LT | Pad::RT => {
                // A trigger: a tall cap, round at the top.
                parent
                    .spawn((
                        Node {
                            width: Val::Px(size * 1.12),
                            height: Val::Px(size),
                            border: UiRect::all(Val::Px(1.5)),
                            border_radius: BorderRadius {
                                top_left: Val::Px(size * 0.5).into(),
                                top_right: Val::Px(size * 0.5).into(),
                                bottom_left: Val::Px(size * 0.16).into(),
                                bottom_right: Val::Px(size * 0.16).into(),
                            },
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            padding: UiRect::top(Val::Px(size * 0.08)),
                            ..default()
                        },
                        BackgroundColor(CAP),
                        BorderColor::all(CAP_EDGE),
                    ))
                    .with_children(|p| label(p, &format!("{pad:?}"), size * 0.42, WHITE));
            }
            Pad::LS | Pad::RS => {
                // A stick click: the ring with a pressed-in centre.
                let letter = if pad == Pad::LS { "L" } else { "R" };
                disc(parent, size, CAP, CAP_EDGE, |p| {
                    p.spawn((
                        Node {
                            width: Val::Px(size * 0.6),
                            height: Val::Px(size * 0.6),
                            border: UiRect::all(Val::Px(1.5)),
                            border_radius: BorderRadius::MAX,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.30, 0.32, 0.36, 1.0)),
                        BorderColor::all(CAP_EDGE),
                    ))
                    .with_children(|p| label(p, letter, size * 0.38, WHITE));
                });
            }
            Pad::View => disc(parent, size, CAP, CAP_EDGE, |p| view_icon(p, size)),
            Pad::Menu => disc(parent, size, CAP, CAP_EDGE, |p| menu_icon(p, size)),
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => dpad(parent, size, pad),
        },
        Glyph::LeftStick | Glyph::RightStick => {
            let letter = if glyph == Glyph::LeftStick { "L" } else { "R" };
            // A stick: a double ring (the cap and its gate).
            disc(parent, size, Color::srgba(0.05, 0.05, 0.06, 0.6), CAP_EDGE, |p| {
                p.spawn((
                    Node {
                        width: Val::Px(size * 0.78),
                        height: Val::Px(size * 0.78),
                        border: UiRect::all(Val::Px(1.5)),
                        border_radius: BorderRadius::MAX,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(CAP),
                    BorderColor::all(CAP_EDGE),
                ))
                .with_children(|p| label(p, letter, size * 0.46, WHITE));
            });
        }
    }
}

fn disc(
    parent: &mut ChildSpawnerCommands,
    size: f32,
    fill: Color,
    edge: Color,
    children: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(fill),
            BorderColor::all(edge),
        ))
        .with_children(children);
}

fn label(parent: &mut ChildSpawnerCommands, text: &str, font_size: f32, colour: Color) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: bevy::text::FontSize::Px(font_size),
            ..default()
        },
        TextColor(colour),
        TextLayout::no_wrap(),
    ));
}

/// The D-pad: a dim cross with the used direction's arm filled white (an
/// arrow-like tab that reads at prompt size).
fn dpad(parent: &mut ChildSpawnerCommands, size: f32, lit: Pad) {
    let arm = (size * 0.38).round();
    let third = ((size - arm) * 0.5).round();
    let dim = Color::srgba(0.30, 0.32, 0.36, 0.95);
    let dim_edge = Color::srgba(0.55, 0.58, 0.62, 0.9);
    parent
        .spawn(Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|p| {
            for (left, top, width, height) in [(third, 0.0, arm, size), (0.0, third, size, arm)] {
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(left),
                        top: Val::Px(top),
                        width: Val::Px(width),
                        height: Val::Px(height),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(size * 0.08)),
                        ..default()
                    },
                    BackgroundColor(dim),
                    BorderColor::all(dim_edge),
                ));
            }
            // The used arm, from its tip to the centre.
            let reach = third + arm * 0.5;
            let (left, top, width, height) = match lit {
                Pad::Up => (third, 0.0, arm, reach),
                Pad::Down => (third, size - reach, arm, reach),
                Pad::Left => (0.0, third, reach, arm),
                _ => (size - reach, third, reach, arm),
            };
            p.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(left),
                    top: Val::Px(top),
                    width: Val::Px(width),
                    height: Val::Px(height),
                    border_radius: BorderRadius::all(Val::Px(size * 0.08)),
                    ..default()
                },
                BackgroundColor(WHITE),
            ));
        });
}

/// View: two overlapping windows.
fn view_icon(parent: &mut ChildSpawnerCommands, size: f32) {
    let box_size = size * 0.3;
    parent
        .spawn(Node {
            width: Val::Px(size * 0.46),
            height: Val::Px(size * 0.4),
            ..default()
        })
        .with_children(|p| {
            for (left, top, filled) in [(0.0, 0.0, false), (size * 0.14, size * 0.1, true)] {
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(left),
                        top: Val::Px(top),
                        width: Val::Px(box_size),
                        height: Val::Px(box_size),
                        border: UiRect::all(Val::Px(1.5)),
                        border_radius: BorderRadius::all(Val::Px(1.5)),
                        ..default()
                    },
                    BackgroundColor(if filled { CAP } else { Color::NONE }),
                    BorderColor::all(WHITE),
                ));
            }
        });
}

/// Menu: three lines.
fn menu_icon(parent: &mut ChildSpawnerCommands, size: f32) {
    parent
        .spawn(Node {
            width: Val::Px(size * 0.42),
            height: Val::Px(size * 0.36),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|p| {
            for _ in 0..3 {
                p.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px((size * 0.07).max(1.5)),
                        border_radius: BorderRadius::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(WHITE),
                ));
            }
        });
}
