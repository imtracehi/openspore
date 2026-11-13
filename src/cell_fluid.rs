//! The Cell water: retail's 2-D SPH particle fluid (`notes\cell-re-notes.md`
//! 1C, `notes\cell-fluid-notes.md`).
//!
//! The retail water is not the advect map itself: it is a fluid of 1500
//! particles (object at `cCellGame+0x4108`, constructor 0xE4ECA0) that lives in
//! the visible background box (look point +- (10, 7.5) world units, 0xE507F0).
//! Every simulation step (0xE59EE0, called from 0xE7F3A0) it
//!  1. buckets the particles into the 20 x 15 grid of 1-unit cells (max 8 per
//!     bucket, 0xE4DDC0),
//!  2. computes density `315 / (64 PI) * sum (1 - r^2)^3`, pressure `0.5 rho`
//!     and `1 / (rho + 2^-16)` (0xE4E8B0),
//!  3. accumulates the pressure (spiky, -45/PI) and viscosity (45/PI * 0.03)
//!     accelerations over the 3 x 3 neighbouring buckets (0xE4E640),
//!  4. rebuilds the velocity / mean-speed / density grids (0xE4DED0),
//!  5. integrates `v += a dt; x += v dt + displacement`, wraps a particle that
//!     left the box (and is not pinned) by one box width and gives it the
//!     advect-map current `strength * map[floor(frac(x / tile) * 127.999985)]`
//!     (0xE56C30; strength = 2 * the stage strength, 0xE5F360).
//!
//! Cells, the audio `flow_field` and the Swarm effect map 0x01010003 read only
//! the 20 x 15 grids: `FlowAt` (0xE4E3A0, bilinear over the cell centres,
//! clamped) and the sum of the four nearest cells' mean speed (0xE4E570, the
//! effect map's `Height` and the `flow_field` game parameter).
//!
//! VERIFIED (code image, addresses above): everything in the list, the
//! constants, the 3 x 3 neighbour order (x outer, y inner), the pinned-particle
//! pool (0xE4DBA0 alloc, 0xE4EC10 release) and the "displacement" input
//! (0xE5A430 writes `target - position` per pinned outline particle of a
//! soft-body cell).
//! The cells ride the water through pinned particles (`cell_softbody`: pool
//! 0xE4DBA0 / 0xE4EC10, displacement 0xE5A430, read-back 0xE553A0): they push
//! it and are pushed by it, which is where the stirring that `Height` and the
//! `flow_field` parameter measure comes from. Only cells that are on screen
//! hold pins (0xE78C00 creates a cell's GFX object, which allocates them, while
//! 0xE5CDE0 says the cell is visible and destroys it, with its pins, when not:
//! `cell_population::pin_visible`, `notes\cell-pop2-notes.md`), so the pool
//! never nears its 1499 pins (about 100-320 in the stage's ten levels, measured)
//! and the water around the avatar stays free.
//! The density grid is retail's: the mean of `1 / entry+0x18` over a cell's
//! bucket entries, where the entry holds the PREVIOUS step's `invRho` (the
//! copy 0xE4DDC0 makes before the density pass recomputes it). Its readers
//! are the three fluid textures (`cell_fluid_tex`, 0xE831A0).
//! ADAPTED (marked at each place): the fixed 1/60 s step with an accumulator
//! (retail steps once per sim step with the frame's `dt`); the initial layout
//! (retail draws uniform random points from its own generator, ours is a
//! seeded ChaCha8 stream so a stage restart is reproducible); a world
//! recentre shifts every free particle (retail 0xE6DB70 only transforms
//! pinned particles; the cells carry their own pins).

use std::sync::Arc;

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::cell_data::{AdvectMap, ADVECT_SIDE};
use crate::cell_engine::{real_size, CellGame, UNIT, VISIBLE_BOX_HALF};

/// Particle count (0x5DC).
pub const PARTICLES: usize = 1500;
/// Grid size in 1-unit cells (0x14 x 0xF).
pub const GRID_W: usize = 20;
pub const GRID_H: usize = 15;
pub const GRID_CELLS: usize = GRID_W * GRID_H;
/// Particles per bucket (0xE4DDC0 stops at 8).
pub const BUCKET_CAP: usize = 8;
/// Kernel radius `h` (.data 0x15A7B98) and the grid scale 1 / h (0x15A7B9C).
const KERNEL_H: f32 = 1.0;
/// Poly6 density constant `315 / (h^3 * PI * 64)` (static init 0x137EA90).
pub const DENSITY_KERNEL: f32 = 315.0 / (KERNEL_H * KERNEL_H * KERNEL_H * std::f32::consts::PI * 64.0);
/// Spiky pressure constant `-45 / (h^6 * PI)` (0x137EAC0).
pub const PRESSURE_KERNEL: f32 = -45.0 / (KERNEL_H * std::f32::consts::PI);
/// Viscosity constant `45 / (h^6 * PI)` (0x137EAF0), scaled by 0.03 (.rdata 0x1483AE4).
pub const VISCOSITY_KERNEL: f32 = 45.0 / (KERNEL_H * std::f32::consts::PI);
pub const VISCOSITY: f32 = 0.03;
/// `pressure = rho * 0.5` (.rdata 0x145BE38).
pub const PRESSURE_FACTOR: f32 = 0.5;
/// `invRho = 1 / (rho + 2^-16)` (.rdata 0x1483AF4).
pub const INV_RHO_EPSILON: f32 = 1.0 / 65536.0;
/// Pairs closer than this are skipped (self / coincident), `2^-32` (.data 0x15A7B8C).
const MIN_PAIR_R2: f32 = 1.0 / 4_294_967_296.0;
/// Displacement writers skip |d|^2 below this (.rdata 0x148512C, 0xE5A7F6).
#[allow(dead_code)]
pub const MIN_DISPLACEMENT_R2: f32 = 1.0 / 65536.0;
/// `FlowAt` clamps the grid coordinate to these (.rdata 0x1483AFC / 0x1483AF8).
const GRID_MAX_X: f32 = 19.999_996;
const GRID_MAX_Y: f32 = 14.999_999;
/// The advect map index scale (.rdata 0x1485480 = 0x42FFFFFE).
const ADVECT_INDEX_SCALE: f32 = 127.999_99;
/// `AdvectLoad` doubles the stage strength and variance (0xE5F3BD, .rdata 0x1470EAC).
pub const ADVECT_STRENGTH_FACTOR: f32 = 2.0;
/// ADAPTED: fixed step (retail runs one step per sim step with the frame dt).
pub const STEP: f32 = 1.0 / 60.0;
/// Steps one frame may run before the accumulator is dropped (a stall).
const MAX_STEPS_PER_FRAME: u32 = 4;

/// The 20 x 15 grids every reader uses (`f+0x1C210` velocity, `f+0x1D020` mean
/// speed, `f+0x1BD60` density), row-major with x fastest, anchored at the
/// visible box's lower corner.
#[derive(Clone, Debug)]
pub struct FluidGrid {
    pub box_min: Vec2,
    /// Mean particle velocity per cell (retail world units per second).
    pub velocity: Vec<Vec2>,
    /// `|mean particle velocity|` per cell.
    pub speed: Vec<f32>,
    /// Mean density per cell: `1 / entry+0x18` averaged over the bucket, the
    /// entry holding the previous step's `invRho` (0xE4DED0; infinite on the
    /// first step, when `invRho` is still zero).
    pub density: Vec<f32>,
}

impl Default for FluidGrid {
    fn default() -> Self {
        Self {
            box_min: Vec2::ZERO,
            velocity: vec![Vec2::ZERO; GRID_CELLS],
            speed: vec![0.0; GRID_CELLS],
            density: vec![0.0; GRID_CELLS],
        }
    }
}

/// 0xE4E150: bilinear weights along one axis whose cell centres sit at `i + 0.5`:
/// (cell, neighbour offset, weight of the cell, weight of the neighbour). The
/// first / last cell has offset 0 (clamped edge).
fn axis_weights(x: f32, cells: usize) -> (usize, i32, f32, f32) {
    let i = x.floor() as i32;
    let frac = x - i as f32;
    let (own, dir) = if 0.5 > frac {
        (frac + 0.5, if i > 0 { -1 } else { 0 })
    } else {
        (1.5 - frac, if i < cells as i32 - 1 { 1 } else { 0 })
    };
    (i.max(0) as usize, dir, own, 1.0 - own)
}

impl FluidGrid {
    /// 0xE4DD00: the position in grid cells, clamped to the box.
    fn grid_position(&self, p: Vec2) -> Vec2 {
        let g = (p - self.box_min) * (1.0 / KERNEL_H);
        Vec2::new(g.x.clamp(0.0, GRID_MAX_X), g.y.clamp(0.0, GRID_MAX_Y))
    }

    /// `FlowAt` (0xE4E3A0): the water's velocity at a retail world position
    /// (x, y plane), bilinear over the cell centres.
    pub fn flow_at(&self, p: Vec2) -> Vec2 {
        let g = self.grid_position(p);
        let (ix, dx, wxa, wxb) = axis_weights(g.x, GRID_W);
        let (iy, dy, wya, wyb) = axis_weights(g.y, GRID_H);
        let at = |x: usize, y: i32| self.velocity[(y as usize) * GRID_W + x];
        let jx = (ix as i32 + dx) as usize;
        let jy = iy as i32 + dy;
        at(ix, iy as i32) * (wxa * wya) + at(jx, iy as i32) * (wxb * wya) + at(ix, jy) * (wxa * wyb) + at(jx, jy) * (wxb * wyb)
    }

    /// 0xE4E570: the sum of the mean speeds of the four cells around `p` (no
    /// weights). The effect map's `Height` and the audio `flow_field` input.
    pub fn speed_sum_at(&self, p: Vec2) -> f32 {
        let g = self.grid_position(p);
        let (ix, dx, _, _) = axis_weights(g.x, GRID_W);
        let (iy, dy, _, _) = axis_weights(g.y, GRID_H);
        let jx = (ix as i32 + dx) as usize;
        let jy = (iy as i32 + dy) as usize;
        let s = |x: usize, y: usize| self.speed[y * GRID_W + x];
        s(jx, iy) + s(ix, iy) + s(ix, jy) + s(jx, jy)
    }
}

/// `Swarm::IEffectMap` 0x01010003 of the retail game (vtable 0x14852B8):
/// `Velocity` = `FlowAt`, `Height` = the four-cell speed sum. The map is in
/// retail world units; the engine's `UNIT` converts to Bevy (x, z).
impl crate::swarm_fx::EffectMapSampler for FluidGrid {
    fn velocity(&self, at: Vec3) -> Vec3 {
        let v = self.flow_at(Vec2::new(at.x, at.z) / UNIT) * UNIT;
        Vec3::new(v.x, 0.0, v.y)
    }
    fn height(&self, at: Vec3) -> f32 {
        self.speed_sum_at(Vec2::new(at.x, at.z) / UNIT)
    }
}

/// One bucket entry: the copy 0xE4DDC0 makes of a particle (+0x0 position,
/// +0xC velocity, +0x18 `invRho` as of the previous step, +0x1C index).
#[derive(Clone, Copy, Default)]
struct Entry {
    pos: Vec2,
    vel: Vec2,
    stale_inv_rho: f32,
    index: u32,
}

/// What the stage feeds the fluid every frame.
pub struct FluidInputs<'a> {
    /// The camera look point in retail world units (the box centre).
    pub look: Vec2,
    /// World frame (`CellGame::origin_abs`); a change shifts the particles.
    pub origin_abs: Vec2,
    pub world_scale: f32,
    /// The stage's advect map, the world size of one map tile
    /// (`realSize[scale] / worldScale`, `cCellGame+0x4`) and the current
    /// strength already multiplied by 2 (`cCellGame+0x8`).
    pub map: Option<&'a Arc<AdvectMap>>,
    pub tile: f32,
    pub strength: f32,
    pub seed: u64,
}

/// The fluid object (`cCellGame+0x4108`).
pub struct CellFluid {
    ready: bool,
    pos: Vec<Vec2>,
    vel: Vec<Vec2>,
    accel: Vec<Vec2>,
    disp: Vec<Vec2>,
    density: Vec<f32>,
    inv_rho: Vec<f32>,
    pressure: Vec<f32>,
    pinned: Vec<bool>,
    /// Pinned-particle pool: allocated count (+0x1BD54), highest index ever
    /// allocated (+0x1BD58), free-list head (+0x1BD5C) and links (+0x1A5E4).
    pin_count: usize,
    pin_high: usize,
    pin_free: i32,
    pin_next: Vec<i32>,
    counts: Vec<u8>,
    entries: Vec<Entry>,
    grid: FluidGrid,
    box_min: Vec2,
    box_max: Vec2,
    advect: Option<Arc<AdvectMap>>,
    advect_tile: f32,
    advect_strength: f32,
    accumulator: f32,
    last_origin: Vec2,
    rng: ChaCha8Rng,
    steps: u64,
}

impl Default for CellFluid {
    fn default() -> Self {
        Self {
            ready: false,
            pos: Vec::new(),
            vel: Vec::new(),
            accel: Vec::new(),
            disp: Vec::new(),
            density: Vec::new(),
            inv_rho: Vec::new(),
            pressure: Vec::new(),
            pinned: Vec::new(),
            pin_count: 0,
            pin_high: 0,
            pin_free: -1,
            pin_next: Vec::new(),
            counts: Vec::new(),
            entries: Vec::new(),
            grid: FluidGrid::default(),
            box_min: Vec2::ZERO,
            box_max: Vec2::ZERO,
            advect: None,
            advect_tile: 1.0,
            advect_strength: 0.0,
            accumulator: 0.0,
            last_origin: Vec2::ZERO,
            rng: ChaCha8Rng::seed_from_u64(0),
            steps: 0,
        }
    }
}

/// `E56C30`: the 128 x 128 map cell a world position falls in
/// (`floor(frac(x / tile) * 127.999985)`, clamped to 0..=128). The retail
/// code runs in x87 precision, so does this in f64.
fn advect_index(tile: f32, p: Vec2) -> (usize, usize) {
    let inv = 1.0 / tile as f64;
    let scale = (1.0 / tile) * ADVECT_INDEX_SCALE;
    let axis = |v: f32| {
        let t = (v as f64 * inv).floor();
        let f = ((v as f64 - t * tile as f64) * scale as f64) as f32;
        (f.floor() as i32).clamp(0, ADVECT_SIDE as i32) as usize
    };
    (axis(p.x), axis(p.y))
}

#[allow(dead_code)]
impl CellFluid {
    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn grid(&self) -> &FluidGrid {
        &self.grid
    }

    /// `FlowAt` at a retail world position; zero until the water exists.
    pub fn flow_at(&self, p: Vec2) -> Vec2 {
        if self.ready {
            self.grid.flow_at(p)
        } else {
            Vec2::ZERO
        }
    }

    /// 0xE4E570 at a retail world position; zero until the water exists.
    pub fn speed_sum_at(&self, p: Vec2) -> f32 {
        if self.ready {
            self.grid.speed_sum_at(p)
        } else {
            0.0
        }
    }

    /// Number of simulation steps taken (diagnostics and tests).
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// One line of diagnostics (`OPENSPORE_FLUID_TRACE=1`): the mean particle
    /// speed, the mean / max cell speed over occupied cells and the share of
    /// the grid whose four-cell speed sum is at least 3.5 (the tracers'
    /// `heightRange` minimum).
    pub fn stats(&self) -> String {
        let n = PARTICLES as f32;
        let mean_speed = self.vel.iter().map(|v| v.length()).sum::<f32>() / n;
        let mean_vel = self.vel.iter().copied().sum::<Vec2>() / n;
        let occupied: Vec<f32> = (0..GRID_CELLS).filter(|&c| self.counts[c] > 0).map(|c| self.grid.speed[c]).collect();
        let mean_cell = occupied.iter().sum::<f32>() / occupied.len().max(1) as f32;
        let max_cell = occupied.iter().copied().fold(0.0, f32::max);
        let mut tall = 0;
        for c in 0..GRID_CELLS {
            let p = self.box_min + Vec2::new((c % GRID_W) as f32 + 0.5, (c / GRID_W) as f32 + 0.5);
            if self.grid.speed_sum_at(p) >= 3.5 {
                tall += 1;
            }
        }
        let (map_max, map_mean) = match &self.advect {
            Some(m) => (
                m.max_magnitude(),
                m.vectors.iter().map(|v| (v[0] * v[0] + v[1] * v[1]).sqrt()).sum::<f32>() / m.vectors.len() as f32,
            ),
            None => (0.0, 0.0),
        };
        // The three dynamic textures (`cell_fluid_tex`): channel means of the RGBA maps.
        let [density, normal, velocity] = crate::cell_fluid_tex::maps(&self.grid);
        let texels = (density.len() / 4) as f32;
        let mean = |map: &[u8], c: usize| map.chunks(4).map(|t| t[c] as f32).sum::<f32>() / texels;
        let lit = density.chunks(4).filter(|t| t[0] > 0).count() as f32 / texels * 100.0;
        let textures = format!(
            "textures: density mean {:.0} (lit {:.0}%), normal mean ({:.0}, {:.0}, {:.0}), velocity mean ({:.0}, {:.0})",
            mean(&density, 0),
            lit,
            mean(&normal, 0),
            mean(&normal, 1),
            mean(&normal, 2),
            mean(&velocity, 0),
            mean(&velocity, 1)
        );
        format!(
            "fluid step {}: mean particle speed {:.3}, mean velocity ({:.3}, {:.3}), cell speed mean {:.3} max {:.3}, sum>=3.5 on {:.0}% of cells, strength {:.3} (map |v| mean {:.3} max {:.3}, tile {:.2}), pinned {}, {textures}",
            self.steps,
            mean_speed,
            mean_vel.x,
            mean_vel.y,
            mean_cell,
            max_cell,
            100.0 * tall as f32 / GRID_CELLS as f32,
            self.advect_strength,
            map_mean,
            map_max,
            self.advect_tile,
            self.pin_count
        )
    }

    pub fn particle(&self, i: usize) -> (Vec2, Vec2) {
        (self.pos[i], self.vel[i])
    }

    pub fn is_pinned(&self, i: usize) -> bool {
        self.pinned[i]
    }

    /// Time of the current frame the fixed steps have not consumed yet
    /// (0 <= lag < `STEP`): the cells extrapolate their read-back position by
    /// it (ADAPTED: retail steps once per frame, so its read-back is current).
    pub fn lag(&self) -> f32 {
        self.accumulator
    }

    /// Overwrite a particle's position and velocity (a soft body moved or
    /// turned by its cell: 0xE5E850, world rescale 0xE6DB70).
    pub fn set_particle(&mut self, index: usize, pos: Vec2, vel: Vec2) {
        if self.ready && index < PARTICLES {
            self.pos[index] = pos;
            self.vel[index] = vel;
        }
    }

    /// `v += dv` (impulses and accelerations of a cell, 0xE560D0 / 0xE7C080).
    pub fn add_velocity(&mut self, index: usize, dv: Vec2) {
        if self.ready && index < PARTICLES {
            self.vel[index] += dv;
        }
    }

    /// The constructor (0xE4ECA0): 1500 particles at uniformly random points of
    /// the box, all at rest (the constructor stores the advect velocity and
    /// then clears the velocity array), no pins.
    /// ADAPTED: the random points come from a seeded ChaCha8 stream.
    pub fn init(&mut self, box_min: Vec2, seed: u64) {
        *self = Self::default();
        self.set_box(box_min + VISIBLE_BOX_HALF);
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ 0xF101_D5EE);
        self.alloc();
        for i in 0..PARTICLES {
            let u = Vec2::new(self.rng.random::<f32>(), self.rng.random::<f32>());
            self.pos[i] = self.box_min + (self.box_max - self.box_min) * u;
        }
        self.ready = true;
    }

    /// A fluid with the given particles (tests and tools): the rest at the
    /// box corner, pinned to nothing.
    pub fn with_particles(box_min: Vec2, positions: &[Vec2]) -> Self {
        let mut f = Self::default();
        f.set_box(box_min + VISIBLE_BOX_HALF);
        f.alloc();
        for i in 0..PARTICLES {
            f.pos[i] = positions.get(i).copied().unwrap_or(box_min);
        }
        f.ready = true;
        f
    }

    fn alloc(&mut self) {
        self.pos = vec![Vec2::ZERO; PARTICLES];
        self.vel = vec![Vec2::ZERO; PARTICLES];
        self.accel = vec![Vec2::ZERO; PARTICLES];
        self.disp = vec![Vec2::ZERO; PARTICLES];
        self.density = vec![0.0; PARTICLES];
        self.inv_rho = vec![0.0; PARTICLES];
        self.pressure = vec![0.0; PARTICLES];
        self.pinned = vec![false; PARTICLES];
        self.pin_next = vec![-1; PARTICLES];
        self.counts = vec![0; GRID_CELLS];
        self.entries = vec![Entry::default(); GRID_CELLS * BUCKET_CAP];
    }

    /// The visible box (0xE507F0): look +- (10, 7.5).
    fn set_box(&mut self, look: Vec2) {
        self.box_min = look - VISIBLE_BOX_HALF;
        self.box_max = look + VISIBLE_BOX_HALF;
        self.grid.box_min = self.box_min;
    }

    /// Per-frame driver: (re)creates the water on first use, follows the box
    /// and the world frame, and runs the fixed steps for `dt` seconds.
    pub fn update(&mut self, dt: f32, input: &FluidInputs) {
        if !self.ready {
            self.init(input.look - VISIBLE_BOX_HALF, input.seed);
            self.last_origin = input.origin_abs;
        }
        // ADAPTED: a world recentre (|origin| > 1000) moves the engine's
        // coordinate frame; keep the particles where they were in space. The
        // continuous world rescale needs nothing: retail transforms only
        // pinned particles in 0xE6DB70 and the box keeps its world size.
        if input.origin_abs != self.last_origin {
            let shift = (self.last_origin - input.origin_abs) / input.world_scale.max(1e-6);
            for (p, pinned) in self.pos.iter_mut().zip(&self.pinned) {
                // The pins belong to the cells, which the recentre moved itself
                // (`cell_motion` carries the pins along with their cell).
                if !*pinned {
                    *p += shift;
                }
            }
            self.last_origin = input.origin_abs;
        }
        self.advect = input.map.cloned();
        self.advect_tile = input.tile.max(1e-6);
        self.advect_strength = input.strength;
        self.accumulator += dt.clamp(0.0, 0.1);
        let mut steps = 0;
        while self.accumulator >= STEP && steps < MAX_STEPS_PER_FRAME {
            self.accumulator -= STEP;
            // 0xE507F0 refreshes the box right before the fluid step.
            self.set_box(input.look);
            self.step(STEP);
            steps += 1;
        }
        if steps == MAX_STEPS_PER_FRAME {
            self.accumulator = 0.0;
        }
    }

    /// One simulation step (0xE59EE0): SPH, grids, integration and wrap.
    pub fn step(&mut self, dt: f32) {
        self.sph();
        self.integrate(dt);
        self.steps += 1;
    }

    /// The bucket cell of a position (0xE4DC70), `None` outside the grid.
    fn cell_of(&self, p: Vec2) -> Option<usize> {
        let ix = ((p.x - self.box_min.x) * (1.0 / KERNEL_H)).floor();
        let iy = ((p.y - self.box_min.y) * (1.0 / KERNEL_H)).floor();
        if ix >= 0.0 && ix < GRID_W as f32 && iy >= 0.0 && iy < GRID_H as f32 {
            Some(iy as usize * GRID_W + ix as usize)
        } else {
            None
        }
    }

    /// The 3 x 3 neighbouring cells (0xE4E210: x outer, y inner, off-grid
    /// cells dropped; the radius is `ceil(width * 0.05 - 2^-16) = 1`).
    fn neighbours(cell: usize) -> ([usize; 9], usize) {
        let (cx, cy) = ((cell % GRID_W) as i32, (cell / GRID_W) as i32);
        let mut out = [0usize; 9];
        let mut n = 0;
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                if (0..GRID_W as i32).contains(&x) && (0..GRID_H as i32).contains(&y) {
                    out[n] = y as usize * GRID_W + x as usize;
                    n += 1;
                }
            }
        }
        (out, n)
    }

    /// 0xE4EBC0: buckets, density / pressure, forces, then the grids.
    fn sph(&mut self) {
        // 0xE4DDC0: copy each particle into its bucket (max 8, in index order).
        self.counts.fill(0);
        for i in 0..PARTICLES {
            if let Some(c) = self.cell_of(self.pos[i]) {
                let n = self.counts[c] as usize;
                if n < BUCKET_CAP {
                    self.entries[c * BUCKET_CAP + n] = Entry {
                        pos: self.pos[i],
                        vel: self.vel[i],
                        stale_inv_rho: self.inv_rho[i],
                        index: i as u32,
                    };
                    self.counts[c] += 1;
                }
            }
        }
        // 0xE4E8B0: density, pressure, 1 / (density + 2^-16).
        for i in 0..PARTICLES {
            let p = self.pos[i];
            let mut sum = 0.0f32;
            if let Some(c) = self.cell_of(p) {
                let (cells, n) = Self::neighbours(c);
                for &nb in &cells[..n] {
                    let bucket = &self.entries[nb * BUCKET_CAP..nb * BUCKET_CAP + self.counts[nb] as usize];
                    for e in bucket {
                        let (dx, dy) = (p.x - e.pos.x, p.y - e.pos.y);
                        let r2 = dy * dy + dx * dx;
                        if r2 <= 1.0 {
                            let t = 1.0 - r2;
                            sum += t * t * t;
                        }
                    }
                }
            }
            let rho = sum * DENSITY_KERNEL;
            self.density[i] = rho;
            self.pressure[i] = rho * PRESSURE_FACTOR;
            self.inv_rho[i] = 1.0 / (rho + INV_RHO_EPSILON);
        }
        // 0xE4E640: pressure + viscosity accelerations over the neighbours.
        let viscosity = VISCOSITY_KERNEL * VISCOSITY;
        for i in 0..PARTICLES {
            let p = self.pos[i];
            let v = self.vel[i];
            let p_i = self.pressure[i];
            let mut a = Vec2::ZERO;
            if let Some(c) = self.cell_of(p) {
                let (cells, n) = Self::neighbours(c);
                for &nb in &cells[..n] {
                    let bucket = &self.entries[nb * BUCKET_CAP..nb * BUCKET_CAP + self.counts[nb] as usize];
                    for e in bucket {
                        let (dx, dy) = (p.x - e.pos.x, p.y - e.pos.y);
                        let r2 = dy * dy + dx * dx;
                        if r2 > 1.0 || MIN_PAIR_R2 > r2 {
                            continue;
                        }
                        let r = r2.sqrt();
                        let j = e.index as usize;
                        let one_minus_r = 1.0 - r;
                        let push = (self.pressure[j] + p_i) * (self.inv_rho[j] / r) * PRESSURE_KERNEL * one_minus_r * one_minus_r
                            * PRESSURE_FACTOR;
                        a.x += (e.pos.x - p.x) * push;
                        a.y += (e.pos.y - p.y) * push;
                        a.x += (e.vel.x - v.x) * viscosity;
                        a.y += (e.vel.y - v.y) * viscosity;
                    }
                }
            }
            self.accel[i] = a * self.inv_rho[i];
        }
        self.rebuild_grids();
    }

    /// 0xE4DED0: per-cell mean velocity, its magnitude and the mean density
    /// (of the previous step's `invRho`, see `Entry`).
    fn rebuild_grids(&mut self) {
        for c in 0..GRID_CELLS {
            let n = self.counts[c] as usize;
            let mut v = Vec2::ZERO;
            let mut rho = 0.0f32;
            for e in &self.entries[c * BUCKET_CAP..c * BUCKET_CAP + n] {
                v += e.vel;
                // 0xE4DED0 divides 1 by the entry's stale invRho (retail: no
                // zero check, the first step reads infinity).
                rho += 1.0 / e.stale_inv_rho;
            }
            if n == 0 {
                self.grid.velocity[c] = Vec2::ZERO;
                self.grid.speed[c] = 0.0;
                self.grid.density[c] = 0.0;
            } else {
                let inv = 1.0 / n as f32;
                let mean = v * inv;
                self.grid.velocity[c] = mean;
                self.grid.speed[c] = mean.length();
                self.grid.density[c] = rho * inv;
            }
        }
        self.grid.box_min = self.box_min;
    }

    /// The advect-map current at a world position (0xE56D40): the map cell of
    /// `E56C30` times the current strength.
    fn advect_velocity(&self, p: Vec2) -> Vec2 {
        let Some(map) = &self.advect else {
            return Vec2::ZERO;
        };
        let (ix, iy) = advect_index(self.advect_tile, p);
        let v = map.at(ix, iy);
        Vec2::new(v[0], v[1]) * self.advect_strength
    }

    /// 0xE59EE0 after the SPH: `v += a dt`, `x += v dt + displacement`, wrap.
    fn integrate(&mut self, dt: f32) {
        let width = self.box_max.x - self.box_min.x;
        let height = self.box_max.y - self.box_min.y;
        for i in 0..PARTICLES {
            let v = self.vel[i] + self.accel[i] * dt;
            self.vel[i] = v;
            let mut p = self.pos[i];
            p.x += v.x * dt + self.disp[i].x;
            p.y += v.y * dt + self.disp[i].y;
            let outside = self.box_min.x > p.x || p.x > self.box_max.x || self.box_min.y > p.y || p.y > self.box_max.y;
            if outside && !self.pinned[i] {
                if self.box_min.x > p.x {
                    p.x += width;
                } else if p.x > self.box_max.x {
                    p.x -= width;
                }
                if self.box_min.y > p.y {
                    p.y += height;
                } else if p.y > self.box_max.y {
                    p.y -= height;
                }
                self.vel[i] = self.advect_velocity(p);
            }
            // Guard (not in retail): a non-finite particle is re-seeded.
            if !(p.x.is_finite() && p.y.is_finite() && self.vel[i].is_finite()) {
                let u = Vec2::new(self.rng.random::<f32>(), self.rng.random::<f32>());
                p = self.box_min + (self.box_max - self.box_min) * u;
                self.vel[i] = Vec2::ZERO;
            }
            self.pos[i] = p;
        }
        self.accel.fill(Vec2::ZERO);
        self.disp.fill(Vec2::ZERO);
    }

    /// 0xE4DBA0: take a particle as a pinned one at `pos` with velocity `vel`
    /// (pinned particles never wrap). `None` when the pool is full (1499).
    pub fn pin(&mut self, pos: Vec2, vel: Vec2) -> Option<usize> {
        if !self.ready || self.pin_count >= PARTICLES - 1 {
            return None;
        }
        let index = if self.pin_count == self.pin_high {
            self.pin_high += 1;
            self.pin_high
        } else {
            let head = self.pin_free as usize;
            self.pin_free = self.pin_next[head];
            head
        };
        self.pin_count += 1;
        self.pos[index] = pos;
        self.vel[index] = vel;
        self.accel[index] = Vec2::ZERO;
        self.pinned[index] = true;
        Some(index)
    }

    /// Particles currently pinned (+0x1BD54).
    pub fn pinned_count(&self) -> usize {
        self.pin_count
    }

    /// 0xE4EC10: free a pinned particle: it returns to a random point of the
    /// box with the advect current there and joins the free list.
    pub fn release(&mut self, index: usize) {
        if !self.ready || index >= PARTICLES || !self.pinned[index] {
            return;
        }
        let u = Vec2::new(self.rng.random::<f32>(), self.rng.random::<f32>());
        let p = self.box_min + (self.box_max - self.box_min) * u;
        self.pos[index] = p;
        self.vel[index] = self.advect_velocity(p);
        self.pin_count -= 1;
        self.pinned[index] = false;
        self.pin_next[index] = self.pin_free;
        self.pin_free = index as i32;
    }

    /// The displacement writers' output (0xE5A430): the position change a
    /// particle takes in the next step (`target - position` of a soft-body
    /// outline particle; skipped below |d|^2 = 2^-16).
    pub fn displace(&mut self, index: usize, d: Vec2) {
        if self.ready && index < PARTICLES && d.length_squared() >= MIN_DISPLACEMENT_R2 {
            self.disp[index] = d;
        }
    }
}

/// The Cell simulation hook: advance the water for this frame (runs after
/// `cell_engine::frame_begin`, before the cells read `FlowAt`).
pub fn update(
    time: Res<Time>,
    mut game: ResMut<CellGame>,
    mut traced: Local<u64>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut uploaded: Local<u64>,
) {
    let game = &mut *game;
    let tile = real_size(game.flow_scale) / game.world_scale.max(1e-6);
    let inputs = FluidInputs {
        look: game.camera_look / UNIT,
        origin_abs: game.origin_abs,
        world_scale: game.world_scale,
        map: game.flow_map.as_ref(),
        tile,
        strength: ADVECT_STRENGTH_FACTOR * game.flow[0],
        seed: game.seed,
    };
    game.fluid.update(time.delta_secs(), &inputs);
    // 0xE831A0 (from the GFX post step 0xE73F60): the grids go into the three
    // dynamic textures every frame the water stepped.
    if game.fluid.ready() && game.fluid.steps() != *uploaded {
        *uploaded = game.fluid.steps();
        crate::cell_fluid_tex::publish(game.fluid.grid());
        if let Some(images) = images.as_deref_mut() {
            crate::cell_fluid_tex::sync(images);
        }
    }
    if std::env::var_os("OPENSPORE_FLUID_TRACE").is_some() && game.fluid.steps() / 300 != *traced {
        *traced = game.fluid.steps() / 300;
        eprintln!("{}", game.fluid.stats());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant_map(v: [f32; 3]) -> Arc<AdvectMap> {
        Arc::new(AdvectMap { vectors: vec![v; ADVECT_SIDE * ADVECT_SIDE] })
    }

    fn inputs(map: Option<&Arc<AdvectMap>>, look: Vec2, strength: f32) -> FluidInputs<'_> {
        FluidInputs { look, origin_abs: Vec2::ZERO, world_scale: 1.0, map, tile: 100.0, strength, seed: 7 }
    }

    /// Density of a particle in the middle of a uniform square lattice, by
    /// the formula 315 / (64 PI) * sum (1 - r^2)^3.
    #[test]
    fn density_of_a_uniform_lattice() {
        let spacing = 0.5f32;
        let box_min = Vec2::new(-10.0, -7.5);
        let mut positions = Vec::new();
        // 1500 particles: a 40 x 37 lattice (1480) plus 20 extra in a corner row.
        let mut k = 0;
        'outer: for y in 0..38 {
            for x in 0..40 {
                if k == PARTICLES {
                    break 'outer;
                }
                positions.push(box_min + Vec2::new(0.25 + x as f32 * spacing, 0.25 + y as f32 * spacing));
                k += 1;
            }
        }
        let mut f = CellFluid::with_particles(box_min, &positions);
        f.sph();
        // The lattice particle nearest to the box centre has a full neighbourhood.
        let centre = positions
            .iter()
            .position(|p| p.distance(Vec2::ZERO) < 0.4)
            .expect("a lattice point near the centre");
        let c = positions[centre];
        let mut expected = 0.0f32;
        for p in &positions {
            let r2 = p.distance_squared(c);
            if r2 <= 1.0 {
                expected += (1.0 - r2).powi(3);
            }
        }
        expected *= 315.0 / (64.0 * std::f32::consts::PI);
        // 3 x 3 buckets of at most 8 hold 4 lattice points per cell here, so
        // nothing is dropped from the neighbourhood.
        assert!((f.density[centre] - expected).abs() < 1e-3, "{} vs {}", f.density[centre], expected);
        assert!((f.pressure[centre] - 0.5 * f.density[centre]).abs() < 1e-6);
        assert!((f.inv_rho[centre] * (f.density[centre] + INV_RHO_EPSILON) - 1.0).abs() < 1e-5);
        // A symmetric lattice has no net force on the middle particle.
        assert!(f.accel[centre].length() < 1e-3, "{:?}", f.accel[centre]);
    }

    /// 0xE4DED0: the density grid averages `1 / entry+0x18`, the bucket copy of
    /// the previous step's `invRho`: infinite on the first step (it is still
    /// zero when 0xE4DDC0 fills the buckets), then last step's `rho + 2^-16`.
    #[test]
    fn the_density_grid_reads_the_previous_steps_inv_rho() {
        let box_min = Vec2::new(-10.0, -7.5);
        let mut f = CellFluid::with_particles(box_min, &[Vec2::new(0.0, 0.0), Vec2::new(0.5, 0.0)]);
        let c = f.cell_of(Vec2::ZERO).unwrap();
        assert_eq!(c, f.cell_of(Vec2::new(0.5, 0.0)).unwrap());
        f.sph();
        assert!(f.grid.density[c].is_infinite(), "{}", f.grid.density[c]);
        let first = f.inv_rho.clone();
        f.sph();
        let want = (1.0 / first[0] + 1.0 / first[1]) / 2.0;
        assert!((f.grid.density[c] - want).abs() < 1e-3, "{} vs {want}", f.grid.density[c]);
        assert!(f.grid.density[c].is_finite());
        // An empty cell has none.
        assert_eq!(f.grid.density[f.cell_of(Vec2::new(8.0, 5.0)).unwrap()], 0.0);
    }

    #[test]
    fn kernel_constants_match_the_static_initialisers() {
        assert!((DENSITY_KERNEL - 315.0 / (64.0 * std::f32::consts::PI)).abs() < 1e-6);
        assert!((PRESSURE_KERNEL + 45.0 / std::f32::consts::PI).abs() < 1e-6);
        assert!((VISCOSITY_KERNEL - 45.0 / std::f32::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn two_close_particles_repel_and_a_lone_one_does_not_move() {
        let box_min = Vec2::new(-10.0, -7.5);
        let mut f = CellFluid::with_particles(box_min, &[Vec2::new(-0.2, 0.0), Vec2::new(0.2, 0.0), Vec2::new(8.0, 5.0)]);
        f.sph();
        assert!(f.accel[0].x < 0.0, "left particle is pushed left: {:?}", f.accel[0]);
        assert!(f.accel[1].x > 0.0, "right particle is pushed right: {:?}", f.accel[1]);
        assert!(f.accel[0].y.abs() < 1e-6);
        // Particle 2 only feels the 1497 particles parked on the box corner
        // (all at r > 1), i.e. nothing.
        assert!(f.accel[2].length() < 1e-6);
    }

    #[test]
    fn viscosity_pulls_a_neighbour_toward_the_mean_velocity() {
        let box_min = Vec2::new(-10.0, -7.5);
        let mut f = CellFluid::with_particles(box_min, &[Vec2::new(0.0, 0.0), Vec2::new(0.5, 0.0)]);
        f.vel[1] = Vec2::new(0.0, 2.0);
        f.sph();
        // The pressure term is along x; the y acceleration is pure viscosity
        // and points toward the faster neighbour.
        assert!(f.accel[0].y > 0.0 && f.accel[1].y < 0.0, "{:?} {:?}", f.accel[0], f.accel[1]);
    }

    #[test]
    fn advect_index_matches_the_retail_formula() {
        let tile = 100.0;
        assert_eq!(advect_index(tile, Vec2::new(0.0, 0.0)), (0, 0));
        // frac 0.5 -> 63.99999 -> 63
        assert_eq!(advect_index(tile, Vec2::new(50.0, 25.0)), (63, 31));
        // Negative positions wrap: -25 -> frac 0.75 -> 95.99999 -> 95.
        assert_eq!(advect_index(tile, Vec2::new(-25.0, 100.0)).0, 95);
        assert_eq!(advect_index(tile, Vec2::new(99.999, 0.0)).0, 127);
        // Tiles repeat.
        assert_eq!(advect_index(tile, Vec2::new(150.0, 250.0)), advect_index(tile, Vec2::new(50.0, 50.0)));
    }

    /// A particle that leaves the box wraps by one box width and takes the
    /// map's current times the strength; a pinned one stays outside.
    #[test]
    fn wrap_takes_the_advect_current() {
        let box_min = Vec2::new(-10.0, -7.5);
        let map = constant_map([0.5, -0.25, 9.0]);
        let mut f = CellFluid::with_particles(box_min, &[Vec2::new(9.99, 0.0), Vec2::new(-9.99, 7.4), Vec2::new(9.99, 1.0)]);
        f.advect = Some(map);
        f.advect_tile = 100.0;
        f.advect_strength = 3.0;
        f.set_box(Vec2::ZERO);
        f.vel[0] = Vec2::new(1.0, 0.0);
        f.vel[1] = Vec2::new(0.0, 6.0);
        f.vel[2] = Vec2::new(1.0, 0.0);
        f.pinned[2] = true;
        f.integrate(0.1);
        // 9.99 + 0.1 = 10.09 > 10 -> 10.09 - 20.
        assert!((f.pos[0].x + 9.91).abs() < 1e-4, "{:?}", f.pos[0]);
        assert!((f.vel[0] - Vec2::new(1.5, -0.75)).length() < 1e-6, "{:?}", f.vel[0]);
        // y: 7.4 + 0.6 = 8.0 > 7.5 -> 8.0 - 15.
        assert!((f.pos[1].y + 7.0).abs() < 1e-4, "{:?}", f.pos[1]);
        assert!((f.vel[1] - Vec2::new(1.5, -0.75)).length() < 1e-6);
        // The pinned one keeps its velocity and stays outside.
        assert!((f.pos[2].x - 10.09).abs() < 1e-4);
        assert_eq!(f.vel[2], Vec2::new(1.0, 0.0));
        // Accelerations and displacements are cleared.
        assert!(f.accel.iter().all(|a| *a == Vec2::ZERO) && f.disp.iter().all(|d| *d == Vec2::ZERO));
    }

    #[test]
    fn displacement_moves_the_particle_once() {
        let mut f = CellFluid::with_particles(Vec2::new(-10.0, -7.5), &[Vec2::new(1.0, 1.0)]);
        f.set_box(Vec2::ZERO);
        f.displace(0, Vec2::new(0.5, 0.0));
        f.displace(1, Vec2::new(0.001, 0.0)); // below the 2^-16 threshold: ignored
        f.integrate(0.0);
        assert!((f.pos[0] - Vec2::new(1.5, 1.0)).length() < 1e-6);
        f.integrate(0.0);
        assert!((f.pos[0] - Vec2::new(1.5, 1.0)).length() < 1e-6, "applied only once");
        f.displace(2, Vec2::new(0.001, 0.0));
        assert_eq!(f.disp[2], Vec2::ZERO);
    }

    /// Bilinear `FlowAt`: cell centres return the cell, the midpoint of two
    /// centres the mean, the box edge clamps.
    #[test]
    fn flow_at_is_bilinear_over_cell_centres() {
        let mut g = FluidGrid { box_min: Vec2::new(-10.0, -7.5), ..default() };
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                g.velocity[y * GRID_W + x] = Vec2::new(x as f32, y as f32 * 10.0);
            }
        }
        let at = |gx: f32, gy: f32| g.flow_at(g.box_min + Vec2::new(gx, gy));
        // The centre of cell (5, 3).
        assert!((at(5.5, 3.5) - Vec2::new(5.0, 30.0)).length() < 1e-4);
        // Midway between the centres of cells (5, 3) and (6, 3).
        assert!((at(6.0, 3.5) - Vec2::new(5.5, 30.0)).length() < 1e-4);
        // The corner shared by (5,3) (6,3) (5,4) (6,4).
        assert!((at(6.0, 4.0) - Vec2::new(5.5, 35.0)).length() < 1e-4);
        // A quarter of the way between two centres.
        assert!((at(5.75, 3.5) - Vec2::new(5.25, 30.0)).length() < 1e-4);
        // Edge cells clamp: the left half of cell 0 and everything outside.
        assert!((at(0.2, 0.2) - Vec2::new(0.0, 0.0)).length() < 1e-4);
        assert!((at(-50.0, -50.0) - Vec2::new(0.0, 0.0)).length() < 1e-4);
        assert!((at(100.0, 100.0) - Vec2::new(19.0, 140.0)).length() < 1e-3);
    }

    /// The four-cell speed sum (0xE4E570): the cells around the point, each
    /// once.
    #[test]
    fn speed_sum_adds_the_four_nearest_cells() {
        let mut g = FluidGrid { box_min: Vec2::ZERO, ..default() };
        g.speed[3 * GRID_W + 5] = 1.0;
        g.speed[3 * GRID_W + 6] = 2.0;
        g.speed[4 * GRID_W + 5] = 4.0;
        g.speed[4 * GRID_W + 6] = 8.0;
        g.speed[7 * GRID_W + 7] = 100.0;
        // The shared corner of the four cells.
        assert_eq!(g.speed_sum_at(Vec2::new(6.0, 4.0)), 15.0);
        // The upper-left quadrant of cell (6, 4) is still the same four cells.
        assert_eq!(g.speed_sum_at(Vec2::new(6.2, 4.2)), 15.0);
        // In the corner cell the clamped edge counts the same cell four times.
        g.speed[0] = 3.0;
        assert_eq!(g.speed_sum_at(Vec2::new(0.25, 0.25)), 12.0);
        // Outside the box clamps to the same corner.
        assert_eq!(g.speed_sum_at(Vec2::new(-40.0, -40.0)), 12.0);
    }

    #[test]
    fn pin_pool_allocates_reuses_and_runs_out() {
        let mut f = CellFluid::with_particles(Vec2::new(-10.0, -7.5), &[]);
        let a = f.pin(Vec2::new(1.0, 2.0), Vec2::new(0.1, 0.2)).unwrap();
        let b = f.pin(Vec2::new(3.0, 4.0), Vec2::ZERO).unwrap();
        assert_eq!((a, b), (1, 2), "the first allocation is index 1 (0xE4DBA0)");
        assert!(f.is_pinned(a) && f.is_pinned(b));
        assert_eq!(f.particle(a), (Vec2::new(1.0, 2.0), Vec2::new(0.1, 0.2)));
        f.release(a);
        assert!(!f.is_pinned(a));
        // A released particle is back inside the box.
        let (p, _) = f.particle(a);
        assert!(p.x >= -10.0 && p.x <= 10.0 && p.y >= -7.5 && p.y <= 7.5);
        assert_eq!(f.pin(Vec2::ZERO, Vec2::ZERO), Some(a), "the free list is reused first");
        let mut n = 2;
        while f.pin(Vec2::ZERO, Vec2::ZERO).is_some() {
            n += 1;
        }
        assert_eq!(n, PARTICLES - 1, "1499 pins at most");
    }

    /// The same seed and inputs give bit-identical water; another seed does not.
    #[test]
    fn the_water_is_deterministic() {
        let map = constant_map([0.8, 0.3, 0.0]);
        let run = |seed: u64| {
            let mut f = CellFluid::default();
            let mut look = Vec2::ZERO;
            for frame in 0..180 {
                look += Vec2::new(0.03, 0.01);
                let mut input = inputs(Some(&map), look, 2.0);
                input.seed = seed;
                f.update(1.0 / 60.0 + (frame % 3) as f32 * 1e-4, &input);
            }
            f
        };
        let (a, b, c) = (run(5), run(5), run(6));
        assert!(a.steps() >= 170);
        assert_eq!(a.steps(), b.steps());
        for i in 0..PARTICLES {
            assert_eq!(a.particle(i).0.to_array().map(f32::to_bits), b.particle(i).0.to_array().map(f32::to_bits));
            assert_eq!(a.particle(i).1.to_array().map(f32::to_bits), b.particle(i).1.to_array().map(f32::to_bits));
        }
        assert!((0..PARTICLES).any(|i| a.particle(i).0 != c.particle(i).0));
    }

    /// Particles never leave the (moving) box for good and stay finite, and
    /// the water at rest in a box that does not move develops a current from
    /// the edge wraps (the only source of motion).
    #[test]
    fn the_current_develops_from_the_edge_wraps() {
        let map = constant_map([1.0, 0.0, 0.0]);
        let mut f = CellFluid::default();
        let input = inputs(Some(&map), Vec2::ZERO, 2.0);
        for _ in 0..1200 {
            f.update(STEP, &input);
        }
        let mean: Vec2 = (0..PARTICLES).map(|i| f.particle(i).1).sum::<Vec2>() / PARTICLES as f32;
        let finite = (0..PARTICLES).all(|i| f.particle(i).0.is_finite() && f.particle(i).1.is_finite());
        assert!(finite);
        eprintln!("mean velocity after 20 s: {mean:?}, steps {}", f.steps());
        let inside = (0..PARTICLES)
            .filter(|&i| {
                let p = f.particle(i).0;
                p.x.abs() <= 10.5 && p.y.abs() <= 8.0
            })
            .count();
        assert!(inside > PARTICLES * 9 / 10, "{inside} of {PARTICLES} inside");
        assert!(mean.x > 0.1, "a current along the map direction develops: {mean:?}");
    }

    /// The effect map answers in Bevy units: velocity x UNIT, height = the
    /// four-cell speed sum (retail vtable 0x14852B8).
    #[test]
    fn effect_map_reports_flow_at_and_the_speed_sum() {
        use crate::swarm_fx::EffectMapSampler;
        let mut g = FluidGrid { box_min: Vec2::new(-10.0, -7.5), ..default() };
        g.velocity.fill(Vec2::new(0.5, -0.25));
        g.speed.fill(2.0);
        let at = Vec3::new(1.0 * UNIT, 3.0, 2.0 * UNIT);
        let v = EffectMapSampler::velocity(&g, at);
        assert!((v - Vec3::new(0.5 * UNIT, 0.0, -0.25 * UNIT)).length() < 1e-5, "{v:?}");
        assert_eq!(EffectMapSampler::height(&g, at), 8.0);
    }

    /// A world recentre moves the engine's frame; the particles keep their
    /// place in space. A rescale changes nothing (free particles).
    #[test]
    fn the_water_follows_a_world_recentre() {
        let mut f = CellFluid::default();
        let mut input = inputs(None, Vec2::ZERO, 0.0);
        f.update(0.0, &input);
        let before: Vec<Vec2> = (0..PARTICLES).map(|i| f.particle(i).0).collect();
        // The engine moved the origin by (50, -20) absolute units at scale 2:
        // world coordinates drop by (25, -10) while the look point follows.
        input.origin_abs = Vec2::new(50.0, -20.0);
        input.world_scale = 2.0;
        f.last_origin = Vec2::ZERO;
        f.update(0.0, &input);
        for i in [0, 100, 1499] {
            let want = before[i] + Vec2::new(-25.0, 10.0);
            assert!((f.particle(i).0 - want).length() < 1e-4, "{i}: {:?} vs {want:?}", f.particle(i).0);
        }
    }

    /// The box slides with the look point: after the camera has moved far, the
    /// water is still inside the box (everything wrapped in).
    #[test]
    fn the_water_follows_a_moving_box() {
        let map = constant_map([0.0, 0.0, 0.0]);
        let mut f = CellFluid::default();
        let mut look = Vec2::ZERO;
        for _ in 0..600 {
            look += Vec2::new(0.1, -0.04);
            f.update(STEP, &inputs(Some(&map), look, 0.0));
        }
        let (lo, hi) = (look - VISIBLE_BOX_HALF - Vec2::splat(0.5), look + VISIBLE_BOX_HALF + Vec2::splat(0.5));
        let inside = (0..PARTICLES).filter(|&i| f.particle(i).0.cmpge(lo).all() && f.particle(i).0.cmple(hi).all()).count();
        assert!(inside > PARTICLES * 95 / 100, "{inside} of {PARTICLES} inside the moving box");
    }

    /// The engine's `FlowAt` and the audio input read the water of the game
    /// resource, not the advect map.
    #[test]
    fn the_cells_and_the_audio_read_the_water() {
        let map = constant_map([0.4, 0.0, 0.0]);
        let mut game = CellGame::default();
        assert_eq!(crate::cell_motion::flow_at(&game, Vec2::ZERO), Vec2::ZERO, "no water yet");
        for _ in 0..600 {
            game.fluid.update(STEP, &inputs(Some(&map), Vec2::ZERO, 2.0));
        }
        let p = Vec2::new(1.0, 1.0);
        let flow = crate::cell_motion::flow_at(&game, p);
        assert_eq!(flow, game.fluid.grid().flow_at(p));
        assert!(flow.length() > 0.0, "{flow:?}");
        let sum = game.fluid.speed_sum_at(p);
        assert!(sum > 0.0);
        assert_eq!(crate::cell_audio::flow_field_param(sum), (0.1 * sum).clamp(0.0, 1.0));
    }

    #[test]
    fn nothing_is_ready_before_the_first_update() {
        let f = CellFluid::default();
        assert!(!f.ready());
        assert_eq!(f.flow_at(Vec2::ZERO), Vec2::ZERO);
        assert_eq!(f.speed_sum_at(Vec2::ZERO), 0.0);
    }
}
