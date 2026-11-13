//! CPU terrain brush engine: a planet's terrain script -> `HeightCube`
//! (6 x 512 x 512 u16), plus the derived normal and control maps.
//!
//! A port of the retail generator (`notes/planet-blend-notes.md`; VERIFIED
//! unless marked):
//! - `cTerrainSphere::Generate` clears the faces to H 33618 (h = +0.0259) and
//!   plays effect `te_terrain_script` in the offline effect world, stepped at
//!   dt = 0.033 s with the main RNG seeded from the planet seed (0x7DAEE0);
//! - its `terrainScript` component (0xF95590) applies the script entries ONE
//!   AFTER ANOTHER: before each entry it reseeds the main RNG with the entry
//!   seed, creates the effect at the entry transform with the entry intensity
//!   and waits until it has stopped;
//! - each `spBrush` emits per update n = round((acc + dt) * rate) stamps while
//!   age = t / life <= 1 (0xF4C2C0): the brushes of one effect stamp in time
//!   order. `sizeGlobal` / cube brushes are one whole-planet pass;
//! - every stamp reads and writes the 16-bit packed height (H = hi*256 + lo,
//!   p = H/65536, h = H/32768 - 1) through the TerrainBrush* pixel program of
//!   its draw mode ([`apply`]), weighted by the piecewise-linear height
//!   condition ([`CondRamp`], 0xF68470) with the per-vertex parameters of
//!   0xF67560 ([`vertex_params`]);
//! - the stamp is the tangent-plane square of half-width `size` (0xF6C8F0);
//!   per cube face one polygon, fanned into triangles whose brush uv is
//!   interpolated linearly (0xF68D00, [`raster_stamp`]);
//! - filter brushes (smooth / sharpen / edge kernels, wind erosion) run the
//!   Filter / FilterErosion programs on a copy of the cube ([`filter_kernel`]).
//!
//! - Swarm `metaParticles` (the Space pool scripts' LP_* / GP_* / GO_* /
//!   Patch1_* effects) run as a small effects world (`planet_meta`,
//!   `notes/planet-meta-notes.md`): each particle carries a child effect whose
//!   brushes stamp along the particle's path (prev -> cur transform per
//!   update), by distance in spacing mode, or record ribbon points drawn as
//!   a textured strip when the brush stops (0xF4D980).
//!
//! ADAPTED / INFERRED (notes section 4): the RNG numbers effect creation
//! consumes (one per selection group), texel sample points at texel centres
//! (retail has a -1/4 texel D3D9 offset), brush rasters sampled bilinearly at
//! mip 0, the Euler order of the cube brushes' rotation.
use crate::planet_data::{
    Brush, BrushLibrary, BrushRaster, Cond, CondKind, DrawMode, EffectDef, PlanetSource, KIND_BRUSH,
    KIND_EFFECT, KIND_META,
};
use crate::planet_meta::{
    align_z, apply_rows, approx_heading, bit, curve as pm_curve, mul_rows, particle_rows, rotate_between,
    MetaState, Particle, Rand, Sphere, Walk, FORCES_MASK, IDENTITY_ROWS, MAX_SUBSTEP, PIN_MASK,
};
use crate::swarm_fx::format::MetaParticleDef;
use crate::planet_query::{face_point, face_uv, HeightCube, Planet, FACE_RES};
use bevy::math::{EulerRot, Mat3, Vec3};
use std::collections::HashMap;

/// Clear colour of the face render targets (0xF67D50): (0.5137255,
/// 0.32156864) = bytes (131, 82).
pub const INITIAL_RAW: u16 = 131 * 256 + 82;
/// Virtual step of the offline effect world (0x7DAEE0: 33 ms).
pub const STEP_DT: f32 = 0.033;
/// ADAPTED guard: retail runs an entry until its effect stops; a brush that
/// never dies (rate 0) would stall the generation.
const MAX_UPDATES: u32 = 4096;
/// Stamps below this many texels run on the calling thread.
const PARALLEL_TEXELS: usize = 24_000;
const BAND_ROWS: usize = 32;

/// Generation report.
#[derive(Clone, Debug, Default)]
pub struct GenStats {
    pub entries: usize,
    pub stamps: usize,
    pub global_passes: usize,
    pub missing_effects: usize,
    pub missing_rasters: usize,
    pub unsupported_filters: usize,
    /// metaParticles components created, particles emitted, child effects,
    /// ribbons drawn.
    pub metas: usize,
    pub particles: usize,
    pub children: usize,
    pub ribbons: usize,
    pub millis: u128,
}

// ---------------------------------------------------------------------------
// Retail RNG

/// `Math::RandomNumberGenerator` as used on the global instance 0x16778DC:
/// SetSeed 0x936000, LCG step 0x936020, RandomFloat 0x936040.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SporeRng {
    seed: u32,
}

impl SporeRng {
    pub fn new(seed: u32) -> Self {
        let mut rng = Self { seed: 0 };
        rng.set_seed(seed);
        rng
    }
    /// ADAPTED: retail takes a time-stamp seed for 0 / -1 (rdtsc); the
    /// offline runner 0x7DAEE0 maps them to 0xDEADBEEF, used here always.
    pub fn set_seed(&mut self, seed: u32) {
        self.seed = if seed == 0 || seed == u32::MAX { 0xDEAD_BEEF } else { seed };
    }
    pub fn seed(&self) -> u32 {
        self.seed
    }
    /// 0x936020: s = s * 0x41C64E6D + 0x3039.
    pub fn lcg_step(&mut self) {
        self.seed = self.seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
    }
    /// 0x936040: s *= 0x278DDE6D; f = (int)s * 2^-32 + 0.5 (1 -> 0).
    pub fn random_float(&mut self) -> f32 {
        if self.seed == 0 {
            self.seed = 0xDEAD_BEEF;
        }
        self.seed = self.seed.wrapping_mul(0x278D_DE6D);
        let f = (self.seed as i32 as f64 * f64::from(2.328_306_4e-10_f32) + 0.5) as f32;
        if f >= 1.0 { 0.0 } else { f }
    }
    /// Math::RandomNumberGenerator::RandomInt 0xA68F70: one LCG step; bits
    /// 16..47 of the 64-bit product times `range`, high word.
    pub fn random_int(&mut self, range: u32) -> u32 {
        let wide = u64::from(self.seed) * 0x41C6_4E6D + 0x3039;
        self.seed = wide as u32;
        let v = (wide >> 16) as u32;
        ((u64::from(v) * u64::from(range)) >> 32) as u32
    }
    /// 0x7D4690: uniform in [a - v, a + v); consumes a number even for v = 0.
    pub fn vary(&mut self, a: f32, v: f32) -> f32 {
        let lo = a - v;
        let r = self.random_float();
        r * ((v + a) - lo) + lo
    }
}

// ---------------------------------------------------------------------------
// Packed heights, conditions, programs

#[inline]
fn raw_p(raw: u16) -> f32 {
    raw as f32 * (1.0 / 65536.0)
}

#[inline]
fn raw_h(raw: u16) -> f32 {
    raw as f32 * (1.0 / 32768.0) - 1.0
}

/// Output stage of every brush program: x = (h/2 + 1/2) * 256, hi = floor(x),
/// lo = (x - hi) * 256/255, both written as UNORM8 (hi saturates, so values
/// outside [0, 1) keep a stray low byte, as on the GPU).
#[inline]
pub fn pack_h(h: f32) -> u16 {
    let x = (h * 0.5 + 0.5) * 256.0;
    if x.is_nan() {
        return 0;
    }
    let hi = x.floor();
    let lo = (x - hi) * (256.0 / 255.0);
    let hb = hi.clamp(0.0, 255.0) as u16;
    let lb = if lo.is_nan() { 0 } else { ((lo.clamp(0.0, 1.0) * 255.0 + 0.5) as u16).min(255) };
    hb * 256 + lb
}

fn curve(values: &[f32], age: f32) -> f32 {
    match values.len() {
        0 => 1.0,
        1 => values[0],
        n => {
            let x = (n - 1) as f32 * age.clamp(0.0, 1.0);
            let i = (x.floor() as usize).min(n - 2);
            let f = x - i as f32;
            values[i] + (values[i + 1] - values[i]) * f
        }
    }
}

/// The height condition as the shader constants c0..c2 (0xF68470), evaluated
/// on the packed scale p. Thresholds t (c0), ramp slopes s0 / s2 (c1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CondRamp {
    t: [f32; 4],
    s0: f32,
    s2: f32,
}

impl CondRamp {
    /// `off` = W - 0.5 for waterLevelIsZero brushes (0xF4C2C0), else 0.
    pub fn new(c: &Cond, off: f32) -> Self {
        let (a, b) = (c.a + off, c.b + off);
        let (lo, hi) = (c.falloff_lo + off, c.falloff_hi + off);
        let mut t = [-2.0f32, -1.0, 1.0, 2.0];
        match c.kind {
            CondKind::Less => {
                t[2] = if c.falloff { lo } else { a };
                t[3] = a;
            }
            // The first falloff value ends the ramp above `a`.
            CondKind::Greater => {
                t[0] = a;
                t[1] = if c.falloff { lo } else { a };
            }
            CondKind::Between => {
                t = [if c.falloff { lo } else { a }, a, b, if c.falloff { hi } else { b }];
            }
            CondKind::None => {}
        }
        t[2] += 1.0 / 128.0;
        t[3] += 1.0 / 128.0;
        let (d0, d2) = (t[1] - t[0], t[3] - t[2]);
        Self {
            t,
            s0: if d0 > 0.0 { 1.0 / d0 } else { 0.0 },
            s2: if d2 > 0.0 { -1.0 / d2 } else { 0.0 },
        }
    }

    /// Shader: one-hot segment flags (p > t_i) times c1 * p - c2.
    #[inline]
    pub fn weight(&self, p: f32) -> f32 {
        let f = |i: usize| if p > self.t[i] { 1.0f32 } else { 0.0 };
        let (f0, f1, f2, f3) = (f(0), f(1), f(2), f(3));
        let sx = f0 * (1.0 - f1);
        let sy = f1 * (1.0 - f2);
        let sz = f2 * (1.0 - f3);
        sx * (self.s0 * p - self.t[0] * self.s0) + sy + sz * (self.s2 * p - (self.t[2] * self.s2 - 1.0))
    }
}

/// The TerrainBrush* pixel programs (material table of 0xF67720).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Texture,
    Gaussian,
    GaussianGlobal,
    TextureLevel,
    GaussianLevel,
    GaussianLevelGlobal,
    Cubemap,
    CubemapLevel,
    AbsoluteMax,
    AbsoluteMin,
    /// 3x3 kernel on the packed scale (TerrainBrushGaussianFilter).
    Filter,
    /// 4-neighbour diffusion in the brush cone (TerrainBrushGaussianFilterErosion).
    FilterErosion,
}

/// Filter kernel of 0xF4C2C0: 3x3 weights (row-major, rows = +v), the
/// normalisation and the blend mode (0 replace, 1 add, 6 average). Tables
/// .rdata 0x148E230..0x148E6BC (VERIFIED); `None` for the types whose kernel
/// the code leaves uninitialised (0, 0x11..0x13, > 0x14).
pub fn filter_kernel(brush: &Brush) -> Option<([f32; 9], f32, u8)> {
    let one = [1.0f32; 9];
    let centre = |c: f32| {
        let mut k = one;
        k[4] = c;
        k
    };
    let (mut k, norm, mut mode) = match brush.filter {
        1 => (one, 1.0, 3),
        2 => (one, 1.0, 2),
        3 => (one, 1.0 / 9.0, 0),
        4 => (centre(2.0), 0.1, 0),
        5 => (centre(4.0), 1.0 / 12.0, 0),
        6 => (centre(12.0), 0.05, 0),
        7 => ([1.0, 2.0, 1.0, 2.0, 4.0, 2.0, 1.0, 2.0, 1.0], 0.0625, 0),
        8 => ([-1.0, -1.0, -1.0, -1.0, 9.0, -1.0, -1.0, -1.0, -1.0], 1.0, 6),
        9 => ([0.0, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0], 0.2, 6),
        10 => ([1.0, -2.0, 1.0, -2.0, 5.0, -2.0, 1.0, -2.0, 1.0], 1.0, 6),
        11 => ([0.0, -1.0, 0.0, -1.0, 20.0, -1.0, 0.0, -1.0, 0.0], 0.0625, 0),
        12 => ([0.0, 1.0, 0.0, 1.0, -4.0, 1.0, 0.0, 1.0, 0.0], 1.0, 1),
        13 => ([1.0, 1.0, 1.0, 1.0, -8.0, 1.0, 1.0, 1.0, 1.0], 1.0, 1),
        14 => ([-1.0, 2.0, -1.0, 2.0, -4.0, 2.0, -1.0, 2.0, -1.0], 1.0, 1),
        0x0F | 0x10 | 0x14 => ([0.0; 9], 1.0, if brush.filter == 0x14 { 11 } else { 7 }),
        _ => return None,
    };
    match brush.filter {
        0x0F | 0x14 => k[4] = brush.filter_value,
        0x10 => k[4] = -brush.filter_value,
        _ => {}
    }
    match brush.filter_mode {
        1 => mode = 0,
        2 => mode = 1,
        3 => mode = 6,
        _ => {}
    }
    Some((k, norm, mode))
}

/// 0xF67720: program of a brush; "gaussian" = no raster or texGaussian.
/// `None` for filter types the retail code cannot run either.
pub fn program(brush: &Brush, has_raster: bool) -> Option<Program> {
    if brush.use_filter {
        return if brush.filter == 0x12 {
            Some(Program::FilterErosion)
        } else {
            filter_kernel(brush).map(|_| Program::Filter)
        };
    }
    let gaussian = !has_raster || brush.tex_gaussian;
    Some(if brush.tex_cube {
        if matches!(brush.draw_mode, DrawMode::Level | DrawMode::LevelRange) {
            Program::CubemapLevel
        } else {
            Program::Cubemap
        }
    } else {
        match brush.draw_mode {
            DrawMode::AbsoluteMax => Program::AbsoluteMax,
            DrawMode::AbsoluteMin => Program::AbsoluteMin,
            DrawMode::Level if gaussian => {
                if brush.size_global {
                    Program::GaussianLevelGlobal
                } else {
                    Program::GaussianLevel
                }
            }
            DrawMode::Level | DrawMode::LevelRange => Program::TextureLevel,
            _ if gaussian => {
                if brush.size_global {
                    Program::GaussianGlobal
                } else {
                    Program::Gaussian
                }
            }
            _ => Program::Texture,
        }
    })
}

/// Per-vertex parameters (x, y, z, w) of 0xF67560. `level` / `lo` / `hi`
/// already carry the water offset (not added to the range of modes 5/6).
pub fn vertex_params(mode: DrawMode, intensity: f32, level: f32, lo: f32, hi: f32) -> [f32; 4] {
    match mode {
        DrawMode::AbsoluteMax if lo == hi => [intensity, level, lo, lo + 1.0],
        DrawMode::AbsoluteMin if lo == hi => [intensity, level, hi - 1.0, hi],
        DrawMode::AbsoluteMax | DrawMode::AbsoluteMin => [intensity, level, lo, hi],
        DrawMode::Level => [intensity, 0.0, 0.0, level],
        DrawMode::LevelRange => [intensity, 1.0, hi, lo],
        DrawMode::Subtract => [intensity, -1.0, 1.0, 0.0],
        DrawMode::Biased => [intensity, 1.0, 2.0, 256.0 / 255.0],
        DrawMode::Add => [intensity, 1.0, 1.0, 0.0],
    }
}

/// One texel of a brush program (`notes/planet-blend-notes.md` 3): `h` the
/// pre-stamp height, `wc` the condition weight, `r` / `a` the raster red /
/// alpha (the gaussian programs pass their cone max(1 - 2|uv - 0.5|, 0) in `r`).
#[inline]
pub fn apply(program: Program, h: f32, wc: f32, r: f32, a: f32, v: [f32; 4]) -> f32 {
    let [x, y, z, w] = v;
    match program {
        Program::Texture | Program::Cubemap => h + wc * (z * r - w) * x * y,
        Program::Gaussian => h + wc * r * x * y,
        Program::GaussianGlobal => h + wc * x * y,
        Program::TextureLevel | Program::CubemapLevel => h + wc * a * x * (w + (z - w) * y * r - h),
        Program::GaussianLevel => h + wc * r * x * (w - h),
        Program::GaussianLevelGlobal => h + wc * x * (w - h),
        // T' maps [-1, 1] onto [-1, 1] with 0 -> the water level y.
        Program::AbsoluteMax => {
            if r <= 0.0 {
                h
            } else {
                let t = r * w + (1.0 - r) * z;
                let t2 = ((1.0 - y) * t).max(0.0) + ((1.0 + y) * t).min(0.0) + y;
                h + x * (h.max(t2) - h)
            }
        }
        Program::AbsoluteMin => {
            if r <= 0.0 {
                h
            } else {
                let t = (w - z) * -r + w;
                let t2 = ((1.0 + y) * t).min(0.0) + ((1.0 - y) * t).max(0.0) + y;
                h + x * (t2.min(h) - h)
            }
        }
        // The filter programs need the neighbourhood: see `Shade::filter`.
        Program::Filter | Program::FilterErosion => h,
    }
}

/// Per-vertex parameters of a filter brush (0xF67560 overrides x, y, z; the
/// brush intensity is not used): mode 6 average, 1 add, else replace.
pub fn filter_params(mode: u8) -> [f32; 4] {
    match mode {
        6 => [1.0, 1.0, 0.5, 0.0],
        1 => [1.0, 1.0, 1.0, 0.0],
        _ => [0.0, 1.0, 1.0, 0.0],
    }
}

/// Everything one stamp's texels need.
#[derive(Clone, Copy)]
struct Shade<'a> {
    program: Program,
    params: [f32; 4],
    cond: CondRamp,
    raster: Option<&'a BrushRaster>,
    cube_rot: Mat3,
    /// Filter kernel (9 weights + normalisation).
    kernel: [f32; 10],
    /// Pre-pass copy of the cube the filter programs read (retail copies the
    /// face to a scratch target first, 0xF677C0).
    snapshot: Option<&'a [u16]>,
    /// Ribbon polygons carry the intensity per vertex (params.x, 0xF4BA60).
    vertex_intensity: bool,
}

impl Shade<'_> {
    /// Filter programs on the packed scale p (VERIFIED, notes 3):
    /// Filter: F = norm * sum(k * p of the 3x3 block), p' = c + W y ((c x + F y) z - c);
    /// FilterErosion: t = sat(c + 0.45 * sum(n - c) over 4 neighbours),
    /// p' = c + W * cone * y * (t - c). Neighbours clamp at the face edge;
    /// ADAPTED: one texel at any resolution (retail offsets 1/512).
    #[inline]
    fn filter(&self, raw: u16, u: f32, v: f32, face: usize, row: usize, col: usize, res: usize) -> u16 {
        let Some(snap) = self.snapshot else { return raw };
        let at = |dr: isize, dc: isize| {
            let r = (row as isize + dr).clamp(0, res as isize - 1) as usize;
            let c = (col as isize + dc).clamp(0, res as isize - 1) as usize;
            raw_p(snap[(face * res + r) * res + c])
        };
        let c = raw_p(raw);
        let wc = self.cond.weight(c);
        let [x, y, z, _] = self.params;
        let p = if self.program == Program::Filter {
            let mut sum = 0.0;
            for (i, k) in self.kernel[..9].iter().enumerate() {
                if *k != 0.0 {
                    sum += k * at(i as isize / 3 - 1, i as isize % 3 - 1);
                }
            }
            let f = sum * self.kernel[9];
            c + wc * y * ((c * x + f * y) * z - c)
        } else {
            let t = (c + 0.45 * (at(-1, 0) + at(1, 0) + at(0, -1) + at(0, 1) - 4.0 * c)).clamp(0.0, 1.0);
            let (du, dv) = (u - 0.5, v - 0.5);
            let cone = (1.0 - 2.0 * (du * du + dv * dv).sqrt()).max(0.0);
            c + wc * cone * y * (t - c)
        };
        pack_h(p * 2.0 - 1.0)
    }

    /// New packed value of one texel at (face, row, col); `u`, `v` = brush uv.
    #[inline]
    fn texel(&self, raw: u16, u: f32, v: f32, face: usize, row: usize, col: usize, res: usize) -> u16 {
        self.texel_i(raw, u, v, face, row, col, res, self.params[0])
    }

    /// `texel` with the intensity (params.x) given per texel.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn texel_i(&self, raw: u16, u: f32, v: f32, face: usize, row: usize, col: usize, res: usize, x: f32) -> u16 {
        let dir = || {
            let inv = 1.0 / res as f32;
            face_point(face, (col as f32 + 0.5) * inv, (row as f32 + 0.5) * inv)
        };
        if matches!(self.program, Program::Filter | Program::FilterErosion) {
            return self.filter(raw, u, v, face, row, col, res);
        }
        let h = raw_h(raw);
        let (r, a) = match self.program {
            Program::Texture | Program::TextureLevel | Program::AbsoluteMax | Program::AbsoluteMin => {
                // An unbound sampler reads (0, 0, 0, 1).
                self.raster.map_or((0.0, 1.0), |t| t.sample_ra(0, u, v))
            }
            Program::Gaussian | Program::GaussianLevel => {
                let (du, dv) = (u - 0.5, v - 0.5);
                ((1.0 - 2.0 * (du * du + dv * dv).sqrt()).max(0.0), 1.0)
            }
            Program::Cubemap | Program::CubemapLevel => {
                self.raster.map_or((0.0, 1.0), |t| t.sample_cube_ra(self.cube_rot * dir()))
            }
            Program::GaussianGlobal | Program::GaussianLevelGlobal | Program::Filter | Program::FilterErosion => {
                (0.0, 1.0)
            }
        };
        // The AbsoluteMax/Min programs have no condition constants.
        let wc = match self.program {
            Program::AbsoluteMax | Program::AbsoluteMin => 1.0,
            _ => self.cond.weight(raw_p(raw)),
        };
        let mut params = self.params;
        params[0] = x;
        pack_h(apply(self.program, h, wc, r, a, params))
    }
}

// ---------------------------------------------------------------------------
// Stamp geometry and rasterisation (0xF6C8F0 / 0xF68D00)

/// One stamp's plane frame.
#[derive(Clone, Copy, Debug)]
struct StampGeom {
    normal: Vec3,
    plane_d: f32,
    axis_u: Vec3,
    axis_v: Vec3,
    inv_width_u: f32,
    inv_width_v: f32,
    /// Corner A (centre - U - V) on the plane: brush uv (0, 1).
    origin: Vec3,
    /// Corner directions A, B, C, D = c -+ U -+ V (A - - , B + -, C + +, D - +).
    corners: [Vec3; 4],
}

impl StampGeom {
    /// `rows` = the stamp rotation (rows are the local axes), `spin_deg` the
    /// random spin of the base axes about local Z.
    fn new(centre: Vec3, rows: [Vec3; 3], half_width: f32, spin_deg: f32) -> Option<Self> {
        let len = centre.length();
        if !(len > 0.01) || !(half_width > 0.0) || !half_width.is_finite() {
            return None;
        }
        let normal = centre / len;
        let (s, c) = spin_deg.to_radians().sin_cos();
        let world = |l: Vec3| rows[0] * l.x + rows[1] * l.y + rows[2] * l.z;
        let (mut u, mut v) = (world(Vec3::new(c, s, 0.0)), world(Vec3::new(-s, c, 0.0)));
        // Axes that do not span the tangent plane: the computed frame.
        if u.cross(v).dot(normal) < 0.95 {
            let helper = if normal.z >= 0.999 { Vec3::Y } else { Vec3::Z };
            let mut t1 = helper.cross(centre);
            if t1.length_squared() < 1e-12 {
                // ADAPTED: retail divides by zero at the south pole.
                t1 = Vec3::X.cross(centre);
            }
            let t2 = centre.cross(t1);
            u = t1.normalize();
            v = t2.normalize();
        }
        let (us, vs) = (u * half_width, v * half_width);
        let a = centre - us - vs;
        let corners = [a, centre + us - vs, centre + us + vs, centre - us + vs].map(|p| p.normalize());
        let plane_d = normal.dot(centre);
        let nd = normal.dot(corners[0]);
        let origin = if nd != 0.0 && plane_d / nd >= 0.0 {
            corners[0] * (plane_d / nd)
        } else {
            a
        };
        Some(Self {
            normal,
            plane_d,
            axis_u: us.normalize(),
            axis_v: vs.normalize(),
            inv_width_u: 1.0 / (2.0 * us.length()),
            inv_width_v: 1.0 / (2.0 * vs.length()),
            origin,
            corners,
        })
    }

    /// Exact brush uv of a direction (the per-vertex value of 0xF68D00).
    fn brush_uv(&self, dir: Vec3) -> (f32, f32) {
        let nd = self.normal.dot(dir);
        let t = if nd != 0.0 { self.plane_d / nd } else { 0.0 };
        let q = dir * t - self.origin;
        (q.dot(self.axis_u) * self.inv_width_u, 1.0 - q.dot(self.axis_v) * self.inv_width_v)
    }
}

/// (u axis, v axis, normal axis, sign) of a cube face (matches `face_uv`).
fn face_axes(face: usize) -> (usize, usize, usize, f32) {
    match face {
        0 => (0, 1, 2, 1.0),
        1 => (0, 1, 2, -1.0),
        2 => (1, 2, 0, 1.0),
        3 => (1, 2, 0, -1.0),
        4 => (2, 0, 1, 1.0),
        _ => (2, 0, 1, -1.0),
    }
}

/// Face uv of a point known to project onto `face`.
fn on_face_uv(face: usize, p: Vec3) -> (f32, f32) {
    let (ua, va, na, s) = face_axes(face);
    let a = p.to_array();
    ((a[ua] / a[na] + 1.0) * 0.5, (a[va] / (s * a[na]) + 1.0) * 0.5)
}

/// Sutherland-Hodgman clip of a polygon of directions to a face's frustum
/// (the four planes through the centre; 0xFC2C30 with the planes at 0x16D6FA8).
fn clip_to_face(poly: &[Vec3], face: usize) -> Vec<Vec3> {
    let (ua, va, na, s) = face_axes(face);
    let dist = |k: usize, p: Vec3| {
        let a = p.to_array();
        let n = s * a[na];
        match k {
            0 => n - a[ua],
            1 => n + a[ua],
            2 => n - a[va],
            _ => n + a[va],
        }
    };
    let mut out: Vec<Vec3> = poly.to_vec();
    for k in 0..4 {
        if out.is_empty() {
            break;
        }
        let input = std::mem::take(&mut out);
        let mut prev = *input.last().unwrap();
        let mut dp = dist(k, prev);
        for &cur in &input {
            let dc = dist(k, cur);
            if dc >= 0.0 {
                if dp < 0.0 {
                    out.push(prev + (cur - prev) * (dp / (dp - dc)));
                }
                out.push(cur);
            } else if dp >= 0.0 {
                out.push(prev + (cur - prev) * (dp / (dp - dc)));
            }
            prev = cur;
            dp = dc;
        }
    }
    out
}

/// A texel-space triangle with affine brush uv.
#[derive(Clone, Copy, Debug)]
struct Tri {
    p: [(f32, f32); 3],
    au: (f32, f32, f32),
    av: (f32, f32, f32),
    /// Affine per-vertex intensity (ribbons; 0 for stamps).
    ai: (f32, f32, f32),
    ymin: f32,
    ymax: f32,
    xmin: f32,
    xmax: f32,
}

impl Tri {
    fn new(p: [(f32, f32); 3], uv: [(f32, f32); 3]) -> Option<Self> {
        Self::new_i(p, uv, [0.0; 3])
    }

    fn new_i(p: [(f32, f32); 3], uv: [(f32, f32); 3], int: [f32; 3]) -> Option<Self> {
        let [(x0, y0), (x1, y1), (x2, y2)] = p;
        let det = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if !det.is_finite() || det.abs() < 1e-9 {
            return None;
        }
        let coef = |f0: f32, f1: f32, f2: f32| {
            let (d1, d2) = (f1 - f0, f2 - f0);
            let a = (d1 * (y2 - y0) - d2 * (y1 - y0)) / det;
            let b = (d2 * (x1 - x0) - d1 * (x2 - x0)) / det;
            (a, b, f0 - a * x0 - b * y0)
        };
        Some(Self {
            p,
            au: coef(uv[0].0, uv[1].0, uv[2].0),
            av: coef(uv[0].1, uv[1].1, uv[2].1),
            ai: coef(int[0], int[1], int[2]),
            ymin: y0.min(y1).min(y2),
            ymax: y0.max(y1).max(y2),
            xmin: x0.min(x1).min(x2),
            xmax: x0.max(x1).max(x2),
        })
    }

    /// Covered x interval [xl, xr) on the row through `yc`; edges are taken in
    /// a canonical endpoint order so triangles sharing an edge split its
    /// texels exactly (each texel centre is drawn once, as on the GPU).
    #[inline]
    fn span(&self, yc: f32) -> Option<(f32, f32)> {
        let mut xs = [0f32; 2];
        let mut k = 0;
        for i in 0..3 {
            let (a, b) = (self.p[i], self.p[(i + 1) % 3]);
            let (top, bot) = if (a.1, a.0) <= (b.1, b.0) { (a, b) } else { (b, a) };
            if top.1 <= yc && yc < bot.1 {
                if k < 2 {
                    xs[k] = top.0 + (yc - top.1) * (bot.0 - top.0) / (bot.1 - top.1);
                }
                k += 1;
            }
        }
        (k >= 2).then(|| (xs[0].min(xs[1]), xs[0].max(xs[1])))
    }
}

/// Draw rows [rows.0, rows.1) of one face band.
fn draw_rows(
    band: &mut [u16],
    band_row0: usize,
    rows: (usize, usize),
    res: usize,
    face: usize,
    tris: &[Tri],
    shade: &Shade,
) {
    for row in rows.0..rows.1 {
        let yc = row as f32 + 0.5;
        let line = &mut band[(row - band_row0) * res..(row - band_row0 + 1) * res];
        for tri in tris {
            if yc < tri.ymin || yc >= tri.ymax {
                continue;
            }
            let Some((xl, xr)) = tri.span(yc) else { continue };
            let c0 = (xl - 0.5).ceil().max(0.0) as usize;
            let c1 = ((xr - 0.5).ceil().min(res as f32)).max(0.0) as usize;
            for col in c0..c1 {
                let xc = col as f32 + 0.5;
                let u = tri.au.0 * xc + tri.au.1 * yc + tri.au.2;
                let v = tri.av.0 * xc + tri.av.1 * yc + tri.av.2;
                line[col] = if shade.vertex_intensity {
                    let x = tri.ai.0 * xc + tri.ai.1 * yc + tri.ai.2;
                    shade.texel_i(line[col], u, v, face, row, col, res, x)
                } else {
                    shade.texel(line[col], u, v, face, row, col, res)
                };
            }
        }
    }
}

/// Run `(face, band_row0, band, rows)` jobs, spread over threads when large.
fn run_jobs<'b>(
    jobs: Vec<(usize, usize, &'b mut [u16], (usize, usize))>,
    parallel: bool,
    f: &(dyn Fn(usize, usize, &mut [u16], (usize, usize)) + Sync),
) {
    if !parallel || jobs.len() < 2 {
        for (face, row0, band, rows) in jobs {
            f(face, row0, band, rows);
        }
        return;
    }
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 16)
        .min(jobs.len());
    let per = jobs.len().div_ceil(threads);
    std::thread::scope(|s| {
        let mut rest = jobs;
        while !rest.is_empty() {
            let take = per.min(rest.len());
            let chunk: Vec<_> = rest.drain(..take).collect();
            s.spawn(move || {
                for (face, row0, band, rows) in chunk {
                    f(face, row0, band, rows);
                }
            });
        }
    });
}

/// Rasterise one stamp into the packed cube: one quad when all corners fall on
/// one face (vertex order A, D, C, B, fan from A), else the polygon clipped to
/// every face it touches (fan from its first vertex).
fn raster_stamp(buf: &mut [u16], res: usize, geom: &StampGeom, shade: &Shade) {
    let [a, b, c, d] = geom.corners;
    let faces = geom.corners.map(|p| face_uv(p).0);
    let mut polys: Vec<(usize, Vec<Vec3>)> = Vec::new();
    if faces.iter().all(|&f| f == faces[0]) {
        polys.push((faces[0], vec![a, d, c, b]));
    } else {
        for f in 0..6 {
            let poly = clip_to_face(&[d, c, b, a], f);
            if poly.len() >= 3 {
                polys.push((f, poly));
            }
        }
    }
    let n = res as f32;
    for (face, poly) in polys {
        let pts: Vec<((f32, f32), (f32, f32))> = poly
            .iter()
            .map(|&p| {
                let (u, v) = on_face_uv(face, p);
                ((u * n, v * n), geom.brush_uv(p.normalize()))
            })
            .collect();
        let tris: Vec<Tri> = (1..pts.len() - 1)
            .filter_map(|i| Tri::new([pts[0].0, pts[i].0, pts[i + 1].0], [pts[0].1, pts[i].1, pts[i + 1].1]))
            .collect();
        if tris.is_empty() {
            continue;
        }
        let ymin = tris.iter().map(|t| t.ymin).fold(f32::MAX, f32::min);
        let ymax = tris.iter().map(|t| t.ymax).fold(f32::MIN, f32::max);
        let xmin = tris.iter().map(|t| t.xmin).fold(f32::MAX, f32::min);
        let xmax = tris.iter().map(|t| t.xmax).fold(f32::MIN, f32::max);
        let r0 = (ymin - 0.5).ceil().clamp(0.0, n) as usize;
        let r1 = (ymax - 0.5).ceil().clamp(0.0, n) as usize;
        if r0 >= r1 {
            continue;
        }
        let texels = (r1 - r0) * ((xmax - xmin).max(1.0) as usize);
        let face_buf = &mut buf[face * res * res..(face + 1) * res * res];
        let mut jobs = Vec::new();
        for (bi, band) in face_buf.chunks_mut(res * BAND_ROWS).enumerate() {
            let band_row0 = bi * BAND_ROWS;
            let lo = r0.max(band_row0);
            let hi = r1.min(band_row0 + BAND_ROWS);
            if lo < hi {
                jobs.push((face, band_row0, band, (lo, hi)));
            }
        }
        let tris = &tris;
        run_jobs(jobs, texels >= PARALLEL_TEXELS, &|face, row0, band, rows| {
            draw_rows(band, row0, rows, res, face, tris, shade)
        });
    }
}

/// A ribbon polygon vertex: position and (u, v, intensity).
#[derive(Clone, Copy, Debug)]
struct PolyVert {
    p: Vec3,
    a: [f32; 3],
}

/// `clip_to_face` for vertices with attributes (0xFC2C30 interpolates every
/// vertex field along the clipped edges).
fn clip_poly_to_face(poly: &[PolyVert], face: usize) -> Vec<PolyVert> {
    let (ua, va, na, s) = face_axes(face);
    let dist = |k: usize, p: Vec3| {
        let a = p.to_array();
        let n = s * a[na];
        match k {
            0 => n - a[ua],
            1 => n + a[ua],
            2 => n - a[va],
            _ => n + a[va],
        }
    };
    let lerp = |a: &PolyVert, b: &PolyVert, t: f32| PolyVert {
        p: a.p + (b.p - a.p) * t,
        a: [0, 1, 2].map(|i| a.a[i] + (b.a[i] - a.a[i]) * t),
    };
    let mut out: Vec<PolyVert> = poly.to_vec();
    for k in 0..4 {
        if out.is_empty() {
            break;
        }
        let input = std::mem::take(&mut out);
        let mut prev = *input.last().unwrap();
        let mut dp = dist(k, prev.p);
        for &cur in &input {
            let dc = dist(k, cur.p);
            if dc >= 0.0 {
                if dp < 0.0 {
                    out.push(lerp(&prev, &cur, dp / (dp - dc)));
                }
                out.push(cur);
            } else if dp >= 0.0 {
                out.push(lerp(&prev, &cur, dp / (dp - dc)));
            }
            prev = cur;
            dp = dc;
        }
    }
    out
}

/// Draw ribbon polygons (0xF6E0D0: each polygon clipped to every face it
/// touches, projected to the face, fanned from vertex 0; brush uv and
/// intensity interpolated linearly).
fn raster_polys(buf: &mut [u16], res: usize, polys: &[Vec<PolyVert>], shade: &Shade) {
    let n = res as f32;
    for face in 0..6 {
        let mut tris: Vec<Tri> = Vec::new();
        for poly in polys {
            let clipped = clip_poly_to_face(poly, face);
            if clipped.len() < 3 {
                continue;
            }
            let pts: Vec<((f32, f32), PolyVert)> = clipped
                .iter()
                .map(|v| {
                    let (u, w) = on_face_uv(face, v.p);
                    ((u * n, w * n), *v)
                })
                .collect();
            for i in 1..pts.len() - 1 {
                let (a, b, c) = (&pts[0], &pts[i], &pts[i + 1]);
                if let Some(t) = Tri::new_i(
                    [a.0, b.0, c.0],
                    [(a.1.a[0], a.1.a[1]), (b.1.a[0], b.1.a[1]), (c.1.a[0], c.1.a[1])],
                    [a.1.a[2], b.1.a[2], c.1.a[2]],
                ) {
                    tris.push(t);
                }
            }
        }
        if tris.is_empty() {
            continue;
        }
        let ymin = tris.iter().map(|t| t.ymin).fold(f32::MAX, f32::min);
        let ymax = tris.iter().map(|t| t.ymax).fold(f32::MIN, f32::max);
        let r0 = (ymin - 0.5).ceil().clamp(0.0, n) as usize;
        let r1 = (ymax - 0.5).ceil().clamp(0.0, n) as usize;
        if r0 >= r1 {
            continue;
        }
        let face_buf = &mut buf[face * res * res..(face + 1) * res * res];
        let mut jobs = Vec::new();
        for (bi, band) in face_buf.chunks_mut(res * BAND_ROWS).enumerate() {
            let band_row0 = bi * BAND_ROWS;
            let lo = r0.max(band_row0);
            let hi = r1.min(band_row0 + BAND_ROWS);
            if lo < hi {
                jobs.push((face, band_row0, band, (lo, hi)));
            }
        }
        let tris = &tris;
        run_jobs(jobs, (r1 - r0) * res >= PARALLEL_TEXELS, &|face, row0, band, rows| {
            draw_rows(band, row0, rows, res, face, tris, shade)
        });
    }
}

/// Whole-planet pass (0xF4BC70 -> 0xF6DEF0: one quad per face whose brush uv
/// is the constant (0, 1)).
fn global_pass(buf: &mut [u16], res: usize, shade: &Shade) {
    let mut jobs = Vec::new();
    for (face, slice) in buf.chunks_mut(res * res).enumerate() {
        for (bi, band) in slice.chunks_mut(res * BAND_ROWS).enumerate() {
            let row0 = bi * BAND_ROWS;
            let rows = (row0, (row0 + BAND_ROWS).min(res));
            jobs.push((face, row0, band, rows));
        }
    }
    run_jobs(jobs, res * res >= PARALLEL_TEXELS, &|face, row0, band, rows| {
        for row in rows.0..rows.1 {
            for col in 0..res {
                let i = (row - row0) * res + col;
                band[i] = shade.texel(band[i], 0.0, 1.0, face, row, col, res);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Script playback: a small Swarm effects world (visual effects, spBrush and
// metaParticles components), stepped like the offline world of 0x7DAEE0.

/// A Swarm transform: offset, rows (local axes in world space), uniform scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub centre: Vec3,
    pub rows: [Vec3; 3],
    pub scale: f32,
}

impl Frame {
    pub const IDENTITY: Self = Self {
        centre: Vec3::ZERO,
        rows: [Vec3::X, Vec3::Y, Vec3::Z],
        scale: 1.0,
    };

    /// Local point -> world (rows, then scale, then offset).
    pub fn point(&self, l: Vec3) -> Vec3 {
        self.centre + apply_rows(&self.rows, l * self.scale)
    }

    /// `rel` applied inside this frame (Transform multiply 0x537F90).
    pub fn compose(&self, rel: &Frame) -> Frame {
        Frame {
            centre: self.point(rel.centre),
            rows: mul_rows(&rel.rows, &self.rows),
            scale: self.scale * rel.scale,
        }
    }

    /// A VisualEffectBlock transform (flags 1 scale, 2 rotation, 4 offset).
    fn of_block(t: &crate::planet_data::BlockTransform) -> Frame {
        let mut f = Frame::IDENTITY;
        if t.flags & 4 != 0 {
            f.centre = t.offset;
        }
        if t.flags & 2 != 0 {
            f.rows = t.rows;
        }
        if t.flags & 1 != 0 && t.scale > 0.0 {
            f.scale = t.scale;
        }
        f
    }
}

/// Emission state of one brush component (0xF4C2C0): accumulator, clock.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Emitter {
    acc: f32,
    t: f32,
    pub dead: bool,
}

#[cfg(test)]
impl Emitter {
    /// One update of `STEP_DT`: the ages of the stamps emitted (empty for
    /// most updates). Kills the emitter at the first stamp past its life.
    /// (`Engine::brush_update` runs the same schedule inline.)
    pub fn update(&mut self, life: f32, rate: f32) -> Vec<f32> {
        let dt = STEP_DT;
        if self.dead {
            return Vec::new();
        }
        if !(rate > 0.0) || !rate.is_finite() {
            // ADAPTED: such a brush never stamps nor dies in retail.
            self.dead = true;
            return Vec::new();
        }
        let inv_rate = 1.0 / rate;
        let inv_life = 1.0 / life;
        let step = dt.min(1.0);
        // 0x5E4D60: x87 round to nearest (even).
        let n = (f64::from(self.acc + step) * f64::from(rate)).round_ties_even().max(0.0) as i64;
        let mut ages = Vec::new();
        for _ in 0..n {
            self.t += inv_rate;
            let age = self.t * inv_life;
            if age > 1.0 {
                self.dead = true;
                return ages;
            }
            ages.push(age);
        }
        self.acc = (self.acc + step) - n as f32 * inv_rate;
        ages
    }
}

/// One recorded ribbon point (0x1C bytes: position, intensity, size).
#[derive(Clone, Copy, Debug)]
struct RibbonPt {
    pos: Vec3,
    intensity: f32,
    size: f32,
}

/// One brush component of a playing effect.
struct Inst<'a> {
    brush: &'a Brush,
    raster: Option<&'a BrushRaster>,
    /// Block transform relative to the owning effect.
    rel: Frame,
    /// Previous / current transform (0xF4B6A0 shifts cur into prev).
    prev: Frame,
    cur: Frame,
    /// First transform with |offset| > 0.01 seen (+0x8C).
    placed: bool,
    running: bool,
    /// Level set at the first placement (0xF4B6A0; mode `level` only).
    level: f32,
    emitter: Emitter,
    /// Effect parameter 4 (+0x98): entry intensity or particle alpha.
    param4: f32,
    /// Spacing mode (+0xA8 = spacing list not empty, 0xF4B3E0).
    space_acc: f32,
    space_vary: f32,
    space_revary: bool,
    ribbon: Vec<RibbonPt>,
}

/// One metaParticles component of a playing effect.
struct MetaInst<'a> {
    def: &'a MetaParticleDef,
    rel: Frame,
    source: Frame,
    st: MetaState,
}

enum Comp<'a> {
    Brush(Inst<'a>),
    Meta(Box<MetaInst<'a>>),
}

/// A visual effect instance: an entry effect or a particle's child effect.
struct Ve<'a> {
    comps: Vec<Comp<'a>>,
    running: bool,
}

impl Ve<'_> {
    fn any_running(&self) -> bool {
        self.comps.iter().any(|c| match c {
            Comp::Brush(b) => b.running,
            Comp::Meta(m) => m.st.running,
        })
    }
}

struct Engine<'a> {
    library: &'a BrushLibrary,
    rasters: &'a HashMap<crate::dbpf::ResourceKey, BrushRaster>,
    rng: SporeRng,
    /// Water level W (WTA.x, or the entry's water level).
    water: f32,
    res: usize,
    buf: Vec<u16>,
    stats: GenStats,
    /// World effect slots and their update order (creation order).
    slab: Vec<Option<Ve<'a>>>,
    order: Vec<usize>,
    free: Vec<usize>,
    /// Slots released during the current update (reused after it).
    released: Vec<usize>,
}

impl crate::planet_meta::Rand for SporeRng {
    fn float(&mut self) -> f32 {
        self.random_float()
    }
}

impl<'a> Engine<'a> {
    /// Effect creation: selection groups, then the components in block order
    /// (nested effect blocks flattened with their transforms composed).
    fn collect(&mut self, effect: &'a EffectDef, rel: Frame, depth: u32, out: &mut Vec<Comp<'a>>) {
        if depth > 8 {
            return;
        }
        let (library, rasters) = (self.library, self.rasters);
        // INFERRED: one block per non-zero group, picked with one RandomFloat
        // weighted by the selection chances.
        let mut chosen: HashMap<u16, usize> = HashMap::new();
        let mut groups: Vec<u16> = effect
            .blocks
            .iter()
            .map(|b| b.selection_group)
            .filter(|&g| g != 0)
            .collect();
        groups.sort_unstable();
        groups.dedup();
        for g in groups {
            let members: Vec<usize> = (0..effect.blocks.len())
                .filter(|&i| effect.blocks[i].selection_group == g)
                .collect();
            let total: f32 = members
                .iter()
                .map(|&i| effect.blocks[i].selection_chance as f32)
                .sum::<f32>()
                .max(1.0);
            let mut pick = self.rng.random_float() * total;
            let mut sel = members[members.len() - 1];
            for &i in &members {
                pick -= effect.blocks[i].selection_chance as f32;
                if pick <= 0.0 {
                    sel = i;
                    break;
                }
            }
            chosen.insert(g, sel);
        }
        for (i, block) in effect.blocks.iter().enumerate() {
            if block.selection_group != 0 && chosen.get(&block.selection_group) != Some(&i) {
                continue;
            }
            let f = rel.compose(&Frame::of_block(&block.transform));
            match block.kind {
                KIND_EFFECT if block.index >= 0 && block.index & crate::planet_data::IMPORT_MASK == 0 => {
                    if let Some(e) = library.effects.get(block.index as usize) {
                        self.collect(e, f, depth + 1, out);
                    }
                }
                KIND_BRUSH => {
                    if let Some(brush) = library.brushes.get(block.index.max(0) as usize) {
                        // A texture that does not load leaves the sampler
                        // empty: the gaussian programs (INFERRED).
                        let raster = match brush.texture {
                            Some(key) => {
                                let r = rasters.get(&key);
                                if r.is_none() {
                                    self.stats.missing_rasters += 1;
                                }
                                r
                            }
                            None => None,
                        };
                        out.push(Comp::Brush(Inst {
                            brush,
                            raster,
                            rel: f,
                            prev: Frame::IDENTITY,
                            cur: Frame::IDENTITY,
                            placed: false,
                            running: false,
                            level: 0.0,
                            emitter: Emitter::default(),
                            param4: 1.0,
                            space_acc: 0.0,
                            space_vary: 1.0,
                            space_revary: true,
                            ribbon: Vec::new(),
                        }));
                    }
                }
                KIND_META => {
                    if let Some(def) = library.metas.get(block.index.max(0) as usize) {
                        self.stats.metas += 1;
                        out.push(Comp::Meta(Box::new(MetaInst {
                            def,
                            rel: f,
                            source: f,
                            st: MetaState::new(def),
                        })));
                    }
                }
                _ => {}
            }
        }
    }

    fn new_ve(&mut self, effect: &'a EffectDef) -> Ve<'a> {
        let mut comps = Vec::new();
        self.collect(effect, Frame::IDENTITY, 0, &mut comps);
        Ve { comps, running: true }
    }

    /// Put an effect into the world (appended to the update order: effects
    /// created during an update run later in the same world update, 0xA72140).
    fn add_ve(&mut self, ve: Ve<'a>) -> usize {
        let id = if let Some(id) = self.free.pop() {
            self.slab[id] = Some(ve);
            id
        } else {
            self.slab.push(Some(ve));
            self.slab.len() - 1
        };
        self.order.push(id);
        id
    }

    fn release(&mut self, id: usize) {
        if self.slab.get(id).is_some_and(|s| s.is_some()) {
            self.slab[id] = None;
            self.released.push(id);
        }
    }

    fn ve_running(&self, id: usize) -> bool {
        self.slab.get(id).and_then(|s| s.as_ref()).is_some_and(|v| v.running)
    }

    /// SetSourceTransform -> 0xA82CF0: every component gets VE x block.
    fn ve_set_transforms(&mut self, ve: &mut Ve<'a>, frame: Frame) {
        for c in ve.comps.iter_mut() {
            match c {
                Comp::Brush(b) => self.brush_set_transform(b, frame),
                Comp::Meta(m) => Self::meta_set_transforms(m, frame),
            }
        }
    }

    /// SetFloatParams(4, ...) on every component.
    fn ve_set_param4(ve: &mut Ve<'a>, value: f32) {
        for c in ve.comps.iter_mut() {
            match c {
                Comp::Brush(b) => b.param4 = value,
                Comp::Meta(m) => m.st.alpha_scale = value,
            }
        }
    }

    fn ve_start(&mut self, ve: &mut Ve<'a>) {
        for c in ve.comps.iter_mut() {
            match c {
                Comp::Brush(b) => {
                    // 0xF4C280: running, clock 0, ribbon cleared.
                    if !b.running {
                        b.running = true;
                        b.emitter.t = 0.0;
                        b.ribbon.clear();
                    }
                }
                Comp::Meta(m) => self.meta_start(m),
            }
        }
        ve.running = ve.any_running();
    }

    /// Stop(0): brushes stop (drawing their ribbon), metas stop emitting.
    fn ve_stop(&mut self, ve: &mut Ve<'a>) {
        for c in ve.comps.iter_mut() {
            match c {
                Comp::Brush(b) => self.brush_stop(b),
                Comp::Meta(m) => m.st.stopped = true,
            }
        }
        ve.running = ve.any_running();
    }

    fn ve_update(&mut self, ve: &mut Ve<'a>, dt: f32) {
        for c in ve.comps.iter_mut() {
            match c {
                Comp::Brush(b) => {
                    if b.running {
                        self.brush_update(b, dt);
                    }
                }
                Comp::Meta(m) => {
                    if m.st.running {
                        self.meta_update(m, dt);
                    }
                }
            }
        }
        ve.running = ve.any_running();
    }

    /// One world update: every live effect in creation order, including the
    /// effects created during this update.
    fn tick(&mut self, dt: f32) {
        let mut i = 0;
        while i < self.order.len() {
            let id = self.order[i];
            i += 1;
            let Some(mut ve) = self.slab[id].take() else { continue };
            if ve.running {
                self.ve_update(&mut ve, dt);
            }
            self.slab[id] = Some(ve);
        }
        let slab = &self.slab;
        self.order.retain(|&id| slab[id].is_some());
        self.free.append(&mut self.released);
    }

    // -- spBrush ------------------------------------------------------------

    /// 0xF4B6A0: prev <- cur, cur <- new; the first placement (|offset| >
    /// 0.01) sets level = drawLevel * vary(1, levelVary), or for drawLevel < -1
    /// the current height at the component position times the vary.
    fn brush_set_transform(&mut self, b: &mut Inst<'a>, frame: Frame) {
        let new = frame.compose(&b.rel);
        b.prev = b.cur;
        b.cur = new;
        if !b.placed {
            b.prev = new;
            if new.centre.length() > 0.01 {
                b.placed = true;
                if b.brush.draw_mode == DrawMode::Level {
                    let vary = self.rng.vary(1.0, b.brush.tex_vary);
                    b.level = if b.brush.draw_level < -1.0 {
                        self.height_at(new.centre) * vary
                    } else {
                        b.brush.draw_level * vary
                    };
                }
            }
        }
    }

    /// 0xF4EEE0: stop; a ribbon brush draws its ribbon now (0xF4D980).
    fn brush_stop(&mut self, b: &mut Inst<'a>) {
        if !b.running {
            return;
        }
        b.running = false;
        b.emitter.dead = true;
        if b.brush.use_ribbon {
            self.draw_ribbon(b);
        }
    }

    /// GPU read-back 0xF67A50: nearest-lower texel, h = 2H/65535 - 1.
    fn height_at(&self, pos: Vec3) -> f32 {
        let (face, u, v) = face_uv(pos);
        let n = self.res;
        let col = ((u * n as f32).max(0.0) as usize).min(n - 1);
        let row = ((v * n as f32).max(0.0) as usize).min(n - 1);
        self.buf[(face * n + row) * n + col] as f32 * (2.0 / 65535.0) - 1.0
    }

    fn shade(&self, b: &Inst<'a>, program: Program, intensity: f32, cube_rot: Mat3) -> Shade<'a> {
        let brush = b.brush;
        let (off_level, off_cond) = if brush.water_level_is_zero {
            (2.0 * self.water - 1.0, self.water - 0.5)
        } else {
            (0.0, 0.0)
        };
        let range_off = match brush.draw_mode {
            DrawMode::AbsoluteMax | DrawMode::AbsoluteMin => 0.0,
            _ => off_level,
        };
        let (lo, hi) = (brush.draw_range[0] + range_off, brush.draw_range[1] + range_off);
        let (params, kernel) = match filter_kernel(brush) {
            Some((k, norm, mode)) if matches!(program, Program::Filter | Program::FilterErosion) => {
                let mut kernel = [0.0; 10];
                kernel[..9].copy_from_slice(&k);
                kernel[9] = norm;
                (filter_params(mode), kernel)
            }
            _ if program == Program::FilterErosion => (filter_params(0), [0.0; 10]),
            _ => (
                vertex_params(brush.draw_mode, intensity, b.level + off_level, lo, hi),
                [0.0; 10],
            ),
        };
        Shade {
            program,
            params,
            cond: CondRamp::new(&brush.cond, off_cond),
            raster: b.raster,
            cube_rot,
            kernel,
            snapshot: None,
            vertex_intensity: false,
        }
    }

    /// One world update of one brush (0xF4C2C0): stamps by rate (or by
    /// distance in spacing mode), each at its fraction of the prev -> cur
    /// segment of this update.
    fn brush_update(&mut self, b: &mut Inst<'a>, dt: f32) {
        let brush = b.brush;
        let Some(program) = program(brush, b.raster.is_some()) else {
            self.stats.unsupported_filters += 1;
            b.running = false;
            b.emitter.dead = true;
            return;
        };
        if brush.size_global || brush.tex_cube {
            let intensity = self.rng.vary(1.0, brush.intensity_vary)
                * brush.intensity.first().copied().unwrap_or(1.0)
                * b.param4;
            self.rng.random_float();
            let rot = [0, 1, 2].map(|i| self.rng.vary(brush.rotate[i], brush.rotate_vary[i]));
            // INFERRED Euler order (0xF6A6C0: X, Y, Z rotations, degrees).
            let cube_rot = Mat3::from_euler(
                EulerRot::XYZ,
                rot[0].to_radians(),
                rot[1].to_radians(),
                rot[2].to_radians(),
            );
            let base = self.shade(b, program, intensity, cube_rot);
            let snap = matches!(program, Program::Filter | Program::FilterErosion).then(|| self.buf.clone());
            let shade = Shade {
                snapshot: snap.as_deref(),
                ..base
            };
            global_pass(&mut self.buf, self.res, &shade);
            self.stats.global_passes += 1;
            self.brush_stop(b);
            return;
        }
        let spacing = !brush.spacing.is_empty();
        if !spacing && (!(brush.rate > 0.0) || !brush.rate.is_finite()) {
            // ADAPTED: such a brush never stamps nor dies in retail.
            self.brush_stop(b);
            return;
        }
        let step = dt.min(1.0);
        let acc0 = b.emitter.acc;
        let inv_life = 1.0 / brush.life;
        let mut t_per = 1.0 / brush.rate;
        // 0x5E4D60: x87 round to nearest (even).
        let mut n = (f64::from(acc0 + step) * f64::from(brush.rate)).round_ties_even().max(0.0) as i64;
        let (mut f0, mut df) = (0.0f32, 0.0f32);
        if spacing {
            if b.space_revary {
                b.space_revary = false;
                b.space_vary = self.rng.vary(1.0, brush.spacing_vary);
            }
            let size0 = curve(&brush.size, 0.0);
            let age_f = ((acc0 + step) * inv_life).clamp(0.0, 1.0);
            let d_stamp = size0 * (b.space_vary * curve(&brush.spacing, age_f)) * b.cur.scale;
            // INFERRED: chord between the unit directions, times 500.
            let dist = (b.prev.centre.normalize_or_zero() - b.cur.centre.normalize_or_zero()).length() * 500.0;
            if !(d_stamp > 0.0) {
                // ADAPTED: retail rounds a NaN / infinite count to INT_MIN.
                n = 0;
            } else if b.space_acc + dist < d_stamp {
                b.space_acc += dist;
                n = 0;
            } else {
                n = (f64::from((b.space_acc + dist) / d_stamp)).round_ties_even() as i64;
                b.space_revary = true;
                if dist > 0.0 {
                    f0 = (d_stamp - b.space_acc) / dist;
                    df = d_stamp / dist;
                }
                b.space_acc += dist;
                b.space_acc -= n as f32 * d_stamp;
                if n != 0 {
                    t_per = step / n as f32;
                }
            }
        }
        for k in 0..n.max(0) {
            b.emitter.t += t_per;
            let age = b.emitter.t * inv_life;
            if age > 1.0 {
                self.brush_stop(b);
                return;
            }
            let mut f = (k as f32 * t_per - acc0) / step;
            if spacing {
                f = f0;
                f0 += df;
            }
            let f = f.clamp(0.0, 1.0);
            let intensity = self.rng.vary(1.0, brush.intensity_vary) * curve(&brush.intensity, age) * b.param4;
            let size = self.rng.vary(1.0, brush.size_vary) * curve(&brush.size, age) * b.cur.scale;
            if brush.rotate.iter().any(|&r| r != 0.0) {
                // Computed by 0xF4C2C0 but never used (dead store).
                for i in 0..3 {
                    self.rng.vary(brush.rotate[0], brush.rotate_vary[i]);
                }
            }
            let (centre, rows) = interp_frame(&b.prev, &b.cur, f);
            if brush.use_ribbon {
                // The ribbon records a point when it moved 0.001 on any axis.
                let moved = b.ribbon.last().is_none_or(|l| {
                    (centre.x - l.pos.x).abs() >= 0.001
                        || (centre.y - l.pos.y).abs() >= 0.001
                        || (centre.z - l.pos.z).abs() >= 0.001
                });
                if moved {
                    b.ribbon.push(RibbonPt {
                        pos: centre,
                        intensity,
                        size,
                    });
                }
                continue;
            }
            let spin = if brush.rotate_vary[0] != 0.0 {
                self.rng.vary(0.0, brush.rotate_vary[0])
            } else {
                0.0
            };
            let Some(geom) = StampGeom::new(centre, rows, size, spin) else {
                continue;
            };
            let base = self.shade(b, program, intensity, Mat3::IDENTITY);
            let snap = matches!(program, Program::Filter | Program::FilterErosion).then(|| self.buf.clone());
            let shade = Shade {
                snapshot: snap.as_deref(),
                ..base
            };
            raster_stamp(&mut self.buf, self.res, &geom, &shade);
            self.stats.stamps += 1;
        }
        b.emitter.acc = acc0 + step - n as f32 * t_per;
    }

    /// 0xF4D980: the recorded points (every `skip`-th) become two Hermite
    /// paths (position; size and intensity), re-parametrised uniformly and
    /// sampled `steps` times (rounded up to a multiple of `tiles`); each step
    /// is a quad of half-width `size` across the path (tangent x radial), the
    /// texture's v split into a start cap (0..1/4), repeated middle tiles
    /// (1/4..3/4) and an end cap (3/4..1), u = one of `strips` columns.
    fn draw_ribbon(&mut self, b: &mut Inst<'a>) {
        let brush = b.brush;
        let [tiles, steps_in, strips, skip] = brush.ribbon;
        let skip = skip.max(1) as usize;
        let pts: Vec<RibbonPt> = b.ribbon.iter().step_by(skip).copied().collect();
        b.ribbon.clear();
        let tiles = tiles.max(1);
        let mut steps = steps_in.max(0);
        if steps % tiles != 0 {
            steps = (steps / tiles + 1) * tiles;
        }
        let spt = steps / tiles;
        // RandomInt(numTexStrips) is drawn even for an empty ribbon.
        let strip = self.rng.random_int(strips.max(0) as u32) as f32;
        if pts.len() < 2 || steps <= 0 {
            return;
        }
        let mut path1 = build_path(&pts.iter().map(|p| p.pos).collect::<Vec<_>>());
        let mut path2 = build_path(&pts.iter().map(|p| Vec3::new(p.size, p.intensity, 0.0)).collect::<Vec<_>>());
        for path in [&mut path1, &mut path2] {
            let n = path.len();
            let total = path[n - 1].len;
            for (i, r) in path.iter_mut().enumerate() {
                r.len = i as f32 / (n - 1) as f32 * total;
            }
        }
        let mut s1 = PathState::new(&path1);
        let mut s2 = PathState {
            pos: Vec3::ZERO,
            ..PathState::new(&path2)
        };
        s1.pos = Vec3::ZERO;
        s1.eval(0.0, &path1);
        s2.eval(0.0, &path2);
        let (d1, d2) = ((s1.end - 0.001) / steps as f32, (s2.end - 0.001) / steps as f32);
        let half = spt / 2;
        let strips_f = strips.max(1) as f32;
        let (u_lo, u_hi) = (strip / strips_f, (strip + 1.0) / strips_f);
        let mut polys: Vec<Vec<PolyVert>> = Vec::with_capacity(steps as usize);
        let (mut q1, mut q2) = (s1, s2);
        for i in 0..steps {
            s1.eval(d1, &path1);
            s2.eval(d2, &path2);
            let (v0, v1) = if i < half {
                (i as f32 / half as f32 * 0.25, (i + 1) as f32 / half as f32 * 0.25)
            } else if i < spt * tiles - half {
                let k = (i - half) % spt;
                (
                    k as f32 / spt as f32 * 0.5 + 0.25,
                    (k + 1) as f32 / spt as f32 * 0.5 + 0.25,
                )
            } else {
                let j = half - spt * tiles + i;
                (j as f32 / half as f32 * 0.25 + 0.75, (j + 1) as f32 / half as f32 * 0.25 + 0.75)
            };
            let side = |pos: Vec3, tan: Vec3, size: f32| tan.normalize().cross(pos.normalize()) * size;
            let sp = side(q1.pos, q1.tan, q2.pos.x);
            let sc = side(s1.pos, s1.tan, s2.pos.x);
            let quad = vec![
                PolyVert {
                    p: q1.pos - sp,
                    a: [u_lo, v0, q2.pos.y],
                },
                PolyVert {
                    p: q1.pos + sp,
                    a: [u_hi, v0, q2.pos.y],
                },
                PolyVert {
                    p: s1.pos + sc,
                    a: [u_hi, v1, s2.pos.y],
                },
                PolyVert {
                    p: s1.pos - sc,
                    a: [u_lo, v1, s2.pos.y],
                },
            ];
            if quad.iter().all(|v| v.p.is_finite() && v.a.iter().all(|x| x.is_finite())) {
                polys.push(quad);
            }
            q1 = s1;
            q2 = s2;
        }
        let Some(program) = program(brush, b.raster.is_some()) else { return };
        let shade = Shade {
            vertex_intensity: true,
            ..self.shade(b, program, 1.0, Mat3::IDENTITY)
        };
        raster_polys(&mut self.buf, self.res, &polys, &shade);
        self.stats.ribbons += 1;
    }

    // -- metaParticles ------------------------------------------------------

    /// 0xA9BAD0: source = effect x block; surfaces re-initialised.
    fn meta_set_transforms(m: &mut MetaInst<'a>, frame: Frame) {
        m.source = frame.compose(&m.rel);
        m.st.surfaces = m
            .def
            .surfaces
            .iter()
            .filter_map(|s| Sphere::from_def(s, &m.source))
            .collect();
    }

    /// 0xA9B1C0 with a soft start (the terrain player starts its entries with
    /// Start(0), so there is no preroll): clocks reset, emit delay drawn.
    fn meta_start(&mut self, m: &mut MetaInst<'a>) {
        let d = m.def;
        if d.component.0 < 0 {
            return;
        }
        m.st.rate_clock = 0.0;
        m.st.stopped = false;
        m.st.clock = 0.0;
        m.st.running = true;
        m.st.loop_count = 0;
        m.st.emit_acc = 0.0;
        m.st.force = Vec3::from_array(d.direction_forces);
        m.st.delay = if d.emit_delay[0] >= 0.0 {
            self.rng.range(d.emit_delay[0], d.emit_delay[1])
        } else if d.emit_retrigger[0] >= 0.0 {
            self.rng.range(d.emit_retrigger[0], d.emit_retrigger[1])
        } else {
            0.0
        };
        m.st.first_update = false;
    }

    /// One update of a meta component (0xAA1560): existing particles move,
    /// new ones are emitted (and moved by the time since their birth), then
    /// every child effect gets the particle transform and alpha (0xA9F0C0).
    fn meta_update(&mut self, m: &mut MetaInst<'a>, dt: f32) {
        let d = m.def;
        let mut dt = dt;
        if d.has(bit::RATE_HOLD) && !m.st.stopped {
            let hold = (1.0 - m.st.rate_clock as f32) * d.rate_loop - 0.01;
            dt = if hold > 0.0 { dt.min(hold) } else { 0.0 };
        }
        m.st.clock += f64::from(dt);
        // Force accumulator (+0xCC): wind / gravity are 0 in the offline world;
        // the direction forces (rigid transform identity).
        m.st.force = Vec3::from_array(d.direction_forces);
        // Particles, newest first.
        let mut i = m.st.particles.len();
        while i > 0 {
            i -= 1;
            let mut p = m.st.particles[i].clone();
            let alive = self.particle_update(m, &mut p, dt);
            if alive {
                m.st.particles[i] = p;
            } else {
                m.st.particles.remove(i);
                self.free_particle(m, p);
            }
        }
        // Emission.
        if !m.st.stopped {
            let mut e = dt;
            let mut emit = true;
            if m.st.delay > 0.0 {
                m.st.delay -= dt;
                if m.st.delay > 0.0 {
                    emit = false;
                } else {
                    e = -m.st.delay;
                }
            }
            if emit {
                let pre = d.preroll;
                if pre > 0.01 && pre - 0.01 < e {
                    e = pre - 0.01;
                }
                if d.has(bit::MAINTAIN) {
                    self.emit_maintain(m, e);
                } else if d.has(bit::INJECT) {
                    self.emit_inject(m, e);
                } else {
                    self.emit_rate(m, e);
                }
            }
        }
        // Child transforms and parameters, newest first.
        for idx in (0..m.st.particles.len()).rev() {
            let p = &m.st.particles[idx];
            let Some(c) = p.child else { continue };
            if !self.ve_running(c) {
                continue;
            }
            let frac = p.age / p.lifetime;
            let size = p.size_vary * pm_curve(&d.size, frac);
            let rows = particle_rows(d, p, frac);
            let alpha = m.st.alpha_scale * p.alpha_vary * pm_curve(&d.alpha, frac);
            let frame = Frame {
                centre: p.pos,
                rows,
                scale: size,
            };
            let Some(mut ve) = self.slab[c].take() else { continue };
            Self::ve_set_param4(&mut ve, alpha);
            self.ve_set_transforms(&mut ve, frame);
            self.slab[c] = Some(ve);
        }
        // Orphans that stopped running are released (0xAA0550).
        let mut k = 0;
        while k < m.st.orphans.len() {
            let c = m.st.orphans[k];
            if self.ve_running(c) {
                k += 1;
            } else {
                m.st.orphans.remove(k);
                self.release(c);
            }
        }
        if m.st.stopped && m.st.particles.is_empty() && m.st.orphans.is_empty() {
            m.st.running = false;
        }
    }

    /// 0xAA0600: the child is stopped (soft); one still running is kept as an
    /// orphan until it stops, otherwise released.
    fn free_particle(&mut self, m: &mut MetaInst<'a>, p: Particle) {
        let Some(c) = p.child else { return };
        let Some(mut ve) = self.slab[c].take() else { return };
        self.ve_stop(&mut ve);
        let running = ve.running;
        self.slab[c] = Some(ve);
        if running {
            m.st.orphans.push(c);
        } else {
            self.release(c);
        }
    }

    /// 0xAA11E0: `inject` emits ceil(rate[0]) particles per rate loop, each
    /// moved by the time since its loop started; `rateCurveCycles` loops.
    fn emit_inject(&mut self, m: &mut MetaInst<'a>, e: f32) {
        let d = m.def;
        let clock = e * m.st.rate_speed as f32 + m.st.rate_clock as f32;
        m.st.rate_clock = f64::from(clock);
        if (m.st.loop_count as f32) > clock {
            return;
        }
        let r = d.rate.first().copied().unwrap_or(0.0);
        let count = {
            let c = r.round_ties_even();
            (if c < r { c + 1.0 } else { c }) as i64
        };
        while m.st.rate_clock >= f64::from(m.st.loop_count) {
            if d.has(bit::RATE_KILL) {
                let parts = std::mem::take(&mut m.st.particles);
                for p in parts {
                    self.free_particle(m, p);
                }
            }
            let t = (m.st.rate_clock as f32 - m.st.loop_count as f32) * d.rate_loop;
            if t < d.life[1] {
                for _ in 0..count.max(0) {
                    if let Some(mut p) = self.emit_particle(m) {
                        if self.particle_update(m, &mut p, t) {
                            m.st.particles.push(p);
                        } else {
                            self.free_particle(m, p);
                        }
                    }
                }
            }
            m.st.loop_count += 1;
            if d.rate_curve_cycles != 0 && d.rate_curve_cycles as u32 <= m.st.loop_count {
                if !d.has(bit::RATE_HOLD) {
                    m.st.stopped = true;
                }
                return;
            }
            if d.emit_retrigger[0] >= 0.0 {
                let r = self.rng.range(d.emit_retrigger[0], d.emit_retrigger[1]);
                let c = m.st.rate_clock as f32 - r * m.st.rate_speed as f32;
                m.st.rate_clock = f64::from(c);
                if c < m.st.loop_count as f32 {
                    m.st.delay = (m.st.loop_count as f32 - c) * d.rate_loop;
                    m.st.rate_clock = f64::from(m.st.loop_count);
                    return;
                }
            }
        }
    }

    /// 0xAA1440: `maintain` keeps ceil(rate[0]) particles alive.
    fn emit_maintain(&mut self, m: &mut MetaInst<'a>, e: f32) {
        let d = m.def;
        let clock = e * m.st.rate_speed as f32 + m.st.rate_clock as f32;
        m.st.rate_clock = f64::from(clock);
        if (m.st.loop_count as f32) > clock {
            return;
        }
        let r = d.rate.first().copied().unwrap_or(0.0);
        let c = r.round_ties_even();
        let want = (if c < r { c + 1.0 } else { c }) as i64;
        for _ in 0..(want - m.st.particles.len() as i64).max(0) {
            if let Some(mut p) = self.emit_particle(m) {
                if self.particle_update(m, &mut p, e) {
                    m.st.particles.push(p);
                } else {
                    self.free_particle(m, p);
                }
            }
        }
    }

    /// 0xAA0E20: continuous rate (curve over the loop), sub-stepped at 1/15 s;
    /// each new particle is moved by its time since birth.
    fn emit_rate(&mut self, m: &mut MetaInst<'a>, e: f32) {
        let d = m.def;
        let scale = 1.0f32;
        let mut rem = e;
        loop {
            let step = if rem <= MAX_SUBSTEP { rem } else { MAX_SUBSTEP };
            let next = m.st.loop_count + 1;
            let clock = step * m.st.rate_speed as f32 + m.st.rate_clock as f32;
            m.st.rate_clock = f64::from(clock);
            let mut retrig = false;
            let mut step_e = step;
            if (next as f32) < clock {
                m.st.loop_count = next;
                if d.has(bit::RATE_KILL) {
                    let parts = std::mem::take(&mut m.st.particles);
                    for p in parts {
                        self.free_particle(m, p);
                    }
                }
                let cycles = d.rate_curve_cycles as u32;
                if cycles == 0 || m.st.loop_count != cycles {
                    if d.emit_retrigger[0] >= 0.0 {
                        retrig = true;
                        step_e -= (m.st.rate_clock as f32 - m.st.loop_count as f32) * d.rate_loop;
                        m.st.rate_clock = f64::from(m.st.loop_count);
                    }
                } else if d.flags >> 2 & 1 == 0 {
                    if !d.has(bit::RATE_HOLD) {
                        m.st.stopped = true;
                    }
                    return;
                } else {
                    m.st.loop_count = cycles - 1;
                    m.st.rate_clock = f64::from(cycles as f32 - 1e-6);
                }
            }
            let frac = m.st.rate_clock as f32 - m.st.loop_count as f32;
            let rate = pm_curve(&d.rate, frac);
            let acc = rate * step_e * scale + m.st.emit_acc;
            let n = acc.floor();
            m.st.emit_acc = acc - n;
            let mut age = rem;
            for _ in 0..(n as i64).max(0) {
                if let Some(mut p) = self.emit_particle(m) {
                    if self.particle_update(m, &mut p, age) {
                        m.st.particles.push(p);
                    } else {
                        self.free_particle(m, p);
                    }
                }
                age -= step_e / n;
            }
            rem -= step_e;
            if retrig {
                let r = self.rng.range(d.emit_retrigger[0], d.emit_retrigger[1]);
                rem -= r;
                if rem < 0.0 {
                    m.st.delay = -rem;
                    return;
                }
            }
            if rem <= 0.0 {
                return;
            }
        }
    }

    /// 0xA9DD20: a new particle (life, position, velocity, varies), its child
    /// effect created, pinned to a surface, placed and started.
    fn emit_particle(&mut self, m: &MetaInst<'a>) -> Option<Particle> {
        let d = m.def;
        let src = m.source;
        let lifetime = self.rng.range(d.life[0], d.life[1]);
        let (lo, hi) = (Vec3::from_array(d.emit_volume[0]), Vec3::from_array(d.emit_volume[1]));
        let pos_local = if d.has(bit::SOURCE_ROUND) && d.torus_width < 0.0 {
            // 0xA814E0: rejection sampling inside the box's ellipsoid.
            let c = (lo + hi) * 0.5;
            let r = hi - c;
            let inv = |x: f32| if x * x > 0.0 { 1.0 / (x * x) } else { 0.0 };
            let (ix, iy, iz) = (inv(r.x), inv(r.y), inv(r.z));
            let mut tries = 0;
            loop {
                let p = self.range3(lo, hi);
                let q = p - c;
                tries += 1;
                if iz * q.z * q.z + iy * q.y * q.y + ix * q.x * q.x <= 1.0 || tries > 1000 {
                    break p;
                }
            }
        } else {
            // ADAPTED: a torus source (torusWidth >= 0, unused) samples the box.
            self.range3(lo, hi)
        };
        let dir = self.range3(Vec3::from_array(d.emit_direction[0]), Vec3::from_array(d.emit_direction[1]));
        let speed = self.rng.range(d.emit_speed[0], d.emit_speed[1]);
        let mut vel = if dir.length_squared() > 0.0 {
            dir * (speed / dir.length())
        } else {
            dir
        };
        let mut size_vary = self.rng.range(1.0 - d.size_vary, 1.0 + d.size_vary);
        let alpha_vary = self.rng.range(1.0 - d.alpha_vary, 1.0 + d.alpha_vary);
        for c in d.color_vary {
            self.rng.range(1.0 - c, 1.0 + c);
        }
        let pitch_vary = if d.pitch_vary > 0.0 { self.rng.sym(d.pitch_vary) } else { 1.0 };
        let roll_vary = if d.roll_vary > 0.0 { self.rng.sym(d.roll_vary) } else { 1.0 };
        let yaw_vary = if d.yaw_vary > 0.0 { self.rng.sym(d.yaw_vary) } else { 1.0 };
        if d.has(bit::SOURCE_SCALE) {
            size_vary *= src.scale;
        }
        let mut pos = src.point(pos_local);
        // The child effect (0xA71490 -> 0xA713B0).
        let child_def = (d.component.1 == KIND_EFFECT as i32 && d.component.0 >= 0)
            .then(|| self.library.effects.get(d.component.0 as usize))
            .flatten();
        let child_ve = child_def.map(|e| self.new_ve(e));
        // Pin the emission point (surfaces with pin / pinMove / pinEmit).
        let mut frame = IDENTITY_ROWS;
        let mut aligned = false;
        for s in &m.st.surfaces {
            if s.flags & PIN_MASK == 0 {
                continue;
            }
            if let Some((p, n)) = s.closest(pos) {
                pos = p + n * s.pin_offset;
                if d.has(bit::ALIGN_SURFACE) {
                    frame = align_z(n);
                    vel = apply_rows(&frame, vel);
                    aligned = true;
                }
                break;
            }
        }
        if !aligned {
            vel = apply_rows(&src.rows, vel);
        }
        if d.has(bit::EMIT_BASE) && speed != 0.0 {
            let s0 = d.size.first().copied().unwrap_or(1.0);
            pos += vel * 0.5 * s0 * (1.0 / speed);
        }
        let heading = if (2..=4).contains(&d.align_mode) {
            -999.0
        } else if !aligned {
            approx_heading(src.rows[0].x, src.rows[0].y)
        } else {
            0.0
        };
        let child = child_ve.map(|mut ve| {
            self.ve_set_transforms(
                &mut ve,
                Frame {
                    centre: pos,
                    ..Frame::IDENTITY
                },
            );
            self.ve_start(&mut ve);
            self.stats.children += 1;
            self.add_ve(ve)
        });
        let walks = if d.has(bit::RANDOM_WALK) {
            let wait = d.has(bit::RANDOM_WALK_WAIT);
            [
                Walk::init(&d.random_walk, dir, wait, &mut self.rng),
                Walk::init(&d.random_walk2, dir, wait, &mut self.rng),
            ]
        } else {
            [Walk::default(); 2]
        };
        self.stats.particles += 1;
        Some(Particle {
            age: 0.0,
            lifetime,
            pos,
            vel,
            size_vary,
            alpha_vary,
            heading,
            walks,
            child,
            frame,
            pitch_vary,
            roll_vary,
            yaw_vary,
        })
    }

    fn range3(&mut self, lo: Vec3, hi: Vec3) -> Vec3 {
        let x = self.rng.range(lo.x, hi.x);
        let y = self.rng.range(lo.y, hi.y);
        let z = self.rng.range(lo.z, hi.z);
        Vec3::new(x, y, z)
    }

    /// 0xA9DB50: age, then the plain integrator 0xA9B6D0 or the sub-stepped
    /// one 0xA9D590 (random walk, surfaces). False when the particle dies.
    fn particle_update(&mut self, m: &MetaInst<'a>, p: &mut Particle, dt: f32) -> bool {
        let d = m.def;
        p.age += dt;
        if p.lifetime <= p.age {
            return false;
        }
        if d.flags & FORCES_MASK == 0 && d.drag * dt <= 0.05 {
            let v0 = p.vel;
            let v1 = (v0 + m.st.force * dt) * (1.0 - d.drag * dt);
            p.vel = v1;
            p.pos += (v1 + v0) * (dt * 0.5);
            return true;
        }
        let mut rem = dt;
        loop {
            let step = if rem <= MAX_SUBSTEP { rem } else { MAX_SUBSTEP };
            p.pos += p.vel * step;
            let mut f = m.st.force;
            if d.has(bit::RANDOM_WALK) {
                p.walks[0].timer -= step;
                p.walks[1].timer -= step;
                if p.walks[0].timer < 0.0 {
                    p.walks[0].retarget(&d.random_walk, &mut self.rng);
                }
                p.walks[0].apply(&mut f);
                if p.walks[1].timer < 0.0 {
                    p.walks[1].retarget(&d.random_walk2, &mut self.rng);
                }
                p.walks[1].apply(&mut f);
            }
            if d.has(bit::ALIGN_SURFACE) {
                f = apply_rows(&p.frame, f);
            }
            f += Vec3::from_array(d.global_forces);
            p.vel = (p.vel + f * step) * (1.0 - d.drag * step);
            if d.has(bit::SURFACE) {
                apply_surfaces(&m.st.surfaces, p, d.has(bit::ALIGN_SURFACE));
            }
            rem -= step;
            if rem <= 0.0 {
                return true;
            }
        }
    }
}

/// 0xA7F830 per surface: pin (project, drop the inward velocity), pinMove
/// (project, velocity made tangent at the same speed, 0xA7EB00), align only.
/// Bounce / death surfaces are not used by the terrain scripts.
fn apply_surfaces(surfaces: &[Sphere], p: &mut Particle, align: bool) {
    for s in surfaces {
        if s.flags & 2 != 0 {
            if let Some((pp, n)) = s.closest(p.pos) {
                p.pos = pp;
                let vn = -p.vel.dot(n);
                if vn > 0.0 {
                    p.vel += n * vn;
                }
            }
        } else if s.flags & 4 != 0 {
            if let Some((pp, n)) = s.closest(p.pos) {
                p.pos = pp;
                let v = p.vel;
                let vl = v.length();
                if vl > 1e-6 {
                    let t = n.cross(v.cross(n));
                    let tl = t.length();
                    if tl > 1e-6 {
                        if align {
                            let (from, to) = (v / vl, t / tl);
                            p.frame = p.frame.map(|r| rotate_between(from, to, r));
                        }
                        p.vel = t * (vl / tl);
                    }
                }
            }
        } else if s.flags & 1 == 0 && s.flags & 0x10 == 0 && s.flags & 0x20 != 0 && align {
            if let Some((_, n)) = s.closest(p.pos) {
                p.frame = align_z(n);
            }
        }
    }
}

/// The stamp frame at fraction `f` of the prev -> cur segment: the centre
/// lerped (snapped below 0.0001 / above 0.9999), the prev rows rotated with the
/// direction (INFERRED from 0xF4C2C0's slerp of the prev -> cur rotation).
fn interp_frame(prev: &Frame, cur: &Frame, f: f32) -> (Vec3, [Vec3; 3]) {
    if prev.centre == cur.centre && prev.rows == cur.rows {
        return (cur.centre, cur.rows);
    }
    let centre = if f < 0.0001 {
        prev.centre
    } else if f > 0.9999 {
        cur.centre
    } else {
        prev.centre.lerp(cur.centre, f)
    };
    let (a, c) = (prev.centre.normalize_or_zero(), centre.normalize_or_zero());
    if a == Vec3::ZERO || c == Vec3::ZERO {
        return (centre, prev.rows);
    }
    (centre, prev.rows.map(|r| rotate_between(a, c, r)))
}

/// A path record (0x1C bytes): position, unit tangent, cumulative length.
#[derive(Clone, Copy, Debug)]
struct PathRec {
    pos: Vec3,
    tan: Vec3,
    len: f32,
}

/// 0xA7AAA0 (identity transform, speed 1): tangent = normalised central
/// difference (one-sided at the ends), cumulative chord length.
fn build_path(points: &[Vec3]) -> Vec<PathRec> {
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    let mut len = 0.0f32;
    for i in 0..n {
        let d = points[(i + 1).min(n - 1)] - points[i.saturating_sub(1)];
        let dl = d.length();
        let tan = if dl > 1e-6 { d / dl } else { Vec3::ZERO };
        if i >= 1 {
            len += (points[i] - points[i - 1]).length();
        }
        out.push(PathRec {
            pos: points[i],
            tan,
            len,
        });
    }
    out
}

/// Path evaluation state (0xA79EE0): clock, end, position, tangent.
#[derive(Clone, Copy, Debug)]
struct PathState {
    t: f32,
    end: f32,
    pos: Vec3,
    tan: Vec3,
}

impl PathState {
    fn new(path: &[PathRec]) -> Self {
        Self {
            t: 0.0,
            end: path.last().map_or(0.0, |r| r.len),
            pos: Vec3::ZERO,
            tan: path.first().map_or(Vec3::ZERO, |r| r.tan),
        }
    }

    /// Advance by `dt` and evaluate the cubic Hermite segment there.
    fn eval(&mut self, dt: f32, path: &[PathRec]) -> bool {
        let t = self.t + dt;
        self.t = t;
        let mut end = self.end;
        if end <= t || path.is_empty() {
            return false;
        }
        let last = path.len() - 1;
        let mut k = 0;
        while k < last && path[k].len <= t {
            k += 1;
        }
        let r = path[k];
        if r.len != 0.0 {
            end = r.len;
            if r.len < t {
                return false;
            }
        }
        let (p0, m0, l0) = if k > 0 {
            (path[k - 1].pos, path[k - 1].tan, path[k - 1].len)
        } else {
            (path[0].pos, path[0].tan, 0.0)
        };
        let l = end - l0;
        if l > 1e-6 {
            let s = (t - l0) / l;
            let s2 = s * s;
            let s3m = s2 * s - s2;
            let h01 = s2 - s3m * 2.0;
            let h00 = 1.0 - h01;
            let h10 = ((s3m - s2) + s) * l;
            let h11 = s3m * l;
            let dh01 = (s * 2.0 - (s2 * 3.0 - s * 2.0) * 2.0) / l;
            let dh00 = -dh01;
            let dh10 = (s2 * 3.0 - s * 2.0 - s * 2.0) + 1.0;
            let dh11 = s2 * 3.0 - s * 2.0;
            self.pos = p0 * h00 + r.pos * h01 + m0 * h10 + r.tan * h11;
            self.tan = p0 * dh00 + r.pos * dh01 + m0 * dh10 + r.tan * dh11;
        }
        true
    }
}

/// Generate the height cube of a planet source at `res` texels per face
/// (retail: 512; stamps keep their world size at any resolution).
pub fn generate_res(source: &PlanetSource, res: usize) -> (HeightCube, GenStats) {
    let start = std::time::Instant::now();
    let d = &source.descriptor;
    let mut engine = Engine {
        library: &source.library,
        rasters: &source.rasters,
        rng: SporeRng::new(d.seed),
        water: d.wta[0],
        res,
        buf: vec![INITIAL_RAW; 6 * res * res],
        stats: GenStats {
            entries: d.entries.len(),
            ..Default::default()
        },
        slab: Vec::new(),
        order: Vec::new(),
        free: Vec::new(),
        released: Vec::new(),
    };
    // 0x7DAEE0: SetSeed(planet seed) and three LCG steps before the script.
    for _ in 0..3 {
        engine.rng.lcg_step();
    }
    // Development trace: one line per entry (stamps, ribbons, particles, height range).
    let trace = std::env::var("OPENSPORE_PLANET_TRACE").is_ok_and(|v| v != "0");
    for (i, entry) in d.entries.iter().enumerate() {
        let before = (engine.stats.stamps, engine.stats.ribbons, engine.stats.particles);
        if d.reseed_entries {
            engine.rng.set_seed(entry.seed);
        }
        if let Some(&w) = d.entry_water_levels.get(i) {
            engine.water = w;
        }
        let Some(effect) = source.library.effect(entry.effect) else {
            engine.stats.missing_effects += 1;
            continue;
        };
        let t = &entry.transform;
        let frame = Frame {
            centre: t.offset,
            rows: t.rows,
            scale: t.scale,
        };
        // 0xF95590: create, SetSourceTransform, SetFloatParams(4, intensity),
        // Start(0).
        let mut ve = engine.new_ve(effect);
        Engine::ve_set_param4(&mut ve, entry.intensity);
        engine.ve_set_transforms(&mut ve, frame);
        engine.ve_start(&mut ve);
        engine.slab.clear();
        engine.order.clear();
        engine.free.clear();
        engine.released.clear();
        let root = engine.add_ve(ve);
        // terrainScriptEffectTimes: a non-zero time stops the entry early.
        let max_updates = if entry.time > 0.0 {
            ((entry.time / STEP_DT).ceil() as u32).clamp(1, MAX_UPDATES)
        } else {
            MAX_UPDATES
        };
        let mut updates = 0;
        for _ in 0..max_updates {
            if !engine.ve_running(root) {
                break;
            }
            engine.tick(STEP_DT);
            updates += 1;
        }
        if trace {
            let (lo, hi) = engine.buf.iter().fold((u16::MAX, 0u16), |(lo, hi), &r| (lo.min(r), hi.max(r)));
            eprintln!(
                "entry {i:3} {:08x} at {:?}: {} updates, {} stamps, {} ribbons, {} particles, h {:.3}..{:.3}",
                entry.effect,
                t.offset.normalize_or_zero(),
                updates,
                engine.stats.stamps - before.0,
                engine.stats.ribbons - before.1,
                engine.stats.particles - before.2,
                raw_h(lo),
                raw_h(hi)
            );
        }
    }
    let mut stats = engine.stats;
    stats.millis = start.elapsed().as_millis();
    (HeightCube::from_raw(res, engine.buf).unwrap(), stats)
}

/// Generate at the retail resolution (512).
pub fn generate(source: &PlanetSource) -> (HeightCube, GenStats) {
    generate_res(source, FACE_RES)
}

/// Build the planet object for a source.
pub fn build_planet(source: &PlanetSource) -> (Planet, GenStats) {
    let (cube, stats) = generate(source);
    (
        Planet::new(cube, source.descriptor.radius(), source.descriptor.water_level()),
        stats,
    )
}

/// ADAPTED player modification (retail `ITerrain::AddPlayerModification`
/// stamps `brush_tribe_flattener` / `brush_city_flattener`): a GaussianLevel
/// stamp of `half_width` units at `dir` toward the height under its centre.
pub fn flatten_pad(cube: &mut HeightCube, dir: Vec3, half_width: f32, radius: f32) {
    let res = cube.res();
    let normal = dir.normalize_or(Vec3::Y);
    // Bilinear height under the centre (the pad meets the ground there).
    let (face, u, v) = face_uv(normal);
    let x = (u * res as f32 - 0.5).clamp(0.0, (res - 1) as f32);
    let y = (v * res as f32 - 0.5).clamp(0.0, (res - 1) as f32);
    let (c0, r0) = (x as usize, y as usize);
    let (c1, r1) = ((c0 + 1).min(res - 1), (r0 + 1).min(res - 1));
    let (fx, fy) = (x - c0 as f32, y - r0 as f32);
    let t = |r: usize, c: usize| raw_h(cube.raw()[(face * res + r) * res + c]);
    let top = t(r0, c0) + (t(r0, c1) - t(r0, c0)) * fx;
    let bottom = t(r1, c0) + (t(r1, c1) - t(r1, c0)) * fx;
    let level = top + (bottom - top) * fy;
    let Some(geom) = StampGeom::new(normal * radius, [Vec3::X, Vec3::Y, Vec3::Z], half_width, 0.0) else {
        return;
    };
    let none = Cond {
        kind: CondKind::None,
        a: 0.0,
        b: 0.0,
        falloff: false,
        falloff_lo: 0.0,
        falloff_hi: 0.0,
    };
    let shade = Shade {
        program: Program::GaussianLevel,
        params: [1.0, 0.0, 0.0, level],
        cond: CondRamp::new(&none, 0.0),
        raster: None,
        cube_rot: Mat3::IDENTITY,
        kernel: [0.0; 10],
        snapshot: None,
        vertex_intensity: false,
    };
    raster_stamp(cube.raw_mut(), res, &geom, &shade);
}

/// Unit normal map of one face (res x res), from the planet surface.
pub fn normal_map(planet: &Planet, face: usize, res: usize) -> Vec<Vec3> {
    let mut out = Vec::with_capacity(res * res);
    for row in 0..res {
        for col in 0..res {
            let d = crate::planet_query::texel_dir(face, row, col, res);
            out.push(planet.ground_normal(d));
        }
    }
    out
}

/// Control map classes per texel (above, beach, cliff, seabed weights 0..255)
/// from height vs the water and beach levels (mBeachLevel 0.025) and slope vs
/// kMin/MaxCliffGradient 32/45 degrees (2C). ADAPTED: the retail map is built
/// by the brush shaders; this derives the same classes on the CPU.
pub fn control_map(planet: &Planet, face: usize, res: usize) -> Vec<[u8; 4]> {
    let cmin = crate::planet_query::MIN_CLIFF_GRADIENT_DEG.to_radians().cos();
    let cmax = crate::planet_query::MAX_CLIFF_GRADIENT_DEG.to_radians().cos();
    let mut out = Vec::with_capacity(res * res);
    for row in 0..res {
        for col in 0..res {
            let d = crate::planet_query::texel_dir(face, row, col, res);
            let h = planet.h_at(d) - planet.water_level;
            let slope = planet.ground_normal(d).dot(d);
            let cliff = ((cmin - slope) / (cmin - cmax)).clamp(0.0, 1.0);
            let seabed = if h < 0.0 { 1.0 } else { 0.0 };
            let beach = if h >= 0.0 && h < crate::planet_query::BEACH_LEVEL {
                1.0 - cliff
            } else {
                0.0
            };
            let above = (1.0 - seabed - beach - cliff * (1.0 - seabed)).max(0.0);
            out.push([above, beach, cliff * (1.0 - seabed), seabed].map(|x| (x * 255.0).round() as u8));
        }
    }
    out
}

/// Land fraction of the planet (texel-area weighted).
pub fn land_fraction(planet: &Planet) -> f32 {
    let res = planet.cube.res();
    let (mut land, mut total) = (0.0f64, 0.0f64);
    for face in 0..6 {
        for row in (0..res).step_by(2) {
            for col in (0..res).step_by(2) {
                let p = face_point(face, (col as f32 + 0.5) / res as f32, (row as f32 + 0.5) / res as f32);
                // Solid-angle weight of a cube texel ~ 1 / |p|^3.
                let w = 1.0 / (p.length_squared() as f64).powf(1.5);
                total += w;
                if planet.cube.texel(face, row, col) > planet.water_level {
                    land += w;
                }
            }
        }
    }
    (land / total) as f32
}

// ---------------------------------------------------------------------------
// --dump-planet

fn shade(planet: &Planet, d: Vec3) -> [u8; 3] {
    let h = planet.h_at(d) - planet.water_level;
    let n = planet.ground_normal(d);
    let light = Vec3::new(0.4, 0.5, 0.75).normalize();
    let up = d.normalize();
    let lambert = (n.dot(light) - up.dot(light) * 0.6 + 0.6).clamp(0.2, 1.3);
    let c = if h < 0.0 {
        let t = (-h / 0.4).clamp(0.0, 1.0);
        [30.0 - 20.0 * t, 110.0 - 70.0 * t, 170.0 - 80.0 * t]
    } else if h < crate::planet_query::BEACH_LEVEL {
        [200.0, 185.0, 130.0]
    } else {
        let t = (h / 0.6).clamp(0.0, 1.0);
        let lo = [70.0, 130.0, 50.0];
        let mid = [130.0, 110.0, 70.0];
        let hi = [235.0, 235.0, 235.0];
        let (a, b, f) = if t < 0.5 { (lo, mid, t * 2.0) } else { (mid, hi, t * 2.0 - 1.0) };
        let mut c = [0.0; 3];
        for i in 0..3 {
            c[i] = (a[i] + (b[i] - a[i]) * f) * lambert;
        }
        c
    };
    c.map(|x| x.clamp(0.0, 255.0) as u8)
}

fn write_png(path: &std::path::Path, w: u32, h: u32, rgb: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgb).map_err(|e| e.to_string())
}

/// Write the six faces (shaded, `face_N.png` 256^2) and a cube cross of one
/// planet into `dir`.
pub fn dump_planet(planet: &Planet, dir: &std::path::Path, label: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    const S: usize = 256;
    let mut faces = Vec::new();
    for face in 0..6 {
        let mut rgb = Vec::with_capacity(S * S * 3);
        for row in 0..S {
            for col in 0..S {
                let d = face_point(face, (col as f32 + 0.5) / S as f32, (row as f32 + 0.5) / S as f32);
                rgb.extend(shade(planet, d));
            }
        }
        write_png(&dir.join(format!("{label}_face_{face}.png")), S as u32, S as u32, &rgb)?;
        faces.push(rgb);
    }
    // Cross (4 x 3 faces): row 1 = +X, +Z(0), -X, -Z(1) around Y; +Y above
    // and -Y below the +Z face. Orientation per face is its own uv.
    let (cw, ch) = (4 * S, 3 * S);
    let mut cross = vec![40u8; cw * ch * 3];
    let place = |cross: &mut Vec<u8>, face: usize, gx: usize, gy: usize| {
        for row in 0..S {
            for col in 0..S {
                let src = &faces[face][(row * S + col) * 3..(row * S + col) * 3 + 3];
                let (x, y) = (gx * S + col, gy * S + (S - 1 - row));
                cross[(y * cw + x) * 3..(y * cw + x) * 3 + 3].copy_from_slice(src);
            }
        }
    };
    for (face, gx, gy) in [(3, 0, 1), (0, 1, 1), (2, 2, 1), (1, 3, 1), (4, 1, 0), (5, 1, 2)] {
        place(&mut cross, face, gx, gy);
    }
    write_png(&dir.join(format!("{label}_cross.png")), cw as u32, ch as u32, &cross)
}

/// `--dump-planet <dir> [family|all]`: generate and dump installed planets.
pub fn run_dump(dir: &std::path::Path, which: Option<&str>) -> Result<(), String> {
    let install = crate::spore_install::discover()?;
    let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
    let scripts = crate::planet_data::first_group(&index);
    if scripts.is_empty() {
        return Err("no FirstGroup planet scripts installed".into());
    }
    // Comma-separated FirstGroup family names, `all`, or terrain script keys
    // `GROUP!INSTANCE` (hex; the Space pool scripts live in 0x4184A1xx..A4xx).
    let mut chosen = Vec::new();
    match which.map(str::to_lowercase) {
        Some(w) if w == "all" => chosen = scripts.clone(),
        Some(w) => {
            for name in w.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                if let Some((g, i)) = name.split_once('!') {
                    let parse = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16);
                    let (Ok(group_id), Ok(instance_id)) = (parse(g), parse(i)) else {
                        return Err(format!("bad terrain script key {name}"));
                    };
                    chosen.push(crate::planet_data::PlanetScript {
                        key: crate::dbpf::ResourceKey {
                            group_id,
                            instance_id,
                            type_id: crate::asset_catalog::PROP,
                        },
                        description: format!("script_{group_id:08x}_{instance_id:08x}"),
                        terrain_script: instance_id,
                        first_group: false,
                        exclude_from_gge: false,
                    });
                } else {
                    chosen.extend(scripts.iter().filter(|s| s.description.to_lowercase() == name).cloned());
                }
            }
        }
        None => chosen.extend(crate::planet_data::choose_homeworld(&scripts, 0).cloned()),
    }
    if chosen.is_empty() {
        return Err(format!("no FirstGroup planet named {which:?}"));
    }
    let library = crate::planet_data::BrushLibrary::load(&index)?;
    let mut report = String::new();
    for script in chosen {
        let descriptor = if script.first_group {
            crate::planet_data::load_descriptor(&index, script.terrain_script)?
        } else {
            crate::planet_data::load_descriptor_key(&index, script.key)?
        };
        let source = PlanetSource::from_parts(&index, script.clone(), descriptor, library.clone());
        let (planet, stats) = build_planet(&source);
        let label = script.description.to_lowercase().replace(' ', "_");
        dump_planet(&planet, dir, &label)?;
        let (lo, hi) = planet.ranges.global();
        let line = format!(
            "{label}: script {:08x} entries {} stamps {} global {} ribbons {} metas {} particles {} missing fx {} rasters {} | {} ms | land {:.1}% | h {:.3}..{:.3}\n",
            script.terrain_script,
            stats.entries,
            stats.stamps,
            stats.global_passes,
            stats.ribbons,
            stats.metas,
            stats.particles,
            stats.missing_effects,
            stats.missing_rasters,
            stats.millis,
            land_fraction(&planet) * 100.0,
            lo,
            hi
        );
        print!("{line}");
        report.push_str(&line);
    }
    std::fs::write(dir.join("planets.txt"), report).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planet_data::Cond;

    fn cond(kind: CondKind, a: f32, b: f32, lo: f32, hi: f32) -> Cond {
        Cond {
            kind,
            a,
            b,
            falloff: true,
            falloff_lo: lo,
            falloff_hi: hi,
        }
    }

    #[test]
    fn retail_rng_formulas() {
        // 0x936020 / 0x936040 / 0x7D4690.
        let mut r = SporeRng::new(1);
        r.lcg_step();
        assert_eq!(r.seed(), 0x41C6_7EA6);
        let mut r = SporeRng::new(1);
        let f = r.random_float();
        assert_eq!(r.seed(), 0x278D_DE6D);
        assert!((f - (0x278D_DE6D_i32 as f64 * 2.328_306_4e-10 + 0.5) as f32).abs() < 1e-7);
        // vary(a, 0) is a and still consumes one number.
        let before = r.seed();
        assert_eq!(r.vary(1.0, 0.0), 1.0);
        assert_ne!(r.seed(), before);
        for _ in 0..1000 {
            let v = r.vary(2.0, 0.5);
            assert!((1.5..2.5).contains(&v));
        }
        assert_eq!(SporeRng::new(0).seed(), 0xDEAD_BEEF);
    }

    #[test]
    fn packed_heights_round_trip_and_saturate() {
        for raw in [0u16, 1, 255, 256, INITIAL_RAW, 32768, 40000, 65535] {
            assert_eq!(pack_h(raw_h(raw)), raw, "{raw}");
        }
        assert!((raw_h(INITIAL_RAW) - 0.025_939_94).abs() < 1e-6);
        assert_eq!(pack_h(-3.0), 0);
        assert_eq!(pack_h(1.5) >> 8, 255);
    }

    #[test]
    fn conditions_match_the_shader_constants() {
        // BR_continent_small~: between 0..0.025, falloff -0.5..0.52.
        let c = cond(CondKind::Between, 0.0, 0.025, -0.5, 0.52);
        let r = CondRamp::new(&c, 0.0);
        assert_eq!(r.t, [-0.5, 0.0, 0.025 + 1.0 / 128.0, 0.52 + 1.0 / 128.0]);
        assert!((r.weight(0.0) - 1.0).abs() < 1e-6);
        assert_eq!(r.weight(0.01), 1.0);
        assert!((r.weight(0.5) - (1.0 - (0.5 - r.t[2]) / (r.t[3] - r.t[2]))).abs() < 1e-5);
        assert_eq!(r.weight(0.6), 0.0);
        // BR_oceans_terrestrial_deepener~: less 0.475 with falloff 0.25.
        let d = CondRamp::new(&cond(CondKind::Less, 0.475, 0.5, 0.25, 0.5), 0.0);
        assert_eq!(d.weight(0.1), 1.0);
        assert!((d.weight(0.37) - (1.0 - (0.37 - d.t[2]) / (d.t[3] - d.t[2]))).abs() < 1e-5);
        assert_eq!(d.weight(0.49), 0.0);
        // greater: the first falloff value ends the ramp.
        let g = CondRamp::new(&cond(CondKind::Greater, 0.6, 0.0, 0.7, 0.9), 0.0);
        assert!((g.weight(0.65) - 0.5).abs() < 1e-5);
        assert_eq!(g.weight(0.8), 1.0);
        // No condition: weight 1 over the whole packed range.
        let none = CondRamp::new(&Cond { kind: CondKind::None, ..c }, 0.0);
        assert_eq!(none.weight(0.0), 1.0);
        assert_eq!(none.weight(0.999), 1.0);
        // A water level offset shifts every edge.
        assert_eq!(CondRamp::new(&c, 0.1).weight(0.11), 1.0);
    }

    #[test]
    fn brush_programs_follow_the_pixel_shaders() {
        let add = vertex_params(DrawMode::Add, 0.5, 0.0, 0.0, 0.0);
        assert!((apply(Program::Texture, -0.2, 1.0, 0.8, 1.0, add) - (-0.2 + 0.4)).abs() < 1e-6);
        let sub = vertex_params(DrawMode::Subtract, 0.5, 0.0, 0.0, 0.0);
        assert!((apply(Program::Texture, -0.2, 0.5, 1.0, 1.0, sub) - (-0.45)).abs() < 1e-6);
        // Biased: value 2R - 256/255 (zero at byte 128).
        let biased = vertex_params(DrawMode::Biased, 1.0, 0.0, 0.0, 0.0);
        assert!((apply(Program::Texture, 0.1, 1.0, 128.0 / 255.0, 1.0, biased) - 0.1).abs() < 1e-6);
        // Level: lerp toward the level by condition x alpha x intensity.
        let level = vertex_params(DrawMode::Level, 1.0, 0.3, 0.0, 0.0);
        assert!((apply(Program::TextureLevel, -0.5, 1.0, 0.2, 0.5, level) - (-0.1)).abs() < 1e-6);
        // Level range: target lerp(lo, hi, R).
        let range = vertex_params(DrawMode::LevelRange, 1.0, 0.0, 0.1, 0.5);
        assert!((apply(Program::TextureLevel, 0.0, 1.0, 0.5, 1.0, range) - 0.3).abs() < 1e-6);
        // Gaussian: cone in r, no raster value.
        assert!((apply(Program::Gaussian, 0.0, 1.0, 0.25, 1.0, add) - 0.125).abs() < 1e-6);
        assert!((apply(Program::GaussianGlobal, 0.0, 1.0, 0.0, 1.0, sub) + 0.5).abs() < 1e-6);
        // AbsoluteMax raises to lerp(lo, hi, R); AbsoluteMin caps at lerp(hi, lo, R).
        let amax = vertex_params(DrawMode::AbsoluteMax, 1.0, 0.0, 0.2, 0.4);
        assert!((apply(Program::AbsoluteMax, -0.5, 1.0, 1.0, 1.0, amax) - 0.4).abs() < 1e-6);
        assert_eq!(apply(Program::AbsoluteMax, 0.9, 1.0, 1.0, 1.0, amax), 0.9);
        assert_eq!(apply(Program::AbsoluteMax, -0.5, 1.0, 0.0, 1.0, amax), -0.5);
        let amin = vertex_params(DrawMode::AbsoluteMin, 1.0, 0.0, 0.2, 0.4);
        assert!((apply(Program::AbsoluteMin, 0.9, 1.0, 1.0, 1.0, amin) - 0.2).abs() < 1e-6);
        // The remap keeps -1 and 1 and moves 0 to the water level.
        let shifted = [1.0, 0.5, 0.0, 0.0];
        assert!((apply(Program::AbsoluteMax, -1.0, 1.0, 1.0, 1.0, shifted) - 0.5).abs() < 1e-6);
        let top = [1.0, 0.5, 1.0, 1.0];
        assert!((apply(Program::AbsoluteMax, -1.0, 1.0, 1.0, 1.0, top) - 1.0).abs() < 1e-6);
        assert!((curve(&[1.0, 3.0], 0.5) - 2.0).abs() < 1e-6);
        assert_eq!(curve(&[], 0.3), 1.0);
    }

    #[test]
    fn emission_matches_the_retail_schedule() {
        // (life, rate) -> update index of each stamp at dt = 0.033.
        let schedule = |life: f32, rate: f32| {
            let mut e = Emitter::default();
            let mut out = Vec::new();
            for u in 1..=400 {
                for _ in e.update(life, rate) {
                    out.push(u);
                }
                if e.dead {
                    break;
                }
            }
            (out, e.dead)
        };
        assert_eq!(schedule(0.1, 11.0), (vec![2], true));
        assert_eq!(schedule(0.02, 51.0), (vec![1], true));
        assert_eq!(schedule(0.01, 101.0), (vec![1], true));
        assert_eq!(schedule(0.1, 10.0).0.len(), 1);
        assert_eq!(schedule(5.0, 1.0).0.len(), 5);
    }

    #[test]
    fn stamps_cover_each_texel_once_and_follow_the_plane() {
        let res = 64;
        let none = CondRamp::new(&cond(CondKind::None, 0.0, 0.0, 0.0, 0.0), 0.0);
        let shade = Shade {
            program: Program::GaussianGlobal,
            params: vertex_params(DrawMode::Add, 0.25, 0.0, 0.0, 0.0),
            cond: none,
            raster: None,
            cube_rot: Mat3::IDENTITY,
            kernel: [0.0; 10],
            snapshot: None,
            vertex_intensity: false,
        };
        // A constant program over a stamp: every covered texel moves exactly once,
        // on one face and across face edges and corners.
        for centre in [
            Vec3::new(0.1, 0.2, 1.0) * 500.0,
            Vec3::new(1.0, 1.0, 0.3).normalize() * 500.0,
            Vec3::new(1.0, 1.0, 1.0).normalize() * 500.0,
        ] {
            let mut buf = vec![pack_h(0.0); 6 * res * res];
            let geom = StampGeom::new(centre, [Vec3::X, Vec3::Y, Vec3::Z], 120.0, 0.0).unwrap();
            raster_stamp(&mut buf, res, &geom, &shade);
            let mut touched = 0;
            for &raw in &buf {
                let h = raw_h(raw);
                assert!(h.abs() < 1e-4 || (h - 0.25).abs() < 1e-4, "texel drawn {h}");
                touched += usize::from(h > 0.1);
            }
            assert!(touched > 100, "{centre}: {touched}");
        }
        // The interpolated brush uv matches the exact plane intersection:
        // exactly where the stamp plane is parallel to the face (the map is
        // affine), within a few percent off-axis (retail interpolates uv
        // linearly over the two triangles of the projected quad).
        for (centre, spin, tol) in [(Vec3::Z, 30.0, 0.005), (Vec3::new(0.2, 0.1, 1.0).normalize(), 30.0, 0.05)] {
            let geom = StampGeom::new(centre * 500.0, [Vec3::X, Vec3::Y, Vec3::Z], 60.0, spin).unwrap();
            let probe = Shade {
                program: Program::Gaussian,
                params: vertex_params(DrawMode::Add, 1.0, 0.0, 0.0, 0.0),
                cond: none,
                raster: None,
                cube_rot: Mat3::IDENTITY,
                kernel: [0.0; 10],
                snapshot: None,
                vertex_intensity: false,
            };
            let res = 256;
            let base = pack_h(-0.999);
            let mut buf = vec![base; 6 * res * res];
            raster_stamp(&mut buf, res, &geom, &probe);
            let mut checked = 0;
            for row in 0..res {
                for col in 0..res {
                    let h = raw_h(buf[row * res + col]) - raw_h(base);
                    if h <= 0.05 {
                        continue;
                    }
                    let d = face_point(0, (col as f32 + 0.5) / res as f32, (row as f32 + 0.5) / res as f32);
                    let (u, v) = geom.brush_uv(d.normalize());
                    let cone = (1.0 - 2.0 * ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt()).max(0.0);
                    assert!((h - cone).abs() < tol, "texel {row},{col}: {h} vs {cone}");
                    checked += 1;
                }
            }
            assert!(checked > 200);
        }
    }

    #[test]
    fn filter_programs_smooth_and_diffuse() {
        let res = 16;
        let low = pack_h(-0.5);
        let mut buf = vec![low; 6 * res * res];
        buf[8 * res + 8] = pack_h(0.5);
        let snap = buf.clone();
        let none = CondRamp::new(&cond(CondKind::None, 0.0, 0.0, 0.0, 0.0), 0.0);
        let mut kernel = [1.0; 10];
        kernel[9] = 1.0 / 9.0;
        let shade = Shade {
            program: Program::Filter,
            params: filter_params(0),
            cond: none,
            raster: None,
            cube_rot: Mat3::IDENTITY,
            kernel,
            snapshot: Some(&snap),
            vertex_intensity: false,
        };
        global_pass(&mut buf, res, &shade);
        // Box smoothing (filter 3, replace) on p: the spike spreads to 3x3.
        let p = |raw: u16| raw_p(raw);
        let expect = 0.25 + (0.75 - 0.25) / 9.0;
        assert!((p(buf[8 * res + 8]) - expect).abs() < 1e-4);
        assert!((p(buf[7 * res + 9]) - expect).abs() < 1e-4);
        assert!((p(buf[5 * res + 5]) - 0.25).abs() < 1e-4);
        // Global erosion: the constant brush uv (0, 1) has a zero cone.
        let erosion = Shade { program: Program::FilterErosion, ..shade };
        let before = buf.clone();
        global_pass(&mut buf, res, &erosion);
        assert_eq!(buf, before);
        let b = |filter: u8, mode: u8| {
            let mut brush = crate::planet_data::Brush {
                use_filter: true,
                filter,
                filter_mode: mode,
                ..test_brush()
            };
            brush.filter_value = 0.5;
            filter_kernel(&brush)
        };
        assert_eq!(b(3, 0).unwrap().2, 0);
        assert_eq!(b(8, 0).unwrap().2, 6);
        assert_eq!(b(12, 3).unwrap().2, 6);
        assert_eq!(b(0x0F, 0).unwrap().0[4], 0.5);
        assert!(b(0x11, 0).is_none());
    }

    fn test_brush() -> Brush {
        let none = cond(CondKind::None, 0.0, 0.0, 0.0, 0.0);
        Brush {
            texture: None,
            draw_mode: DrawMode::Add,
            tex_gaussian: false,
            tex_gaussian_value: 0.0,
            life: 0.1,
            size: vec![50.0],
            intensity: vec![1.0],
            water_level_is_zero: true,
            intensity_vary: 0.0,
            size_vary: 0.0,
            tex_offset: [0.0; 2],
            draw_level: 0.0,
            tex_vary: 0.0,
            draw_range: [0.0; 2],
            rate: 11.0,
            use_filter: false,
            size_global: false,
            filter: 0,
            filter_mode: 0,
            filter_value: 0.0,
            spacing: vec![],
            spacing_vary: 0.0,
            cond: none,
            gradient_cond: none,
            tex_cube: false,
            rotate: [0.0; 3],
            rotate_vary: [0.0; 3],
            use_ribbon: false,
            ribbon: [1, 20, 1, 1],
        }
    }

    #[test]
    fn retail_random_int_is_one_lcg_step() {
        let mut r = SporeRng::new(0x1234_5678);
        let mut l = r;
        l.lcg_step();
        let v = r.random_int(7);
        assert_eq!(r.seed(), l.seed());
        assert!(v < 7);
        let mut hits = [0usize; 4];
        for _ in 0..4000 {
            hits[r.random_int(4) as usize] += 1;
        }
        assert!(hits.iter().all(|&h| h > 800), "{hits:?}");
        assert_eq!(r.random_int(1), 0);
    }

    #[test]
    fn ribbon_paths_are_hermite_through_the_points() {
        let pts: Vec<Vec3> = (0..5).map(|i| Vec3::new(i as f32 * 10.0, 0.0, 500.0)).collect();
        let path = build_path(&pts);
        assert_eq!(path.len(), 5);
        assert!((path[4].len - 40.0).abs() < 1e-4);
        assert!((path[2].tan - Vec3::X).length() < 1e-5);
        let mut st = PathState::new(&path);
        assert!(st.eval(0.0, &path));
        assert!((st.pos - pts[0]).length() < 1e-4);
        let mut last = st.pos.x;
        for _ in 0..39 {
            assert!(st.eval(1.0, &path));
            assert!(st.pos.x > last - 1e-4, "monotonic");
            last = st.pos.x;
        }
        assert!((st.pos.x - 39.0).abs() < 0.01);
        assert!(!st.eval(1.0, &path), "stops at the end");
    }

    /// A synthetic library: an entry effect with one inject metaParticles
    /// component pinned to the R = 500 sphere, moving along +X; its child
    /// effect has one gaussian add brush.
    fn synthetic_meta_source() -> PlanetSource {
        use crate::planet_data::{BlockTransform, EffectBlock, PlanetDescriptor, PlanetScript, TerrainEntry};
        use crate::swarm_fx::format::{ResourceId, Surface};
        let block = |kind: u8, index: i32| EffectBlock {
            kind,
            transform: BlockTransform {
                flags: 0,
                scale: 1.0,
                rows: [Vec3::X, Vec3::Y, Vec3::Z],
                offset: Vec3::ZERO,
            },
            selection_group: 0,
            selection_chance: 0,
            index,
        };
        let brush = Brush {
            size: vec![40.0],
            intensity: vec![0.05],
            water_level_is_zero: false,
            rate: 30.0,
            life: 2.0,
            ..test_brush()
        };
        let meta = MetaParticleDef {
            flags: 1 | 1 << 12,
            life: [1.0, 1.0],
            emit_delay: [-1.0, -1.0],
            emit_retrigger: [-1.0, -1.0],
            emit_direction: [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            emit_speed: [300.0, 300.0],
            torus_width: -1.0,
            rate: vec![1.0],
            rate_loop: 0.01,
            rate_curve_cycles: 1,
            size: vec![1.0],
            pitch: vec![0.0],
            roll: vec![0.0],
            yaw: vec![0.0],
            alpha: vec![1.0],
            component: (0, 0),
            death_effect: -1,
            surfaces: vec![Surface {
                flags: 4,
                map: ResourceId {
                    group: 0,
                    instance: crate::planet_meta::SURFACE_SPHERE,
                },
                bounce: 1.0,
                slide: 1.0,
                collision_radius: 0.0,
                death_probability: 0.0,
                pin_offset: 0.0,
                collide_effect: -1,
                death_effect: -1,
                points: vec![[0.0, 0.0, 0.0], [500.0, 0.0, 0.0]],
            }],
            ..Default::default()
        };
        let mut library = BrushLibrary {
            brushes: vec![brush],
            effects: vec![
                EffectDef {
                    blocks: vec![block(KIND_BRUSH, 0)],
                },
                EffectDef {
                    blocks: vec![block(KIND_META, 0)],
                },
            ],
            metas: vec![meta],
            ..Default::default()
        };
        library.exports.insert(0xABCD, 1);
        let key = crate::dbpf::ResourceKey {
            group_id: 0,
            instance_id: 1,
            type_id: 0,
        };
        PlanetSource {
            script: PlanetScript {
                key,
                description: "synthetic".into(),
                terrain_script: 1,
                first_group: false,
                exclude_from_gge: false,
            },
            descriptor: PlanetDescriptor {
                script: key,
                seed: 7,
                entries: vec![TerrainEntry {
                    effect: 0xABCD,
                    transform: crate::planet_data::EntryTransform {
                        offset: Vec3::new(0.0, 0.0, 500.0),
                        scale: 1.0,
                        rows: [Vec3::X, Vec3::Y, Vec3::Z],
                    },
                    seed: 7,
                    intensity: 1.0,
                    time: 0.0,
                }],
                wta: [0.5; 3],
                planet_info: [32.0, 32.0, 500.0],
                properties: Default::default(),
                reseed_entries: true,
                entry_water_levels: vec![],
            },
            library,
            rasters: HashMap::new(),
        }
    }

    #[test]
    fn metaparticle_children_stamp_along_the_particle_path() {
        let source = synthetic_meta_source();
        let (cube, stats) = generate_res(&source, 64);
        assert_eq!(stats.metas, 1);
        assert_eq!(stats.particles, 1);
        assert_eq!(stats.children, 1);
        // 1 s of flight at rate 30: the brush stamps until the particle dies.
        assert!((25..=32).contains(&stats.stamps), "{} stamps", stats.stamps);
        let planet = Planet::new(cube.clone(), 500.0, 0.0);
        let base = raw_h(INITIAL_RAW);
        // The path runs from +Z over 300 units (0.6 rad) toward +X.
        let at = |a: f32| planet.h_at(Vec3::new(a.sin(), 0.0, a.cos()));
        for a in [0.05f32, 0.3, 0.55] {
            assert!(at(a) > base + 0.02, "raised at {a}: {}", at(a));
        }
        assert!((at(-0.6) - base).abs() < 1e-3, "behind the start untouched");
        assert!((planet.h_at(Vec3::new(0.0, 0.6f32.sin(), 0.6f32.cos())) - base).abs() < 1e-3);
        let (again, _) = generate_res(&source, 64);
        assert!(again == cube, "deterministic");
    }

    #[test]
    fn ribbon_brushes_draw_a_strip_when_they_stop() {
        let mut source = synthetic_meta_source();
        source.library.brushes[0].use_ribbon = true;
        source.library.brushes[0].ribbon = [1, 8, 1, 1];
        // Size and intensity must vary along the ribbon (a constant second
        // path has zero length: retail draws nothing then).
        source.library.metas[0].size = vec![0.5, 1.5];
        let (cube, stats) = generate_res(&source, 64);
        assert_eq!(stats.stamps, 0);
        assert_eq!(stats.ribbons, 1);
        let planet = Planet::new(cube, 500.0, 0.0);
        let base = raw_h(INITIAL_RAW);
        let at = |a: f32| planet.h_at(Vec3::new(a.sin(), 0.0, a.cos()));
        assert!(at(0.3) > base + 0.01, "ribbon raised the path: {}", at(0.3));
        assert!((at(-0.6) - base).abs() < 1e-3);
    }

    #[test]
    fn flatten_pad_levels_around_the_centre() {
        let mut cube = HeightCube::filled(64, 0.0);
        // A slope across face 4 (+Y).
        for row in 0..64 {
            for col in 0..64 {
                let i = cube.index(4, row, col);
                cube.raw_mut()[i] = crate::planet_query::h_to_raw(col as f32 / 64.0 * 0.4);
            }
        }
        let before = Planet::new(cube.clone(), 500.0, 0.0);
        let dir = Vec3::Y;
        let h0 = before.h_at(dir);
        flatten_pad(&mut cube, dir, 40.0, 500.0);
        let after = Planet::new(cube, 500.0, 0.0);
        let side = (Vec3::Y * 500.0 + Vec3::Z * 8.0).normalize();
        assert!((after.h_at(side) - h0).abs() < (before.h_at(side) - h0).abs() * 0.5);
        // Far away untouched.
        let far = Vec3::new(0.0, 0.3, 1.0).normalize();
        assert!((after.h_at(far) - before.h_at(far)).abs() < 1e-4);
    }

    /// Land / relief census of every installed terrain script (the 354 Space
    /// pool scripts + the 15 FirstGroup homeworld families) at 128^2 per face.
    /// Writes `target\census_<OPENSPORE_CENSUS_TAG>.csv` and
    /// prints a summary; used to compare blend implementations.
    #[test]
    #[ignore = "reads the installed game; ~1 min"]
    fn installed_terrain_script_census() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut library = Some(crate::planet_data::BrushLibrary::load(&index).unwrap());
        let mut jobs: Vec<(String, crate::planet_data::PlanetScript, crate::planet_data::PlanetDescriptor)> = Vec::new();
        for script in crate::planet_data::first_group(&index) {
            let d = crate::planet_data::load_descriptor(&index, script.terrain_script).unwrap();
            jobs.push((format!("home:{}", script.description), script, d));
        }
        for pool in crate::space_worldgen_data::POOLS {
            for e in pool.iter() {
                let key = crate::dbpf::ResourceKey {
                    group_id: e.0,
                    instance_id: e.1,
                    type_id: crate::asset_catalog::PROP,
                };
                let d = crate::planet_data::load_descriptor_key(&index, key).unwrap();
                let script = crate::planet_data::PlanetScript {
                    key,
                    description: format!("{:08x}!{:08x}", e.0, e.1),
                    terrain_script: e.1,
                    first_group: false,
                    exclude_from_gge: false,
                };
                jobs.push((format!("pool:{:08x}!{:08x}", e.0, e.1), script, d));
            }
        }
        let mut csv = String::from("name,land,h_min,h_max,std_all,std_land,mean_land,stamps,ms,particles,ribbons\n");
        let (mut wet, mut dry, mut flat, mut mixed, mut n) = (0, 0, 0, 0, 0);
        let (mut home_wet, mut home_dry, mut home_flat, mut home_mixed) = (0, 0, 0, 0);
        let mut total_ms = 0u128;
        for (name, script, d) in jobs {
            let source = PlanetSource::from_parts(&index, script, d, library.take().unwrap());
            let (cube, stats) = generate_res(&source, 128);
            let water = source.descriptor.water_level();
            let planet = Planet::new(cube, source.descriptor.radius(), water);
            library = Some(source.library);
            let land = land_fraction(&planet);
            let (lo, hi) = planet.ranges.global();
            let (mut s, mut s2, mut ls, mut ls2, mut ln) = (0f64, 0f64, 0f64, 0f64, 0f64);
            let raw = planet.cube.raw();
            for &r in raw {
                let h = (r as f64 - 32768.0) / 32768.0;
                s += h;
                s2 += h * h;
                if h > water as f64 {
                    ls += h;
                    ls2 += h * h;
                    ln += 1.0;
                }
            }
            let cnt = raw.len() as f64;
            let std_all = (s2 / cnt - (s / cnt).powi(2)).max(0.0).sqrt();
            let (mean_land, std_land) = if ln > 0.0 {
                (ls / ln, (ls2 / ln - (ls / ln).powi(2)).max(0.0).sqrt())
            } else {
                (0.0, 0.0)
            };
            let is_wet = land < 0.01;
            let is_dry = land > 0.99;
            let is_flat = hi - lo < 0.05;
            let is_mixed = land > 0.05 && land < 0.95 && hi - lo >= 0.1;
            if name.starts_with("home:") {
                home_wet += usize::from(is_wet);
                home_dry += usize::from(is_dry);
                home_flat += usize::from(is_flat);
                home_mixed += usize::from(is_mixed);
            } else {
                n += 1;
                wet += usize::from(is_wet);
                dry += usize::from(is_dry);
                flat += usize::from(is_flat);
                mixed += usize::from(is_mixed);
            }
            total_ms += stats.millis;
            csv.push_str(&format!(
                "{name},{land:.4},{lo:.4},{hi:.4},{std_all:.4},{std_land:.4},{mean_land:.4},{},{},{},{}\n",
                stats.stamps, stats.millis, stats.particles, stats.ribbons
            ));
        }
        let tag = std::env::var("OPENSPORE_CENSUS_TAG").unwrap_or_else(|_| "run".into());
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("census_{tag}.csv"));
        std::fs::write(&path, &csv).unwrap();
        println!(
            "census {tag}: pool {n} scripts: {wet} all under water, {dry} all land, {flat} flat (< 0.05 h), {mixed} land+sea with relief; \
             homeworlds: {home_wet} wet, {home_dry} dry, {home_flat} flat, {home_mixed} mixed; {total_ms} ms total -> {}",
            path.display()
        );
        assert_eq!(n, 354);
    }

    #[test]
    #[ignore = "reads the installed game"]
    fn installed_pool_metaparticles_decode_and_play() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::planet_data::BrushLibrary::load(&index).unwrap();
        assert_eq!(library.metas.len(), 673);
        // GO_polar_oceans~ -> effect -> metaParticles (inject, sphere pinEmit).
        let id = 0x0448_602E; // GO_polar_oceans~ (reg_file.txt)
        let effect = library.effect(id).expect("GO_polar_oceans~");
        let inner = &library.effects[effect.blocks[0].index as usize];
        let meta = &library.metas[inner.blocks[0].index as usize];
        assert_eq!(inner.blocks[0].kind, KIND_META);
        assert!(meta.has(crate::planet_meta::bit::INJECT));
        assert!(library.effects.get(meta.component.0 as usize).is_some());
        assert_eq!(meta.surfaces[0].map.instance, crate::planet_meta::SURFACE_SPHERE);
        // A pool script with metaParticles plays them, deterministically.
        let mut played = 0;
        for e in crate::space_worldgen_data::POOL_T0.iter().take(12) {
            let key = crate::dbpf::ResourceKey {
                group_id: e.0,
                instance_id: e.1,
                type_id: crate::asset_catalog::PROP,
            };
            let d = crate::planet_data::load_descriptor_key(&index, key).unwrap();
            let script = crate::planet_data::PlanetScript {
                key,
                description: String::new(),
                terrain_script: e.1,
                first_group: false,
                exclude_from_gge: false,
            };
            let source = PlanetSource::from_parts(&index, script, d, library.clone());
            let (cube, stats) = generate_res(&source, 64);
            if stats.particles == 0 {
                continue;
            }
            assert!(stats.children > 0 && stats.stamps > 0, "{stats:?}");
            let (again, _) = generate_res(&source, 64);
            assert!(again == cube, "{:08x} deterministic", e.1);
            played += 1;
            if played >= 2 {
                break;
            }
        }
        assert!(played >= 1);
    }

    #[test]
    #[ignore = "reads the installed game"]
    fn installed_first_group_planets_generate_plausibly_and_deterministically() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let scripts = crate::planet_data::first_group(&index);
        assert_eq!(scripts.len(), 15);
        for script in &scripts {
            let source = PlanetSource::load(&index, script).unwrap();
            let (cube, stats) = generate_res(&source, 128);
            let planet = Planet::new(cube.clone(), 500.0, source.descriptor.water_level());
            let land = land_fraction(&planet);
            let (lo, hi) = planet.ranges.global();
            eprintln!(
                "{:>10}: land {:5.1}% h {lo:.2}..{hi:.2} stamps {} global {} {} ms",
                script.description,
                land * 100.0,
                stats.stamps,
                stats.global_passes,
                stats.millis
            );
            assert_eq!(stats.missing_effects, 0, "{}", script.description);
            assert!(land > 0.05 && land < 0.95, "{}: land {land}", script.description);
            assert!(lo < -0.05 && hi > 0.05, "{}: oceans and land", script.description);
            assert!(lo >= -1.0 && hi <= 1.0);
            let (again, _) = generate_res(&source, 128);
            assert!(again == cube, "{} deterministic", script.description);
        }
    }
}
