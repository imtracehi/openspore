//! The minimap of the planet panel: the layout's "Minimap holder" window
//! (`0190722E`, shown when the Terraforming/Minimap toggle is on) is where the
//! retail minimap window (the class built by 0xE0FC70, the same one the Civ HUD
//! uses) draws the map of the planet in view.
//!
//! ORIGINAL (decoded from the code image, march2017 addresses):
//! * The window projects a world position onto the map with 0xE0B560. For the
//!   Civ and Space game modes (`GetCurrentGameMode` 0x1654C04 / 0x1654C05) it
//!   is a plain longitude / latitude map of the planet about the axis
//!   `(0, 1, 0)` with the reference direction `(0, 0, -1)` in the middle: the
//!   position is normalised, the longitude is the angle of its projection on
//!   the equator plane from the reference (`acos(-ref . m)`, mirrored by the
//!   sign of `m . (ref x axis)`), `u = longitude / (2 pi)`, `v = acos(axis . n)
//!   / pi` (the constructor's pole angle is `(1 - 0) pi`). The constructor
//!   call of the Space layout (0x1073B9E..0x1073BEF) passes those two vectors,
//!   the map size of the holder window and the angle 0.
//! * The marker positions are then squeezed row by row into a rounded
//!   rectangle (0xE0AA60): the table at `+0x280` (0xE0E820) holds the left
//!   transparent run of every row of the 64 x 64 corner mask
//!   `ui-minimap-corner` (instance 0x361F52ED, type 0x03E421ED) for the 128
//!   half rows, mirrored, and `x' = (t + (256 - 2 t) x / W) W / 256`.
//! * The camera is drawn as an arrow (`minimap-camera`) turned along the
//!   projected camera direction (0xE0D2C0: the points `pos` +- 0.3 `dir`
//!   projected, the longitude difference wrapped around 0.5); cities are
//!   `minimap-myCity` (the player's) and `minimap-city` icons.
//!
//! * A click on the map (the retail window's picture -> position path, decoded
//!   from 0xE0DE20 / 0xE0F0E0 / 0xE0B970): the picture column is taken back
//!   through the row squeeze (0xE0A9C0, the inverse of 0xE0AA60; a point in
//!   the transparent corners is not on the window, 0xE0AB10), `u = column /
//!   width`, `v = row / height`, the direction of that map position on the
//!   planet is the inverse of 0xE0B560, and the activation 0xE0B970 for the
//!   Space game mode (0x1654C05) does nothing while the game is paused or
//!   outside the planet context; otherwise it sends the UFO there:
//!   `cSimulatorPlayerUFO::Get()->0xFFC200(position, 0, 1, 0, 0)`, the goto
//!   command (`SpaceControls::planet_goto`, flown like the flight button).
//!
//! ADAPTED: the terrain picture is the world's own colours (`Look::surface`,
//! the colours the orbit atlas is baked with) shown through the same row
//! squeeze (the retail picture's source texture and the way its rows are drawn
//! were not decoded); the camera arrow comes from the real camera of the view
//! with a fixed angular step instead of 0.3 units; icons are the installed
//! house art of the Creature / Tribal map (`land_minimap::Icon`); the window
//! message that reaches 0xE0F0E0 (which button, whether a drag repeats) was
//! not found: the left button going down on the picture sends the UFO; the
//! point is on the sea surface where the world has water; the ignore test of
//! 0xE0B970 for a point within 0.0316 of the camera position (`+0x304`) is not
//! applied (the goto's own arrival distance covers it).
//!
//! The picture is one CPU-painted image shown by a plain UI node over the
//! holder window, placed in percentages of the screen like the Creature map.

use std::f32::consts::{PI, TAU};
use std::sync::Arc;

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::{SpaceHudActive, SpaceHudRoot, SpaceHudUi};
use crate::{
    dbpf::ResourceKey,
    game::RunProgress,
    land_minimap::{self, Icon, MapSprites, MAP_H, MAP_W},
    planet_query::Planet,
    space_planets::{Look, PlanetMinimapSource},
    stages::{GameStage, StageOwned},
};

/// The planet axis of the map (the constructor's second vector).
pub const AXIS: Vec3 = Vec3::Y;
/// The direction in the middle of the map (the constructor's first vector).
pub const REFERENCE: Vec3 = Vec3::NEG_Z;
/// The pole angle of the map (`(1 - 0) pi`, the constructor's float argument 0).
const POLE_ANGLE: f32 = PI;
/// 0xE0B560 normalises with `sqrt(|v|^2 + 1e-8)`.
const EPSILON: f32 = 1.0e-8;
/// Scale of the corner table (`+0x239` = 0: 256; with the flag 128).
const SCALE: f32 = 256.0;
/// Half rows of the corner table (129 entries).
const ROWS: usize = 129;
/// Seconds between repaints of the markers.
const PERIOD: f32 = 0.08;
/// Frames before the picture is shown (the render world prepares the texture).
const SETTLE_FRAMES: u32 = 4;
/// The corner mask (`ui-minimap-corner`): instance, type, group.
const CORNER_KEY: ResourceKey = ResourceKey { type_id: 0x03E4_21ED, group_id: 0, instance_id: 0x361F_52ED };

fn unit(v: Vec3) -> Vec3 {
    v / (v.length_squared() + EPSILON).sqrt()
}

/// 0xE0B560 (Civ and Space branch): the map position `(u, v)` of a world
/// position, both in `0..=1`: `u` is the longitude about [`AXIS`] from the
/// middle direction [`REFERENCE`] (the reference is at `u = 0.5`), `v` the
/// angle from the axis over [`POLE_ANGLE`].
pub fn project(position: Vec3) -> Vec2 {
    let n = unit(position);
    // The projection on the equator plane and the side vector.
    let m = unit(n - AXIS * AXIS.dot(n));
    let side = unit(REFERENCE.cross(AXIS));
    let phi = (-REFERENCE.dot(m)).clamp(-1.0, 1.0).acos();
    let longitude = if m.dot(side) >= 0.0 { phi } else { TAU - phi };
    let colatitude = AXIS.dot(n).clamp(-1.0, 1.0).acos();
    Vec2::new(longitude / (2.0 * POLE_ANGLE), colatitude / POLE_ANGLE)
}

/// The direction of a map position (the inverse of [`project`] on the unit
/// sphere): `u` 0 is `+z`, 0.25 `+x`, 0.5 the reference `-z`, 0.75 `-x`.
pub fn unproject(uv: Vec2) -> Vec3 {
    let (lon, colat) = (uv.x * TAU, uv.y * PI);
    let ring = colat.sin();
    Vec3::new(ring * lon.sin(), colat.cos(), ring * lon.cos())
}

/// The corner table of the minimap: per half row the left transparent run of
/// the corner mask (0xE0E820).
#[derive(Clone, Debug, PartialEq)]
pub struct Corner {
    run: [u8; ROWS],
}

impl Corner {
    /// 0xE0E820: the leading transparent pixels of every row of the mask
    /// (RGBA, `width` x `height`, alpha is byte 3) for the first 64 rows,
    /// entry `k` and its mirror `127 - k`; entry 128 repeats entry 127.
    pub fn from_mask(width: usize, height: usize, rgba: &[u8]) -> Self {
        let mut run = [0u8; ROWS];
        let width = if width * 2 > SCALE as usize { SCALE as usize / 2 } else { width };
        let rows = if height * 2 > 128 { 64 } else { height };
        for k in 0..rows {
            let mut n = 0usize;
            while n < width && rgba.get((k * width.max(1) + n) * 4 + 3).copied().unwrap_or(255) == 0 {
                n += 1;
            }
            run[k] = n.min(255) as u8;
            run[127 - k] = n.min(255) as u8;
        }
        run[128] = run[127];
        Self { run }
    }

    /// The corner mask of the install (a 20-byte header of big-endian words:
    /// 0, width, height, format, byte size, then RGBA).
    pub fn from_install(index: &crate::asset_catalog::ResourceIndex) -> Option<Self> {
        let bytes = index.read(CORNER_KEY).ok()?;
        let word = |i: usize| bytes.get(i * 4..i * 4 + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
        let (width, height) = (word(1)?, word(2)?);
        let data = bytes.get(20..)?;
        (width > 0 && height > 0 && data.len() >= width * height * 4).then(|| Self::from_mask(width, height, data))
    }

    /// ADAPTED: without the install a quarter circle of the installed mask's
    /// radius (56 of 64 pixels).
    pub fn fallback() -> Self {
        let mut mask = vec![255u8; 64 * 64 * 4];
        for y in 0..64usize {
            for x in 0..64usize {
                let (dx, dy) = (56.0 - x as f32 - 0.5, 56.0 - y as f32 - 0.5);
                if x < 56 && y < 56 && dx * dx + dy * dy > 56.0 * 56.0 {
                    mask[(y * 64 + x) * 4 + 3] = 0;
                }
            }
        }
        Self::from_mask(64, 64, &mask)
    }

    fn row(&self, y: f32, height: f32) -> f32 {
        let k = ((128.0 / height.max(1.0)) * y).floor().clamp(0.0, 128.0) as usize;
        self.run[k] as f32
    }

    /// 0xE0AA60: the column of map column `x` (pixels of a `width` x `height`
    /// picture) in row `y` once the row is squeezed into the rounded shape.
    pub fn squeeze(&self, x: f32, y: f32, width: f32, height: f32) -> f32 {
        let t = self.row(y, height);
        (t + (SCALE - 2.0 * t) * x / width.max(1.0)) * (width / SCALE)
    }

    /// The map column under the picture column `x` of row `y`, `None` outside
    /// the rounded shape.
    pub fn unsqueeze(&self, x: f32, y: f32, width: f32, height: f32) -> Option<f32> {
        let t = self.row(y, height);
        let inner = 1.0 - 2.0 * t / SCALE;
        if inner <= 0.0 {
            return None;
        }
        let column = (x - t * width / SCALE) / inner;
        (0.0..=width).contains(&column).then_some(column)
    }
}

/// Where a planet-local direction lands in the picture (pixels).
pub fn pixel_of(corner: &Corner, dir: Vec3) -> Vec2 {
    let uv = project(dir);
    let (w, h) = (MAP_W as f32, MAP_H as f32);
    let y = uv.y * h;
    Vec2::new(corner.squeeze(uv.x * w, y, w, h), y)
}

/// 0xE0DE20 (picture -> direction): the planet-local direction of the map
/// position under a picture pixel, `None` outside the rounded shape (the window
/// does not contain the point, 0xE0AB10).
pub fn direction_at(corner: &Corner, pixel: Vec2) -> Option<Vec3> {
    let (w, h) = (MAP_W as f32, MAP_H as f32);
    let column = corner.unsqueeze(pixel.x, pixel.y, w, h)?;
    Some(unproject(Vec2::new(column / w, (pixel.y / h).clamp(0.0, 1.0))))
}

/// Colour of a world's dot: an alien empire's own colour, the player's colonies
/// green, scanned worlds blue, the rest pale.
pub fn world_color(empire: Option<crate::space_galaxy::EmpireId>, colonized: bool, surveyed: bool) -> Color {
    if let Some(e) = empire {
        let [r, g, b] = crate::space_galaxy::EMPIRES.get(e as usize).map_or([0.9, 0.3, 0.3], |e| e.1);
        return Color::srgb(r, g, b);
    }
    if colonized {
        Color::srgb(0.2, 1.0, 0.6)
    } else if surveyed {
        Color::srgb(0.3, 0.7, 1.0)
    } else {
        Color::srgb(0.7, 0.7, 0.9)
    }
}

/// The terrain picture of a world: RGBA, the world's colours through the row
/// squeeze, transparent outside the rounded shape.
pub fn bake_terrain(planet: &Planet, look: &Look, corner: &Corner) -> Vec<u8> {
    let (w, h) = (MAP_W as usize, MAP_H as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        for x in 0..w {
            let Some(column) = corner.unsqueeze(x as f32 + 0.5, y as f32 + 0.5, w as f32, h as f32) else {
                continue;
            };
            let dir = unproject(Vec2::new(column / w as f32, v));
            let (c, _) = look.surface(planet, dir);
            let at = (y * w + x) * 4;
            for k in 0..3 {
                rgba[at + k] = (c[k].clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            rgba[at + 3] = 255;
        }
    }
    rgba
}

/// The arrow of the camera: the picture position and the angle (clockwise,
/// 0 = up) of the camera's ground direction. The direction is the projected
/// difference of two points a small angle ahead of and behind the camera
/// (0xE0D2C0 projects `pos` +- 0.3 `dir`), the longitude difference wrapped.
pub fn camera_arrow(corner: &Corner, position: Vec3, forward: Vec3, fallback: Vec3) -> (Vec2, f32) {
    const STEP: f32 = 0.01;
    let n = unit(position);
    let mut ahead = forward - n * n.dot(forward);
    if ahead.length_squared() < 1.0e-6 {
        ahead = fallback - n * n.dot(fallback);
    }
    let ahead = unit(ahead);
    let (a, b) = (project(n - ahead * STEP), project(n + ahead * STEP));
    let mut du = b.x - a.x;
    if du > 0.5 {
        du -= 1.0;
    } else if du < -0.5 {
        du += 1.0;
    }
    let (w, h) = (MAP_W as f32, MAP_H as f32);
    let here = pixel_of(corner, n);
    // The row's squeeze shortens the horizontal step.
    let inner = 1.0 - 2.0 * corner.row(here.y, h) / SCALE;
    let d = Vec2::new(du * w * inner.max(0.05), (b.y - a.y) * h);
    let angle = if d.length_squared() > 1.0e-12 { d.x.atan2(-d.y) } else { 0.0 };
    (here, angle)
}

/// Draw the cities and the camera over a copy of the terrain.
pub fn paint(
    terrain: &[u8],
    corner: &Corner,
    cities: &[(Vec3, Icon, [u8; 3])],
    camera: Option<(Vec2, f32)>,
    sprites: &MapSprites,
) -> Vec<u8> {
    let mut canvas = terrain.to_vec();
    canvas.resize((MAP_W * MAP_H * 4) as usize, 0);
    for (dir, icon, tint) in cities {
        land_minimap::blit(&mut canvas, sprites.get(*icon), *icon, 0, pixel_of(corner, *dir), 1.0, 0.0, *tint);
    }
    if let Some((at, angle)) = camera {
        land_minimap::blit(&mut canvas, sprites.get(Icon::Camera), Icon::Camera, 0, at, 1.0, angle, [255; 3]);
    }
    canvas
}

/// The UI node that shows the picture.
#[derive(Component)]
pub struct MinimapNode {
    image: Handle<Image>,
}

/// Spawn the node while the HUD is up, drop it when the HUD goes.
pub fn manage_minimap(
    mut commands: Commands,
    active: Res<SpaceHudActive>,
    mut images: ResMut<Assets<Image>>,
    nodes: Query<Entity, With<MinimapNode>>,
) {
    if !active.0 {
        for e in &nodes {
            commands.entity(e).despawn();
        }
        return;
    }
    if !nodes.is_empty() {
        return;
    }
    let image = images.add(Image::new(
        Extent3d { width: MAP_W, height: MAP_H, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; (MAP_W * MAP_H * 4) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    commands.spawn((
        MinimapNode { image: image.clone() },
        StageOwned,
        Node { position_type: PositionType::Absolute, ..default() },
        ImageNode::new(image),
        Visibility::Hidden,
        Pickable::IGNORE,
        GlobalZIndex(101),
    ));
}

/// What the painter keeps between frames.
#[derive(Default)]
pub struct Cache {
    corner: Option<Corner>,
    terrain: Option<(u32, Arc<Planet>, Look, Vec<u8>)>,
    seen: u32,
    since_paint: f32,
}

/// Put the picture on the holder: bake the world's terrain when the world or
/// its look changed, repaint the cities and the camera arrow, place the node.
/// Hidden unless the planet panel shows the minimap.
#[allow(clippy::too_many_arguments)]
pub fn update_minimap(
    time: Res<Time>,
    stage: Res<State<GameStage>>,
    hud: Res<super::SpaceHudState>,
    hud_ui: Res<SpaceHudUi>,
    source: Option<Res<PlanetMinimapSource>>,
    originals: Option<Res<crate::Originals>>,
    campaign: Res<RunProgress>,
    view: Option<Res<crate::space_view::SpaceView>>,
    camera: Query<&Transform, (With<crate::GameCamera>, Without<MinimapNode>)>,
    roots: Query<(&crate::spui::view::SpuiState, &SpaceHudRoot, Option<&Visibility>), Without<MinimapNode>>,
    mut cache: Local<Cache>,
    mut sprites: ResMut<MapSprites>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<(&MinimapNode, &mut Node, &mut Visibility)>,
) {
    let Ok((minimap, mut node, mut visibility)) = nodes.single_mut() else {
        cache.seen = 0;
        return;
    };
    let holder = roots
        .iter()
        // The picture goes with its layout while the options menu hides the HUD
        // layers (`front_end::pause::layers`).
        .find(|(ui, kind, shown)| **kind == SpaceHudRoot::Main && ui.ready && shown.is_none_or(|v| *v != Visibility::Hidden))
        .and_then(|(ui, _, _)| ui.find(super::world::pid::MINIMAP_HOLDER).map(|w| (ui.visible(w), ui.rect(w), ui.screen)));
    let shown = *stage.get() == GameStage::Space
        && hud_ui.minimap
        && !hud_ui.planet_hidden
        && hud.zoom == super::HudZoom::Planet
        && hud.planet.is_some();
    let (Some((true, rect, screen)), Some(source), true) = (holder, source.as_deref(), shown) else {
        cache.seen = 0;
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    cache.seen = cache.seen.saturating_add(1);
    cache.since_paint += time.delta_secs();
    if cache.since_paint < PERIOD && cache.seen > 1 {
        place(&mut node, &mut visibility, rect, screen, cache.seen);
        return;
    }
    cache.since_paint = 0.0;
    if let Some(originals) = originals.as_deref() {
        sprites.load(&originals.resource_index);
    }
    if cache.corner.is_none() {
        cache.corner = Some(
            originals
                .as_deref()
                .and_then(|o| Corner::from_install(&o.resource_index))
                .unwrap_or_else(Corner::fallback),
        );
    }
    let corner = cache.corner.clone().unwrap_or_else(Corner::fallback);
    let fresh = cache
        .terrain
        .as_ref()
        .is_some_and(|(world, planet, look, _)| *world == source.world && Arc::ptr_eq(planet, &source.planet) && *look == source.look);
    if !fresh {
        let pixels = bake_terrain(&source.planet, &source.look, &corner);
        cache.terrain = Some((source.world, source.planet.clone(), source.look.clone(), pixels));
    }
    // The cities: the player's colonies and the alien empire's, as the world's
    // owner says.
    let empire = campaign.space_campaign.world_empire(source.world);
    let (icon, tint) = match empire {
        Some(e) => (Icon::Village, land_minimap::tint_of(world_color(Some(e), false, true))),
        None => (Icon::PlayerNest, [255; 3]),
    };
    let cities: Vec<(Vec3, Icon, [u8; 3])> = source.cities.iter().map(|d| (*d, icon, tint)).collect();
    // The camera in the planet's frame (the surface root's rotation).
    let arrow = camera.iter().next().map(|t| {
        let local = source.rotation.inverse() * (t.translation - source.centre);
        let forward = source.rotation.inverse() * *t.forward();
        let heading = view
            .as_deref()
            .and_then(|v| v.planet.as_ref())
            .map_or(Vec3::NEG_Z, |p| source.rotation.inverse() * p.heading);
        camera_arrow(&corner, local, forward, heading)
    });
    let terrain = cache.terrain.as_ref().map_or(&[][..], |t| &t.3[..]);
    let data = paint(terrain, &corner, &cities, arrow, &sprites);
    if let Some(mut picture) = images.get_mut(&minimap.image) {
        picture.data = Some(data);
    }
    place(&mut node, &mut visibility, rect, screen, cache.seen);
}

/// The mouse handler of the planet minimap (0xE0F0E0 -> 0xE0B970, Space
/// branch): the left button going down on the picture sends the UFO to the
/// world point of the map position (`SpaceControls::planet_goto`, a world
/// direction from the planet's centre). The press is marked `over_ui` so the
/// world behind the map (and `space_controls::gather`, which runs after) never
/// sees it. Runs in the Controls set before the HUD's key injection, like the
/// Creature / Tribal map's pointer (`land_minimap`).
#[allow(clippy::too_many_arguments)]
pub fn minimap_pointer(
    stage: Res<State<GameStage>>,
    hud: Res<super::SpaceHudState>,
    hud_ui: Res<SpaceHudUi>,
    source: Option<Res<PlanetMinimapSource>>,
    originals: Option<Res<crate::Originals>>,
    roots: Query<(&crate::spui::view::SpuiState, &SpaceHudRoot, Option<&Visibility>)>,
    mut pointer: ResMut<crate::spui::view::SpuiPointer>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    controls: Option<ResMut<crate::space_controls::SpaceControls>>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    mut corner: Local<Option<Corner>>,
) {
    let shown = *stage.get() == GameStage::Space
        && hud_ui.minimap
        && !hud_ui.planet_hidden
        && hud.zoom == super::HudZoom::Planet
        && hud.planet.is_some();
    let rect = roots
        .iter()
        .find(|(ui, kind, shown)| **kind == SpaceHudRoot::Main && ui.ready && shown.is_none_or(|v| *v != Visibility::Hidden))
        .and_then(|(ui, _, _)| ui.find(super::world::pid::MINIMAP_HOLDER).filter(|w| ui.visible(*w)).map(|w| ui.rect(w)));
    let (Some(rect), Some(source), true) = (rect, source.as_deref(), shown) else {
        return;
    };
    let corner = corner.get_or_insert_with(|| {
        originals
            .as_deref()
            .and_then(|o| Corner::from_install(&o.resource_index))
            .unwrap_or_else(Corner::fallback)
    });
    click(
        corner,
        rect,
        source,
        &mut pointer,
        mouse.as_deref(),
        controls,
        pause.as_deref().is_some_and(|p| p.open),
    );
}

/// One pointer frame over the map: see [`minimap_pointer`].
fn click(
    corner: &Corner,
    rect: crate::spui::solve::Rect,
    source: &PlanetMinimapSource,
    pointer: &mut crate::spui::view::SpuiPointer,
    mouse: Option<&ButtonInput<MouseButton>>,
    controls: Option<ResMut<crate::space_controls::SpaceControls>>,
    paused: bool,
) {
    let Some(pixel) = pointer.position.and_then(|p| land_minimap::picture_pixel(rect, p)) else {
        return;
    };
    // Only the rounded shape is the window (0xE0AB10).
    let Some(dir) = direction_at(corner, pixel) else {
        return;
    };
    pointer.over_ui = true;
    if paused || !mouse.is_some_and(|m| m.just_pressed(MouseButton::Left)) {
        return;
    }
    if let Some(mut controls) = controls {
        controls.planet_goto = Some((source.rotation * dir).normalize_or(Vec3::Y));
    }
}

/// Put the node over the holder window (percentages of the full-screen UI keep
/// it on the window at any resolution or UI scale).
fn place(node: &mut Node, visibility: &mut Visibility, rect: crate::spui::solve::Rect, screen: [f32; 2], seen: u32) {
    let pct = |v: f32, total: f32| Val::Percent(100.0 * v / total.max(1.0));
    node.left = pct(rect.x1, screen[0]);
    node.top = pct(rect.y1, screen[1]);
    node.width = pct(rect.width(), screen[0]);
    node.height = pct(rect.height(), screen[1]);
    let want = if seen >= SETTLE_FRAMES { Visibility::Inherited } else { Visibility::Hidden };
    if *visibility != want {
        *visibility = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_query::{HeightCube, PLANET_RADIUS};

    fn near(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1.0e-3
    }

    #[test]
    fn the_projection_puts_the_reference_in_the_middle_and_the_pole_on_top() {
        // 0xE0B560: the reference direction (0, 0, -1) is the middle of the map.
        assert!(near(project(Vec3::NEG_Z), Vec2::new(0.5, 0.5)));
        // +z is the left edge, +x a quarter in, -x three quarters in.
        assert!(near(project(Vec3::Z), Vec2::new(0.0, 0.5)));
        assert!(near(project(Vec3::X), Vec2::new(0.25, 0.5)));
        assert!(near(project(Vec3::NEG_X), Vec2::new(0.75, 0.5)));
        // The axis is the top row, its opposite the bottom row; the length of
        // the position does not matter.
        assert!(project(Vec3::Y * 700.0).y < 1.0e-3);
        assert!((project(-Vec3::Y).y - 1.0).abs() < 1.0e-3);
        assert!(near(project(Vec3::X * 123.0), project(Vec3::X)));
        // 45 degrees of latitude is a quarter of the height.
        assert!((project(Vec3::new(0.0, 1.0, -1.0)).y - 0.25).abs() < 1.0e-3);
    }

    #[test]
    fn unproject_inverts_the_projection() {
        for (u, v) in [(0.1, 0.2), (0.5, 0.5), (0.77, 0.9), (0.99, 0.35), (0.25, 0.6)] {
            let uv = Vec2::new(u, v);
            let back = project(unproject(uv));
            assert!(near(back, uv), "{uv:?} -> {back:?}");
        }
    }

    /// A mask whose rows start with 56, 50 .. transparent pixels.
    fn mask() -> Vec<u8> {
        let mut m = vec![255u8; 64 * 64 * 4];
        for y in 0..64usize {
            let run = 56usize.saturating_sub(y);
            for x in 0..run {
                m[(y * 64 + x) * 4 + 3] = 0;
            }
        }
        m
    }

    #[test]
    fn the_corner_table_is_the_left_transparent_run_mirrored() {
        let corner = Corner::from_mask(64, 64, &mask());
        assert_eq!(corner.run[0], 56);
        assert_eq!(corner.run[10], 46);
        // Rows 64..127 mirror 63..0, entry 128 repeats 127.
        assert_eq!(corner.run[127], 56);
        assert_eq!(corner.run[117], 46);
        assert_eq!(corner.run[128], corner.run[127]);
        assert_eq!(corner.run[63], 0);
        assert_eq!(corner.run[64], 0);
    }

    #[test]
    fn the_squeeze_insets_the_rows_by_the_corner_and_inverts() {
        let corner = Corner::from_mask(64, 64, &mask());
        let (w, h) = (226.0, 102.0);
        // The middle rows are not squeezed.
        assert!((corner.squeeze(0.0, h * 0.5, w, h)).abs() < 1.0e-4);
        assert!((corner.squeeze(w, h * 0.5, w, h) - w).abs() < 1.0e-3);
        // The top row (run 56 of 256): both ends move in by 56 / 256 of the width.
        let left = corner.squeeze(0.0, 0.2, w, h);
        let right = corner.squeeze(w, 0.2, w, h);
        assert!((left - w * 56.0 / SCALE).abs() < 1.0e-3, "{left}");
        assert!((right - (w - w * 56.0 / SCALE)).abs() < 1.0e-3, "{right}");
        // The inverse maps back and rejects what lies outside the shape.
        let x = corner.squeeze(100.0, 0.2, w, h);
        assert!((corner.unsqueeze(x, 0.2, w, h).unwrap() - 100.0).abs() < 1.0e-3);
        assert!(corner.unsqueeze(2.0, 0.2, w, h).is_none());
        assert!(corner.unsqueeze(2.0, h * 0.5, w, h).is_some());
    }

    #[test]
    fn the_fallback_corner_is_a_quarter_circle() {
        let corner = Corner::fallback();
        assert!(corner.run[0] > 40 && corner.run[0] <= 56);
        assert_eq!(corner.run[60], 0);
        assert_eq!(corner.run[67], 0);
    }

    #[test]
    fn a_city_lands_in_the_picture_where_the_projection_says() {
        let corner = Corner::from_mask(64, 64, &mask());
        let at = pixel_of(&corner, Vec3::NEG_Z);
        // The reference direction on the equator: the middle of the picture.
        assert!((at.x - MAP_W as f32 * 0.5).abs() < 0.5 && (at.y - MAP_H as f32 * 0.5).abs() < 0.5, "{at:?}");
    }

    #[test]
    fn the_camera_arrow_points_the_way_the_camera_looks() {
        let corner = Corner::from_mask(64, 64, &mask());
        // On the equator at the reference, looking toward +x (u shrinks): left; toward -x: right.
        let (_, east) = camera_arrow(&corner, Vec3::new(0.0, 0.0, -100.0), Vec3::X, Vec3::X);
        assert!((east + PI * 0.5).abs() < 0.05, "{east}");
        let (_, west) = camera_arrow(&corner, Vec3::new(0.0, 0.0, -100.0), Vec3::NEG_X, Vec3::X);
        assert!((west - PI * 0.5).abs() < 0.05, "{west}");
        // Looking toward the axis: up. Straight down uses the fallback.
        let (_, north) = camera_arrow(&corner, Vec3::new(0.0, 0.0, -100.0), Vec3::Y, Vec3::X);
        assert!(north.abs() < 0.05, "{north}");
        let (_, down) = camera_arrow(&corner, Vec3::new(0.0, 0.0, -100.0), Vec3::Z, Vec3::X);
        assert!((down + PI * 0.5).abs() < 0.05, "{down}");
        // Across the seam of the map (u 0 / 1) the difference wraps (+x grows u from +z).
        let (_, seam) = camera_arrow(&corner, Vec3::new(0.0, 0.0, 100.0), Vec3::X, Vec3::X);
        assert!((seam - PI * 0.5).abs() < 0.05, "{seam}");
    }

    #[test]
    fn the_terrain_picture_follows_the_world_inside_the_rounded_shape() {
        let res = 32;
        let mut cube = HeightCube::filled(res, 0.0);
        for face in 0..6 {
            for row in 0..res {
                for col in 0..res {
                    let d = crate::planet_query::texel_dir(face, row, col, res);
                    let i = cube.index(face, row, col);
                    cube.raw_mut()[i] = crate::planet_query::h_to_raw(if d.z < 0.0 { 0.5 } else { -0.5 });
                }
            }
        }
        let planet = Planet::new(cube, PLANET_RADIUS, 0.0);
        let look = crate::space_planets::look_for(
            true,
            None,
            None,
            crate::space_planets::Climate { info: [0.0, 1.0], base_water: 0.5, now: (0.5, 0.5), t_score: 3 },
        );
        let corner = Corner::from_mask(64, 64, &mask());
        let rgba = bake_terrain(&planet, &look, &corner);
        let at = |x: usize, y: usize| &rgba[(y * MAP_W as usize + x) * 4..][..4];
        // The top-left corner is outside the shape, the middle is inside.
        assert_eq!(at(1, 1)[3], 0);
        assert_eq!(at(MAP_W as usize / 2, MAP_H as usize / 2)[3], 255);
        // The reference half (-z, u 0.25..0.75) is land, the other half sea: different colours.
        let land = at(MAP_W as usize / 2, MAP_H as usize / 2);
        let sea = at(MAP_W as usize / 16, MAP_H as usize / 2);
        assert_ne!(land[..3], sea[..3]);
    }

    #[test]
    fn a_click_inside_the_shape_is_the_direction_of_the_map_position() {
        let corner = Corner::from_mask(64, 64, &mask());
        let (w, h) = (MAP_W as f32, MAP_H as f32);
        // The middle of the picture is the reference direction (-z).
        let middle = direction_at(&corner, Vec2::new(w * 0.5, h * 0.5)).unwrap();
        assert!(middle.distance(Vec3::NEG_Z) < 0.05, "{middle:?}");
        // A quarter of the way in on the equator is +x.
        let quarter = direction_at(&corner, Vec2::new(w * 0.25, h * 0.5)).unwrap();
        assert!(quarter.distance(Vec3::X) < 0.05, "{quarter:?}");
        // The top row is near the pole (+y), through the squeeze.
        let top = direction_at(&corner, Vec2::new(w * 0.5, 0.5)).unwrap();
        assert!(top.y > 0.99, "{top:?}");
        // The transparent corners are not the window.
        assert!(direction_at(&corner, Vec2::new(2.0, 2.0)).is_none());
        // A picture pixel round trips through the squeeze and the projection.
        let want = Vec3::new(0.3, 0.2, -0.8).normalize();
        let back = direction_at(&corner, pixel_of(&corner, want)).unwrap();
        assert!(back.distance(want) < 0.03, "{back:?}");
    }

    #[test]
    fn worlds_are_coloured_by_owner_and_knowledge() {
        let owned = world_color(None, true, true);
        let scanned = world_color(None, false, true);
        let unknown = world_color(None, false, false);
        assert!(owned != scanned && scanned != unknown && owned != unknown);
        // An empire's world carries the empire's colour whatever else is known.
        assert_eq!(world_color(Some(2), false, false), world_color(Some(2), true, true));
        assert_ne!(world_color(Some(2), false, false), world_color(Some(3), false, false));
    }
}
