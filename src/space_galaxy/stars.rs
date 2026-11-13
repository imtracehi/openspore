//! The galaxy map's star glow field (helper `space-stars`, `notes/space-stars-notes.md`).
//!
//! Retail draws the stars of the galaxy view as soft glows, one particle per
//! star: `sg_galaxy` (effect directory 0xC1195706, decoded by
//! `tools/stardump.py`) holds, for every star kind, a `subdivide` distribute
//! whose component is a distribute of particles with the same emit map and
//! start index as the matching message distribute of `SG_galaxy_messages~`
//! (so each glow sits on its star): red M 10 x 10,000, yellow G 10 x 6,000,
//! blue O 8 x 16,000, the six binaries 6 x 150..200, protoplanetary disks
//! 6 x 1,500, black holes 6 x 400. The particles use the material
//! `effectStarMaterial` over `SG_Star_Strip` (or the `sg_binary*_strip`
//! rasters): a screen-space sprite. The material's renderer is the builder
//! 0x700000F3 `effectStarShader` of the shader pack 0x40212004!3 (vs_3_0 /
//! ps_3_0; `space-stars2\re\builders.py`), whose fragments (HLSL pack
//! 0x40212002, `space-visuals\re\frag\fragments.txt`) are the vertex stages
//! `effectSetupVF`, `effectTransSetupVF` (`scale = size`), `effectDistanceFadeAdditiveVF`,
//! `effectToNormalClipVF`, `effectAnimateTilesVF`, `effectScreenSpaceAxisClampedVF`,
//! `effectOffsetClipPosVF`, `effectOutputAnimVF` and the pixel stages
//! `effectInputAnimPF`, `effectAnimPS`. (The earlier landing inferred
//! `effectClipTransSetupVF`, `scale = size / 2`: every glow was half the size.)
//!
//! Material registers (VERIFIED data, `MaterialDef` of the effect directory):
//! c0.x = animBlendRate (strip frames per second), c0.y = stripLength,
//! c0.z / c0.w = minScaleRatio / maxScaleRatio (the clamp of the sprite's
//! side in clip units), c1.x / c1.y = fadeStart / fadeEnd (camera
//! distance over which the colour fades out, `effectDistanceFadeAdditiveVF`).
//! With the galaxy camera's near / far (spaceNearClip 0.05, spaceFarClip
//! 10,000, `space_view::NEAR_CLIP`; 0x101BF30 passes the data values for the
//! galaxy context) the perspective term `(1 - z) * size` of the axis is
//! `0.05 size / d`: a glow keeps `minScaleRatio` x the screen height (22-80 px
//! at 900 px) until the camera is nearer than the particle's size (red 12-19
//! units, yellow 19-24, blue 35, binaries 20-35, black hole vortex 50) and
//! then grows as 1 / d up to `maxScaleRatio`. Its colour is lit by the distance
//! fade: red M glows exist within 125 units of the camera, most yellow / blue
//! ones within 200, and the first 1,000 stars of every yellow / blue subdivision
//! (the LOD-independent distributes) out to 900 / 2,500 - the zoomed-out galaxy
//! shows those, the near zoom the rest. Each particle has its own size (a list
//! entry times the `sizeVary` spread) and its HDR colour reaches the pixel stage
//! unclamped (shader model 3).
//!
//! The black holes carry a second particle, the 50-unit vortex (distribute #64,
//! material 0xC11C834C: the same `effectStarMaterial` over the single 64 x 64
//! raster 0x578680C4 (a bright ring with a dark centre), `c0.y` stripLength 1,
//! `c0.z` 0.1 / `c0.w` 20 (so near a black hole the ring grows up to 20 screen
//! heights), fade 40 .. 80 units; size 50 (vary 0.3), colour (1, 1, 1) +- 0.5).
//!
//! The "star objects" of the retail galaxy view are not drawn stars: see
//! `proxies.rs` (invisible `Unit Sphere` hit proxies; `starScaleGraph` scales the
//! solar view's sun, `space-look2-notes` 1). So below radius 80 retail shows only
//! these particles (and `disks.rs`); the sprite stand-in of the earlier landings
//! is gone and only a headless run (no render material) draws it.
//!
//! ADAPTED (marked where used): which stars of a subdivision are the "first
//! 1,000" (the retail emit order is not modelled: the star order within a kind
//! block stands in), the random picks (size / colour / alpha list entries and
//! the `*Vary` spreads use a hash of the star index), the colour map's use
//! (multiplied into the colour of the red / yellow glows). The 0.1-0.15 unit disk sprites of protoplanetary disks /
//! black holes (materials 0x188566c8 `galacticSpinTiltAdditive`, 0xbf4f930a
//! `galacticZpoleTiltAdditive`; they only show inside 100 units of the camera)
//! are drawn by `disks.rs` with the ported galactic vertex stages
//! (`swarm_fx::galactic`).

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

use super::{GalaxyStarLayer, StarKind};
use crate::dbpf::ResourceKey;
use crate::space_worldgen::GalaxyStar;

/// spaceNearClip / spaceFarClip of the galaxy camera (`space_view::NEAR_CLIP` /
/// `FAR_CLIP`, the data values): the retail glow axis depends on them.
#[allow(dead_code)] // the shader carries the values; a test keeps them equal
pub const NEAR_CLIP: f32 = 0.05;
#[allow(dead_code)]
pub const FAR_CLIP: f32 = 10_000.0;

/// The four object scale graphs of `SpaceGalaxyEffect` (02AE0C7E!A0CB680A,
/// VERIFIED data, dumped from the install): (distance, scale) pairs and the
/// scalar companion (`...FromPlanet`) of each. The loader 0x10346F0 fills them
/// in the order planet, moon, gas giant, star (graph ids 0x195F001 / 3 / 7 / 5,
/// scalar ids 0x195F002 / 4 / 8 / 6) and 0x1034640 (graph index, distance,
/// flag) evaluates them: with the flag set and the scalar not 0 the scalar is
/// the scale, otherwise the graph at the distance (clamped at both ends).
/// Callers (all scale the object's model matrix by the result, distance from
/// the camera to the object): 0xC84BA3 (planet objects, camera of the solar
/// or galaxy context) and 0xE97CA8 (`cPlanetGfx`) take the index from 0x1034690
/// (moon 1, gas giant 2, every other planet 0) with the flag `GetCurrentContext()
/// == Planet`; 0xC865EA scales the star object (index 3, flag 0). So the
/// `starScaleGraph` is the size of the star object, and the planets, moons and
/// gas giants of the solar view grow with the camera distance too (the earlier
/// reading that all three callers scale stars was wrong).
pub const PLANET_SCALE_GRAPH: [(f32, f32); 2] = [(25.0, 0.5), (500.0, 1.5)];
pub const MOON_SCALE_GRAPH: [(f32, f32); 2] = [(25.0, 0.35), (500.0, 0.75)];
pub const GAS_GIANT_SCALE_GRAPH: [(f32, f32); 2] = [(25.0, 0.75), (500.0, 1.5)];
pub const STAR_SCALE_GRAPH: [(f32, f32); 2] = [(25.0, 0.95), (500.0, 1.2)];
/// `planetScaleFromPlanet`, `moonScaleFromPlanet`, `gasGiantScaleFromPlanet`,
/// `starScaleFromPlanet`.
pub const SCALE_FROM_PLANET: [f32; 4] = [0.5, 0.4, 1.0, 0.0];

/// The object kinds of the scale graphs, in the retail graph order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleKind {
    Planet = 0,
    Moon = 1,
    GasGiant = 2,
    Star = 3,
}

/// 0x1034640: the scale of an object `distance` (retail units) from the
/// camera. `from_planet` is the `GetCurrentContext() == Planet` flag the planet
/// callers pass (the star caller passes 0).
pub fn scale_graph_value(kind: ScaleKind, distance: f32, from_planet: bool) -> f32 {
    let scalar = SCALE_FROM_PLANET[kind as usize];
    if from_planet && scalar != 0.0 {
        return scalar;
    }
    let graph = match kind {
        ScaleKind::Planet => &PLANET_SCALE_GRAPH,
        ScaleKind::Moon => &MOON_SCALE_GRAPH,
        ScaleKind::GasGiant => &GAS_GIANT_SCALE_GRAPH,
        ScaleKind::Star => &STAR_SCALE_GRAPH,
    };
    super::graph_value(graph, distance)
}

/// `galaxyMaxZoomDustColor` and `galaxyDustEffectLerpMS` (same property list, VERIFIED data). Decoded by
/// `space-stars2` (0x1034D50 / 0x1036950, `notes/space-stars2-notes.md` 3): the dust is not a layer of the galaxy
/// view. `cSpaceGfx` keeps its colour for the two background dust effects of the solar system sky (0x3EAF98C at
/// +0x80, 0x3EBF4B5 at +0x88, shader parameter 5): 0x1036950 samples the colour map {0xAFA8DEB9, 0x03E421ED} at the
/// position of the star the ship arrives at (a 2 s `galaxyDustEffectLerpMS` blend from the previous colour) and
/// 0x1034D50 blends it toward `galaxyMaxZoomDustColor` by the camera distance / 600. The galaxy map therefore takes
/// no dust tint any more; the solar sky effect is not ported (the constants stay for it).
#[allow(dead_code)]
pub const MAX_ZOOM_DUST_COLOR: [f32; 3] = [0.27, 0.2, 0.8];
#[allow(dead_code)]
pub const DUST_LERP_SECONDS: f32 = 2.0;

/// Colour map of the red / yellow glows (`SG_G_type_suns_color`, the distribute's
/// `colorMap`): a 32 x 32 32-bit map over the same +-1000 rectangle as the
/// star emit maps.
const G_TYPE_COLOR_MAP: ResourceKey = ResourceKey {
    group_id: 0,
    instance_id: 0x625B_B6AE,
    type_id: 0x03E4_21ED,
};

/// Which raster strip a population samples (64 x 512: eight 64 x 64 frames).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strip {
    Star,
    BinaryXx,
    BinaryOg,
    BinaryOm,
    BinaryGm,
    /// The black hole vortex: one 64 x 64 raster (0x578680C4, no registered name), `stripLength` 1.
    Vortex,
}

/// The raster of the black hole vortex (distribute #64, material 0xC11C834C texture slot 2).
pub const VORTEX_RASTER: u32 = 0x5786_80C4;

impl Strip {
    pub const ALL: [Strip; 6] =
        [Strip::Star, Strip::BinaryXx, Strip::BinaryOg, Strip::BinaryOm, Strip::BinaryGm, Strip::Vortex];

    pub fn raster(self) -> &'static str {
        match self {
            Strip::Star => "SG_Star_Strip",
            Strip::BinaryXx => "sg_binaryxx_strip",
            Strip::BinaryOg => "sg_binaryog_strip",
            Strip::BinaryOm => "sg_binaryom_strip",
            Strip::BinaryGm => "sg_binarygm_strip",
            Strip::Vortex => "0x578680C4",
        }
    }

    /// The instance id of the raster (a registered name hashes to it; the vortex raster has none).
    pub fn instance(self) -> u32 {
        match self {
            Strip::Vortex => VORTEX_RASTER,
            _ => crate::world_objects::name_id(self.raster()),
        }
    }
}

/// One glow distribute of `sg_galaxy` with its `effectStarMaterial`.
#[allow(dead_code)] // `label` documents the table
#[derive(Clone, Copy, Debug)]
pub struct Population {
    pub label: &'static str,
    pub kind: StarKind,
    /// The stars of the kind repeat in blocks of `period` (the density of the
    /// message distribute); this population covers stars `first..first + count`
    /// of every block.
    pub period: u32,
    pub first: u32,
    pub count: u32,
    /// Distribute `size` list (a random entry per glow) and `sizeVary`.
    pub sizes: &'static [f32],
    pub size_vary: f32,
    /// Distribute `color` list and `colorVary`.
    pub colors: &'static [[f32; 3]],
    pub color_vary: f32,
    pub alpha: f32,
    pub alpha_vary: f32,
    /// The glow takes the colour map of `SG_G_type_suns_color`.
    pub color_map: bool,
    pub strip: Strip,
    /// c0.x, c0.y, c0.z, c0.w, c1.x, c1.y of the material.
    pub anim_rate: f32,
    pub strip_length: f32,
    pub min_scale: f32,
    pub max_scale: f32,
    pub fade_start: f32,
    pub fade_end: f32,
}

const RED: [[f32; 3]; 1] = [[2.0, 0.3, 0.2]];
const YELLOWS: [[f32; 3]; 5] = [
    [1.0, 0.878, 0.62],
    [1.0, 0.78, 0.616],
    [1.0, 0.89, 0.549],
    [1.0, 0.671, 0.478],
    [0.969, 0.851, 0.639],
];
const BLUE: [[f32; 3]; 1] = [[0.8, 0.8, 3.0]];
const WHITE: [[f32; 3]; 1] = [[1.0, 1.0, 1.0]];
const YELLOW_SIZES: [f32; 5] = [20.0, 24.0, 19.0, 20.0, 22.0];

/// The particle distributes of `sg_galaxy` drawn with `effectStarMaterial` (all VERIFIED data except where the
/// module docs say ADAPTED): indices 26, 31, 30, 36, 35, 45-50, 58, 66 (the glows) and 64 (the black hole vortex)
/// of the effect directory's distribute section.
pub const POPULATIONS: [Population; 14] = [
    Population {
        label: "red M (#26, material 1DD9D0D4)",
        kind: StarKind::Red,
        period: 10_000,
        first: 0,
        count: 10_000,
        sizes: &[15.0, 18.0, 16.0, 19.0, 12.0],
        size_vary: 0.0,
        colors: &RED,
        color_vary: 0.05,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: true,
        strip: Strip::Star,
        anim_rate: 2.75,
        strip_length: 8.0,
        min_scale: 0.05,
        max_scale: 0.15,
        fade_start: 10.0,
        fade_end: 125.0,
    },
    Population {
        label: "yellow G first 1000 (#31, material 53013BF1)",
        kind: StarKind::Yellow,
        period: 6_000,
        first: 0,
        count: 1_000,
        sizes: &YELLOW_SIZES,
        size_vary: 0.0,
        colors: &YELLOWS,
        color_vary: 0.25,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: true,
        strip: Strip::Star,
        anim_rate: 3.0,
        strip_length: 8.0,
        min_scale: 0.05,
        max_scale: 0.2,
        fade_start: 75.0,
        fade_end: 900.0,
    },
    Population {
        label: "yellow G rest (#30, material CDE0533E)",
        kind: StarKind::Yellow,
        period: 6_000,
        first: 1_000,
        count: 5_000,
        sizes: &YELLOW_SIZES,
        size_vary: 0.0,
        colors: &YELLOWS,
        color_vary: 0.25,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: true,
        strip: Strip::Star,
        anim_rate: 3.0,
        strip_length: 8.0,
        min_scale: 0.05,
        max_scale: 0.2,
        fade_start: 70.0,
        fade_end: 200.0,
    },
    Population {
        label: "blue O first 1000 (#36, material 4F5EA129)",
        kind: StarKind::Blue,
        period: 16_000,
        first: 0,
        count: 1_000,
        sizes: &[35.0],
        size_vary: 0.0,
        colors: &BLUE,
        color_vary: 0.1,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::Star,
        anim_rate: 2.75,
        strip_length: 8.0,
        min_scale: 0.08,
        max_scale: 0.25,
        fade_start: 50.0,
        fade_end: 2500.0,
    },
    Population {
        label: "blue O rest (#35, material C4839F28)",
        kind: StarKind::Blue,
        period: 16_000,
        first: 1_000,
        count: 15_000,
        sizes: &[35.0],
        size_vary: 0.0,
        colors: &BLUE,
        color_vary: 0.1,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::Star,
        anim_rate: 3.0,
        strip_length: 8.0,
        min_scale: 0.08,
        max_scale: 0.25,
        fade_start: 80.0,
        fade_end: 200.0,
    },
    Population {
        label: "binary OO (#45, material 789FC5B9)",
        kind: StarKind::BinaryOO,
        period: 150,
        first: 0,
        count: 150,
        sizes: &[35.0, 30.0, 32.0],
        size_vary: 0.0,
        colors: &BLUE,
        color_vary: 0.1,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::BinaryXx,
        anim_rate: 2.0,
        strip_length: 8.0,
        min_scale: 0.09,
        max_scale: 0.3,
        fade_start: 50.0,
        fade_end: 2000.0,
    },
    Population {
        label: "binary OM (#46, material DB8F17E1)",
        kind: StarKind::BinaryOM,
        period: 200,
        first: 0,
        count: 200,
        sizes: &[22.0, 24.0, 21.0, 20.0, 22.0],
        size_vary: 0.0,
        colors: &WHITE,
        color_vary: 0.15,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::BinaryOg,
        anim_rate: 1.0,
        strip_length: 8.0,
        min_scale: 0.08,
        max_scale: 0.28,
        fade_start: 130.0,
        fade_end: 530.0,
    },
    Population {
        label: "binary OG (#47, material 16233BC7)",
        kind: StarKind::BinaryOG,
        period: 200,
        first: 0,
        count: 200,
        sizes: &YELLOW_SIZES,
        size_vary: 0.0,
        colors: &WHITE,
        color_vary: 0.15,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::BinaryOm,
        anim_rate: 1.0,
        strip_length: 8.0,
        min_scale: 0.08,
        max_scale: 0.25,
        fade_start: 130.0,
        fade_end: 520.0,
    },
    Population {
        label: "binary GG (#48, material 2DCF47D9)",
        kind: StarKind::BinaryGG,
        period: 200,
        first: 0,
        count: 200,
        sizes: &YELLOW_SIZES,
        size_vary: 0.0,
        colors: &YELLOWS,
        color_vary: 0.25,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::BinaryXx,
        anim_rate: 2.0,
        strip_length: 8.0,
        min_scale: 0.05,
        max_scale: 0.25,
        fade_start: 100.0,
        fade_end: 170.0,
    },
    Population {
        label: "binary GM (#49, material 8F151ACF)",
        kind: StarKind::BinaryGM,
        period: 200,
        first: 0,
        count: 200,
        sizes: &YELLOW_SIZES,
        size_vary: 0.0,
        colors: &YELLOWS,
        color_vary: 0.25,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::BinaryGm,
        anim_rate: 1.0,
        strip_length: 8.0,
        min_scale: 0.05,
        max_scale: 0.2,
        fade_start: 100.0,
        fade_end: 150.0,
    },
    Population {
        label: "binary MM (#50, material 4A1990C1)",
        kind: StarKind::BinaryMM,
        period: 200,
        first: 0,
        count: 200,
        sizes: &[19.0, 22.0, 18.0, 21.0, 17.0],
        size_vary: 0.0,
        colors: &RED,
        color_vary: 0.05,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::BinaryXx,
        anim_rate: 2.0,
        strip_length: 8.0,
        min_scale: 0.05,
        max_scale: 0.15,
        fade_start: 50.0,
        fade_end: 130.0,
    },
    Population {
        label: "protoplanetary disk glow (#58, material 8A7F67E1)",
        kind: StarKind::ProtoPlanetary,
        period: 1_500,
        first: 0,
        count: 1_500,
        sizes: &[18.0, 22.0, 17.0, 15.0, 18.0],
        size_vary: 0.1,
        colors: &[[1.0, 0.3, 0.1]],
        color_vary: 0.0,
        alpha: 0.8,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::Star,
        anim_rate: 0.75,
        strip_length: 8.0,
        min_scale: 0.02,
        max_scale: 0.15,
        fade_start: 20.0,
        fade_end: 200.0,
    },
    Population {
        label: "black hole glow (#66, material 68F5E8FD)",
        kind: StarKind::BlackHole,
        period: 400,
        first: 0,
        count: 400,
        sizes: &[5.0],
        size_vary: 0.2,
        colors: &WHITE,
        color_vary: 0.2,
        alpha: 0.75,
        alpha_vary: 0.2,
        color_map: false,
        strip: Strip::Star,
        anim_rate: 2.75,
        strip_length: 8.0,
        min_scale: 0.2,
        max_scale: 0.5,
        fade_start: 150.0,
        fade_end: 315.0,
    },
    // #64 (material C11C834C): the 50-unit vortex of every black hole (VERIFIED: size 50 vary 0.3, colour (1, 1, 1)
    // vary 0.5, alpha 1, 400 per subdivision from start 400000 on the black hole emit map B6412D00; the texture
    // slot's draw flags 0x23 (light, noFog, user1) and render buffer 3 are not used: the layer is additive).
    Population {
        label: "black hole vortex (#64, material C11C834C)",
        kind: StarKind::BlackHole,
        period: 400,
        first: 0,
        count: 400,
        sizes: &[50.0],
        size_vary: 0.3,
        colors: &WHITE,
        color_vary: 0.5,
        alpha: 1.0,
        alpha_vary: 0.0,
        color_map: false,
        strip: Strip::Vortex,
        anim_rate: 2.75,
        strip_length: 1.0,
        min_scale: 0.1,
        max_scale: 20.0,
        fade_start: 40.0,
        fade_end: 80.0,
    },
];

/// The population index of the black hole vortex (#64).
pub const VORTEX_POPULATION: usize = 13;

/// Number of 4-float rows of the shader table (two per population).
pub const TABLE_ROWS: usize = 32;

/// The shader's per-population table: row 2k = (animBlendRate, stripLength,
/// minScaleRatio, maxScaleRatio), row 2k + 1 = (fadeStart, fadeEnd, the mean
/// authored size, 0). The builder of `effectStarMaterial` (0x700000F3
/// `effectStarShader`) uses `effectTransSetupVF`, `scale = In.normal.w` (the whole
/// size, not `effectClipTransSetupVF`'s half): `space-stars2` notes 7.
pub fn shader_table() -> [Vec4; TABLE_ROWS] {
    let mut table = [Vec4::ZERO; TABLE_ROWS];
    for (i, p) in POPULATIONS.iter().enumerate() {
        let mean = p.sizes.iter().sum::<f32>() / p.sizes.len() as f32;
        table[2 * i] = Vec4::new(p.anim_rate, p.strip_length, p.min_scale, p.max_scale);
        table[2 * i + 1] = Vec4::new(p.fade_start, p.fade_end, mean, 0.0);
    }
    table
}

/// Every population a star takes: its index within the stars of its kind (the
/// generation order) decides which distributes of the subdivision it is in (a
/// black hole is in two: the glow #66 and the vortex #64).
pub fn populations_of(kind: StarKind, index_in_kind: u32) -> impl Iterator<Item = usize> {
    POPULATIONS.iter().enumerate().filter_map(move |(i, p)| {
        let at = index_in_kind % p.period;
        (p.kind == kind && at >= p.first && at < p.first + p.count).then_some(i)
    })
}

/// The first population a star takes (its glow).
#[cfg(test)]
pub fn population_of(kind: StarKind, index_in_kind: u32) -> Option<usize> {
    populations_of(kind, index_in_kind).next()
}

/// `starScaleGraph` at a camera-to-star distance. 0xC84AE0's loop 0xC865EA (`push 3, distance, 0`) scales the
/// `cCelestialBody` of type Star of the **solar system** (its effect and its hit sphere, `[body+0xC0]` /
/// `[body+0xC4]`), not an object of the galaxy view: the galaxy view's star objects are invisible hit proxies
/// (`proxies.rs`).
pub fn star_object_scale(distance: f32) -> f32 {
    scale_graph_value(ScaleKind::Star, distance, false)
}

/// A small integer hash -> [0, 1).
pub(super) fn hash01(index: u32, salt: u32) -> f32 {
    let mut h = index.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// The 32 x 32 `SG_G_type_suns_color` map (BGRA, header 20 bytes).
pub struct ColorMap {
    size: usize,
    rgb: Vec<[f32; 3]>,
}

impl ColorMap {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 20 {
            return None;
        }
        let width = u32::from_be_bytes(bytes[4..8].try_into().ok()?) as usize;
        let height = u32::from_be_bytes(bytes[8..12].try_into().ok()?) as usize;
        if width == 0 || width != height {
            return None;
        }
        let data = bytes.get(20..20 + width * height * 4)?;
        let rgb = data
            .chunks_exact(4)
            .map(|p| [p[2] as f32 / 255.0, p[1] as f32 / 255.0, p[0] as f32 / 255.0])
            .collect();
        Some(Self { size: width, rgb })
    }

    /// Bilinear colour at a galactic position (the map spans +-1000, rows
    /// follow +y like the emit maps).
    pub fn at(&self, position: Vec2) -> [f32; 3] {
        let n = self.size as f32;
        let u = ((position.x / super::GALAXY_MAP_HALF + 1.0) * 0.5 * n - 0.5).clamp(0.0, n - 1.0);
        let v = ((position.y / super::GALAXY_MAP_HALF + 1.0) * 0.5 * n - 0.5).clamp(0.0, n - 1.0);
        let (x0, y0) = (u.floor() as usize, v.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.size - 1), (y0 + 1).min(self.size - 1));
        let (fx, fy) = (u - x0 as f32, v - y0 as f32);
        let at = |x: usize, y: usize| self.rgb[y * self.size + x];
        let mut out = [0.0; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let top = at(x0, y0)[c] * (1.0 - fx) + at(x1, y0)[c] * fx;
            let bottom = at(x0, y1)[c] * (1.0 - fx) + at(x1, y1)[c] * fx;
            *o = top * (1.0 - fy) + bottom * fy;
        }
        out
    }
}

/// Per-star vertex data of one glow.
#[derive(Clone, Copy, Debug)]
pub struct Glow {
    pub position: Vec2,
    pub population: u8,
    /// Strip frame the animation starts at (`id x 255` of the vertex id).
    pub frame: u8,
    /// This particle's size over the population's mean authored size: the
    /// distribute's size list entry and `sizeVary` spread (ADAPTED hash picks).
    pub size: f32,
    /// Colour at full fade, linear-in-value HDR (not clamped before the pixel
    /// stage: the effect programs are vs_3_0 / ps_3_0), alpha last.
    pub color: [f32; 4],
}

/// The glow of one star (`None` when its kind has no glow distribute).
#[cfg(test)]
pub fn glow_of(
    star: &GalaxyStar,
    index: u32,
    index_in_kind: u32,
    map: Option<&ColorMap>,
) -> Option<Glow> {
    let population = population_of(star.kind, index_in_kind)?;
    Some(glow_for(star, index, population, map))
}

/// The particle of one star for one population.
pub fn glow_for(star: &GalaxyStar, index: u32, population: usize, map: Option<&ColorMap>) -> Glow {
    let p = &POPULATIONS[population];
    // ADAPTED: the list picks and the vary spreads are hashes of the star
    // index (the Swarm distribute draws them from its own generator). The
    // second particle of a star (the vortex) draws its own picks.
    let salt = if population >= VORTEX_POPULATION { 64 } else { 0 };
    let pick = (hash01(index, 1 + salt) * p.colors.len() as f32) as usize;
    let base = p.colors[pick.min(p.colors.len() - 1)];
    let mut color = [0.0f32; 4];
    let tint = if p.color_map {
        map.map_or([1.0; 3], |m| m.at(star.position))
    } else {
        [1.0; 3]
    };
    for c in 0..3 {
        let vary = (hash01(index, 2 + c as u32 + salt) * 2.0 - 1.0) * p.color_vary;
        color[c] = ((base[c] + vary) * tint[c]).max(0.0);
    }
    color[3] = (p.alpha * (1.0 + (hash01(index, 6 + salt) * 2.0 - 1.0) * p.alpha_vary)).clamp(0.0, 1.0);
    // The size: a list entry (`size`) times the `sizeVary` spread, over the mean the shader table carries.
    let mean = p.sizes.iter().sum::<f32>() / p.sizes.len() as f32;
    let entry = p.sizes[((hash01(index, 8 + salt) * p.sizes.len() as f32) as usize).min(p.sizes.len() - 1)];
    let size = entry / mean * (1.0 + (hash01(index, 9 + salt) * 2.0 - 1.0) * p.size_vary);
    Glow {
        position: star.position,
        population: population as u8,
        frame: (hash01(index, 7 + salt) * 256.0).min(255.0) as u8,
        size,
        color,
    }
}

/// The shader's `effectStarMaterial`: 32-row population table, (dim, 1, 0, 0),
/// the strip raster.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct StarGlowMaterial {
    #[uniform(0)]
    pub table: [Vec4; TABLE_ROWS],
    #[uniform(1)]
    pub state: Vec4,
    #[texture(2)]
    #[sampler(3)]
    pub strip: Handle<Image>,
}

impl Material for StarGlowMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://openspore/shaders/space_stars.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/space_stars.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// Marks a glow layer (one per strip raster); `dim` follows the map filter.
#[derive(Component)]
pub struct StarGlowLayer;

/// The size factor of a glow is carried as `size / SIZE_CODE` in the fraction of `uv_b.x`.
pub const SIZE_CODE: f32 = 4.0;

/// Four corners per glow; the vertex program places them (screen-space quad).
pub fn glow_mesh(glows: &[Glow]) -> Mesh {
    let n = glows.len() * 4;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(n);
    let mut uv_b: Vec<[f32; 2]> = Vec::with_capacity(n);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(n);
    let mut indices: Vec<u32> = Vec::with_capacity(glows.len() * 6);
    for g in glows {
        let base = positions.len() as u32;
        for corner in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            // On the plane the camera looks at (the galaxy camera's target is at y = 0).
            positions.push([g.position.x, super::STAR_PLANE, g.position.y]);
            uvs.push(corner);
            // The population in the integer part, the size factor / 4 (0 .. 0.5) in the fraction.
            uv_b.push([g.population as f32 + (g.size / SIZE_CODE).clamp(0.0, 0.499), g.frame as f32 / 255.0]);
            colors.push(g.color);
        }
        // Counter-clockwise on screen (u right, v down), as the cloud quads.
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv_b);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Glows of a galaxy grouped by strip raster.
pub fn glows_by_strip(stars: &[GalaxyStar], map: Option<&ColorMap>) -> Vec<(Strip, Vec<Glow>)> {
    let mut groups: Vec<(Strip, Vec<Glow>)> = Strip::ALL.iter().map(|s| (*s, Vec::new())).collect();
    let mut seen = std::collections::HashMap::<StarKind, u32>::new();
    for (i, star) in stars.iter().enumerate() {
        let n = seen.entry(star.kind).or_insert(0);
        let index_in_kind = *n;
        *n += 1;
        for population in populations_of(star.kind, index_in_kind) {
            let glow = glow_for(star, i as u32, population, map);
            let strip = POPULATIONS[population].strip;
            groups.iter_mut().find(|(s, _)| *s == strip).unwrap().1.push(glow);
        }
    }
    groups
}

/// `true` when the glow material is registered (render apps); headless runs
/// keep the sprite layer of the earlier landings.
pub fn available(world: &World) -> bool {
    world.contains_resource::<Assets<StarGlowMaterial>>()
}

/// Spawn the star layers under the galaxy map root: the retail particle field when the glow material is registered
/// and the strip rasters are installed; otherwise (headless runs, a missing raster) only the sprite layer (`legacy`)
/// of the earlier landings. Retail has no star objects to draw (`proxies.rs`), so with the particle field there is
/// no sprite layer at any radius.
pub fn spawn_layers(world: &mut World, root: Entity, stars: std::sync::Arc<Vec<GalaxyStar>>, legacy: Handle<StandardMaterial>) {
    let loaded = if available(world) { load_strips(world) } else { None };
    if loaded.is_none() {
        spawn_legacy(world, root, &stars, legacy);
    }
    // The disk and black-hole sprites of the galactic programs (`disks.rs`); needs the galactic material.
    if world.contains_resource::<Assets<crate::swarm_fx::galactic_draw::GalacticMaterial>>() {
        super::disks::spawn_layer(world, root, &stars);
    }
    let Some((strips, map)) = loaded else {
        return;
    };
    if let Some(mut materials) = world.get_resource_mut::<super::GalaxyMapMaterials>() {
        materials.glows = true;
    }
    let started = std::time::Instant::now();
    let groups = glows_by_strip(&stars, map.as_ref());
    bevy::log::info!(
        "galaxy star glows: {} glows in {} strip layers built in {:?}",
        groups.iter().map(|(_, g)| g.len()).sum::<usize>(),
        groups.iter().filter(|(_, g)| !g.is_empty()).count(),
        started.elapsed()
    );
    for (strip, glows) in groups {
        let Some(image) = strips.iter().find(|(s, _)| *s == strip).map(|(_, i)| i.clone()) else {
            continue;
        };
        if glows.is_empty() {
            continue;
        }
        let mesh = world.resource_mut::<Assets<Mesh>>().add(glow_mesh(&glows));
        let material = world.resource_mut::<Assets<StarGlowMaterial>>().add(StarGlowMaterial {
            table: shader_table(),
            state: Vec4::new(1.0, 1.0, 0.0, 0.0),
            strip: image,
        });
        let layer = world
            .spawn((
                NotShadowCaster,
                GalaxyStarLayer,
                StarGlowLayer,
                Name::new(format!("galaxy star glows ({})", strip.raster())),
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                // The shader moves the corners; keep the layer from being
                // culled by the (point-only) bounds of the mesh.
                bevy::camera::visibility::NoFrustumCulling,
            ))
            .id();
        world.entity_mut(root).add_child(layer);
    }
}

fn load_strips(world: &mut World) -> Option<(Vec<(Strip, Handle<Image>)>, Option<ColorMap>)> {
    let originals = world.get_resource::<crate::Originals>()?;
    let index = &originals.resource_index;
    let mut images = Vec::new();
    for strip in Strip::ALL {
        let key = ResourceKey {
            group_id: 0,
            instance_id: strip.instance(),
            type_id: crate::asset_catalog::RW4,
        };
        // Display values: the retail blend runs in gamma space, the shader
        // converts its result (so the strip is sampled as stored).
        images.push((strip, crate::planet_sky::rw4_image(index, key, false, false)));
    }
    let map = index.read(G_TYPE_COLOR_MAP).ok().and_then(|b| ColorMap::parse(&b));
    // Every strip is needed (the glows of a missing one would be dropped).
    if images.iter().any(|(_, i)| i.is_none()) {
        return None;
    }
    let mut assets = world.resource_mut::<Assets<Image>>();
    let handles = images
        .into_iter()
        .map(|(s, i)| (s, assets.add(i.unwrap())))
        .collect();
    Some((handles, map))
}

/// One small additive sprite per star (the fallback when the particle field cannot be drawn).
fn spawn_legacy(world: &mut World, root: Entity, stars: &[GalaxyStar], material: Handle<StandardMaterial>) {
    let mesh = super::quad_mesh(
        stars.iter().enumerate().map(|(i, s)| {
            let [r, g, b] = s.kind.color();
            (s.position, super::sprite_size(s.kind, i as u32 == super::HOME_STAR), [r, g, b, 1.0])
        }),
        super::STAR_PLANE,
    );
    let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
    let layer = world
        .spawn((
            NotShadowCaster,
            GalaxyStarLayer,
            Name::new("galaxy stars"),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::IDENTITY,
        ))
        .id();
    world.entity_mut(root).add_child(layer);
}

/// Keeps the glow layers' state current: the map filter dims them (as it dims
/// the sprite layer).
pub fn glow_state(
    view: Option<Res<crate::space_view::SpaceView>>,
    mut materials: ResMut<Assets<StarGlowMaterial>>,
    layers: Query<&MeshMaterial3d<StarGlowMaterial>, With<StarGlowLayer>>,
) {
    let Some(view) = view else {
        return;
    };
    let dim = if view.filter == crate::space_view::GalaxyFilter::All { 1.0 } else { 0.3 };
    for handle in &layers {
        let Some(material) = materials.get(&handle.0) else {
            continue;
        };
        let state = Vec4::new(dim, 1.0, 0.0, 0.0);
        if (material.state - state).abs().max_element() > 1.0e-4 {
            if let Some(mut material) = materials.get_mut(&handle.0) {
                material.state = state;
            }
        }
    }
}

/// Registration (render apps: `space_galaxy::build_render`, which embeds the shader).
pub fn build(app: &mut App) {
    app.add_plugins(MaterialPlugin::<StarGlowMaterial>::default());
    app.add_systems(
        Update,
        glow_state.run_if(in_state(crate::stages::GameStage::Space)),
    );
    super::disks::build(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space_worldgen::STAR_COUNTS;

    #[test]
    fn every_population_follows_a_message_distribute() {
        // The visual distributes cover the message distributes' stars: the
        // blocks the populations of a kind take together cover the message
        // density (red 10,000 / yellow 6,000 / blue 16,000 per subdivision,
        // the binaries 150 / 200, 1,500 disks, 400 black holes; the black hole
        // vortex #64 covers the same 400 as its glow #66).
        for (kind, _, density) in STAR_COUNTS {
            let covered = (0..density).filter(|i| populations_of(kind, *i).next().is_some()).count() as u32;
            if covered == 0 {
                assert!(matches!(kind, StarKind::GalacticCore), "{kind:?} has no glow");
            } else {
                assert_eq!(covered, density, "{kind:?}");
            }
        }
        // A black hole is in two populations: the glow and the vortex.
        let both: Vec<usize> = populations_of(StarKind::BlackHole, 17).collect();
        assert_eq!(both, vec![VORTEX_POPULATION - 1, VORTEX_POPULATION]);
        assert_eq!(POPULATIONS[VORTEX_POPULATION].strip, Strip::Vortex);
        // The other kinds keep one.
        assert_eq!(populations_of(StarKind::Red, 3).count(), 1);
        for p in &POPULATIONS {
            assert!(p.first + p.count <= p.period, "{}", p.label);
            assert!(!p.sizes.is_empty() && !p.colors.is_empty());
            assert!(p.min_scale <= p.max_scale && p.fade_start < p.fade_end);
        }
        assert!(POPULATIONS.len() * 2 <= TABLE_ROWS);
    }

    #[test]
    fn populations_partition_each_subdivision() {
        // First 1000 of a yellow subdivision: the far-visible set; the rest the near one.
        assert_eq!(population_of(StarKind::Yellow, 0), Some(1));
        assert_eq!(population_of(StarKind::Yellow, 999), Some(1));
        assert_eq!(population_of(StarKind::Yellow, 1000), Some(2));
        assert_eq!(population_of(StarKind::Yellow, 5999), Some(2));
        // The next subdivision starts again.
        assert_eq!(population_of(StarKind::Yellow, 6000), Some(1));
        assert_eq!(population_of(StarKind::Blue, 15_999), Some(4));
        assert_eq!(population_of(StarKind::Blue, 16_000), Some(3));
        assert_eq!(population_of(StarKind::Red, 99_999), Some(0));
        assert_eq!(population_of(StarKind::GalacticCore, 0), None);
    }

    #[test]
    fn the_object_scale_graphs_follow_the_data_and_the_flag_selects_the_scalar() {
        // 0x1034640: the graph at the distance, clamped at both ends.
        assert_eq!(scale_graph_value(ScaleKind::Planet, 10.0, false), 0.5);
        assert_eq!(scale_graph_value(ScaleKind::Planet, 500.0, false), 1.5);
        assert!((scale_graph_value(ScaleKind::Planet, 262.5, false) - 1.0).abs() < 1e-5);
        assert_eq!(scale_graph_value(ScaleKind::Moon, 25.0, false), 0.35);
        assert_eq!(scale_graph_value(ScaleKind::Moon, 9_999.0, false), 0.75);
        assert_eq!(scale_graph_value(ScaleKind::GasGiant, 0.0, false), 0.75);
        assert_eq!(scale_graph_value(ScaleKind::GasGiant, 500.0, false), 1.5);
        // The flag (planet context) takes the scalar when it is not 0.
        assert_eq!(scale_graph_value(ScaleKind::Planet, 400.0, true), 0.5);
        assert_eq!(scale_graph_value(ScaleKind::Moon, 400.0, true), 0.4);
        assert_eq!(scale_graph_value(ScaleKind::GasGiant, 400.0, true), 1.0);
        // The star's scalar is 0: the graph stays.
        assert_eq!(scale_graph_value(ScaleKind::Star, 500.0, true), 1.2);
    }

    #[test]
    fn star_scale_graph_follows_the_data() {
        assert_eq!(star_object_scale(1.0), 0.95);
        assert!((star_object_scale(262.5) - 1.075).abs() < 1e-5);
        assert_eq!(star_object_scale(1200.0), 1.2);
        // The dust constants (the solar sky effect's colour, not a galaxy layer).
        assert_eq!(MAX_ZOOM_DUST_COLOR, [0.27, 0.2, 0.8]);
        assert_eq!(DUST_LERP_SECONDS, 2.0);
    }

    #[test]
    fn shader_carries_the_galaxy_cameras_clip_planes() {
        assert_eq!((NEAR_CLIP, FAR_CLIP), (crate::space_view::NEAR_CLIP, crate::space_view::FAR_CLIP));
        let shader = include_str!("../shaders/space_stars.wesl");
        assert!(shader.contains(&format!("const NEAR: f32 = {NEAR_CLIP};")));
        assert!(shader.contains(&format!("const FAR: f32 = {FAR_CLIP:.1};")));
    }

    #[test]
    fn the_black_hole_vortex_follows_distribute_64() {
        let p = &POPULATIONS[VORTEX_POPULATION];
        assert_eq!(p.kind, StarKind::BlackHole);
        // Material C11C834C: c0 = (2.75, 1, 0.1, 20), c1 = (40, 80), size 50 (vary 0.3), colour (1, 1, 1) vary 0.5.
        assert_eq!((p.anim_rate, p.strip_length, p.min_scale, p.max_scale), (2.75, 1.0, 0.1, 20.0));
        assert_eq!((p.fade_start, p.fade_end), (40.0, 80.0));
        assert_eq!((p.sizes, p.size_vary, p.color_vary, p.alpha), (&[50.0][..], 0.3, 0.5, 1.0));
        assert_eq!(Strip::Vortex.instance(), 0x5786_80C4);
        // The shader table row carries the whole mean size (effectTransSetupVF: scale = size).
        let table = shader_table();
        assert_eq!(table[2 * VORTEX_POPULATION], Vec4::new(2.75, 1.0, 0.1, 20.0));
        assert_eq!(table[2 * VORTEX_POPULATION + 1], Vec4::new(40.0, 80.0, 50.0, 0.0));
        // The vortex of a star draws its own picks (not the glow's).
        let hole = GalaxyStar {
            position: Vec2::new(5.0, 6.0),
            kind: StarKind::BlackHole,
            tech: crate::space_worldgen::TechLevel::None,
            empire: None,
            rare: false,
        };
        let glow = glow_for(&hole, 9, VORTEX_POPULATION - 1, None);
        let vortex = glow_for(&hole, 9, VORTEX_POPULATION, None);
        assert_ne!(glow.color, vortex.color);
        // (1, 1, 1) +- 0.5 per channel, alpha 1.
        assert!(vortex.color[..3].iter().all(|c| (0.5..=1.5).contains(c)), "{:?}", vortex.color);
        assert_eq!(vortex.color[3], 1.0);
        // The size: 50 (the only list entry) over the mean 50, spread 0.3.
        assert!((0.7..=1.3).contains(&vortex.size), "{}", vortex.size);
    }

    #[test]
    fn particle_sizes_follow_the_size_list_and_the_vary_spread_and_ride_in_uv_b() {
        // Red M: the list 15, 18, 16, 19, 12 (mean 16), no spread.
        let red = GalaxyStar {
            position: Vec2::new(1.0, 2.0),
            kind: StarKind::Red,
            tech: crate::space_worldgen::TechLevel::None,
            empire: None,
            rare: false,
        };
        let mut seen = std::collections::BTreeSet::new();
        for index in 0..400 {
            let glow = glow_for(&red, index, 0, None);
            let entry = glow.size * 16.0;
            assert!([15.0, 18.0, 16.0, 19.0, 12.0].iter().any(|e| (e - entry).abs() < 1e-3), "{entry}");
            seen.insert((entry * 10.0) as i32);
        }
        assert_eq!(seen.len(), 5, "every list entry is picked");
        // The black hole glow (size 5, vary 0.2) spreads 0.8 .. 1.2.
        let hole = GalaxyStar { kind: StarKind::BlackHole, ..red };
        for index in 0..100 {
            let glow = glow_for(&hole, index, VORTEX_POPULATION - 1, None);
            assert!((0.8 - 1e-4..=1.2 + 1e-4).contains(&glow.size), "{}", glow.size);
        }
        // The mesh carries population + size / 4: the shader's floor / fract x 4 give both back.
        let glows: Vec<Glow> = (0..3).map(|i| glow_for(&red, i, 0, None)).collect();
        let mesh = glow_mesh(&glows);
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv_b)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1) else {
            panic!("uv_b");
        };
        for (g, quad) in glows.iter().zip(uv_b.chunks(4)) {
            assert_eq!(quad[0][0].floor() as u8, g.population);
            assert!((quad[2][0].fract() * SIZE_CODE - g.size).abs() < 1e-5);
        }
    }

    #[test]
    fn colour_map_parses_and_interpolates() {
        let mut bytes = vec![0u8; 20];
        bytes[7] = 2;
        bytes[11] = 2;
        // BGRA: black, white / red, blue.
        for px in [[0u8, 0, 0, 255], [255, 255, 255, 255], [0, 0, 255, 255], [255, 0, 0, 255]] {
            bytes.extend_from_slice(&px);
        }
        let map = ColorMap::parse(&bytes).unwrap();
        assert_eq!(map.at(Vec2::new(-1000.0, -1000.0)), [0.0, 0.0, 0.0]);
        let mid = map.at(Vec2::ZERO);
        assert!((mid[0] - 0.5).abs() < 1e-5 && (mid[2] - 0.5).abs() < 1e-5, "{mid:?}");
        assert!(ColorMap::parse(&bytes[..10]).is_none());
    }

    #[test]
    fn glows_take_their_distribute_values() {
        let red = GalaxyStar {
            position: Vec2::new(10.0, 20.0),
            kind: StarKind::Red,
            tech: crate::space_worldgen::TechLevel::None,
            empire: None,
            rare: false,
        };
        let g = glow_of(&red, 5, 5, None).unwrap();
        assert_eq!(g.population, 0);
        // (2.0, 0.3, 0.2) +- 0.05.
        assert!((g.color[0] - 2.0).abs() <= 0.05 + 1e-6);
        assert!((g.color[1] - 0.3).abs() <= 0.05 + 1e-6);
        assert_eq!(g.color[3], 1.0);
        let core = GalaxyStar { kind: StarKind::GalacticCore, ..red };
        assert!(glow_of(&core, 0, 0, None).is_none());
        // Same star, same glow.
        let again = glow_of(&red, 5, 5, None).unwrap();
        assert_eq!((again.frame, again.color), (g.frame, g.color));
    }

    #[test]
    fn glows_cover_every_star_but_the_core() {
        let galaxy = super::super::Galaxy::generate(None);
        let groups = glows_by_strip(&galaxy.stars, None);
        let total: usize = groups.iter().map(|(_, g)| g.len()).sum();
        // One glow per star but the core, and a vortex per black hole.
        let holes = galaxy.stars.iter().filter(|s| s.kind == StarKind::BlackHole).count();
        assert_eq!(total, galaxy.stars.len() - 1 + holes);
        let vortices = groups.iter().find(|(s, _)| *s == Strip::Vortex).map_or(0, |(_, g)| g.len());
        assert_eq!(vortices, holes);
        // The far-visible subsets (fade end 900 / 2500) are a small part of the field.
        let far: usize = groups
            .iter()
            .flat_map(|(_, g)| g.iter())
            .filter(|g| POPULATIONS[g.population as usize].fade_end >= 900.0)
            .count();
        assert!(far > 15_000 && far < 25_000, "{far}");
    }
}
