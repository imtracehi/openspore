//! The planet minimap of the retail Creature and Tribal HUDs.
//!
//! `GlobalUICreature3-7` and `GlobalUITribe3-6` carry the same map panel: the
//! state-holder toggle `0562A0F7` whose child group holds the frame `053F4AD8`,
//! the planet name `0562A2DA`, the Hide toggle `0562A141`, the camera buttons
//! (`04CAB570` zoom in, `04CAB571` zoom out, `04CAB56F` / `04CAB56E` rotate; the
//! retail callout calls them "Zoom and Rotate") and the empty "Minimap holder"
//! window `0190722E`, where the game code draws the map. The picture is painted
//! on the CPU into an image that a plain UI node shows over the holder window,
//! as `civ_hud::world` does for the Civ HUD.
//!
//! ORIGINAL DATA: the markers are the installed icon strips of the minimap
//! (`011C0BF5!minimap-*`, four frames each, frame 0 drawn: the green arrow of
//! the camera, the white house of the player's nest or hut, the question mark
//! of an unexplored nest, the smiling face of a befriended one, the skull of an
//! extinct one, the creature nests, fruit groves and fishing spots, the
//! neighbouring tribes' huts) and the legend text of the retail Spore Guide
//! (`CRG_CreaturePhase` "The Mini Map", `TRG_TribePhase` "The Mini-Map"):
//!
//! * Creature: your home nest, nests you have not explored, nests you have
//!   explored with the relationship to their species, nests of extinct species
//!   and the migration path;
//! * Tribal: fruit groves, creature nests, fishing hot spots, the neighbouring
//!   tribes as huts in their tribal colours (yours is larger), and the tribe
//!   members as dots. Clicking the map sends the camera there, double clicking
//!   cuts the camera there, a right click sends the selection (tribal.md T5,
//!   tribal-hud-notes).
//!
//! On the planet (planet-re 4E step 4) the Creature map shows the planet
//! around the avatar: the stage chart (world x, z; the planet's height field
//! and sea level under each pixel) centred on the avatar, north up.
//! ADAPTED: the map is north-up and flat (the retail map's projection, zoom
//! and rotation are not decoded); which icon frame is which state, the
//! colours of the terrain (the palette of the installed home planet, shaded by
//! height) and the red reticle (`minimap-dot-attack`, shown for epic and rogue
//! creatures, which the retail legend does not list) are not decoded. A nest
//! counts as explored once the avatar has been near it in this session or the
//! player has befriended or defeated one of its creatures (the rewrite keeps no
//! "explored" flag). In Creature a click turns the camera toward the point
//! (the camera is bound to the avatar), in Tribal it moves it.

use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::{
    asset_catalog::ResourceIndex,
    creature_hud::{CreatureHudActive, CreatureHudRoot},
    dbpf::ResourceKey,
    game::{Bush, Player, RunProgress, TribeMate},
    game_audio::{Cue, PlayCue},
    spui::{
        solve::Rect,
        view::{self, SpuiPointer, SpuiState},
    },
    stages::{GameStage, StageOwned},
    tribal_hud::{TribalHudActive, TribalHudRoot},
};

/// Control ids of the map panel (the same in both layouts).
#[allow(dead_code)]
pub mod id {
    pub const TOGGLE: u32 = 0x0562_A0F7;
    pub const NAME: u32 = 0x0562_A2DA;
    pub const HIDE: u32 = 0x0562_A141;
    pub const HOLDER: u32 = 0x0190_722E;
    pub const CAMERA_CLUSTER: u32 = 0x053F_4AE0;
}

/// Size of the picture in pixels (the holder window is 225 x 103 in the
/// Creature layout and 227 x 102 in the Tribal one).
pub const MAP_W: u32 = 226;
pub const MAP_H: u32 = 102;
/// Seconds between repaints of the markers.
const MAP_PERIOD: f32 = 0.08;
/// Frames before the picture is shown (the render world prepares the texture
/// asynchronously).
const SETTLE_FRAMES: u32 = 4;
/// Two clicks within this many seconds are a double click (the camera cuts).
pub const DOUBLE_CLICK_SECS: f64 = 0.4;
/// ADAPTED: a nest is "explored" once the avatar has come this close (a chord;
/// 9 units on the island at a tenth of the scale).
pub const EXPLORE_RADIUS: f32 = 25.0;
/// ADAPTED: the Creature map shows this many world units top to bottom (the
/// casting activation distance, 100, each way and a margin), and follows the
/// avatar in steps of `CREATURE_MAP_STEP` (the terrain is repainted per step).
pub const CREATURE_MAP_SPAN: f32 = 240.0;
pub const CREATURE_MAP_STEP: f32 = 8.0;
/// ADAPTED: the Tribal map shows this many world units top to bottom around
/// the camera's focus (the nearby tribe's ring, at most ~285 from the hut, fits
/// above and below the hut; the retail `miniMapZoomLevel` 0.7 is not decoded),
/// following the focus in steps of `TRIBAL_MAP_STEP`.
pub const TRIBAL_MAP_SPAN: f32 = 640.0;
pub const TRIBAL_MAP_STEP: f32 = 32.0;

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Which part of the world the picture shows: a north-up map (x east, z south)
/// centred on `center` and `span` world units high.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapView {
    pub center: Vec2,
    pub span: f32,
}

impl MapView {
    /// The Creature map before the avatar is known: around the stage pole.
    pub fn creature() -> Self {
        Self { center: Vec2::ZERO, span: CREATURE_MAP_SPAN }
    }

    /// The Creature map around the avatar (world x, z), its centre snapped to
    /// `CREATURE_MAP_STEP`.
    pub fn creature_around(avatar: Vec2) -> Self {
        let center = (avatar / CREATURE_MAP_STEP).round() * CREATURE_MAP_STEP;
        Self { center, span: CREATURE_MAP_SPAN }
    }

    /// The Tribal map around the hut (before the camera is known).
    pub fn tribal() -> Self {
        Self { center: Vec2::ZERO, span: TRIBAL_MAP_SPAN }
    }

    /// The Tribal map around the camera's focus (world x, z), its centre
    /// snapped to `TRIBAL_MAP_STEP`.
    pub fn tribal_around(focus: Vec2) -> Self {
        if !focus.is_finite() {
            return Self::tribal();
        }
        let center = (focus / TRIBAL_MAP_STEP).round() * TRIBAL_MAP_STEP;
        Self { center, span: TRIBAL_MAP_SPAN }
    }

    pub fn for_stage(stage: &GameStage) -> Self {
        match stage {
            GameStage::Tribal => Self::tribal(),
            _ => Self::creature(),
        }
    }

    fn scale(&self) -> f32 {
        MAP_H as f32 / self.span.max(1.0)
    }

    /// Pixel of a world point (x, z).
    pub fn pixel(&self, world: Vec2) -> Vec2 {
        let s = self.scale();
        Vec2::new(
            MAP_W as f32 * 0.5 + (world.x - self.center.x) * s,
            MAP_H as f32 * 0.5 + (world.y - self.center.y) * s,
        )
    }

    /// World point (x, z) under a pixel of the picture.
    pub fn world(&self, pixel: Vec2) -> Vec2 {
        let s = self.scale();
        Vec2::new(
            (pixel.x - MAP_W as f32 * 0.5) / s + self.center.x,
            (pixel.y - MAP_H as f32 * 0.5) / s + self.center.y,
        )
    }
}

/// The pixel of the picture under a screen point, when the point is over the
/// holder window.
pub fn picture_pixel(holder: Rect, point: Vec2) -> Option<Vec2> {
    holder.contains(point.x, point.y).then(|| {
        Vec2::new(
            (point.x - holder.x1) / holder.width().max(1.0) * MAP_W as f32,
            (point.y - holder.y1) / holder.height().max(1.0) * MAP_H as f32,
        )
    })
}

/// Camera azimuth (the convention of `play_camera::CreatureRig`: the camera
/// sits at `(sin az, .., cos az)` from its anchor) that looks along the ground
/// direction `d` (x, z).
pub fn azimuth_toward(d: Vec2) -> f32 {
    (-d.x).atan2(-d.y)
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

/// Marker art: the installed `minimap-*` strips.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    /// The green arrow of the camera.
    Camera,
    /// The player's own nest (Creature) or hut (Tribal): a white house.
    PlayerNest,
    /// A neighbouring tribe's hut: the small white house, tinted.
    Village,
    /// A creature nest: the grey oval.
    Nest,
    Fruit,
    Fish,
    /// The green question mark of an unexplored nest.
    UnknownNest,
    /// The smiling face of a befriended nest.
    BefriendedNest,
    /// The skull of an extinct species' nest.
    ExtinctNest,
    /// A creature or tribe member: a white dot, tinted.
    Dot,
    /// ADAPTED: the red reticle (`minimap-dot-attack`) for epic and rogue creatures.
    Danger,
}

impl Icon {
    pub const ALL: [Icon; 11] = [
        Icon::Camera,
        Icon::PlayerNest,
        Icon::Village,
        Icon::Nest,
        Icon::Fruit,
        Icon::Fish,
        Icon::UnknownNest,
        Icon::BefriendedNest,
        Icon::ExtinctNest,
        Icon::Dot,
        Icon::Danger,
    ];

    /// Registry name of the PNG and its resource.
    pub fn name(self) -> &'static str {
        match self {
            Icon::Camera => "minimap-camera",
            Icon::PlayerNest => "minimap-player-nest",
            Icon::Village => "minimap-city",
            Icon::Nest => "minimap-nest",
            Icon::Fruit => "minimap-fruit",
            Icon::Fish => "minimap-fish",
            Icon::UnknownNest => "minimap-unknown-nest",
            Icon::BefriendedNest => "minimap-befriended-nest",
            Icon::ExtinctNest => "minimap-extincted-nest",
            Icon::Dot => "minimap-vehicle",
            Icon::Danger => "minimap-dot-attack",
        }
    }

    pub fn key(self) -> ResourceKey {
        let group_id = match self {
            Icon::Danger => 0x011C_0C1C,
            _ => crate::ui_art::GLOBAL_UI_GROUP,
        };
        ResourceKey {
            type_id: crate::ui_art::UI_PNG_TYPE,
            group_id,
            instance_id: crate::world_objects::name_id(self.name()),
        }
    }

    /// Frames of the strip: the art repeats one icon in four states (frame 0
    /// is the plain one); the arrow and the reticle are single pictures.
    pub fn frames(self) -> u32 {
        match self {
            Icon::Camera | Icon::Danger => 1,
            _ => 4,
        }
    }

    /// Draw order (low first); the camera arrow goes over everything.
    fn layer(self) -> u8 {
        match self {
            Icon::Fruit | Icon::Fish | Icon::Nest => 0,
            Icon::UnknownNest | Icon::BefriendedNest | Icon::ExtinctNest => 1,
            Icon::Village => 2,
            Icon::PlayerNest => 3,
            Icon::Dot => 4,
            Icon::Danger => 5,
            Icon::Camera => 6,
        }
    }

    /// ADAPTED: the glyphs of fruit groves, fishing spots and creature nests
    /// are translucent white with a dark rim (the game tints them); their fill
    /// is drawn twice as opaque so that it reads on the bright land.
    fn fill_boost(self) -> u32 {
        match self {
            Icon::Fruit | Icon::Fish | Icon::Nest => 2,
            _ => 1,
        }
    }

    /// Colour and half size of the stand-in disc when the art is not
    /// available (no install).
    fn fallback(self) -> ([u8; 3], i32) {
        match self {
            Icon::Camera => ([140, 230, 60], 4),
            Icon::PlayerNest => ([245, 245, 245], 3),
            Icon::Village => ([240, 240, 240], 2),
            Icon::Nest => ([150, 160, 150], 2),
            Icon::Fruit => ([150, 200, 120], 1),
            Icon::Fish => ([150, 190, 200], 1),
            Icon::UnknownNest => ([60, 220, 90], 2),
            Icon::BefriendedNest => ([60, 200, 90], 3),
            Icon::ExtinctNest => ([235, 235, 235], 2),
            Icon::Dot => ([250, 250, 250], 1),
            Icon::Danger => ([240, 40, 40], 4),
        }
    }
}

/// One decoded icon strip.
#[derive(Clone, Debug)]
pub struct Sprite {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    rgba: Vec<u8>,
}

impl Sprite {
    pub fn from_rgba(width: u32, height: u32, frames: u32, rgba: Vec<u8>) -> Option<Self> {
        (rgba.len() == (width * height * 4) as usize && width > 0 && height > 0).then_some(Self {
            width,
            height,
            frames: frames.max(1),
            rgba,
        })
    }

    pub fn from_png(bytes: &[u8], frames: u32) -> Result<Self, String> {
        let (width, height, rgba) = crate::ui_art::decode_png(bytes)?;
        Self::from_rgba(width, height, frames, rgba).ok_or_else(|| "icon size does not match its pixels".to_string())
    }

    /// Width of one frame (the strips are not always an exact multiple: 47
    /// pixels hold four frames of 12).
    pub fn frame_width(&self) -> u32 {
        ((self.width + self.frames / 2) / self.frames).clamp(1, self.width)
    }

    /// Texel of `frame` at (x, y) in frame pixels; transparent outside.
    fn texel(&self, frame: u32, x: i32, y: i32) -> [u8; 4] {
        let fw = self.frame_width() as i32;
        if x < 0 || y < 0 || x >= fw || y >= self.height as i32 {
            return [0; 4];
        }
        let sx = frame.min(self.frames - 1) as i32 * fw + x;
        if sx >= self.width as i32 {
            return [0; 4];
        }
        let at = ((y as u32 * self.width + sx as u32) * 4) as usize;
        [self.rgba[at], self.rgba[at + 1], self.rgba[at + 2], self.rgba[at + 3]]
    }
}

/// The icons of the map, loaded from the install once.
#[derive(Resource, Default)]
pub struct MapSprites {
    sprites: HashMap<Icon, Sprite>,
    loaded: bool,
}

impl MapSprites {
    /// Read every icon of the index (an empty index leaves the stand-ins).
    pub fn load(&mut self, index: &ResourceIndex) {
        if self.loaded || index.records.is_empty() {
            return;
        }
        self.loaded = true;
        for icon in Icon::ALL {
            match index.read(icon.key()).and_then(|bytes| Sprite::from_png(&bytes, icon.frames())) {
                Ok(sprite) => {
                    self.sprites.insert(icon, sprite);
                }
                Err(e) => warn!("minimap icon {}: {e}", icon.name()),
            }
        }
    }

    pub fn get(&self, icon: Icon) -> Option<&Sprite> {
        self.sprites.get(&icon)
    }
}

// ---------------------------------------------------------------------------
// The scene and its painter
// ---------------------------------------------------------------------------

/// One marker: an icon at a world point (x, z), multiplied by `tint`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapMarker {
    pub icon: Icon,
    pub at: Vec2,
    pub tint: [u8; 3],
    /// Size relative to the art.
    pub scale: f32,
}

impl MapMarker {
    pub fn new(icon: Icon, at: Vec2) -> Self {
        Self { icon, at, tint: [255; 3], scale: 1.0 }
    }

    pub fn tinted(mut self, tint: [u8; 3]) -> Self {
        self.tint = tint;
        self
    }

    pub fn scaled(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }
}

/// What the picture shows besides the terrain.
#[derive(Resource, Clone, Debug, Default)]
pub struct MapScene {
    pub view: Option<MapView>,
    pub markers: Vec<MapMarker>,
    /// The migration path: dots along a line (world points).
    pub path: Option<(Vec2, Vec2)>,
    /// The camera: where it stands and the ground direction it looks along.
    pub camera: Option<(Vec2, Vec2)>,
}

fn color_u8(c: [f32; 3]) -> [u8; 3] {
    [(c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8]
}

/// Colour of a Bevy colour for a marker tint.
pub fn tint_of(color: Color) -> [u8; 3] {
    let s = color.to_srgba();
    color_u8([s.red, s.green, s.blue])
}

/// Terrain colours of the picture: water, beach and land shaded by height and
/// relief. ORIGINAL: the palette of the installed home planet (the land colours
/// by elevation, the beach and the water); ADAPTED: the coast follows the
/// island's analytic shore and the relief shading is the rewrite's own.
pub fn paint_terrain(
    view: MapView,
    surface: Option<&crate::terrain::LandSurface>,
    palette: Option<&crate::world_catalog::PlanetPalette>,
) -> Vec<[u8; 3]> {
    if let Some(planet) = surface.and_then(|s| s.planet()) {
        return paint_planet_terrain(view, planet, palette);
    }
    let extent = surface.map_or(42.0, |s| s.extent());
    let seed = surface.map_or(0, |s| s.seed);
    let phase = (seed ^ (seed >> 32)) as u32 as f32 / u32::MAX as f32 * 6.28;
    let water = color_u8(palette.map_or([0.03, 0.14, 0.24], |p| p.water));
    let mut out = Vec::with_capacity((MAP_W * MAP_H) as usize);
    for y in 0..MAP_H {
        for x in 0..MAP_W {
            let p = view.world(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
            let depth = crate::terrain::coast_depth(p, extent, phase);
            let Some(surface) = surface else {
                out.push(water);
                continue;
            };
            if depth < -0.25 {
                // Deeper water darkens a little with the distance from the shore.
                let t = (-depth / 14.0).clamp(0.0, 1.0);
                out.push(water.map(|c| (c as f32 * (1.0 - 0.45 * t)) as u8));
                continue;
            }
            let h = surface.height(p.x, p.y);
            let rise = ((h + 0.6) / 2.0).clamp(0.0, 1.0);
            let land = palette.map_or_else(
                || {
                    let a = rise.clamp(0.10, 0.92);
                    [0.12 + a * 0.15, 0.24 + a * 0.25, 0.10 + a * 0.12]
                },
                |pal| pal.sample(rise),
            );
            let mixed = match palette {
                Some(pal) => {
                    let t = (depth / 1.6).clamp(0.0, 1.0);
                    let beach = 1.0 - t * t * (3.0 - 2.0 * t);
                    std::array::from_fn(|c| land[c] * (1.0 - beach) + pal.beach[c] * beach)
                }
                None => land,
            };
            // Relief: light from the north-west.
            let d = 0.8;
            let gx = surface.height(p.x - d, p.y) - surface.height(p.x + d, p.y);
            let gz = surface.height(p.x, p.y - d) - surface.height(p.x, p.y + d);
            let shade = (1.0 + (gx + gz) * 0.35).clamp(0.78, 1.22);
            out.push(color_u8(mixed.map(|c| c * shade)));
        }
    }
    out
}

/// Terrain colours of the planet under the view. ORIGINAL: the stage planet's
/// height field and sea level (`IsInWater`), the beach band (`mBeachLevel`)
/// and the home planet's palette; ADAPTED: the elevation ramp (0.6 of the
/// altitude range, as the land shader), the depth shading and the relief
/// light. Beyond the planet's limb the picture is dark.
fn paint_planet_terrain(
    view: MapView,
    planet: &crate::planet_query::StagePlanet,
    palette: Option<&crate::world_catalog::PlanetPalette>,
) -> Vec<[u8; 3]> {
    let water = color_u8(palette.map_or([0.03, 0.14, 0.24], |p| p.water));
    let (w, h) = (MAP_W as usize, MAP_H as usize);
    let centre = planet.frame.centre;
    let sea = planet.planet.water_radius();
    let range = planet.planet.altitude_range;
    // Elevation above the sea at every pixel corner, along the direction of
    // the sea-level point on the vertical line (NaN past the limb).
    let mut elevation = vec![f32::NAN; (w + 1) * (h + 1)];
    for y in 0..=h {
        for x in 0..=w {
            let p = view.world(Vec2::new(x as f32, y as f32));
            let (dx, dz) = (p.x - centre.x, p.y - centre.z);
            let d2 = dx * dx + dz * dz;
            if d2 < sea * sea {
                let dir = Vec3::new(dx, (sea * sea - d2).sqrt(), dz) / sea;
                elevation[y * (w + 1) + x] = planet.planet.height(planet.frame.dir_to_local(dir)) - sea;
            }
        }
    }
    let units_per_pixel = view.span / MAP_H as f32;
    let at = |x: usize, y: usize| elevation[y * (w + 1) + x];
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let corners = [at(x, y), at(x + 1, y), at(x, y + 1), at(x + 1, y + 1)];
            if corners.iter().any(|e| e.is_nan()) {
                out.push([8, 10, 16]);
                continue;
            }
            let e = corners.iter().sum::<f32>() * 0.25;
            if e < 0.0 {
                // Deeper water darkens.
                let t = (-e / 30.0).clamp(0.0, 1.0);
                out.push(water.map(|c| (c as f32 * (1.0 - 0.45 * t)) as u8));
                continue;
            }
            let rise = (e / (0.6 * range)).clamp(0.0, 1.0);
            let land = palette.map_or_else(
                || {
                    let a = rise.clamp(0.10, 0.92);
                    [0.12 + a * 0.15, 0.24 + a * 0.25, 0.10 + a * 0.12]
                },
                |pal| pal.sample(rise),
            );
            let mixed = match palette {
                Some(pal) => {
                    let t = (e / (crate::planet_query::BEACH_LEVEL * range)).clamp(0.0, 1.0);
                    let beach = 1.0 - t * t * (3.0 - 2.0 * t);
                    std::array::from_fn(|c| land[c] * (1.0 - beach) + pal.beach[c] * beach)
                }
                None => land,
            };
            // Relief: light from the north-west.
            let gx = (corners[0] + corners[2]) - (corners[1] + corners[3]);
            let gz = (corners[0] + corners[1]) - (corners[2] + corners[3]);
            let shade = (1.0 + (gx + gz) / units_per_pixel * 0.35).clamp(0.78, 1.22);
            out.push(color_u8(mixed.map(|c| c * shade)));
        }
    }
    out
}

/// Alpha-blend `rgb` with coverage `alpha` over a pixel of the canvas.
fn put(canvas: &mut [u8], x: i32, y: i32, rgb: [u8; 3], alpha: u8) {
    if x < 0 || y < 0 || x >= MAP_W as i32 || y >= MAP_H as i32 || alpha == 0 {
        return;
    }
    let at = ((y as u32 * MAP_W + x as u32) * 4) as usize;
    let a = alpha as u32;
    for c in 0..3 {
        canvas[at + c] = ((rgb[c] as u32 * a + canvas[at + c] as u32 * (255 - a)) / 255) as u8;
    }
    canvas[at + 3] = 255;
}

/// Draw a frame of an icon centred at a pixel, turned by `angle` (radians,
/// clockwise on screen) and scaled, multiplied by `tint`. Without the art a
/// disc of the icon's stand-in colour is drawn.
#[allow(clippy::too_many_arguments)]
pub fn blit(
    canvas: &mut [u8],
    sprite: Option<&Sprite>,
    icon: Icon,
    frame: u32,
    center: Vec2,
    scale: f32,
    angle: f32,
    tint: [u8; 3],
) {
    let Some(sprite) = sprite else {
        let (rgb, half) = icon.fallback();
        let half = ((half as f32 * scale).round() as i32).max(1);
        let (cx, cy) = (center.x.round() as i32, center.y.round() as i32);
        for dy in -half..=half {
            for dx in -half..=half {
                if dx * dx + dy * dy <= half * half + half {
                    let edge = dx * dx + dy * dy > (half - 1) * (half - 1);
                    let rgb = if edge { [12, 22, 32] } else { rgb.map(|c| (c as u32 * tint[0] as u32 / 255) as u8) };
                    put(canvas, cx + dx, cy + dy, rgb, 255);
                }
            }
        }
        return;
    };
    let fw = sprite.frame_width() as f32;
    let fh = sprite.height as f32;
    let (sin, cos) = angle.sin_cos();
    let radius = 0.5 * (fw * fw + fh * fh).sqrt() * scale + 1.0;
    let (x0, x1) = ((center.x - radius).floor() as i32, (center.x + radius).ceil() as i32);
    let (y0, y1) = ((center.y - radius).floor() as i32, (center.y + radius).ceil() as i32);
    // Plain copies are snapped to whole pixels so that the pixel art stays sharp.
    let plain = angle == 0.0 && scale == 1.0;
    let anchor = if plain {
        Vec2::new((center.x - fw * 0.5).round() + fw * 0.5, (center.y - fh * 0.5).round() + fh * 0.5)
    } else {
        center
    };
    for y in y0..=y1 {
        for x in x0..=x1 {
            let rel = Vec2::new(x as f32 + 0.5 - anchor.x, y as f32 + 0.5 - anchor.y);
            // Back to sprite space: undo the turn, then the scale.
            let src = Vec2::new(rel.x * cos + rel.y * sin, -rel.x * sin + rel.y * cos) / scale
                + Vec2::new(fw * 0.5, fh * 0.5);
            let [r, g, b, a] = sprite.texel(frame, src.x.floor() as i32, src.y.floor() as i32);
            if a == 0 {
                continue;
            }
            let a = (a as u32 * icon.fill_boost()).min(255) as u8;
            let rgb = [
                (r as u32 * tint[0] as u32 / 255) as u8,
                (g as u32 * tint[1] as u32 / 255) as u8,
                (b as u32 * tint[2] as u32 / 255) as u8,
            ];
            put(canvas, x, y, rgb, a);
        }
    }
}

/// Paint the markers of a scene over a copy of the terrain.
pub fn paint_scene(terrain: &[[u8; 3]], scene: &MapScene, sprites: &MapSprites) -> Vec<u8> {
    let mut canvas = vec![255u8; (MAP_W * MAP_H * 4) as usize];
    for (i, rgb) in terrain.iter().enumerate().take((MAP_W * MAP_H) as usize) {
        canvas[i * 4..i * 4 + 3].copy_from_slice(rgb);
    }
    let Some(view) = scene.view else {
        return canvas;
    };
    // The migration path: pale dots every few pixels along the line.
    if let Some((from, to)) = scene.path {
        let (a, b) = (view.pixel(from), view.pixel(to));
        let steps = (a.distance(b) / 5.0).floor().max(1.0) as i32;
        for k in 1..steps {
            let p = a.lerp(b, k as f32 / steps as f32);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                put(&mut canvas, p.x as i32 + dx, p.y as i32 + dy, [236, 224, 255], 235);
            }
        }
    }
    let mut order: Vec<&MapMarker> = scene.markers.iter().collect();
    order.sort_by_key(|m| m.icon.layer());
    for marker in order {
        let at = view.pixel(marker.at);
        blit(&mut canvas, sprites.get(marker.icon), marker.icon, 0, at, marker.scale, 0.0, marker.tint);
    }
    if let Some((at, forward)) = scene.camera {
        // The arrow points up for a camera looking north (-z).
        let angle = forward.x.atan2(-forward.y);
        blit(&mut canvas, sprites.get(Icon::Camera), Icon::Camera, 0, view.pixel(at), 1.0, angle, [255; 3]);
    }
    canvas
}

// ---------------------------------------------------------------------------
// Scenes of the two stages
// ---------------------------------------------------------------------------

/// Nests the avatar has come near in this session (ADAPTED: the rewrite keeps
/// no explored flag; befriended and defeated nests are always known).
#[derive(Resource, Default, Debug)]
pub struct Explored {
    nests: HashSet<usize>,
    seed: u64,
}

impl Explored {
    /// Mark the nests within [`EXPLORE_RADIUS`] (a chord) of the avatar.
    pub fn visit(&mut self, run: &RunProgress, avatar: Vec3) {
        let seed = run.species_world.seed;
        if self.seed != seed {
            self.nests.clear();
            self.seed = seed;
        }
        for (i, nest) in run.species_world.nests.iter().enumerate() {
            if Vec3::from_array(nest.position).distance(avatar) <= EXPLORE_RADIUS {
                self.nests.insert(i);
            }
        }
    }

    pub fn contains(&self, nest: usize) -> bool {
        self.nests.contains(&nest)
    }
}

/// The icon of a nest of the Creature world: extinct species show the skull,
/// befriended ones the smiling face, explored wild ones the creature nest
/// (angry species in red) and the rest the green question mark.
pub fn nest_icon(run: &RunProgress, index: usize, explored: bool) -> (Icon, [u8; 3]) {
    let world = &run.species_world;
    let Some(nest) = world.nests.get(index) else {
        return (Icon::UnknownNest, [255; 3]);
    };
    if nest.surviving() == 0 || world.species_extinct(nest.species) {
        return (Icon::ExtinctNest, [255; 3]);
    }
    let touched = nest.members.iter().any(|m| m.relationship != crate::species::Relationship::Neutral);
    if world.species_allied(nest.species) {
        (Icon::BefriendedNest, [255; 3])
    } else if explored || touched {
        let angry = world.species_aggravated(nest.species);
        (Icon::Nest, if angry { [255, 120, 110] } else { [255; 3] })
    } else {
        (Icon::UnknownNest, [255; 3])
    }
}

/// The Creature scene: your nest, the other nests, the migration path, epic
/// and rogue creatures and the camera at the avatar; on the map around the
/// avatar (`MapView::creature_around`). Nests farther than the map can show
/// (a chord) are left out, so a nest on another side of the planet never
/// lands on the chart.
pub fn creature_scene(
    run: &RunProgress,
    explored: &Explored,
    camera: Option<(Vec3, Vec3)>,
    roamers: impl IntoIterator<Item = (Vec3, crate::species::RoamerKind)>,
) -> MapScene {
    let flat = |p: [f32; 3]| Vec2::new(p[0], p[2]);
    let home = flat(run.creature_lifecycle.home);
    let near = |p: [f32; 3]| camera.is_none_or(|(at, _)| Vec3::from_array(p).distance(at) <= CREATURE_MAP_SPAN * 1.5);
    let mut markers = Vec::new();
    for (i, nest) in run.species_world.nests.iter().enumerate() {
        let at = flat(nest.position);
        if at.distance(home) < 2.0 || !near(nest.position) {
            continue;
        }
        let (icon, tint) = nest_icon(run, i, explored.contains(i));
        markers.push(MapMarker::new(icon, at).tinted(tint));
    }
    markers.push(MapMarker::new(Icon::PlayerNest, home));
    let mut path = None;
    if let Some(migration) = run.creature_progression.pending_migration {
        let to = flat(migration.destination);
        path = Some((flat(migration.origin), to));
        // The new nest is not yet one of the world's (a nest already there has
        // its own marker).
        if !run.species_world.nests.iter().any(|n| flat(n.position).distance(to) < 2.0) {
            markers.push(MapMarker::new(Icon::UnknownNest, to));
        }
    }
    for (at, kind) in roamers {
        let marker = MapMarker::new(Icon::Danger, Vec2::new(at.x, at.z));
        markers.push(match kind {
            crate::species::RoamerKind::Epic => marker.scaled(0.5),
            crate::species::RoamerKind::Rogue => marker.scaled(0.4).tinted([255, 170, 70]),
        });
    }
    MapScene {
        view: Some(camera.map_or_else(MapView::creature, |(at, _)| MapView::creature_around(Vec2::new(at.x, at.z)))),
        markers,
        path,
        camera: camera.map(|(at, forward)| (Vec2::new(at.x, at.z), Vec2::new(forward.x, forward.z))),
    }
}

/// What the Tribal scene is made of (so that it can be built without a world).
#[derive(Default)]
pub struct TribalInputs {
    /// The state of each rival tribe that has come into view (`None`: not yet).
    pub villages: Vec<Option<crate::tribal::RivalStatus>>,
    pub herd_homes: Vec<Vec3>,
    pub bushes: Vec<Vec3>,
    pub fishing: Vec<Vec3>,
    /// Members of the player's tribe and of the rival villages (with theirs).
    pub members: Vec<Vec3>,
    pub rivals: Vec<(Vec3, usize)>,
    pub epics: Vec<Vec3>,
    /// The camera: its focus and the ground direction it looks along.
    pub camera: Option<(Vec3, Vec3)>,
}

/// ADAPTED tints of the fruit groves and the fishing spots (see
/// [`Icon::fill_boost`]).
const FRUIT_TINT: [u8; 3] = [255, 205, 90];
const FISH_TINT: [u8; 3] = [150, 225, 255];

/// The Tribal scene of the retail legend: fruit groves, creature nests,
/// fishing hot spots, the huts of the neighbouring tribes in their colours
/// (yours larger), and the tribe members.
pub fn tribal_scene(inputs: &TribalInputs) -> MapScene {
    let flat = |p: Vec3| Vec2::new(p.x, p.z);
    let mut markers = Vec::new();
    for p in &inputs.bushes {
        markers.push(MapMarker::new(Icon::Fruit, flat(*p)).tinted(FRUIT_TINT));
    }
    for p in &inputs.herd_homes {
        markers.push(MapMarker::new(Icon::Nest, flat(*p)));
    }
    for p in &inputs.fishing {
        markers.push(MapMarker::new(Icon::Fish, flat(*p)).tinted(FISH_TINT));
    }
    for (index, status) in inputs.villages.iter().enumerate() {
        let Some(status) = status else {
            continue;
        };
        let tint = tint_of(crate::tribal::village_tint(*status, index));
        markers.push(MapMarker::new(Icon::Village, flat(crate::tribal::village_position(index))).tinted(tint));
    }
    markers.push(MapMarker::new(Icon::PlayerNest, flat(crate::tribal::home_position())));
    for p in &inputs.members {
        markers.push(MapMarker::new(Icon::Dot, flat(*p)).tinted([255, 240, 150]));
    }
    for (p, village) in &inputs.rivals {
        // A tribe that has not come into view has no dots on the map either.
        let Some(Some(status)) = inputs.villages.get(*village) else {
            continue;
        };
        markers.push(MapMarker::new(Icon::Dot, flat(*p)).tinted(tint_of(crate::tribal::village_tint(*status, *village))));
    }
    for p in &inputs.epics {
        markers.push(MapMarker::new(Icon::Danger, flat(*p)).scaled(0.5));
    }
    MapScene {
        // A focus beyond the chart's hemisphere (the camera circled the planet)
        // has no map position: the map stays at the hut.
        view: Some(
            inputs
                .camera
                .filter(|(at, _)| at.y > -crate::planet_query::PLANET_RADIUS)
                .map_or_else(MapView::tribal, |(at, _)| MapView::tribal_around(flat(at))),
        ),
        markers,
        path: None,
        camera: inputs.camera.map(|(at, forward)| (flat(at), Vec2::new(forward.x, forward.z))),
    }
}

// ---------------------------------------------------------------------------
// Resources and systems
// ---------------------------------------------------------------------------

/// The picture, its UI node and the panel's state.
#[derive(Resource, Default)]
pub struct LandMinimapState {
    image: Option<Handle<Image>>,
    node: Option<Entity>,
    /// Frames the picture has existed.
    seen: u32,
    since_paint: f32,
    terrain: Option<Vec<[u8; 3]>>,
    terrain_key: Option<(u64, u32, bool, bool, (u32, u32, u32), [u32; 4])>,
    /// The Hide toggle is off: the holder and the picture show.
    pub open: bool,
    /// The pointer is over the picture (clicks belong to the map).
    pub over: bool,
    /// The layout root whose Hide toggle was last reset.
    panel_root: Option<Entity>,
    palette: Option<crate::world_catalog::PlanetPalette>,
    palette_tried: bool,
}

/// Plain UI node showing the picture.
#[derive(Component)]
pub struct LandMinimapNode;

fn stage_hud_active(stage: &GameStage, creature: &CreatureHudActive, tribal: &TribalHudActive) -> bool {
    match stage {
        GameStage::Creature => creature.0,
        GameStage::Tribal => tribal.0,
        _ => false,
    }
}

/// Planet name of the panel header. ADAPTED: the home planet of the campaign
/// (the first of the install's planet names, as the Space HUD lists it).
pub fn planet_name(originals: &crate::Originals) -> String {
    crate::space::campaign_world_names(originals).into_iter().next().unwrap_or_else(|| "Homeworld".to_string())
}

/// The panel of the main layout: shown, named, and the Hide toggle decides
/// whether the holder (and so the picture) shows.
pub fn bind_panel(ui: &mut SpuiState, root: Entity, state: &mut LandMinimapState, planet: &str) {
    if let Some(w) = ui.find(id::TOGGLE) {
        ui.set_visible(w, true);
    }
    if let Some(w) = ui.find(id::NAME) {
        ui.set_text(w, planet);
    }
    if let Some(hide) = ui.find(id::HIDE) {
        // The toggle belongs to the layout (a click flips it); a new layout
        // starts with the map shown.
        if state.panel_root != Some(root) {
            state.panel_root = Some(root);
            ui.set_selected(hide, false);
        }
        state.open = ui.states[hide] & crate::spui::ids::state::SELECTED == 0;
    } else {
        state.open = true;
    }
    if let Some(holder) = ui.find(id::HOLDER) {
        ui.set_visible(holder, state.open);
    }
}

fn bind_creature_panel(
    originals: Res<crate::Originals>,
    mut state: ResMut<LandMinimapState>,
    mut roots: Query<(Entity, &mut SpuiState, &CreatureHudRoot)>,
    stage: Res<State<GameStage>>,
) {
    if *stage.get() != GameStage::Creature {
        return;
    }
    let planet = planet_name(&originals);
    for (entity, mut ui, kind) in &mut roots {
        if *kind == CreatureHudRoot::Main && ui.ready {
            bind_panel(&mut ui, entity, &mut state, &planet);
        }
    }
}

fn bind_tribal_panel(
    originals: Res<crate::Originals>,
    mut state: ResMut<LandMinimapState>,
    mut roots: Query<(Entity, &mut SpuiState, &TribalHudRoot)>,
    stage: Res<State<GameStage>>,
) {
    if *stage.get() != GameStage::Tribal {
        return;
    }
    let planet = planet_name(&originals);
    for (entity, mut ui, kind) in &mut roots {
        if *kind == TribalHudRoot::Main && ui.ready {
            bind_panel(&mut ui, entity, &mut state, &planet);
        }
    }
}

/// Collect the Creature scene each frame the HUD is up.
#[allow(clippy::too_many_arguments)]
fn collect_creature(
    stage: Res<State<GameStage>>,
    active: Res<CreatureHudActive>,
    run: Res<RunProgress>,
    camera: Option<Res<crate::play_camera::PlayCamera>>,
    mut explored: ResMut<Explored>,
    mut scene: ResMut<MapScene>,
    player: Query<&Transform, With<Player>>,
    roamers: Query<(&Transform, &crate::epic_creatures::RoamerActor), Without<Player>>,
) {
    if *stage.get() != GameStage::Creature || !active.0 {
        return;
    }
    let avatar = player.single().ok().map(|t| t.translation);
    if let Some(at) = avatar {
        explored.visit(&run, at);
    }
    let looking = avatar.zip(camera.as_deref()).map(|(at, camera)| (at, camera.horizontal_move_basis().0));
    *scene = creature_scene(&run, &explored, looking, roamers.iter().map(|(t, r)| (t.translation, r.kind)));
}

/// Collect the Tribal scene each frame the HUD is up.
#[allow(clippy::too_many_arguments)]
fn collect_tribal(
    stage: Res<State<GameStage>>,
    active: Res<TribalHudActive>,
    run: Res<RunProgress>,
    rts: Option<Res<crate::tribal_rts::TribalRts>>,
    camera: Option<Res<crate::tribal_controls::TribalCamera>>,
    mut scene: ResMut<MapScene>,
    bushes: Query<(&Transform, &Bush), Without<crate::tribal_rts::livestock::EggPile>>,
    fishing: Query<&Transform, With<crate::tribal_rts::FishingSpot>>,
    members: Query<&Transform, With<TribeMate>>,
    player: Query<&Transform, With<Player>>,
    rivals: Query<(&Transform, &crate::tribal_rts::RivalMember)>,
    epics: Query<&Transform, With<crate::tribal_rts::EpicPredator>>,
    stage_layout: Option<Res<crate::tribal_planet::StageLayout>>,
) {
    let _layout = crate::tribal_planet::scope(stage_layout.as_deref());
    if *stage.get() != GameStage::Tribal || !active.0 {
        return;
    }
    let inputs = TribalInputs {
        villages: (0..crate::tribal::RIVAL_VILLAGES)
            .map(|i| run.tribal_campaign.village(i).map(|v| v.status))
            .collect(),
        herd_homes: rts.map(|r| r.herd_homes.clone()).unwrap_or_default(),
        bushes: bushes.iter().filter(|(_, b)| b.food_left > 0.0).map(|(t, _)| t.translation).collect(),
        fishing: fishing.iter().map(|t| t.translation).collect(),
        members: members.iter().chain(player.iter()).map(|t| t.translation).collect(),
        rivals: rivals.iter().map(|(t, m)| (t.translation, m.village)).collect(),
        epics: epics.iter().map(|t| t.translation).collect(),
        camera: camera.as_deref().filter(|c| c.rig.is_ready()).map(|c| (c.rig.focus(), c.rig.ground_axes().0)),
    };
    *scene = tribal_scene(&inputs);
}

/// Repaint the picture and keep its node.
#[allow(clippy::too_many_arguments)]
fn paint(
    mut commands: Commands,
    time: Res<Time>,
    stage: Res<State<GameStage>>,
    creature: Res<CreatureHudActive>,
    tribal: Res<TribalHudActive>,
    originals: Res<crate::Originals>,
    scene: Res<MapScene>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    theme: Option<Res<crate::world_presentation::LandTheme>>,
    mut state: ResMut<LandMinimapState>,
    mut sprites: ResMut<MapSprites>,
    mut images: ResMut<Assets<Image>>,
    nodes: Query<(), With<LandMinimapNode>>,
) {
    // The stage change despawns the node (`StageOwned`): spawn a new one.
    if state.node.is_some_and(|node| !nodes.contains(node)) {
        state.node = None;
        state.seen = 0;
    }
    if !stage_hud_active(stage.get(), &creature, &tribal) {
        for e in [state.node.take()].into_iter().flatten() {
            if let Ok(mut entity) = commands.get_entity(e) {
                entity.despawn();
            }
        }
        state.seen = 0;
        state.panel_root = None;
        return;
    }
    sprites.load(&originals.resource_index);
    let image = state
        .image
        .get_or_insert_with(|| {
            images.add(Image::new(
                Extent3d { width: MAP_W, height: MAP_H, depth_or_array_layers: 1 },
                TextureDimension::D2,
                vec![0; (MAP_W * MAP_H * 4) as usize],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            ))
        })
        .clone();
    if state.node.is_none() {
        state.node = Some(
            commands
                .spawn((
                    LandMinimapNode,
                    Node { position_type: PositionType::Absolute, ..default() },
                    ImageNode::new(image.clone()),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    GlobalZIndex(101),
                    StageOwned,
                ))
                .id(),
        );
    }
    state.since_paint += time.delta_secs();
    if state.since_paint < MAP_PERIOD && state.seen > 0 {
        state.seen = state.seen.saturating_add(1);
        return;
    }
    state.since_paint = 0.0;
    state.seen = state.seen.saturating_add(1);
    // The planet's palette: the theme the terrain uses, else the installed home
    // planet's, read once.
    if theme.is_none() && !state.palette_tried && !originals.resource_index.records.is_empty() {
        state.palette_tried = true;
        state.palette =
            crate::world_presentation::LandTheme::load(&originals.resource_index).ok().map(|t| t.palette);
    }
    let palette = theme.as_deref().map(|t| &t.palette).or(state.palette.as_ref());
    // The scene's view (the Creature map follows the avatar).
    let view = scene.view.unwrap_or_else(|| MapView::for_stage(stage.get()));
    let frame = surface
        .as_ref()
        .and_then(|s| s.planet())
        .map_or([0; 4], |p| p.frame.rotation.to_array().map(f32::to_bits));
    let key = (
        surface.as_ref().map_or(0, |s| s.seed),
        surface.as_ref().map_or(0, |s| s.extent().to_bits()),
        palette.is_some(),
        matches!(stage.get(), GameStage::Tribal),
        (view.center.x.to_bits(), view.center.y.to_bits(), view.span.to_bits()),
        frame,
    );
    if state.terrain_key != Some(key) || state.terrain.is_none() {
        state.terrain = Some(paint_terrain(view, surface.as_deref(), palette));
        state.terrain_key = Some(key);
    }
    let Some(terrain) = state.terrain.as_ref() else {
        return;
    };
    let data = paint_scene(terrain, &scene, &sprites);
    if let Some(mut picture) = images.get_mut(&image) {
        picture.data = Some(data);
    }
}

/// Put the node over the holder window of the HUD and show it while the map is
/// open.
fn place(
    state: Res<LandMinimapState>,
    stage: Res<State<GameStage>>,
    creature: Query<(&SpuiState, &CreatureHudRoot, &Visibility), Without<LandMinimapNode>>,
    tribal: Query<(&SpuiState, &TribalHudRoot, &Visibility), Without<LandMinimapNode>>,
    mut nodes: Query<(&mut Node, &mut Visibility), With<LandMinimapNode>>,
) {
    let Some(node) = state.node else {
        return;
    };
    let Ok((mut n, mut visibility)) = nodes.get_mut(node) else {
        return;
    };
    let window = match stage.get() {
        // The layout is hidden as a whole while a window covers the HUD (the
        // stage timeline): the picture goes with it.
        GameStage::Creature => creature
            .iter()
            .find(|(ui, kind, shown)| **kind == CreatureHudRoot::Main && ui.ready && **shown != Visibility::Hidden)
            .and_then(|(ui, _, _)| ui.find(id::HOLDER).map(|w| (ui.rect(w), ui.screen))),
        GameStage::Tribal => tribal
            .iter()
            .find(|(ui, kind, shown)| **kind == TribalHudRoot::Main && ui.ready && **shown != Visibility::Hidden)
            .and_then(|(ui, _, _)| ui.find(id::HOLDER).map(|w| (ui.rect(w), ui.screen))),
        _ => None,
    };
    let want = match window {
        Some((rect, screen)) if state.seen >= SETTLE_FRAMES && state.open => {
            // Percentages of the full-screen root keep the node on the window at
            // any resolution or UI scale.
            let pct = |v: f32, total: f32| Val::Percent(100.0 * v / total.max(1.0));
            n.left = pct(rect.x1, screen[0]);
            n.top = pct(rect.y1, screen[1]);
            n.width = pct(rect.width(), screen[0]);
            n.height = pct(rect.height(), screen[1]);
            Visibility::Inherited
        }
        _ => Visibility::Hidden,
    };
    if *visibility != want {
        *visibility = want;
    }
}

/// The holder window of a main layout while the map shows.
fn holder_rect(ui: &SpuiState, open: bool) -> Option<Rect> {
    if !open || !ui.ready {
        return None;
    }
    ui.find(id::HOLDER).filter(|w| ui.visible(*w)).map(|w| ui.rect(w))
}

/// The pointer over the picture: the press belongs to the map, not to the
/// world behind it. Returns the world point under the pointer.
fn read_pointer(
    pointer: &mut SpuiPointer,
    state: &mut LandMinimapState,
    view: MapView,
    holder: Option<Rect>,
) -> Option<Vec2> {
    let over = holder.zip(pointer.position).and_then(|(rect, p)| picture_pixel(rect, p));
    let on = over.is_some();
    if state.over != on {
        state.over = on;
    }
    pointer.over_ui |= on;
    over.map(|pixel| view.world(pixel))
}

/// Creature: a click or drag on the map turns the camera toward the point
/// (ADAPTED: the retail camera follows the avatar, so there is nothing to
/// pan).
#[allow(clippy::too_many_arguments)]
fn creature_pointer(
    mut pointer: ResMut<SpuiPointer>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut state: ResMut<LandMinimapState>,
    roots: Query<(&SpuiState, &CreatureHudRoot)>,
    player: Query<&Transform, With<Player>>,
    camera: Option<ResMut<crate::play_camera::PlayCamera>>,
    scene: Option<Res<MapScene>>,
) {
    let holder = roots
        .iter()
        .find(|(_, kind)| **kind == CreatureHudRoot::Main)
        .and_then(|(ui, _)| holder_rect(ui, state.open));
    let open = state.open;
    let view = scene.and_then(|s| s.view).unwrap_or_else(MapView::creature);
    let Some(point) = read_pointer(&mut pointer, &mut state, view, holder) else {
        return;
    };
    let (Some(mouse), Some(mut camera)) = (mouse.filter(|_| open), camera) else {
        return;
    };
    if !(mouse.just_pressed(MouseButton::Left) || mouse.pressed(MouseButton::Left)) {
        return;
    }
    let Ok(avatar) = player.single().map(|t| Vec2::new(t.translation.x, t.translation.z)) else {
        return;
    };
    let to = point - avatar;
    if to.length() > 0.5 {
        camera.rig_mut().turn_to(azimuth_toward(to));
    }
}

/// Tribal: a click sends the camera to the point, a double click cuts it
/// there, a right click sends the selection (`TribalControls::goto_request` /
/// `move_request`).
#[allow(clippy::too_many_arguments)]
fn tribal_pointer(
    time: Res<Time>,
    mut pointer: ResMut<SpuiPointer>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut state: ResMut<LandMinimapState>,
    roots: Query<(&SpuiState, &TribalHudRoot)>,
    surface: Option<Res<crate::terrain::LandSurface>>,
    mut controls: ResMut<crate::tribal_controls::TribalControls>,
    mut camera: ResMut<crate::tribal_controls::TribalCamera>,
    mut last_click: Local<Option<f64>>,
    scene: Option<Res<MapScene>>,
) {
    let holder = roots
        .iter()
        .find(|(_, kind)| **kind == TribalHudRoot::Main)
        .and_then(|(ui, _)| holder_rect(ui, state.open));
    // The picture's own view (it follows the camera).
    let view = scene.and_then(|s| s.view).unwrap_or_else(MapView::tribal);
    let Some(point) = read_pointer(&mut pointer, &mut state, view, holder) else {
        return;
    };
    let Some(mouse) = mouse else {
        return;
    };
    let ground = surface.as_ref().map_or(0.0, |s| s.height(point.x, point.y));
    let at = Vec3::new(point.x, ground, point.y);
    let now = time.elapsed_secs_f64();
    if mouse.just_pressed(MouseButton::Left) {
        if last_click.is_some_and(|before| now - before <= DOUBLE_CLICK_SECS) {
            // Double click: the camera cuts there.
            camera.rig.place(at);
            controls.follow = None;
            *last_click = None;
        } else {
            controls.goto_request = Some(at);
            *last_click = Some(now);
        }
    } else if mouse.pressed(MouseButton::Left) {
        // Dragging over the map keeps the camera moving with the pointer.
        controls.goto_request = Some(at);
    }
    if mouse.just_pressed(MouseButton::Right) {
        controls.move_request = Some(at);
    }
}

/// Creature: the four camera buttons of the panel (held) zoom and turn the
/// camera, as in the Tribal HUD (`ui_camera_zoom` when one goes down).
#[allow(clippy::too_many_arguments)]
fn creature_camera_buttons(
    time: Res<Time>,
    roots: Query<(&SpuiState, &CreatureHudRoot)>,
    camera: Option<ResMut<crate::play_camera::PlayCamera>>,
    mut cues: MessageWriter<PlayCue>,
    mut was_held: Local<bool>,
) {
    let held = roots
        .iter()
        .find(|(ui, kind)| **kind == CreatureHudRoot::Main && ui.ready)
        .map(|(ui, _)| crate::tribal_hud::held_camera_buttons(ui))
        .unwrap_or_default();
    let any = held.zoom_in || held.zoom_out || held.rotate_left || held.rotate_right;
    if any && !*was_held {
        cues.write(PlayCue(Cue::CameraZoom));
    }
    *was_held = any;
    let Some(mut camera) = camera else {
        return;
    };
    let axis = |positive: bool, negative: bool| match (positive, negative) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    };
    let dt = time.delta_secs();
    let zoom = axis(held.zoom_in, held.zoom_out);
    let rotate = axis(held.rotate_right, held.rotate_left);
    if zoom != 0.0 {
        camera.rig_mut().zoom_keys(zoom, dt);
    }
    if rotate != 0.0 {
        camera.rig_mut().rotate_keys(rotate, dt);
    }
}

/// Forget the explored nests when a new Creature stage starts.
fn reset_explored(mut explored: ResMut<Explored>) {
    explored.nests.clear();
}

pub struct LandMinimapPlugin;

impl Plugin for LandMinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LandMinimapState>()
            .init_resource::<MapSprites>()
            .init_resource::<MapScene>()
            .init_resource::<Explored>()
            .add_systems(OnEnter(GameStage::Creature), reset_explored)
            // The panel's frame, name and Hide toggle are bound with the layout.
            .add_systems(
                Update,
                (bind_creature_panel, bind_tribal_panel).before(view::sync).after(view::input),
            )
            // The picture follows the laid-out holder window.
            .add_systems(
                Update,
                (collect_creature, collect_tribal, paint, place).chain().after(view::sync),
            )
            // Clicks on the map are read before the HUD and the controls see the
            // press (`over_ui` keeps the world from acting on it).
            .add_systems(
                Update,
                (creature_pointer, creature_camera_buttons)
                    .chain()
                    .in_set(crate::GameSystems::Controls)
                    .before(crate::player_hud::actions)
                    .run_if(in_state(GameStage::Creature))
                    .run_if(crate::editor::simulation_active)
                    .run_if(crate::mechanical_creator::simulation_active)
                    .run_if(crate::pause_menu::simulation_active),
            )
            .add_systems(
                Update,
                tribal_pointer
                    .in_set(crate::GameSystems::Controls)
                    .before(crate::tribal_hud::swallow_world_clicks)
                    .run_if(in_state(GameStage::Tribal))
                    .run_if(crate::editor::simulation_active)
                    .run_if(crate::mechanical_creator::simulation_active)
                    .run_if(crate::pause_menu::simulation_active),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        species::{Member, Nest, Relationship, RoamerKind},
        spui::solve::RootPolicy,
    };

    fn nest(species: u32, at: [f32; 3], relations: &[Relationship]) -> Nest {
        Nest {
            species,
            position: at,
            members: relations
                .iter()
                .enumerate()
                .map(|(i, r)| Member { relationship: *r, ..Member::new(species * 10 + i as u32) })
                .collect(),
            rewarded: false,
            herd_size: 0,
            respawn_timer: 0.0,
        }
    }

    #[test]
    fn icons_are_the_installed_minimap_strips() {
        // The registry names hash to the instance ids the install lists
        // (`findart.py minimap`: the group 011C0BF5 holds the in-game strips).
        let expected = [
            (Icon::Camera, 0xDC03_65BE),
            (Icon::PlayerNest, 0xBFA9_0B2B),
            (Icon::Village, 0xB408_0F62),
            (Icon::Nest, 0x389B_8725),
            (Icon::Fruit, 0xE9DF_4A0F),
            (Icon::Fish, 0xCA22_A88D),
            (Icon::UnknownNest, 0xCCD1_F01E),
            (Icon::BefriendedNest, 0xB39A_F63E),
            (Icon::ExtinctNest, 0x71AB_D9CA),
            (Icon::Dot, 0x806C_0CA1),
            (Icon::Danger, 0x0BE6_9B39),
        ];
        assert_eq!(expected.len(), Icon::ALL.len());
        for (icon, instance) in expected {
            assert_eq!(icon.key().instance_id, instance, "{}", icon.name());
            assert_eq!(icon.key().type_id, crate::ui_art::UI_PNG_TYPE);
        }
        assert_eq!(Icon::Danger.key().group_id, 0x011C_0C1C);
        assert_eq!(Icon::Fruit.key().group_id, 0x011C_0BF5);
    }

    #[test]
    fn the_installed_icons_decode_into_four_frame_strips() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let mut sprites = MapSprites::default();
        sprites.load(&index);
        for icon in Icon::ALL {
            let sprite = sprites.get(icon).unwrap_or_else(|| panic!("{} is installed", icon.name()));
            assert_eq!(sprite.frames, icon.frames());
            assert!(sprite.frame_width() >= 8 && sprite.frame_width() <= sprite.width, "{}", icon.name());
            // Frame 0 has pixels.
            let fw = sprite.frame_width() as i32;
            assert!(
                (0..sprite.height as i32).any(|y| (0..fw).any(|x| sprite.texel(0, x, y)[3] > 0)),
                "{} frame 0 is empty",
                icon.name()
            );
        }
        // 47 pixels hold four frames of 12; the house is 14 wide.
        assert_eq!(sprites.get(Icon::UnknownNest).unwrap().frame_width(), 12);
        assert_eq!(sprites.get(Icon::PlayerNest).unwrap().frame_width(), 14);
        assert_eq!(sprites.get(Icon::Camera).unwrap().frame_width(), 17);
    }

    #[test]
    fn the_view_is_a_north_up_map_centred_on_the_island() {
        for view in [MapView::creature(), MapView::tribal()] {
            let centre = view.pixel(view.center);
            assert!((centre - Vec2::new(MAP_W as f32 * 0.5, MAP_H as f32 * 0.5)).length() < 1e-4);
            // North (-z) is up, east (+x) is right.
            assert!(view.pixel(Vec2::new(0.0, -10.0)).y < centre.y);
            assert!(view.pixel(Vec2::new(10.0, 0.0)).x > centre.x);
            for world in [Vec2::new(3.5, -7.25), Vec2::new(-20.0, 15.0)] {
                assert!((view.world(view.pixel(world)) - world).length() < 1e-3);
            }
        }
        // The casting activation distance fits the picture's height.
        let half = crate::creature_ecology::casting_thresholds().0;
        let creature = MapView::creature();
        assert!(creature.pixel(Vec2::new(half, half)).y < MAP_H as f32 - 4.0);
        assert!(creature.pixel(Vec2::new(-half, -half)).y > 4.0);
        // Around the hut the nearby tribe's ring (225 from the origin, which
        // is at most 60 from the hut) fits the picture's height.
        let half = crate::tribal_rules::NEARBY_NPC_TRIBE_MARKER.distance_from_origin.1 + 60.0;
        let tribal = MapView::tribal();
        assert!(tribal.pixel(Vec2::new(half, half)).y < MAP_H as f32 - 4.0);
        assert!(tribal.pixel(Vec2::new(-half, -half)).y > 4.0);
        assert_eq!(MapView::tribal_around(Vec2::new(40.0, -17.0)).center, Vec2::new(32.0, -32.0));
        assert_eq!(MapView::for_stage(&GameStage::Tribal), tribal);
        assert_eq!(MapView::for_stage(&GameStage::Creature), creature);
    }

    #[test]
    fn the_holder_window_maps_onto_the_picture() {
        let holder = Rect::new(0.0, 747.0, 225.0, 850.0);
        let centre = picture_pixel(holder, Vec2::new(112.5, 798.5)).unwrap();
        assert!((centre - Vec2::new(MAP_W as f32 * 0.5, MAP_H as f32 * 0.5)).length() < 1e-3);
        let corner = picture_pixel(holder, Vec2::new(0.0, 747.0)).unwrap();
        assert_eq!(corner, Vec2::ZERO);
        assert!(picture_pixel(holder, Vec2::new(300.0, 800.0)).is_none());
        assert!(picture_pixel(holder, Vec2::new(100.0, 700.0)).is_none());
    }

    #[test]
    fn the_camera_azimuth_looks_toward_the_point() {
        // The Creature camera sits at (sin az, cos az) from its anchor and looks
        // the other way: `horizontal_move_basis` forward is (-sin az, -cos az).
        for d in [Vec2::new(0.0, -1.0), Vec2::new(1.0, 0.0), Vec2::new(-3.0, 4.0), Vec2::new(2.0, 2.0)] {
            let az = azimuth_toward(d);
            let forward = Vec2::new(-az.sin(), -az.cos());
            assert!((forward - d.normalize()).length() < 1e-5, "{d:?}");
        }
        assert!(azimuth_toward(Vec2::new(0.0, -1.0)).abs() < 1e-6, "north is the default view");
    }

    #[test]
    fn terrain_is_water_beyond_the_shore_and_land_on_the_island() {
        let surface = crate::terrain::LandSurface::generate(GameStage::Creature, 123, &[]);
        let view = MapView::creature();
        let terrain = paint_terrain(view, Some(&surface), None);
        assert_eq!(terrain.len(), (MAP_W * MAP_H) as usize);
        let at = |p: Vec2| {
            let px = view.pixel(p);
            terrain[(px.y as u32 * MAP_W + px.x as u32) as usize]
        };
        let water = color_u8([0.03, 0.14, 0.24]);
        assert_ne!(at(Vec2::ZERO), water, "the middle of the island is land");
        // Far out is open water: the sea colour, darker the farther from the shore.
        let deep = at(Vec2::new(-80.0, 0.0));
        assert!(deep.iter().zip(water).all(|(d, w)| d <= &w && *d as f32 >= w as f32 * 0.4), "{deep:?}");
        assert!(deep[2] > deep[0], "blue water");
        // Without a surface everything is water.
        assert!(paint_terrain(view, None, None).iter().all(|c| *c == water));
    }

    #[test]
    fn the_creature_map_paints_the_planet_around_the_avatar() {
        let surface = crate::creature_planet::tests::island_surface(200.0);
        let view = MapView::creature_around(Vec2::new(150.0, 0.0));
        let terrain = paint_terrain(view, Some(&surface), None);
        assert_eq!(terrain.len(), (MAP_W * MAP_H) as usize);
        let at = |p: Vec2| {
            let px = view.pixel(p);
            terrain[(px.y as u32 * MAP_W + px.x as u32) as usize]
        };
        let water = color_u8([0.03, 0.14, 0.24]);
        assert_ne!(at(Vec2::new(120.0, 0.0)), water, "the land west of the avatar");
        let sea = at(Vec2::new(260.0, 0.0));
        assert!(sea[2] > sea[0] && sea.iter().zip(water).all(|(s, w)| s <= &w), "the sea east of it: {sea:?}");
        // A nest on the far side of the planet is not drawn on the chart.
        let mut run = RunProgress::default();
        run.species_world.nests = vec![nest(2, [12.0, -900.0, -5.0], &[Relationship::Neutral])];
        let scene = creature_scene(&run, &Explored::default(), Some((Vec3::new(10.0, 0.0, 0.0), Vec3::NEG_Z)), []);
        assert!(scene.markers.iter().all(|m| m.icon == Icon::PlayerNest));
    }

    #[test]
    fn a_marker_is_drawn_tinted_and_clipped_without_art() {
        let mut canvas = vec![0u8; (MAP_W * MAP_H * 4) as usize];
        // The stand-in disc of the reticle: its colour appears at the middle.
        blit(&mut canvas, None, Icon::Danger, 0, Vec2::new(50.0, 40.0), 1.0, 0.0, [255; 3]);
        let at = ((40 * MAP_W + 50) * 4) as usize;
        assert_eq!(&canvas[at..at + 4], &[240, 40, 40, 255]);
        // Off the picture nothing is drawn and nothing panics.
        blit(&mut canvas, None, Icon::Danger, 0, Vec2::new(-30.0, -30.0), 1.0, 0.0, [255; 3]);
        blit(&mut canvas, None, Icon::Danger, 0, Vec2::new(1000.0, 90.0), 1.0, 0.0, [255; 3]);
    }

    #[test]
    fn a_sprite_frame_is_copied_scaled_turned_and_tinted() {
        // A 6 x 2 strip of two 3 x 2 frames: frame 0 red, frame 1 blue, the
        // top-left texel of frame 0 white.
        let mut rgba = Vec::new();
        for _y in 0..2 {
            for x in 0..6 {
                rgba.extend_from_slice(if x < 3 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
            }
        }
        rgba[0..4].copy_from_slice(&[255, 255, 255, 255]);
        let sprite = Sprite::from_rgba(6, 2, 2, rgba).unwrap();
        assert_eq!(sprite.frame_width(), 3);
        assert_eq!(sprite.texel(1, 0, 0), [0, 0, 255, 255]);
        assert_eq!(sprite.texel(0, 3, 0), [0; 4], "outside the frame is clear");
        let read = |canvas: &[u8], x: u32, y: u32| {
            let at = ((y * MAP_W + x) * 4) as usize;
            [canvas[at], canvas[at + 1], canvas[at + 2]]
        };
        let blank = || vec![0u8; (MAP_W * MAP_H * 4) as usize];
        // Plain copy: 3 x 2 pixels around the centre (10, 10) snapped to the
        // pixel grid: x 9..12, y 9..11.
        let mut canvas = blank();
        blit(&mut canvas, Some(&sprite), Icon::Dot, 0, Vec2::new(10.0, 10.0), 1.0, 0.0, [255; 3]);
        assert_eq!(read(&canvas, 9, 9), [255, 255, 255], "top-left texel");
        assert_eq!(read(&canvas, 11, 10), [255, 0, 0]);
        assert_eq!(read(&canvas, 12, 10), [0, 0, 0], "frame 0 is only 3 wide");
        // The second frame, tinted yellow: blue * (255, 255, 0) is black.
        let mut canvas = blank();
        blit(&mut canvas, Some(&sprite), Icon::Dot, 1, Vec2::new(10.0, 10.0), 1.0, 0.0, [255, 255, 0]);
        assert_eq!(read(&canvas, 9, 10), [0, 0, 0]);
        assert_eq!(canvas[((10 * MAP_W + 9) * 4 + 3) as usize], 255, "and it is drawn");
        // Doubled: the strip covers twice the width and height.
        let mut canvas = blank();
        blit(&mut canvas, Some(&sprite), Icon::Dot, 0, Vec2::new(20.0, 20.0), 2.0, 0.0, [255; 3]);
        let lit = (0..MAP_W * MAP_H).filter(|i| canvas[(*i * 4 + 3) as usize] == 255).count();
        assert!((20..=28).contains(&lit), "a 3 x 2 sprite doubled is about 6 x 4 pixels, got {lit}");
        // A quarter turn clockwise moves the white top-left texel to the top-right.
        let mut canvas = blank();
        // (The centre is where the turned 2 x 3 sprite's pixel edges meet.)
        blit(&mut canvas, Some(&sprite), Icon::Dot, 0, Vec2::new(21.0, 20.5), 1.0, std::f32::consts::FRAC_PI_2, [255; 3]);
        let drawn: Vec<(u32, u32)> = (0..MAP_H)
            .flat_map(|y| (0..MAP_W).map(move |x| (x, y)))
            .filter(|(x, y)| canvas[((*y * MAP_W + *x) * 4 + 3) as usize] == 255)
            .collect();
        let white: Vec<(u32, u32)> = drawn.iter().copied().filter(|(x, y)| read(&canvas, *x, *y) == [255, 255, 255]).collect();
        assert_eq!(white.len(), 1, "{drawn:?}");
        let (min_x, max_x) = (drawn.iter().map(|c| c.0).min().unwrap(), drawn.iter().map(|c| c.0).max().unwrap());
        let (min_y, max_y) = (drawn.iter().map(|c| c.1).min().unwrap(), drawn.iter().map(|c| c.1).max().unwrap());
        assert_eq!((max_x - min_x, max_y - min_y), (1, 2), "turned a quarter, the 3 x 2 sprite is 2 wide and 3 high");
        assert_eq!(white[0], (max_x, min_y), "the top-left corner is now top-right");
    }

    #[test]
    fn the_painter_orders_the_layers_and_the_camera_goes_on_top() {
        let view = MapView::creature();
        let at = Vec2::new(5.0, 5.0);
        let scene = MapScene {
            view: Some(view),
            markers: vec![MapMarker::new(Icon::PlayerNest, at), MapMarker::new(Icon::Fruit, at)],
            path: None,
            camera: Some((at, Vec2::new(0.0, -1.0))),
        };
        let terrain = vec![[10, 20, 30]; (MAP_W * MAP_H) as usize];
        let canvas = paint_scene(&terrain, &scene, &MapSprites::default());
        let p = view.pixel(at);
        let pixel = |x: i32, y: i32| {
            let i = ((y as u32 * MAP_W + x as u32) * 4) as usize;
            [canvas[i], canvas[i + 1], canvas[i + 2]]
        };
        // The stand-in arrow (green) covers the others at the middle.
        assert_eq!(pixel(p.x as i32, p.y as i32), [140, 230, 60]);
        // Away from every marker the terrain shows.
        assert_eq!(pixel(1, 1), [10, 20, 30]);
        // A scene without a view paints just the terrain.
        let bare = paint_scene(&terrain, &MapScene::default(), &MapSprites::default());
        assert!(bare.chunks(4).all(|c| c[..3] == [10, 20, 30] && c[3] == 255));
    }

    #[test]
    fn the_migration_path_is_dotted_between_the_nests() {
        let view = MapView::creature();
        let scene = MapScene {
            view: Some(view),
            path: Some((Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0))),
            ..default()
        };
        let terrain = vec![[0, 0, 0]; (MAP_W * MAP_H) as usize];
        let canvas = paint_scene(&terrain, &scene, &MapSprites::default());
        let y = view.pixel(Vec2::ZERO).y as usize;
        let lit: Vec<usize> = (0..MAP_W as usize).filter(|x| canvas[(y * MAP_W as usize + x) * 4] > 100).collect();
        assert!(lit.len() >= 8, "dots along the line: {}", lit.len());
        assert!(lit.len() < 60, "dots, not a solid line");
        assert!(lit.windows(2).any(|w| w[1] - w[0] > 2), "gaps between the dots");
    }

    #[test]
    fn nests_show_what_the_player_knows_of_them() {
        use Relationship::{Allied, Defeated, Neutral};
        let mut run = RunProgress::default();
        run.species_world.nests = vec![
            nest(1, [10.0, 0.0, 10.0], &[Neutral, Neutral]),
            nest(2, [-10.0, 0.0, 10.0], &[Neutral]),
            nest(3, [10.0, 0.0, -10.0], &[Defeated]),
            nest(4, [-10.0, 0.0, -10.0], &[Allied]),
            nest(5, [0.0, 0.0, -20.0], &[Neutral, Defeated]),
        ];
        // Nobody has been near: question marks, except the ones the player
        // has already dealt with.
        assert_eq!(nest_icon(&run, 0, false).0, Icon::UnknownNest);
        assert_eq!(nest_icon(&run, 2, false).0, Icon::ExtinctNest, "every creature gone: the skull");
        assert_eq!(nest_icon(&run, 3, false).0, Icon::BefriendedNest);
        assert_eq!(nest_icon(&run, 4, false).0, Icon::Nest, "one creature of two defeated: explored, wild");
        // Explored wild nests are the grey nest; an angry species is tinted.
        assert_eq!(nest_icon(&run, 0, true), (Icon::Nest, [255; 3]));
        run.species_world.aggravated_species.push(2);
        let angry = nest_icon(&run, 1, true);
        assert_eq!(angry.0, Icon::Nest);
        assert_ne!(angry.1, [255; 3]);
        // Out of range index.
        assert_eq!(nest_icon(&run, 99, true).0, Icon::UnknownNest);
    }

    #[test]
    fn exploring_remembers_the_nests_the_avatar_came_near() {
        let mut run = RunProgress::default();
        run.species_world.seed = 7;
        run.species_world.nests = vec![
            nest(1, [3.0, 0.0, 3.0], &[Relationship::Neutral]),
            nest(2, [30.0, 0.0, 0.0], &[Relationship::Neutral]),
        ];
        let mut explored = Explored::default();
        explored.visit(&run, Vec3::new(0.0, 0.0, 0.0));
        assert!(explored.contains(0) && !explored.contains(1));
        // Walking away keeps what was seen; walking to the far nest adds it.
        explored.visit(&run, Vec3::new(28.0, 0.0, 0.0));
        assert!(explored.contains(0) && explored.contains(1));
        // A new world forgets.
        run.species_world.seed = 8;
        explored.visit(&run, Vec3::new(100.0, 0.0, 100.0));
        assert!(!explored.contains(0) && !explored.contains(1));
    }

    #[test]
    fn the_creature_scene_follows_the_retail_legend() {
        let mut run = RunProgress::default();
        run.creature_lifecycle.home = [0.0, 0.0, 8.0];
        run.species_world.nests = vec![
            // The home nest is a nest of the world too: it is drawn once, as the house.
            nest(1, [0.0, 0.0, 8.0], &[Relationship::Neutral]),
            nest(2, [12.0, 0.0, -5.0], &[Relationship::Neutral]),
        ];
        run.creature_progression.pending_migration = Some(crate::creature_progression::PendingMigration {
            origin: [0.0, 0.0, 8.0],
            destination: [-15.0, 0.0, -10.0],
        });
        let explored = Explored::default();
        let roamers = [(Vec3::new(5.0, 0.0, 5.0), RoamerKind::Epic), (Vec3::new(-5.0, 0.0, 5.0), RoamerKind::Rogue)];
        let camera = Some((Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, -1.0)));
        let scene = creature_scene(&run, &explored, camera, roamers);
        let count = |icon: Icon| scene.markers.iter().filter(|m| m.icon == icon).count();
        assert_eq!(count(Icon::PlayerNest), 1, "your home nest, once");
        assert_eq!(scene.markers.iter().find(|m| m.icon == Icon::PlayerNest).unwrap().at, Vec2::new(0.0, 8.0));
        // One unexplored nest and the migration destination.
        assert_eq!(count(Icon::UnknownNest), 2);
        assert_eq!(count(Icon::Danger), 2);
        assert_eq!(scene.path, Some((Vec2::new(0.0, 8.0), Vec2::new(-15.0, -10.0))));
        assert_eq!(scene.camera, Some((Vec2::new(0.0, 8.0), Vec2::new(0.0, -1.0))));
        // The map follows the avatar.
        assert_eq!(scene.view, Some(MapView::creature_around(Vec2::new(0.0, 8.0))));
        assert_eq!(MapView::creature_around(Vec2::new(0.0, 8.0)).center, Vec2::new(0.0, 8.0));
        assert_eq!(MapView::creature_around(Vec2::new(101.0, -3.0)).center, Vec2::new(104.0, 0.0));
        // The epic is bigger than the rogue; the rogue is orange.
        let danger: Vec<&MapMarker> = scene.markers.iter().filter(|m| m.icon == Icon::Danger).collect();
        assert!(danger[0].scale > danger[1].scale);
        assert_ne!(danger[1].tint, [255; 3]);
        // A destination on an existing nest adds no second marker.
        run.creature_progression.pending_migration = Some(crate::creature_progression::PendingMigration {
            origin: [0.0, 0.0, 8.0],
            destination: [12.0, 0.0, -5.0],
        });
        let same = creature_scene(&run, &explored, None, []);
        assert_eq!(same.markers.iter().filter(|m| m.icon == Icon::UnknownNest).count(), 1);
        assert!(same.path.is_some());
        // Without a migration there is no path.
        run.creature_progression.pending_migration = None;
        let calm = creature_scene(&run, &explored, None, []);
        assert!(calm.path.is_none() && calm.camera.is_none());
        assert_eq!(calm.markers.iter().filter(|m| m.icon == Icon::UnknownNest).count(), 1);
    }

    #[test]
    fn the_tribal_scene_follows_the_retail_legend() {
        use crate::tribal::RivalStatus;
        let inputs = TribalInputs {
            // Only the tribes that have come into view are on the map.
            villages: vec![Some(RivalStatus::Hostile), Some(RivalStatus::Allied), None, None, None],
            herd_homes: vec![Vec3::new(10.0, 0.0, 10.0); 3],
            bushes: vec![Vec3::new(-4.0, 0.0, 6.0); 5],
            fishing: vec![Vec3::new(30.0, 0.0, 0.0); 2],
            members: vec![Vec3::new(1.0, 0.0, 1.0); 4],
            rivals: vec![(Vec3::new(20.0, 0.0, 5.0), 0), (Vec3::new(21.0, 0.0, 5.0), 4)],
            epics: vec![Vec3::new(-20.0, 0.0, -20.0)],
            camera: Some((Vec3::new(2.0, 0.0, 3.0), Vec3::new(1.0, 0.0, 0.0))),
        };
        let scene = tribal_scene(&inputs);
        let of = |icon: Icon| scene.markers.iter().filter(|m| m.icon == icon).collect::<Vec<_>>();
        assert_eq!(of(Icon::Fruit).len(), 5);
        assert_eq!(of(Icon::Nest).len(), 3);
        assert_eq!(of(Icon::Fish).len(), 2);
        assert_eq!(of(Icon::Danger).len(), 1);
        // Two huts in their tribal colours (the allied one lighter), yours at home.
        let huts = of(Icon::Village);
        assert_eq!(huts.len(), 2);
        assert_ne!(huts[0].tint, huts[1].tint);
        let first = crate::tribal::village_position(0);
        assert_eq!(huts[0].at, Vec2::new(first.x, first.z));
        assert_eq!(of(Icon::PlayerNest)[0].at, Vec2::ZERO);
        // Four members of the tribe and one rival dot (the other tribe is not in view).
        assert_eq!(of(Icon::Dot).len(), 5);
        assert_eq!(scene.camera, Some((Vec2::new(2.0, 3.0), Vec2::new(1.0, 0.0))));
        assert_eq!(scene.view, Some(MapView::tribal()));
    }

    // ---- the panel and the pointer, on the installed layouts ------------------

    fn installed_layout(name: &str) -> Option<SpuiState> {
        let install = crate::spore_install::discover().ok()?;
        let index = ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return None;
        }
        let layout = crate::spui::view::SpuiAssets::default().layout(&index, name)?;
        Some(SpuiState::new(layout, RootPolicy::Fit))
    }

    /// A laid-out main layout: the holder window where the layout puts it.
    fn laid_out(name: &str) -> Option<SpuiState> {
        let mut ui = installed_layout(name)?;
        let holder = ui.find(id::HOLDER)?;
        ui.rects[holder] = Rect::new(0.0, 747.0, 225.0, 850.0);
        ui.ready = true;
        Some(ui)
    }

    #[test]
    fn the_installed_layouts_have_the_panel_the_map_uses() {
        for layout in [crate::creature_hud::MAIN_LAYOUT, crate::tribal_hud::MAIN_LAYOUT] {
            let Some(ui) = installed_layout(layout) else {
                return;
            };
            for control in [id::TOGGLE, id::NAME, id::HIDE, id::HOLDER, id::CAMERA_CLUSTER] {
                assert!(ui.find(control).is_some(), "{layout}: {control:08X}");
            }
            for control in crate::tribal_hud::CAMERA_BUTTON_IDS {
                assert!(ui.find(control).is_some(), "{layout}: camera button {control:08X}");
            }
        }
    }

    #[test]
    fn the_panel_is_shown_named_and_hidden_by_its_toggle() {
        let Some(mut ui) = laid_out(crate::creature_hud::MAIN_LAYOUT) else {
            return;
        };
        let mut world = World::new();
        let (root, other) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut state = LandMinimapState::default();
        bind_panel(&mut ui, root, &mut state, "Planet Test");
        let toggle = ui.find(id::TOGGLE).unwrap();
        let name = ui.find(id::NAME).unwrap();
        let holder = ui.find(id::HOLDER).unwrap();
        let hide = ui.find(id::HIDE).unwrap();
        assert!(ui.visible(toggle) && ui.visible(holder));
        assert_eq!(ui.captions[name].as_deref(), Some("Planet Test"));
        assert!(state.open, "a new layout starts with the map shown");
        // A click on the Hide toggle flips it: the holder goes.
        ui.states[hide] ^= crate::spui::ids::state::SELECTED;
        bind_panel(&mut ui, root, &mut state, "Planet Test");
        assert!(!state.open);
        assert!(!ui.visible(holder));
        // And back.
        ui.states[hide] ^= crate::spui::ids::state::SELECTED;
        bind_panel(&mut ui, root, &mut state, "Planet Test");
        assert!(state.open && ui.visible(holder));
        // A new layout (the stage restarted) shows the map again whatever the file says.
        ui.states[hide] |= crate::spui::ids::state::SELECTED;
        bind_panel(&mut ui, other, &mut state, "Planet Test");
        assert!(state.open);
    }

    fn pointer_app(position: Vec2) -> App {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<LandMinimapState>()
            .insert_resource(SpuiPointer { position: Some(position), ..default() });
        app.world_mut().resource_mut::<LandMinimapState>().open = true;
        app
    }

    fn press(app: &mut App, button: MouseButton) {
        // `over_ui` is reset by the SPUI input every frame.
        app.world_mut().resource_mut::<SpuiPointer>().over_ui = false;
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.release(MouseButton::Left);
        mouse.release(MouseButton::Right);
        mouse.clear();
        mouse.press(button);
    }

    #[test]
    fn a_tribal_click_goes_to_the_camera_a_double_click_cuts_a_right_click_sends_the_selection() {
        let Some(ui) = laid_out(crate::tribal_hud::MAIN_LAYOUT) else {
            return;
        };
        // 40 px right of the middle of the holder: east of the island's centre.
        let mut app = pointer_app(Vec2::new(112.5 + 40.0, 798.5));
        app.init_resource::<crate::tribal_controls::TribalControls>()
            .init_resource::<crate::tribal_controls::TribalCamera>()
            .add_systems(Update, tribal_pointer);
        app.world_mut().spawn((ui, TribalHudRoot::Main));
        press(&mut app, MouseButton::Left);
        app.update();
        let world_x = 40.0 * (MAP_W as f32 / 225.0) / (MAP_H as f32 / MapView::tribal().span);
        let goto = app.world().resource::<crate::tribal_controls::TribalControls>().goto_request.expect("goto");
        assert!((goto.x - world_x).abs() < 0.5, "{goto:?} vs {world_x}");
        assert!(goto.z.abs() < 0.5);
        assert!(app.world().resource::<SpuiPointer>().over_ui, "the press belongs to the map");
        assert!(app.world().resource::<LandMinimapState>().over);
        // A second press within the double click time cuts the camera there.
        app.world_mut().resource_mut::<crate::tribal_controls::TribalControls>().goto_request = None;
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(200));
        press(&mut app, MouseButton::Left);
        app.update();
        let controls = app.world().resource::<crate::tribal_controls::TribalControls>();
        assert!(controls.goto_request.is_none(), "a cut is not a glide");
        let camera = app.world().resource::<crate::tribal_controls::TribalCamera>();
        assert!(camera.rig.is_ready() && (camera.rig.focus().x - world_x).abs() < 0.5);
        // A press a long time later is a plain click again.
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs(2));
        press(&mut app, MouseButton::Left);
        app.update();
        assert!(app.world().resource::<crate::tribal_controls::TribalControls>().goto_request.is_some());
        // Right: the selection walks there.
        app.world_mut().resource_mut::<crate::tribal_controls::TribalControls>().goto_request = None;
        press(&mut app, MouseButton::Right);
        app.update();
        let controls = app.world().resource::<crate::tribal_controls::TribalControls>();
        assert!(controls.move_request.is_some() && controls.goto_request.is_none());
    }

    #[test]
    fn a_click_on_the_tribal_map_moves_the_camera_in_the_whole_game() {
        let Some(ui) = laid_out(crate::tribal_hud::MAIN_LAYOUT) else {
            return;
        };
        let mut app = crate::runtime::headless_app(RunProgress::default());
        app.world_mut().resource_mut::<RunProgress>().enter_stage(GameStage::Tribal);
        app.world_mut().resource_mut::<NextState<GameStage>>().set(GameStage::Tribal);
        app.update();
        app.update();
        app.init_resource::<ButtonInput<MouseButton>>();
        app.world_mut().spawn((ui, TribalHudRoot::Main));
        let position = Vec2::new(112.5 + 30.0, 798.5);
        {
            let mut pointer = app.world_mut().resource_mut::<SpuiPointer>();
            pointer.position_override = Some(position);
            pointer.position = Some(position);
        }
        // A frame to bind the panel (the map is open) and settle the camera.
        app.update();
        assert!(app.world().resource::<LandMinimapState>().open);
        let before = app.world().resource::<crate::tribal_controls::TribalCamera>().rig.goal_focus();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        let after = app.world().resource::<crate::tribal_controls::TribalCamera>().rig.goal_focus();
        let east = 30.0 * (MAP_W as f32 / 225.0) / (MAP_H as f32 / MapView::tribal().span);
        assert!((after.x - east).abs() < 1.0 && (after.x - before.x) > 10.0, "{before:?} -> {after:?}, wanted x {east}");
        // (`SpuiPointer::over_ui` is reset by the SPUI input every frame: the
        // map sets it before the controls read it.)
        assert!(app.world().resource::<LandMinimapState>().over, "the click was the map's");
    }

    #[test]
    fn clicks_off_the_map_or_on_a_hidden_map_are_the_worlds() {
        let Some(ui) = laid_out(crate::tribal_hud::MAIN_LAYOUT) else {
            return;
        };
        let mut app = pointer_app(Vec2::new(600.0, 300.0));
        app.init_resource::<crate::tribal_controls::TribalControls>()
            .init_resource::<crate::tribal_controls::TribalCamera>()
            .add_systems(Update, tribal_pointer);
        app.world_mut().spawn((ui, TribalHudRoot::Main));
        press(&mut app, MouseButton::Left);
        app.update();
        let controls = app.world().resource::<crate::tribal_controls::TribalControls>();
        assert!(controls.goto_request.is_none());
        assert!(!app.world().resource::<SpuiPointer>().over_ui);
        // Over the map but hidden by the toggle: still the world's.
        app.world_mut().insert_resource(SpuiPointer { position: Some(Vec2::new(100.0, 800.0)), ..default() });
        app.world_mut().resource_mut::<LandMinimapState>().open = false;
        press(&mut app, MouseButton::Left);
        app.update();
        assert!(app.world().resource::<crate::tribal_controls::TribalControls>().goto_request.is_none());
        assert!(!app.world().resource::<SpuiPointer>().over_ui);
    }

    #[test]
    fn a_creature_click_turns_the_camera_toward_the_point() {
        let Some(ui) = laid_out(crate::creature_hud::MAIN_LAYOUT) else {
            return;
        };
        // 40 px right of the middle: east of the avatar (at the centre).
        let mut app = pointer_app(Vec2::new(112.5 + 40.0, 798.5));
        app.insert_resource(crate::play_camera::PlayCamera::new(GameStage::Creature))
            .add_systems(Update, creature_pointer);
        app.world_mut().spawn((ui, CreatureHudRoot::Main));
        app.world_mut().spawn((Transform::from_xyz(0.0, 0.0, 0.0), Player));
        let before = app.world().resource::<crate::play_camera::PlayCamera>().rig().goal_azimuth();
        assert!(before.abs() < 1e-6);
        press(&mut app, MouseButton::Left);
        app.update();
        let az = app.world().resource::<crate::play_camera::PlayCamera>().rig().goal_azimuth();
        // East: the camera sits west of the avatar and looks east.
        assert!((az - azimuth_toward(Vec2::X)).abs() < 0.1, "{az}");
        assert!(app.world().resource::<SpuiPointer>().over_ui);
        // Not over the map: the camera stays.
        app.world_mut().insert_resource(SpuiPointer { position: Some(Vec2::new(900.0, 300.0)), ..default() });
        app.world_mut().resource_mut::<crate::play_camera::PlayCamera>().rig_mut().turn_to(0.0);
        press(&mut app, MouseButton::Left);
        app.update();
        assert!(app.world().resource::<crate::play_camera::PlayCamera>().rig().goal_azimuth().abs() < 1e-6);
    }

    #[test]
    fn the_creature_camera_buttons_zoom_and_turn_the_camera() {
        use crate::spui::ids::state;
        let Some(mut ui) = laid_out(crate::creature_hud::MAIN_LAYOUT) else {
            return;
        };
        let zoom_in = ui.find(crate::tribal_hud::CAMERA_BUTTON_IDS[0]).unwrap();
        let rotate_right = ui.find(crate::tribal_hud::CAMERA_BUTTON_IDS[3]).unwrap();
        ui.states[zoom_in] |= state::CLICKED;
        let mut app = App::new();
        app.init_resource::<Time>()
            .add_message::<PlayCue>()
            .insert_resource(crate::play_camera::PlayCamera::new(GameStage::Creature))
            .add_systems(Update, creature_camera_buttons);
        let root = app.world_mut().spawn((ui, CreatureHudRoot::Main)).id();
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
        let distance = app.world().resource::<crate::play_camera::PlayCamera>().rig().goal_distance();
        app.update();
        let camera = app.world().resource::<crate::play_camera::PlayCamera>();
        assert!(camera.rig().goal_distance() < distance, "zoom in brings the camera closer");
        assert!(camera.rig().goal_azimuth().abs() < 1e-6);
        // The button going down plays ui_camera_zoom once.
        let cues: Vec<_> = app.world_mut().resource_mut::<Messages<PlayCue>>().drain().collect();
        assert_eq!(cues.len(), 1);
        app.update();
        assert!(app.world_mut().resource_mut::<Messages<PlayCue>>().drain().next().is_none(), "held, not repeated");
        // Release it and hold the right rotate button: the camera swings right.
        let mut ui = app.world_mut().get_mut::<SpuiState>(root).unwrap();
        ui.states[zoom_in] &= !state::CLICKED;
        ui.states[rotate_right] |= state::CLICKED;
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
        app.update();
        assert!(app.world().resource::<crate::play_camera::PlayCamera>().rig().goal_azimuth() > 0.0);
    }
}
