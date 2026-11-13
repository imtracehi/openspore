//! Compact gameplay controls. Native fonts/art are reused when available;
//! widget layout and input mapping are currently a rewrite, not SPUI parity.
use crate::{game::*, stages::GameStage};
use bevy::{prelude::*, ui::Pressed, ui_widgets::Button};

// Source pixel dimensions from the five installed crg-gui-btmPanel PNGs.
// SPUI's standard image tiling repeats the 20 px center instead of tinting or
// stretching the parchment frame to a guessed fixed-width viewport.
const CREATURE_PANEL_LEFT_INDENT: f32 = 38.;
const CREATURE_PANEL_LEFT_DIP: f32 = 89.;
const CREATURE_PANEL_SIDE_TILE: f32 = 41.;
const CREATURE_PANEL_RIGHT_DIP: f32 = 89.;
const CREATURE_PANEL_HEIGHT: f32 = 59.;
const CREATURE_PANEL_MID_HEIGHT: f32 = 46.;
const CREATURE_PANEL_MID_BOTTOM: f32 = 6.;

#[derive(Component)]
pub struct HudRoot;
#[derive(Component)]
pub(crate) struct Meter(u8);
#[derive(Component)]
pub(crate) struct Readout(u8);
#[derive(Component)]
pub(crate) struct HudAction(KeyCode);
#[derive(Component)]
pub struct NativeHudArt(pub crate::dbpf::ResourceKey);
#[derive(Component)]
pub(crate) struct SkillIcon(usize);
#[derive(Component)]
pub(crate) struct NativeMeter(u8);
#[derive(Component)]
pub(crate) struct BrainMarker(u8);
#[derive(Resource, Default)]
pub struct InjectedKeys(Vec<KeyCode>);

impl InjectedKeys {
    /// Remember a key a HUD button pressed so [`release`] lets it go again.
    pub fn push(&mut self, key: KeyCode) {
        self.0.push(key);
    }
}

fn text(value: impl Into<String>, size: f32) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font_size: bevy::text::FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgb(0.93, 0.97, 1.0)),
    )
}
fn panel() -> impl Bundle {
    (
        BackgroundColor(Color::srgba(0.07, 0.15, 0.20, 0.62)),
        BorderColor::all(Color::srgba(0.68, 0.84, 0.88, 0.48)),
    )
}
fn bar_track(parent: &mut ChildSpawnerCommands, id: u8, color: Color) {
    parent
        .spawn((
            Node {
                width: Val::Px(146.),
                height: Val::Px(7.),
                margin: UiRect::bottom(Val::Px(4.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.06, 0.8)),
        ))
        .with_children(|track| {
            track.spawn((
                Node {
                    width: Val::Percent(100.),
                    height: Val::Percent(100.),
                    ..default()
                },
                BackgroundColor(color),
                Meter(id),
            ));
        });
}

fn bar(parent: &mut ChildSpawnerCommands, id: u8, caption: &str, color: Color) {
    parent.spawn((text(caption, 11.), Readout(id)));
    bar_track(parent, id, color);
}

fn stage_bar(
    parent: &mut ChildSpawnerCommands,
    id: u8,
    caption: &str,
    color: Color,
    icon: Option<&crate::ui_art::ImportedUiTexture>,
) {
    parent
        .spawn(Node {
            height: Val::Px(16.),
            column_gap: Val::Px(4.),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|row| {
            if let Some(icon) = icon {
                let size = fitted_image_size(icon.width, icon.height, Vec2::splat(14.));
                row.spawn((
                    ImageNode::new(icon.image.clone()),
                    NativeHudArt(icon.source),
                    Node {
                        width: Val::Px(size.x),
                        height: Val::Px(size.y),
                        ..default()
                    },
                ));
            }
            row.spawn((text(caption, 11.), Readout(id)));
        });
    bar_track(parent, id, color);
}

fn fitted_image_size(width: u32, height: u32, bounds: Vec2) -> Vec2 {
    if width == 0 || height == 0 {
        return Vec2::ZERO;
    }
    let scale = (bounds.x / width as f32)
        .min(bounds.y / height as f32)
        .min(1.);
    Vec2::new(width as f32 * scale, height as f32 * scale)
}

fn native_meter(
    parent: &mut ChildSpawnerCommands,
    id: u8,
    caption: &str,
    texture: &crate::ui_art::ImportedUiTexture,
) -> bool {
    const FRAME_WIDTH: u32 = 84;
    const FRAME_HEIGHT: u32 = 16;
    if texture.width != FRAME_WIDTH * 16 || texture.height != FRAME_HEIGHT {
        return false;
    }
    parent.spawn((text(caption, 12.), Readout(id)));
    parent.spawn((
        ImageNode {
            rect: Some(Rect::new(0., 0., FRAME_WIDTH as f32, FRAME_HEIGHT as f32)),
            ..ImageNode::new(texture.image.clone())
        },
        NativeHudArt(texture.source),
        NativeMeter(id),
        Node {
            width: Val::Px(146.),
            height: Val::Px(28.),
            margin: UiRect::bottom(Val::Px(4.)),
            ..default()
        },
    ));
    true
}

fn native_progress(
    parent: &mut ChildSpawnerCommands,
    id: u8,
    texture: &crate::ui_art::ImportedUiTexture,
    rounding_left: &crate::ui_art::ImportedUiTexture,
    rounding_right: &crate::ui_art::ImportedUiTexture,
) {
    parent
        .spawn(Node {
            position_type: PositionType::Relative,
            width: Val::Px(146.),
            height: Val::Px(30.),
            margin: UiRect::bottom(Val::Px(3.)),
            ..default()
        })
        .with_children(|frame| {
            frame.spawn((
                ImageNode::new(texture.image.clone()),
                NativeHudArt(texture.source),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.),
                    top: Val::Px(0.),
                    width: Val::Percent(100.),
                    height: Val::Percent(100.),
                    ..default()
                },
            ));
            frame
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(8.),
                        right: Val::Px(8.),
                        top: Val::Px(11.),
                        height: Val::Px(7.),
                        border_radius: BorderRadius::all(Val::Px(3.)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.13, 0.15, 0.13, 0.74)),
                ))
                .with_children(|track| {
                    track.spawn((
                        Node {
                            width: Val::Percent(100.),
                            height: Val::Percent(100.),
                            border_radius: BorderRadius::all(Val::Px(3.)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.37, 0.72, 0.91)),
                        Meter(id),
                    ));
                });
            for (texture, left) in [(rounding_left, true), (rounding_right, false)] {
                frame.spawn((
                    ImageNode::new(texture.image.clone()),
                    NativeHudArt(texture.source),
                    Node {
                        position_type: PositionType::Absolute,
                        left: if left { Val::Px(0.) } else { Val::Auto },
                        right: if left { Val::Auto } else { Val::Px(0.) },
                        top: Val::Px(0.),
                        width: Val::Px(9.),
                        height: Val::Px(26.),
                        ..default()
                    },
                ));
            }
        });
}

fn bottom_panel(parent: &mut ChildSpawnerCommands, originals: &crate::Originals) {
    let texture = |name| {
        originals
            .ui_art
            .textures
            .iter()
            .find(|texture| texture.name == name)
    };
    parent
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.),
            right: Val::Px(0.),
            bottom: Val::Px(0.),
            height: Val::Px(CREATURE_PANEL_HEIGHT),
            ..default()
        })
        .with_children(|panel| {
            if let Some(asset) = texture("crg-gui-btmPanel-leftIndent") {
                panel.spawn((
                    ImageNode::new(asset.image.clone()),
                    NativeHudArt(asset.source),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.),
                        bottom: Val::Px(0.),
                        width: Val::Px(asset.width as f32),
                        height: Val::Px(asset.height as f32),
                        ..default()
                    },
                ));
            }
            if let Some(asset) = texture("crg-gui-btmPanel-lftDip") {
                panel.spawn((
                    ImageNode::new(asset.image.clone()),
                    NativeHudArt(asset.source),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(CREATURE_PANEL_LEFT_INDENT),
                        bottom: Val::Px(0.),
                        width: Val::Px(asset.width as f32),
                        height: Val::Px(asset.height as f32),
                        ..default()
                    },
                ));
            }
            if let Some(asset) = texture("crg-gui-btmPanel-mid") {
                panel.spawn((
                    ImageNode {
                        image: asset.image.clone(),
                        image_mode: bevy::ui::widget::NodeImageMode::Tiled {
                            tile_x: true,
                            tile_y: false,
                            stretch_value: 1.0,
                        },
                        ..default()
                    },
                    NativeHudArt(asset.source),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(CREATURE_PANEL_LEFT_INDENT + CREATURE_PANEL_LEFT_DIP),
                        right: Val::Px(CREATURE_PANEL_SIDE_TILE + CREATURE_PANEL_RIGHT_DIP),
                        bottom: Val::Px(CREATURE_PANEL_MID_BOTTOM),
                        height: Val::Px(CREATURE_PANEL_MID_HEIGHT),
                        ..default()
                    },
                ));
            }
            if let Some(asset) = texture("crg-gui-btmPanel-sideTile") {
                panel.spawn((
                    ImageNode::new(asset.image.clone()),
                    NativeHudArt(asset.source),
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(89.),
                        bottom: Val::Px(0.),
                        width: Val::Px(asset.width as f32),
                        height: Val::Px(asset.height as f32),
                        ..default()
                    },
                ));
            }
            if let Some(asset) = texture("crg-gui-btmPanel-rgtDip") {
                panel.spawn((
                    ImageNode::new(asset.image.clone()),
                    NativeHudArt(asset.source),
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(0.),
                        bottom: Val::Px(0.),
                        width: Val::Px(asset.width as f32),
                        height: Val::Px(asset.height as f32),
                        ..default()
                    },
                ));
            }
        });
}

fn native_meter_frame(fraction: f32) -> Rect {
    const FRAME_WIDTH: f32 = 84.;
    const FRAME_HEIGHT: f32 = 16.;
    const FRAME_COUNT: u8 = 16;
    let frame = (fraction.clamp(0., 1.) * f32::from(FRAME_COUNT - 1)).round() as u8;
    let left = f32::from(frame) * FRAME_WIDTH;
    Rect::new(left, 0., left + FRAME_WIDTH, FRAME_HEIGHT)
}
fn action(parent: &mut ChildSpawnerCommands, key: KeyCode, label: &str) {
    parent
        .spawn((
            Button::default(),
            HudAction(key),
            Node {
                border_radius: BorderRadius::all(Val::Px(10.)),
                min_width: Val::Px(54.),
                height: Val::Px(46.),
                padding: UiRect::axes(Val::Px(5.), Val::Px(3.)),
                border: UiRect::all(Val::Px(1.)),
                flex_shrink: 0.,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            panel(),
        ))
        .with_children(|b| {
            b.spawn(text(label, 10.));
        });
}

fn native_labeled_action(
    parent: &mut ChildSpawnerCommands,
    key: KeyCode,
    label: &str,
    texture: &crate::ui_art::ImportedUiTexture,
) {
    // Preserve each registered icon's aspect ratio inside this reflowed HUD
    // button. This layout is an OpenSpore adaptation, not recovered SPUI geometry.
    let icon_size = fitted_image_size(texture.width, texture.height, Vec2::splat(32.));
    parent
        .spawn((
            Button::default(),
            HudAction(key),
            Node {
                width: Val::Px(58.),
                height: Val::Px(62.),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(1.),
                flex_shrink: 0.,
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.13, 0.18, 0.48)),
            BorderColor::all(Color::srgba(0.75, 0.86, 0.88, 0.45)),
        ))
        .with_children(|button| {
            button.spawn((
                ImageNode::new(texture.image.clone()),
                NativeHudArt(texture.source),
                Node {
                    width: Val::Px(icon_size.x),
                    height: Val::Px(icon_size.y),
                    ..default()
                },
            ));
            button.spawn(text(label, 8.));
        });
}

fn native_action(
    parent: &mut ChildSpawnerCommands,
    key: KeyCode,
    label: &str,
    image: &Handle<Image>,
    source: crate::dbpf::ResourceKey,
    slot: Option<usize>,
    background: Option<(&Handle<Image>, crate::dbpf::ResourceKey)>,
) {
    parent
        .spawn((
            Button::default(),
            HudAction(key),
            Node {
                width: Val::Px(52.),
                height: Val::Px(58.),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(1.),
                border_radius: BorderRadius::all(Val::Px(12.)),
                border: UiRect::all(Val::Px(1.)),
                flex_shrink: 0.,
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.13, 0.18, 0.40)),
            BorderColor::all(Color::srgba(0.75, 0.86, 0.88, 0.45)),
        ))
        .with_children(|button| {
            if let Some((background, background_source)) = background {
                button.spawn((
                    ImageNode::new(background.clone()),
                    NativeHudArt(background_source),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.),
                        left: Val::Px(2.),
                        width: Val::Px(49.),
                        height: Val::Px(42.),
                        ..default()
                    },
                ));
            }
            let mut icon = button.spawn((
                ImageNode::new(image.clone()),
                NativeHudArt(source),
                Node {
                    width: Val::Px(36.),
                    height: Val::Px(36.),
                    ..default()
                },
            ));
            if let Some(slot) = slot {
                icon.insert(SkillIcon(slot));
            }
            button.spawn((text(label, 8.),));
        });
}

/// One of Spore's top-left mission cards: a parchment title over a dark body.
#[derive(Component)]
pub(crate) struct GoalCard(u8);

/// Height of the DNA strip along the bottom edge.
const STRIP_HEIGHT: f32 = 30.;

/// Distance from the screen bottom that clears the strip (or, in Creature,
/// the native parchment tray it sits in).
fn strip_clearance(stage: &GameStage) -> f32 {
    if *stage == GameStage::Creature {
        CREATURE_PANEL_HEIGHT + 4.
    } else {
        STRIP_HEIGHT + 6.
    }
}

fn small_action(parent: &mut ChildSpawnerCommands, key: KeyCode, label: &str) {
    parent
        .spawn((
            Button::default(),
            HudAction(key),
            Node {
                height: Val::Px(22.),
                padding: UiRect::axes(Val::Px(8.), Val::Px(2.)),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(11.)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_shrink: 0.,
                ..default()
            },
            panel(),
        ))
        .with_children(|b| {
            b.spawn(text(label, 11.));
        });
}

/// Spore's bottom strip: the glossy stage orb, the DNA counter and the long
/// evolution bar with its growth/brain notches. Layout and colours follow
/// the retail HUD by eye; the native SPUI geometry is not decoded.
fn spore_strip(parent: &mut ChildSpawnerCommands, stage: &GameStage, originals: &crate::Originals) {
    let creature = *stage == GameStage::Creature;
    let native = |name: &str| {
        originals
            .ui_art
            .textures
            .iter()
            .find(|texture| texture.name == name)
    };
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                right: Val::Px(0.),
                bottom: Val::Px(if creature { 14. } else { 0. }),
                height: Val::Px(STRIP_HEIGHT),
                align_items: AlignItems::Center,
                padding: UiRect::new(Val::Px(64.), Val::Px(10.), Val::Px(0.), Val::Px(0.)),
                column_gap: Val::Px(8.),
                border: UiRect::top(Val::Px(if creature { 0. } else { 2. })),
                ..default()
            },
            BackgroundColor(if creature {
                Color::NONE
            } else {
                Color::srgba(0.02, 0.07, 0.19, 0.94)
            }),
            BorderColor::all(Color::srgba(0.42, 0.62, 0.95, 0.75)),
        ))
        .with_children(|strip| {
            strip
                .spawn((
                    Node {
                        min_width: Val::Px(70.),
                        padding: UiRect::axes(Val::Px(8.), Val::Px(2.)),
                        border: UiRect::all(Val::Px(1.)),
                        border_radius: BorderRadius::all(Val::Px(9.)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.14, 0.32, 0.95)),
                    BorderColor::all(Color::srgba(0.5, 0.75, 1.0, 0.7)),
                ))
                .with_children(|b| {
                    b.spawn((text("", 13.), Readout(5)));
                });
            strip
                .spawn((
                    Node {
                        flex_grow: 1.,
                        height: Val::Px(14.),
                        border: UiRect::all(Val::Px(1.)),
                        border_radius: BorderRadius::all(Val::Px(7.)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.05, 0.13, 0.42)),
                    BorderColor::all(Color::srgba(0.55, 0.75, 1.0, 0.85)),
                ))
                .with_children(|track| {
                    track.spawn((
                        Node {
                            width: Val::Percent(0.),
                            height: Val::Percent(100.),
                            border_radius: BorderRadius::all(Val::Px(7.)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.33, 0.86, 0.24)),
                        Meter(2),
                    ));
                    let notches: &[f32] = match stage {
                        GameStage::Creature => &[25., 50., 75., 100.],
                        _ => &[20., 40., 60., 80.],
                    };
                    for (index, at) in notches.iter().enumerate() {
                        let brain = creature
                            .then(|| native(&format!("crg-p-bar-brain-{}", index + 1)))
                            .flatten();
                        let size = if brain.is_some() { 24. } else { 3. };
                        let mut notch = track.spawn(Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(*at),
                            top: Val::Px(if brain.is_some() { -6. } else { -3. }),
                            margin: UiRect::left(Val::Px(-size * 0.5)),
                            width: Val::Px(size),
                            height: Val::Px(if brain.is_some() { 24. } else { 18. }),
                            ..default()
                        });
                        if let Some(texture) = brain {
                            notch.insert((
                                ImageNode::new(texture.image.clone()),
                                NativeHudArt(texture.source),
                                BrainMarker(index as u8 + 1),
                            ));
                        } else {
                            notch.insert(BackgroundColor(Color::srgba(0.78, 0.9, 1.0, 0.9)));
                        }
                    }
                });
            small_action(strip, KeyCode::F5, "Save");
            small_action(strip, KeyCode::Escape, "Menu");
        });
    let orb = match stage {
        GameStage::Cell => native("level-icon-cell"),
        GameStage::Civilization => native("cvg_levelicon"),
        _ => None,
    };
    let mut badge = parent.spawn(Node {
        position_type: PositionType::Absolute,
        left: Val::Px(2.),
        bottom: Val::Px(if creature { 8. } else { -8. }),
        width: Val::Px(58.),
        height: Val::Px(58.),
        border: UiRect::all(Val::Px(2.)),
        border_radius: BorderRadius::all(Val::Percent(50.)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    });
    if let Some(orb) = orb {
        badge.insert((ImageNode::new(orb.image.clone()), NativeHudArt(orb.source)));
    } else {
        badge
            .insert((
                BackgroundColor(Color::srgba(0.10, 0.25, 0.55, 0.95)),
                BorderColor::all(Color::srgba(0.7, 0.85, 1.0, 0.9)),
            ))
            .with_children(|b| {
                b.spawn(text(&stage.title()[..1], 24.));
            });
    }
}

/// Bottom-right status card: Spore shows the player's portrait here with its
/// health (and, on land, hunger) meters.
fn portrait_card(
    parent: &mut ChildSpawnerCommands,
    stage: &GameStage,
    originals: &crate::Originals,
) {
    let native = |name: &str| {
        originals
            .ui_art
            .textures
            .iter()
            .find(|texture| texture.name == name)
    };
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(8.),
                bottom: Val::Px(strip_clearance(stage) + 2.),
                width: Val::Px(176.),
                padding: UiRect::all(Val::Px(8.)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(12.)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.09, 0.22, 0.82)),
            BorderColor::all(Color::srgba(0.45, 0.65, 0.95, 0.6)),
        ))
        .with_children(|p| {
            p.spawn(text(stage.title(), 15.));
            if *stage == GameStage::Creature {
                if !native("crg-ingame_hp-hrz")
                    .is_some_and(|texture| native_meter(p, 0, "Health", texture))
                {
                    bar(p, 0, "Health", Color::srgb(0.34, 0.85, 0.43));
                }
                if !native("crg-ingame_hunger-hrz")
                    .is_some_and(|texture| native_meter(p, 1, "Food", texture))
                {
                    bar(p, 1, "Food", Color::srgb(0.95, 0.60, 0.18));
                }
            } else {
                bar(p, 0, "Health", Color::srgb(0.34, 0.85, 0.43));
                if *stage == GameStage::Space {
                    bar(p, 1, "Energy", Color::srgb(0.95, 0.60, 0.18));
                }
            }
        });
}

/// Up to three mission cards in the top-left corner; empty cards hide.
fn goal_cards(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.),
            top: Val::Px(10.),
            width: Val::Px(270.),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.),
            ..default()
        })
        .with_children(|column| {
            for index in 0..3u8 {
                column
                    .spawn((
                        GoalCard(index),
                        Node {
                            flex_direction: FlexDirection::Column,
                            border: UiRect::all(Val::Px(1.)),
                            border_radius: BorderRadius::all(Val::Px(6.)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BorderColor::all(Color::srgba(0.30, 0.25, 0.15, 0.8)),
                    ))
                    .with_children(|card| {
                        card.spawn((
                            Node {
                                padding: UiRect::axes(Val::Px(8.), Val::Px(2.)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.88, 0.82, 0.62)),
                        ))
                        .with_children(|title| {
                            title.spawn((
                                Text::new(""),
                                TextFont {
                                    font_size: bevy::text::FontSize::Px(12.),
                                    ..default()
                                },
                                TextColor(Color::srgb(0.16, 0.12, 0.07)),
                                Readout(10 + index),
                            ));
                        });
                        card.spawn((
                            Node {
                                padding: UiRect::axes(Val::Px(8.), Val::Px(4.)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.07, 0.07, 0.06, 0.82)),
                        ))
                        .with_children(|body| {
                            body.spawn((text("", 12.), Readout(20 + index)));
                        });
                    });
            }
        });
}

/// Spore's mating call button at the centre of the Cell tray.
fn mating_call(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Button::default(),
            HudAction(KeyCode::F2),
            Node {
                width: Val::Px(64.),
                height: Val::Px(64.),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(2.)),
                border_radius: BorderRadius::all(Val::Percent(50.)),
                flex_shrink: 0.,
                ..default()
            },
            BackgroundColor(Color::srgba(0.55, 0.12, 0.35, 0.85)),
            BorderColor::all(Color::srgba(1.0, 0.7, 0.85, 0.9)),
        ))
        .with_children(|b| {
            b.spawn(text("Call", 12.));
            b.spawn(text("mate F2", 10.));
        });
}

/// Mission card contents per stage: (title, body).
fn goals(stage: &GameStage, run: &RunProgress, goal: f32) -> [(String, String); 3] {
    let empty = || (String::new(), String::new());
    match stage {
        GameStage::Cell => {
            let found = run.cell_found_parts.len();
            let total = crate::cell::DISCOVERABLE_KINDS.len();
            let ready = run.dna_this_stage >= goal;
            [
                (
                    "Eat".into(),
                    format!(
                        "Eat food to grow larger: size {}/5.",
                        crate::cell::growth_tier(run.dna_this_stage) + 1
                    ),
                ),
                (
                    "Collect".into(),
                    if found < total {
                        format!(
                            "Break meteor shards or defeat cells to find new parts ({found}/{total})."
                        )
                    } else {
                        "Every cell part has been found.".into()
                    },
                ),
                (
                    if ready { "Evolve!" } else { "Evolve" }.into(),
                    if ready {
                        "Your cell is ready. Call a mate and grow legs!".into()
                    } else {
                        format!(
                            "Fill the DNA bar ({:.0}/{goal:.0}), then call a mate.",
                            run.dna_this_stage
                        )
                    },
                ),
            ]
        }
        GameStage::Creature => [
            (
                "Befriend or hunt".into(),
                format!(
                    "Impress or hunt other species to earn DNA. Brain {}/4.",
                    run.creature_progression.brain_level
                ),
            ),
            (
                "Evolve".into(),
                format!(
                    "Call a mate at your nest (M) to change your creature. Generation {}.",
                    run.creature_lifecycle.generation
                ),
            ),
            (
                "Progress".into(),
                format!("DNA {:.0}/{goal:.0} to become tribal.", run.dna_this_stage),
            ),
        ],
        GameStage::Tribal => [
            (
                "Unite the villages".into(),
                format!(
                    "Impress or conquer the other tribes ({}/5).",
                    run.tribal_campaign.resolved_count()
                ),
            ),
            (
                "Gather".into(),
                format!("Food stored: {:.0}.", run.dna_this_stage),
            ),
            empty(),
        ],
        GameStage::Civilization => [
            (
                "Unify the planet".into(),
                format!(
                    "Bring every city under your rule ({}/5).",
                    run.civ_campaign.owned_count()
                ),
            ),
            ("Spice".into(), format!("Stockpile: {:.0}.", run.spice)),
            empty(),
        ],
        GameStage::Space => [
            (
                run.space_campaign.mission_name().to_string(),
                run.space_campaign.next_action(),
            ),
            (
                "Empire".into(),
                format!(
                    "{} colonies • Spice {:.0}",
                    run.space_campaign.new_colony_count(),
                    run.spice
                ),
            ),
            empty(),
        ],
        GameStage::MainMenu => [empty(), empty(), empty()],
    }
}

/// Spore's opening choice for a new cell: plant eater or meat eater.
fn diet_choice(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.),
                right: Val::Px(0.),
                top: Val::Px(0.),
                bottom: Val::Px(0.),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.02, 0.08, 0.55)),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        width: Val::Px(560.),
                        padding: UiRect::all(Val::Px(22.)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(14.),
                        border: UiRect::all(Val::Px(2.)),
                        border_radius: BorderRadius::all(Val::Px(18.)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.03, 0.10, 0.26, 0.92)),
                    BorderColor::all(Color::srgba(0.55, 0.78, 1.0, 0.8)),
                ))
                .with_children(|panel| {
                    panel.spawn(text(
                        "A meteor has seeded life in the primordial ocean.",
                        18.,
                    ));
                    panel.spawn(text("What will your first cell eat?", 15.));
                    panel
                        .spawn(Node {
                            column_gap: Val::Px(18.),
                            ..default()
                        })
                        .with_children(|row| {
                            for (key, title, body, color) in [
                                (
                                    KeyCode::Digit1,
                                    "Herbivore  (1)",
                                    "Filter mouth: graze on plant specks",
                                    Color::srgba(0.12, 0.42, 0.18, 0.95),
                                ),
                                (
                                    KeyCode::Digit2,
                                    "Carnivore  (2)",
                                    "Jaw: hunt cells and eat meat",
                                    Color::srgba(0.48, 0.10, 0.12, 0.95),
                                ),
                            ] {
                                row.spawn((
                                    Button::default(),
                                    HudAction(key),
                                    Node {
                                        width: Val::Px(230.),
                                        padding: UiRect::all(Val::Px(14.)),
                                        flex_direction: FlexDirection::Column,
                                        align_items: AlignItems::Center,
                                        row_gap: Val::Px(6.),
                                        border: UiRect::all(Val::Px(2.)),
                                        border_radius: BorderRadius::all(Val::Px(14.)),
                                        ..default()
                                    },
                                    BackgroundColor(color),
                                    BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                                ))
                                .with_children(|b| {
                                    b.spawn(text(title, 17.));
                                    b.spawn(text(body, 12.));
                                });
                            }
                        });
                });
        });
}

pub fn update(
    mut commands: Commands,
    (state, run, editor, options, originals, encounter, space_view, galaxy, mobility): (
        Res<State<GameStage>>,
        Res<RunProgress>,
        Res<crate::editor::EditorState>,
        Res<crate::HudOptions>,
        Res<crate::Originals>,
        Res<crate::creature_actions::Encounter>,
        Option<Res<crate::space_view::SpaceView>>,
        Option<Res<crate::space_galaxy::Galaxy>>,
        Option<Res<crate::creature::mobility::CreatureMobility>>,
    ),
    roots: Query<Entity, With<HudRoot>>,
    camera: Query<Entity, With<crate::GameCamera>>,
    player: Query<(&Health, Option<&Hunger>, &Transform), With<Player>>,
    planets: Query<(&crate::persistence::WorldId, &Transform, &Planet)>,
    actors: Query<(
        &crate::persistence::WorldId,
        &crate::creature_actions::CreatureMind,
    )>,
    mut readouts: Query<(&Readout, &mut Text)>,
    (mut meters, mut cards): (
        Query<(&Meter, &mut Node), Without<GoalCard>>,
        Query<(&GoalCard, &mut Node), Without<Meter>>,
    ),
    (cell_notice, cell_intro, spui_cell, spui_creature, spui_tribal, spui_civ, spui_space, front_end): (
        Option<Res<crate::cell::CellNotice>>,
        Option<Res<crate::cell::CellIntro>>,
        Option<Res<crate::cell_hud::CellHudActive>>,
        Option<Res<crate::creature_hud::CreatureHudActive>>,
        Option<Res<crate::tribal_hud::TribalHudActive>>,
        Option<Res<crate::civ_hud::CivHudActive>>,
        Option<Res<crate::space_hud::SpaceHudActive>>,
        Option<Res<crate::front_end::FrontEndActive>>,
    ),
    mut image_nodes: Query<
        (
            &mut ImageNode,
            Option<&SkillIcon>,
            Option<&NativeMeter>,
            Option<&BrainMarker>,
        ),
        Or<(With<SkillIcon>, With<NativeMeter>, With<BrainMarker>)>,
    >,
    mut last: Local<String>,
) {
    let stage = state.get();
    let hidden = editor.open || options.diagnostics;
    let mode = if encounter.mode == crate::creature_actions::EncounterMode::Combat {
        1
    } else {
        0
    };
    // The diet step runs on the main menu before a new Cell stage.
    let choosing_diet =
        *stage == GameStage::MainMenu && cell_intro.as_deref().is_some_and(|intro| intro.choosing);
    let space_zoom = space_view
        .as_deref()
        .map(crate::space_view::SpaceView::zoom)
        .unwrap_or(crate::space_view::SpaceZoom::System);
    // The retail SPUI Cell HUD replaces the legacy one; only the diet choice
    // overlay (not part of that layout) still comes from here.
    // The retail SPUI Creature HUD (`creature_hud`) replaces it the same way.
    // The retail SPUI Tribal (`tribal_hud`), Civ (`civ_hud`) and Space (`space_hud`) HUDs replace it too.
    let spui_cell = (*stage == GameStage::Cell && spui_cell.as_deref().is_some_and(|a| a.0))
        || (*stage == GameStage::Creature && spui_creature.as_deref().is_some_and(|a| a.0))
        || (*stage == GameStage::Tribal && spui_tribal.as_deref().is_some_and(|a| a.0))
        || (*stage == GameStage::Civilization && spui_civ.as_deref().is_some_and(|a| a.0))
        || (*stage == GameStage::Space && spui_space.as_deref().is_some_and(|a| a.0));
    // The retail galaxy screen (`front_end`) replaces the legacy main menu.
    let gge = *stage == GameStage::MainMenu && front_end.as_deref().is_some_and(|a| a.0);
    let signature = format!("{stage:?}/{hidden}/{mode}/{choosing_diet}/{space_zoom:?}/{spui_cell}/{gge}");
    if *last != signature || (!hidden && roots.is_empty()) {
        for e in &roots {
            commands.entity(e).despawn();
        }
        *last = signature;
        if hidden {
            return;
        }
        let target = camera.single().ok();
        let root = commands
            .spawn((
                HudRoot,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.),
                    height: Val::Percent(100.),
                    ..default()
                },
            ))
            .id();
        if let Some(target) = target {
            commands.entity(root).insert(UiTargetCamera(target));
        }
        commands.entity(root).with_children(|ui| {
            if gge {
                // OpenSpore's Enter shortcut still asks the diet here.
                if choosing_diet {
                    diet_choice(ui);
                }
                return;
            }
            if *stage == GameStage::MainMenu {
                if let Some(logo) = originals
                    .ui_art
                    .textures
                    .iter()
                    .find(|t| t.name == "title-logo")
                {
                    ui.spawn((
                        NativeHudArt(logo.source),
                        ImageNode::new(logo.image.clone()),
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(34.),
                            left: Val::Percent(50.),
                            margin: UiRect::left(Val::Px(-260.)),
                            width: Val::Px(520.),
                            height: Val::Px(520. * logo.height as f32 / logo.width as f32),
                            ..default()
                        },
                    ));
                }
                ui.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(28.),
                        bottom: Val::Px(50.),
                        width: Val::Px(290.),
                        padding: UiRect::all(Val::Px(20.)),
                        row_gap: Val::Px(12.),
                        flex_direction: FlexDirection::Column,
                        border: UiRect::all(Val::Px(1.)),
                        ..default()
                    },
                    panel(),
                ))
                .with_children(|p| {
                    p.spawn(text("A new universe awaits", 20.));
                    action(p, KeyCode::Enter, "New game   Enter");
                    action(p, KeyCode::KeyL, "Continue   L");
                    action(p, KeyCode::F2, "Creature creator   F2");
                    p.spawn(text("OpenSpore", 12.));
                });
                if choosing_diet {
                    diet_choice(ui);
                }
                return;
            }
            if spui_cell {
                if choosing_diet {
                    diet_choice(ui);
                }
                return;
            }
            if *stage == GameStage::Creature {
                // The native SPUI reader treats standard tiling as repeated
                // source pixels. This five-piece Creature tray uses its
                // authored 38/89/20/41/89 px edge geometry and untinted
                // colors; the center strip repeats to the viewport width.
                bottom_panel(ui, &originals);
            }
            spore_strip(ui, stage, &originals);
            portrait_card(ui, stage, &originals);
            goal_cards(ui);
            if choosing_diet {
                diet_choice(ui);
            }
            ui.spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(220.),
                right: Val::Px(200.),
                bottom: Val::Px(strip_clearance(stage) + 4.),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.),
                ..default()
            })
            .with_children(|area| {
                if *stage == GameStage::Creature {
                    area.spawn((text("", 11.), Readout(6)));
                }
                area.spawn((
                    text("", 13.),
                    Readout(4),
                    BackgroundColor(Color::srgba(0.03, 0.09, 0.22, 0.6)),
                    Node {
                        padding: UiRect::axes(Val::Px(8.), Val::Px(3.)),
                        border_radius: BorderRadius::all(Val::Px(8.)),
                        ..default()
                    },
                ));
                area.spawn(Node {
                    column_gap: Val::Px(4.),
                    align_items: AlignItems::FlexEnd,
                    justify_content: JustifyContent::Center,
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|p| match stage {
                    GameStage::Cell => {
                        if crate::editor::cell_abilities(&run, &originals).cell_electric > 0 {
                            action(p, KeyCode::Space, "Electric\nSpace");
                        }
                        mating_call(p);
                    }
                    GameStage::Creature => {
                        for (key, label, asset) in [
                            (
                                // The stance buttons press their own keys;
                                // Tab toggles (E strafes, F targets).
                                crate::creature_controls::KEY_STANCE_SOCIAL,
                                "Social · Tab",
                                if mode == 0 {
                                    "social_tray_sel_norm"
                                } else {
                                    "social_tray_norm"
                                },
                            ),
                            (
                                crate::creature_controls::KEY_STANCE_COMBAT,
                                "Combat · Tab",
                                if mode == 1 {
                                    "atk_tray_sel_norm"
                                } else {
                                    "atk_tray_norm"
                                },
                            ),
                        ] {
                            if let Some(icon) = originals
                                .ui_art
                                .textures
                                .iter()
                                .find(|texture| texture.name == asset)
                            {
                                native_action(p, key, label, &icon.image, icon.source, None, None);
                            } else {
                                action(p, key, label);
                            }
                        }
                        let names = if mode == 1 {
                            crate::creature_actions::COMBAT
                        } else {
                            crate::creature_actions::SOCIAL
                        };
                        for (slot, (key, label)) in [
                            KeyCode::Digit1,
                            KeyCode::Digit2,
                            KeyCode::Digit3,
                            KeyCode::Digit4,
                        ]
                        .into_iter()
                        .zip(names)
                        .enumerate()
                        {
                            if let Some(icon) = originals
                                .ui_art
                                .action_icons
                                .iter()
                                .find(|icon| icon.action.eq_ignore_ascii_case(label))
                            {
                                native_action(
                                    p,
                                    key,
                                    &format!("{} · {}", label, slot + 1),
                                    &icon.image,
                                    icon.source,
                                    Some(slot),
                                    None,
                                );
                            } else {
                                action(p, key, &format!("{}\n{}", label, slot + 1));
                            }
                        }
                        action(p, KeyCode::KeyH, "Rest\nH");
                        action(p, KeyCode::KeyM, "Mate\nM");
                        action(p, KeyCode::Space, "Jump / glide\nSpace");
                        action(p, KeyCode::ShiftLeft, "Sprint\nShift");
                        action(p, KeyCode::KeyZ, "Sneak\nZ");
                    }
                    GameStage::Tribal => {
                        action(p, crate::tribal_controls::KEY_SELECT_TRIBE, "Select tribe\nCtrl+A");
                        action(p, KeyCode::KeyZ, "Gather\nZ");
                        action(p, KeyCode::KeyX, "Fish\nX");
                        action(p, KeyCode::KeyH, "Hunt\nH");
                        action(p, KeyCode::KeyK, "Tame\nK");
                        action(p, KeyCode::KeyJ, "Eggs\nJ");
                        action(p, KeyCode::KeyV, "Rally home\nV");
                        action(p, KeyCode::KeyT, "Next tool\nT");
                        action(p, KeyCode::KeyB, "Build / equip\nB");
                        action(p, crate::tribal_controls::KEY_PERFORM, "Perform\nright-click");
                        action(p, KeyCode::KeyG, "Give food\nG");
                        action(p, KeyCode::KeyC, "Stance\nC");
                        if let Some(icon) = originals
                            .ui_art
                            .textures
                            .iter()
                            .find(|texture| texture.name == "trg_attack_norm")
                        {
                            native_labeled_action(p, KeyCode::KeyF, "Attack · F", icon);
                        } else {
                            action(p, KeyCode::KeyF, "Attack\nF");
                        }
                        action(p, KeyCode::KeyR, "Baby / heal\nR");
                    }
                    GameStage::Civilization => {
                        action(p, KeyCode::KeyQ, "Select fleet\nQ");
                        action(p, KeyCode::KeyX, "Claim spice\nX");
                        action(p, KeyCode::KeyG, "Build land\nG");
                        action(p, KeyCode::KeyH, "Build air\nH");
                        for (key, label, icon_name) in [
                            (KeyCode::Digit1, "Military · 1", "icon_headerMilitary"),
                            (KeyCode::Digit2, "Religious · 2", "icon_headerReligious"),
                            (KeyCode::Digit3, "Economic · 3", "icon_headerEconomic"),
                        ] {
                            if let Some(icon) = originals
                                .ui_art
                                .textures
                                .iter()
                                .find(|texture| texture.name == icon_name)
                            {
                                native_labeled_action(p, key, label, icon);
                            } else {
                                action(p, key, label);
                            }
                        }
                        action(p, KeyCode::KeyF, "Send fleet\nF");
                        action(p, KeyCode::KeyB, "Build city\nB");
                        action(p, KeyCode::KeyT, "Turret\nT");
                        action(p, KeyCode::KeyE, "Engine\nE");
                        action(p, KeyCode::Digit4, "House\n4");
                        action(p, KeyCode::Digit5, "Factory\n5");
                        action(p, KeyCode::Digit6, "Entertainment\n6");
                        action(p, KeyCode::Space, "Next unit\nSpace");
                        action(p, KeyCode::Backspace, "City\nBackspace");
                        action(p, KeyCode::F3, "Creator\nF3");
                    }
                    GameStage::Space => {
                        action(p, KeyCode::F3, "Creator\nF3");
                        if space_zoom == crate::space_view::SpaceZoom::Galaxy {
                            action(p, KeyCode::Enter, "Jump\nEnter");
                            action(p, KeyCode::Home, "Select here\nHome");
                            action(p, KeyCode::Tab, "Filter\nTab");
                            action(p, KeyCode::KeyZ, "Zoom out\nZ");
                            action(p, KeyCode::KeyX, "Return\nX");
                        } else {
                            action(p, KeyCode::KeyQ, "Descend\nQ");
                            action(p, KeyCode::KeyE, "Climb\nE");
                            match space_zoom {
                                crate::space_view::SpaceZoom::Planet => {
                                    action(p, KeyCode::KeyZ, "System view\nZ");
                                    action(p, KeyCode::KeyX, "Closer\nX");
                                }
                                crate::space_view::SpaceZoom::System => {
                                    action(p, KeyCode::KeyZ, "Galaxy\nZ");
                                    action(p, KeyCode::KeyX, "Planet view\nX");
                                }
                                crate::space_view::SpaceZoom::Galaxy => {
                                    action(p, KeyCode::KeyZ, "Zoom out\nZ");
                                    action(p, KeyCode::KeyX, "Return\nX");
                                }
                            }
                        }
                        for (key, label, icon_name) in [
                            (KeyCode::KeyV, "Scan · V", None),
                            (
                                KeyCode::KeyT,
                                "Next tool · T",
                                Some("spg-gui-btn-terra-nml"),
                            ),
                            (KeyCode::KeyY, "Use tool · Y", None),
                            (KeyCode::KeyU, "Beam · U", None),
                            (KeyCode::KeyR, "Power · R", None),
                            (KeyCode::KeyC, "Colony · C", None),
                            (KeyCode::KeyB, "Spice · B", None),
                            (KeyCode::KeyK, "Comms · K", Some("spg-gui-btn-comm-nml")),
                            (KeyCode::KeyM, "Missions · M", Some("spg-icon-mission")),
                            (KeyCode::KeyF, "Laser · F", Some("spg-gui-btn-attack-nml")),
                        ] {
                            let icon = icon_name.and_then(|name| {
                                originals
                                    .ui_art
                                    .textures
                                    .iter()
                                    .find(|texture| texture.name == name)
                            });
                            if let Some(icon) = icon {
                                native_labeled_action(p, key, label, icon);
                            } else {
                                action(p, key, label);
                            }
                        }
                    }
                    _ => {}
                });
            });
        });
    }
    if hidden || *stage == GameStage::MainMenu {
        return;
    }
    let (health, food) = player
        .single()
        .map(|(h, f, _)| {
            (
                h.frac(),
                f.map(|f| f.frac()).unwrap_or_else(|| {
                    if *stage == GameStage::Space {
                        run.space_campaign.fuel_fraction()
                    } else {
                        1.0
                    }
                }),
            )
        })
        .unwrap_or((1., 1.));
    let goal = crate::game::tuning_for(stage).dna_to_advance;
    let ranks = if *stage == GameStage::Creature {
        let abilities = if originals.part_catalog.parts.is_empty() {
            crate::asset_catalog::Capabilities {
                bite: 1,
                sing: 1,
                ..default()
            }
        } else {
            originals.part_catalog.abilities_in(
                crate::asset_catalog::CREATURE_PARTS,
                run.parts.iter().map(|part| part.instance),
            )
        };
        Some(crate::creature_actions::levels(abilities, encounter.mode))
    } else {
        None
    };
    let progress = match stage {
        GameStage::Space => run.space_campaign.claimed as f32 / 5.,
        GameStage::Tribal => run.tribal_campaign.resolved_count() as f32 / 5.,
        GameStage::Civilization => run.civ_campaign.owned_count() as f32 / 5.,
        // Retail: the bar shows the DNA into the current brain level over that
        // level's cost, not the stage total.
        GameStage::Creature => crate::creature_rules::level_progress(run.dna_this_stage),
        _ => run.dna_this_stage / goal.max(1.),
    };
    for (m, mut node) in &mut meters {
        node.width = Val::Percent(
            match m.0 {
                0 => health,
                1 => food,
                _ => progress,
            }
            .clamp(0., 1.)
                * 100.,
        );
    }
    for (mut image, skill, native_meter, brain) in &mut image_nodes {
        if let Some(skill) = skill {
            image.color = if ranks.is_some_and(|ranks| ranks[skill.0] == 0) {
                Color::srgba(0.40, 0.40, 0.40, 0.60)
            } else {
                Color::WHITE
            };
        }
        if let Some(native_meter) = native_meter {
            image.rect = Some(native_meter_frame(if native_meter.0 == 0 {
                health
            } else {
                food
            }));
        }
        if let Some(brain) = brain {
            image.color = if run.creature_progression.brain_level >= brain.0 {
                Color::WHITE
            } else {
                Color::srgba(0.40, 0.40, 0.40, 0.60)
            };
        }
    }
    let objective = match stage {
        GameStage::Cell => format!(
            "Eat and grow • {}/5 growth • DNA {:.0}/{goal:.0}",
            crate::cell::growth_tier(run.dna_this_stage) + 1,
            run.dna_this_stage
        ),
        GameStage::Creature => format!(
            "Brain {}/4 • DNA {:.0}/{goal:.0} • generation {}",
            run.creature_progression.brain_level,
            run.dna_this_stage,
            run.creature_lifecycle.generation
        ),
        GameStage::Tribal => format!(
            "Unite the villages • Food {:.0} • {} settled",
            run.dna_this_stage,
            run.tribal_campaign.resolved_count()
        ),
        GameStage::Civilization => format!(
            "Unify the cities • Spice {:.0} • {} cities",
            run.spice,
            run.civ_campaign.owned_count()
        ),
        GameStage::Space => format!(
            "{} • {} colonies • Spice {:.0}",
            run.space_campaign.mission_name(),
            run.space_campaign.new_colony_count(),
            run.spice
        ),
        _ => String::new(),
    };
    let feedback = match stage {
        GameStage::Creature => crate::creature_actions::hud(
            &encounter,
            &run,
            &originals,
            actors.iter().map(|(id, mind)| (id.0, mind.clone())),
        )
        .lines()
        .take(2)
        .collect::<Vec<_>>()
        .join(" • "),
        GameStage::Tribal => {
            if run.tribal_campaign.message.is_empty() {
                "Ctrl+A selects the tribe • right-click orders • WASD pans • Space follows • Z gathers • X fishes • H hunts".into()
            } else {
                run.tribal_campaign.message.clone()
            }
        }
        GameStage::Civilization => {
            if run.civ_campaign.message.is_empty() {
                "Q selects the fleet • right-click orders • X claims spice".into()
            } else {
                run.civ_campaign.message.clone()
            }
        }
        GameStage::Space if space_zoom == crate::space_view::SpaceZoom::Galaxy => galaxy
            .as_deref()
            .zip(space_view.as_deref())
            .map(|(galaxy, view)| {
                crate::space_galaxy::galaxy_caption(galaxy, &run.space_campaign, view)
            })
            .unwrap_or_else(|| "WASD select a star • Enter jumps • Home selects your star".into()),
        GameStage::Space => {
            let action = if run.space_campaign.message.is_empty() {
                run.space_campaign.next_action()
            } else {
                run.space_campaign.message.clone()
            };
            format!("Click / right mouse fly • WASD camera (planet: fly) • Wheel +/- zoom • End/Home levels • {action}")
        }
        GameStage::Cell => match cell_notice.as_deref() {
            Some(notice) if notice.remaining > 0.0 => notice.text.clone(),
            _ => "Hold the mouse or WASD to swim • Eat to grow • F2 calls a mate".into(),
        },
        _ => "WASD / arrows move • Select an action below • F3 details".into(),
    };
    let mut cards_text = goals(stage, &run, goal);
    if *stage == GameStage::Tribal {
        let status = run.tribal_campaign.status_summary();
        cards_text[2] = (
            "Tribe needs".into(),
            format!(
                "Energy {:.0}% · {} hungry · {} growing{}\nAnimals {}/3 · eggs {:.0}/40 · {} (C)",
                status.average_energy_percent,
                status.hungry_members,
                status.pending_births,
                status
                    .next_birth_seconds
                    .map_or_else(String::new, |seconds| format!(" ({seconds:.0}s)")),
                run.tribal_campaign.livestock.len(),
                run.tribal_campaign.livestock_eggs,
                if run.tribal_campaign.friendly_stance { "Friendly" } else { "Aggressive" },
            ),
        );
    } else if *stage == GameStage::Space && space_zoom != crate::space_view::SpaceZoom::Galaxy {
        if let Ok((_, _, player_pose)) = player.single() {
            if let Some((id, _, planet)) = planets.iter().min_by(|a, b| {
                player_pose
                    .translation
                    .distance(a.1.translation)
                    .total_cmp(&player_pose.translation.distance(b.1.translation))
                    .then(a.0 .0.cmp(&b.0 .0))
            }) {
                cards_text[2] = (
                    format!(
                        "{} · {}",
                        planet.name,
                        run.space_campaign.planetary_tool.label()
                    ),
                    format!(
                        "{}\nT next tool · Y use · C colony",
                        run.space_campaign.ecology_summary(id.0)
                    ),
                );
            }
        }
    }
    for (card, mut node) in &mut cards {
        let shown = !cards_text[card.0 as usize].0.is_empty();
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    let counter = match stage {
        GameStage::Cell | GameStage::Creature => format!("DNA {:.0}", run.editor_dna),
        GameStage::Tribal => format!("Food {:.0}", run.dna_this_stage),
        _ => format!("Spice {:.0}", run.spice),
    };
    for (r, mut label) in &mut readouts {
        let value = match r.0 {
            5 => counter.clone(),
            6 => mobility.as_deref().map_or_else(String::new, |mobility| {
                mobility.hud_summary(crate::creature_actions::player_abilities(&run, &originals))
            }),
            10..=12 => cards_text[(r.0 - 10) as usize].0.clone(),
            20..=22 => cards_text[(r.0 - 20) as usize].1.clone(),
            0 => format!("Health {:.0}%", health * 100.),
            1 => format!(
                "{} {:.0}%",
                if *stage == GameStage::Space {
                    "Energy"
                } else {
                    "Food"
                },
                food * 100.
            ),
            2 => format!("Progress {:.0}%", progress.clamp(0., 1.) * 100.),
            3 => objective.clone(),
            _ => feedback.clone(),
        };
        if label.0 != value {
            label.0 = value;
        }
    }
}
pub fn actions(
    stage: Res<State<GameStage>>,
    mut buttons: Query<(Entity, &HudAction, Has<Pressed>, &mut BackgroundColor)>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut injected: ResMut<InjectedKeys>,
    mut held: Local<Vec<Entity>>,
) {
    let mut current = vec![];
    for (entity, action, pressed, mut color) in &mut buttons {
        color.0 = if pressed {
            Color::srgba(0.26, 0.56, 0.70, 0.9)
        } else {
            Color::srgba(0.07, 0.15, 0.20, 0.62)
        };
        if pressed {
            current.push(entity);
            let repeat = action.0 == KeyCode::KeyF
                && matches!(
                    stage.get(),
                    GameStage::Tribal | GameStage::Civilization | GameStage::Space
                );
            if (repeat || !held.contains(&entity)) && !keys.pressed(action.0) {
                keys.press(action.0);
                injected.0.push(action.0);
            }
        }
    }
    *held = current;
}
pub fn release(mut keys: ResMut<ButtonInput<KeyCode>>, mut injected: ResMut<InjectedKeys>) {
    for key in injected.0.drain(..) {
        keys.release(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creature_hud_frame_keeps_original_fixed_edges_and_repeatable_center() {
        assert_eq!(CREATURE_PANEL_LEFT_INDENT + CREATURE_PANEL_LEFT_DIP, 127.);
        assert_eq!(CREATURE_PANEL_SIDE_TILE + CREATURE_PANEL_RIGHT_DIP, 130.);
        assert_eq!(CREATURE_PANEL_HEIGHT, 59.);
        assert_eq!(CREATURE_PANEL_MID_HEIGHT, 46.);
        assert_eq!(CREATURE_PANEL_MID_BOTTOM, 6.);
    }

    #[test]
    fn native_meter_chooses_nearest_clamped_84_pixel_frame() {
        let empty = native_meter_frame(0.);
        assert_eq!(empty.min, Vec2::new(0., 0.));
        assert_eq!(empty.max, Vec2::new(84., 16.));

        let half = native_meter_frame(0.5);
        assert_eq!(half.min, Vec2::new(8. * 84., 0.));
        assert_eq!(half.max, Vec2::new(9. * 84., 16.));

        let full = native_meter_frame(1.);
        assert_eq!(full.min, Vec2::new(15. * 84., 0.));
        assert_eq!(full.max, Vec2::new(16. * 84., 16.));
        assert_eq!(native_meter_frame(-1.), empty);
        assert_eq!(native_meter_frame(2.), full);
    }

    #[test]
    fn native_stage_icons_fit_without_stretching_or_exceeding_their_bounds() {
        assert_eq!(
            fitted_image_size(128, 64, Vec2::splat(32.)),
            Vec2::new(32., 16.)
        );
        assert_eq!(
            fitted_image_size(96, 64, Vec2::new(24., 28.)),
            Vec2::new(24., 16.)
        );
        assert_eq!(fitted_image_size(0, 64, Vec2::splat(32.)), Vec2::ZERO);
    }

    fn press_pointer(world: &mut World, entity: Entity) {
        world.trigger(bevy::picking::events::PointerPress {
            entity,
            pointer: bevy::picking::prelude::Pointer::new(
                bevy::picking::pointer::PointerId::Mouse,
                bevy::picking::pointer::Location {
                    target: bevy::camera::NormalizedRenderTarget::None {
                        width: 1280,
                        height: 900,
                    },
                    position: Vec2::new(30., 50.),
                },
            ),
            button: bevy::picking::prelude::PointerButton::Primary,
            hit: bevy::picking::backend::HitData {
                camera: Entity::PLACEHOLDER,
                depth: 0.,
                position: None,
                normal: None,
                extra: None,
            },
            count: 1,
        });
        world.flush();
        world.flush();
    }
    #[test]
    fn real_menu_button_enters_cell_without_leaving_a_key_held() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.add_plugins(bevy::ui_widgets::ButtonPlugin);
        let world = app.world_mut();
        let button = world
            .query::<(Entity, &HudAction)>()
            .iter(world)
            .find(|(_, a)| a.0 == KeyCode::Enter)
            .unwrap()
            .0;
        press_pointer(world, button);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<State<GameStage>>().get(),
            &GameStage::Cell
        );
        assert!(!app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::Enter));
    }
    #[test]
    fn mouse_press_does_not_release_an_already_held_physical_key() {
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.add_plugins(bevy::ui_widgets::ButtonPlugin);
        let world = app.world_mut();
        let button = world
            .query::<(Entity, &HudAction)>()
            .iter(world)
            .find(|(_, a)| a.0 == KeyCode::F2)
            .unwrap()
            .0;
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F2);
        press_pointer(world, button);
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::F2));
    }
}
