//! The Creature stage's friendship meter: `SocialRainbow`, a two-sided bar
//! bent into an arc over the creature's relationship face.
//!
//! VERIFIED owner: the creature game UI (0xD2CB10, the one that loads
//! `PosseBar`) loads `SocialRainbow` through 0xE32DA0; the bar rows are
//! written by the social behaviour (0xD9AFF0 row 0, 0xD9B030 row 1 ->
//! 0xE33A70). `SocialBar` (the parchment with five faces) belongs to the
//! Tribal posse UI (0xCCDEF0 -> 0xE31630) and is not used here.
//!
//! What the code draws (0xE32540 places, 0xE33230 paints, 0xE31D20 draws an
//! arc band, 0xE32AE0 a radial quad):
//! * The arc: its ends are the two knob windows (the right knob is moved to
//!   the left knob's height), its centre lies between them at the height of
//!   the marker window 0563BB18 (hidden), so the arc rises over the face; the
//!   angles run `pi/2 -+ acos(..)` (right end = start, left end = end) with a
//!   point at angle `a` at `(cx + r cos a, cy - r sin a)`. The knobs are
//!   turned along it (`-+(theta + pi/2)`).
//! * A wood band (drawable of 0564EEB8) over the whole arc; the texture
//!   advances one width per radian, drawn in 0.1 rad steps; the band spans
//!   `[r - h, r]` (h = drawable height).
//! * Row 0 (the creature's demonstrations) grows from the start (right end)
//!   toward the left, row 1 (the player's answers) from the end (left end)
//!   toward the right, both with the fill drawable of 0563BB38, each led by
//!   its cap (0563BB08 / 0563BB10) and shown in its knob (0562C1B8 /
//!   0562C170); the bar is full when they meet (0xE32EC0 then shows
//!   0562C178 / 0564F020).
//! * `2 x rounds + 1` ticks (drawable of 0563BB18, colour 0x8F000000) split
//!   the arc into half-rounds.
//! * The face (053DAA70, eight frames) shows the species' relationship
//!   (0xE31C70: 1 -> frame 2, 2 -> 1, 3 -> 3, 4 -> 4, 5 -> 0, 6 -> 5).
//!
//! ADAPTED: the band is drawn as rotated rectangles (Bevy UI nodes) instead
//! of a triangle strip; the radius is the knob centres' distance plus half the
//! band; the full-bar glow (0564EE80 over the whole arc) is shown only while
//! the bar is full (its cached draw condition is not traced).

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;

use super::{world::SocialMeter, CreatureHudRoot, CreatureHudState};
use crate::spui::{
    model::{Drawable, ImageRef},
    view::{SpuiAssets, SpuiState},
};

pub const LAYOUT: &str = "SocialRainbow";

/// Control ids of `SocialRainbow`.
#[allow(dead_code)]
pub mod id {
    pub const ROOT: u32 = 0x053D_AA60;
    /// The box whose window procedure paints the arc.
    pub const FRAME: u32 = 0x0562_BD40;
    /// Eight-frame relationship face and the star burst behind it.
    pub const FACE: u32 = 0x053D_AA70;
    pub const STAR: u32 = 0x741F_F10A;
    /// Knobs: left = the player's end (row 1), right = the creature's (row 0).
    pub const KNOB_LEFT: u32 = 0x0563_BB50;
    pub const KNOB_RIGHT: u32 = 0x0562_C1A0;
    pub const KNOB_FILL_PLAYER: u32 = 0x0562_C170;
    pub const KNOB_FILL_CREATURE: u32 = 0x0562_C1B8;
    pub const KNOB_FULL: [u32; 2] = [0x0562_C178, 0x0564_F020];
    /// Drawable templates the paint uses (never shown themselves).
    pub const CAP_CREATURE: u32 = 0x0563_BB08;
    pub const CAP_PLAYER: u32 = 0x0563_BB10;
    pub const TICK: u32 = 0x0563_BB18;
    pub const FILL: u32 = 0x0563_BB38;
    pub const BAND: u32 = 0x0564_EEB8;
    pub const GLOW: u32 = 0x0564_EE80;
    pub const TEMPLATES: [u32; 6] = [CAP_CREATURE, CAP_PLAYER, TICK, FILL, BAND, GLOW];
}

/// Tick colour (0xE33230: 0x8F000000).
const TICK_COLOR: u32 = 0x8F00_0000;
/// Arc step of the band (0xE31D20 called with 0.1).
const STEP: f32 = 0.1;

/// Face frame of a relationship (0xE31C70 jump table).
pub fn face_frame(relationship: u8) -> Option<u32> {
    match relationship {
        1 => Some(2),
        2 => Some(1),
        3 => Some(3),
        4 => Some(4),
        5 => Some(0),
        6 => Some(5),
        _ => None,
    }
}

/// The arc through the knob centres (0xE32540).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcShape {
    pub centre: Vec2,
    /// Outer radius of the band.
    pub radius: f32,
    /// Right end (row 0 starts here) and left end (row 1), radians.
    pub start: f32,
    pub end: f32,
    /// Knob turn (left; the right knob turns the other way), radians clockwise.
    pub knob_turn: f32,
}

/// Pure: the arc of knob centres `left` / `right` (the right one at the left
/// one's height) and the marker height `marker_y`, for a band `band` high.
pub fn arc_shape(left: Vec2, right_x: f32, marker_y: f32, band: f32) -> Option<ArcShape> {
    let mid = 0.5 * (left.x + right_x);
    let centre = Vec2::new(mid, marker_y);
    let to_left = left - centre;
    let r = to_left.length();
    if !(r > 1.0) || to_left.x >= 0.0 {
        return None;
    }
    // Angle of the right end in the code's convention (y up from the centre).
    let a = (-(to_left.y)).atan2(-to_left.x);
    let theta = (-(left.y - marker_y) / (left.x - mid)).atan();
    Some(ArcShape {
        centre,
        radius: r + 0.5 * band,
        start: a,
        end: std::f32::consts::PI - a,
        knob_turn: -theta - FRAC_PI_2,
    })
}

impl ArcShape {
    fn point(&self, angle: f32, radius: f32) -> Vec2 {
        self.centre + Vec2::new(angle.cos(), -angle.sin()) * radius
    }
}

/// Visibility, frames and knob turns of the layout's own windows.
pub fn bind(ui: &mut SpuiState, meter: Option<&SocialMeter>) {
    let Some(root) = ui.find(id::ROOT) else {
        return;
    };
    ui.set_visible(root, meter.is_some());
    let Some(m) = meter else {
        return;
    };
    let full = m.creature + m.player >= 1.0 - 1e-3;
    let show = |ui: &mut SpuiState, control: u32, on: bool| {
        if let Some(w) = ui.find(control) {
            ui.set_visible(w, on);
        }
    };
    for control in id::TEMPLATES {
        show(ui, control, false);
    }
    // The frame's design-time white canvas: its window procedure paints the
    // arc instead.
    if let Some(w) = ui.find(id::FRAME) {
        ui.set_tint(w, 0x00FF_FFFF);
    }
    show(ui, id::KNOB_FILL_CREATURE, m.creature > 0.0);
    show(ui, id::KNOB_FILL_PLAYER, m.player > 0.0);
    for control in id::KNOB_FULL {
        show(ui, control, full);
    }
    show(ui, id::STAR, full);
    if let Some(w) = ui.find(id::FACE) {
        ui.set_frame(w, face_frame(m.species_relation));
    }
    if let Some(shape) = shape_of(ui) {
        if let Some(w) = ui.find(id::KNOB_LEFT) {
            ui.set_rotation(w, shape.knob_turn);
        }
        if let Some(w) = ui.find(id::KNOB_RIGHT) {
            ui.set_rotation(w, -shape.knob_turn);
        }
    }
}

fn centre(r: crate::spui::solve::Rect) -> Vec2 {
    Vec2::new(0.5 * (r.x1 + r.x2), 0.5 * (r.y1 + r.y2))
}

fn image_of(ui: &SpuiState, control: u32) -> Option<ImageRef> {
    match &ui.layout.window(ui.find(control)?).drawable {
        Some(Drawable::Image(d)) => d.image.clone(),
        _ => None,
    }
}

fn shape_of(ui: &SpuiState) -> Option<ArcShape> {
    let left = centre(ui.rect(ui.find(id::KNOB_LEFT)?));
    let right = centre(ui.rect(ui.find(id::KNOB_RIGHT)?));
    let marker = centre(ui.rect(ui.find(id::TICK)?));
    let band = ui.rect(ui.find(id::BAND)?).height();
    arc_shape(left, right.x, marker.y, band)
}

/// The arc pieces under the frame window (rebuilt when the meter changes).
#[derive(Resource, Default)]
pub struct RainbowArc {
    holder: Option<Entity>,
    frame: Option<Entity>,
    signature: Vec<i32>,
}

#[derive(Component)]
pub struct RainbowPiece;

fn argb(c: u32) -> Color {
    Color::srgba_u8((c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8)
}

/// One rectangle of the arc: centred at `at`, `size` (tangent, radial),
/// turned so its top faces outward at `angle`, showing `u0..u1` of `image`.
struct Piece {
    at: Vec2,
    size: Vec2,
    angle: f32,
    image: ImageRef,
    u: [f32; 2],
    color: u32,
}

/// Pure: the pieces of the meter for fills `creature` / `player` (fractions
/// of the whole bar) and `rounds` (ticks).
fn pieces(
    shape: &ArcShape,
    images: &[Option<ImageRef>; 6],
    band: Vec2,
    creature: f32,
    player: f32,
    rounds: u8,
    caps: [Vec2; 2],
    tick: Vec2,
) -> Vec<Piece> {
    let [cap_c, cap_p, tick_img, fill, band_img, glow] = images;
    let mid = shape.radius - 0.5 * band.y;
    let range = shape.end - shape.start;
    let mut out = Vec::new();
    let strip = |out: &mut Vec<Piece>, image: &Option<ImageRef>, from: f32, to: f32| {
        let Some(image) = image else { return };
        let (from, to) = (from.min(to), from.max(to));
        let mut a = from;
        while a < to - 1e-4 {
            let b = (a + STEP).min(to);
            let u0 = (a - shape.start).rem_euclid(1.0);
            let u1 = u0 + (b - a);
            let centre_angle = 0.5 * (a + b);
            out.push(Piece {
                at: shape.point(centre_angle, mid),
                size: Vec2::new(mid * (b - a) + 1.5, band.y),
                angle: centre_angle,
                image: image.clone(),
                u: [u0, u1.min(1.0).max(u0 + 1e-3)],
                color: 0xFFFF_FFFF,
            });
            a = b;
        }
    };
    strip(&mut out, band_img, shape.start, shape.end);
    let full = creature + player >= 1.0 - 1e-3;
    let creature_end = shape.start + creature.clamp(0.0, 1.0) * range;
    let player_end = shape.end - player.clamp(0.0, 1.0) * range;
    if creature > 0.0 {
        strip(&mut out, fill, shape.start, creature_end);
    }
    if player > 0.0 {
        strip(&mut out, fill, player_end, shape.end);
    }
    if full {
        strip(&mut out, glow, shape.start, shape.end);
    }
    let radial = |out: &mut Vec<Piece>, image: &Option<ImageRef>, angle: f32, size: Vec2, color: u32| {
        if let Some(image) = image {
            out.push(Piece { at: shape.point(angle, mid), size, angle, image: image.clone(), u: [0.0, 1.0], color });
        }
    };
    if creature > 0.0 && !full {
        radial(&mut out, cap_c, creature_end, caps[0], 0xFFFF_FFFF);
    }
    if player > 0.0 && !full {
        radial(&mut out, cap_p, player_end, caps[1], 0xFFFF_FFFF);
    }
    let n = 2 * rounds.max(1) as usize;
    for k in 0..=n {
        let angle = (shape.start + range * k as f32 / n as f32).min(shape.end - 0.01);
        radial(&mut out, tick_img, angle, tick, TICK_COLOR);
    }
    out
}

/// Spawn the arc under the meter's frame window whenever the meter changes.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    hud: Res<CreatureHudState>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut textures: ResMut<Assets<Image>>,
    mut arc: ResMut<RainbowArc>,
    roots: Query<(&SpuiState, &CreatureHudRoot)>,
) {
    let Some(ui) = roots.iter().find(|(ui, kind)| **kind == CreatureHudRoot::Social && ui.ready).map(|(ui, _)| ui) else {
        return;
    };
    let meter = hud.social_meter.as_ref().filter(|_| !hud.paused);
    let (Some(frame_id), Some(shape)) = (ui.find(id::FRAME), shape_of(ui)) else {
        return;
    };
    let frame = ui.entities[frame_id];
    let frame_rect = ui.rect(frame_id);
    let mut signature = vec![frame_rect.x1 as i32, frame_rect.y1 as i32, (shape.radius * 4.0) as i32];
    if let Some(m) = meter {
        signature.extend([(m.creature * 400.0) as i32, (m.player * 400.0) as i32, m.rounds as i32, 1]);
    }
    if arc.frame == Some(frame) && arc.signature == signature {
        return;
    }
    if let Some(old) = arc.holder.take() {
        if let Ok(mut e) = commands.get_entity(old) {
            e.despawn();
        }
    }
    arc.frame = Some(frame);
    arc.signature = signature;
    let Some(m) = meter else {
        return;
    };
    let index = &originals.resource_index;
    let images: [Option<ImageRef>; 6] = id::TEMPLATES.map(|c| image_of(ui, c));
    let size_of = |c: u32| ui.find(c).map_or(Vec2::ZERO, |w| {
        let r = ui.rect(w);
        Vec2::new(r.width(), r.height())
    });
    let band = size_of(id::BAND);
    let list = pieces(&shape, &images, band, m.creature, m.player, m.rounds, [size_of(id::CAP_CREATURE), size_of(id::CAP_PLAYER)], size_of(id::TICK));
    let holder = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(frame_rect.width()),
                height: Val::Px(frame_rect.height()),
                ..default()
            },
            RainbowPiece,
        ))
        .id();
    // Painted first (under the knobs), as the frame's own paint is.
    commands.entity(frame).insert_children(0, &[holder]);
    arc.holder = Some(holder);
    for p in list {
        let Some(tex) = assets.texture(index, &mut textures, p.image.file) else {
            continue;
        };
        let [tw, th] = [tex.size[0] as f32, tex.size[1] as f32];
        let [x1, y1, x2, y2] = p.image.uv;
        let w = x2 - x1;
        let rect = Rect::new((x1 + w * p.u[0]) * tw, y1 * th, (x1 + w * p.u[1]) * tw, y2 * th);
        let local = p.at - Vec2::new(frame_rect.x1, frame_rect.y1);
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(local.x - 0.5 * p.size.x),
                top: Val::Px(local.y - 0.5 * p.size.y),
                width: Val::Px(p.size.x),
                height: Val::Px(p.size.y),
                ..default()
            },
            ImageNode { image: tex.handle.clone(), rect: Some(rect), color: argb(p.color), ..default() },
            UiTransform { rotation: Rot2::radians(FRAC_PI_2 - p.angle), ..default() },
            RainbowPiece,
            ChildOf(holder),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arc_rises_over_the_face_between_the_knobs() {
        // The installed layout: knob centres (557, 116) / (723, 116), marker
        // centre y 176, band 32 px.
        let s = arc_shape(Vec2::new(557.0, 116.0), 723.0, 176.0, 32.0).unwrap();
        assert_eq!(s.centre, Vec2::new(640.0, 176.0));
        // pi/2 -+ acos(..): symmetric about the top.
        assert!((s.start + s.end - std::f32::consts::PI).abs() < 1e-5);
        assert!(s.start > 0.0 && s.start < FRAC_PI_2);
        let mid = s.radius - 16.0;
        let right = s.point(s.start, mid);
        let left = s.point(s.end, mid);
        assert!((right - Vec2::new(723.0, 116.0)).length() < 0.5, "{right}");
        assert!((left - Vec2::new(557.0, 116.0)).length() < 0.5, "{left}");
        // The top of the arc is above the knobs.
        assert!(s.point(FRAC_PI_2, mid).y < 80.0);
        // Knob turns: theta = atan(-0.723) -> -(theta) - pi/2.
        assert!((s.knob_turn - (0.6261 - FRAC_PI_2)).abs() < 1e-3, "{}", s.knob_turn);
    }

    #[test]
    fn faces_follow_the_relationship_table() {
        let frames: Vec<Option<u32>> = (0..8).map(face_frame).collect();
        assert_eq!(frames, vec![None, Some(2), Some(1), Some(3), Some(4), Some(0), Some(5), None]);
    }

    #[test]
    fn rows_grow_from_opposite_ends_with_caps_and_ticks() {
        let s = arc_shape(Vec2::new(557.0, 116.0), 723.0, 176.0, 32.0).unwrap();
        let img = ImageRef { file: crate::dbpf::ResourceKey { type_id: 1, group_id: 2, instance_id: 3 }, uv: [0.0, 0.0, 1.0, 1.0], dims: None };
        let images: [Option<ImageRef>; 6] = std::array::from_fn(|_| Some(img.clone()));
        let band = Vec2::new(128.0, 32.0);
        let empty = pieces(&s, &images, band, 0.0, 0.0, 3, [Vec2::new(8.0, 32.0); 2], Vec2::new(4.0, 32.0));
        let band_pieces = ((s.end - s.start) / STEP).ceil() as usize;
        assert_eq!(empty.len(), band_pieces + 7, "band + 2 x 3 + 1 ticks");
        let some = pieces(&s, &images, band, 0.25, 0.1, 3, [Vec2::new(8.0, 32.0); 2], Vec2::new(4.0, 32.0));
        // The creature's fill starts at the right end, the player's at the left.
        let fills: Vec<&Piece> = some[band_pieces..].iter().filter(|p| p.size.y == 32.0 && p.size.x > 5.0 && p.color == 0xFFFF_FFFF && p.size.x != 8.0).collect();
        assert!(fills.iter().any(|p| p.at.x > 640.0) && fills.iter().any(|p| p.at.x < 640.0));
        let full = pieces(&s, &images, band, 0.5, 0.5, 3, [Vec2::new(8.0, 32.0); 2], Vec2::new(4.0, 32.0));
        assert!(full.len() > some.len(), "the glow covers the whole arc when full");
    }
}
