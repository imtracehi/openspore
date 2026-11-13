//! The soft bodies of the Cell stage: retail moves foreground cells through
//! their pinned water particles (`notes\cell-fluid-notes.md` section 5, decoded
//! again in `notes\cell-fluid2-notes.md`).
//!
//! A foreground cell whose record is not cellType None / Poison, whose
//! transform scale (`+0x58`, retail world units) is at least 0.3 and that is on
//! screen (its sphere of 1.7 sizes touches the visible box: 0xE78C00 creates the
//! GFX object, which allocates the pins, only then and destroys it, with the
//! pins, when the cell leaves; `cell_population::pin_visible`) owns "outline
//! pins": particles of the SPH water (`cell_fluid`, pool 0xE4DBA0 / 0xE4EC10)
//! that never wrap and that sit at the centres of the quadtree nodes of the
//! unit square (the tree built by 0xE4D530 / 0xE4D290 / 0xE4D1B0: depth 3,
//! children in the order (-,-) (+,-) (-,+) (+,+), z = 0) at the depth the size
//! selects: size >= 2 -> 3 (64 pins), >= 1 -> 2 (16), >= 0.3 -> 1 (4), else 0
//! (no pins, the cell keeps the `FlowAt` drag of `cell_motion`) (0xE5A1A0,
//! 0xE6D97B). Per simulation step (0xE7C8C0, per cell, after the water step):
//!  1. steering turns the cell: the pins are rotated about the cell position by
//!     the change of orientation (0xE5E850);
//!  2. shape matching (0xE5A430 -> 0xE85D70): centre of mass of the pins,
//!     `A = sum p_i (size q_i)^T`, the rotation `R` of the polar decomposition
//!     of `A` (0xE85170: Jacobi steps in the x / y plane), goals
//!     `com + R size q_i`; every pin whose `goal - position` is at least 2^-8
//!     long gets that as the water's one-step displacement (`f+0x11944`, applied
//!     by the next water step: `x += v dt + disp`, so the pins return to the
//!     rigid shape every step and keep their own velocity); the cell's rotation
//!     is `R` (transform matrix +0x5C, flag |= 2). Cells with `fixedOrientation`
//!     (record +0xAC) skip this step;
//!  3. 0xE7C080 case (a): the cell's acceleration times dt is added to the
//!     velocity of every pin (thrust impulses too, 0xE560D0), the water
//!     integrates the pins with everything else (SPH pressure and viscosity:
//!     the cell pushes the water and is pushed and dragged by it, no `FlowAt`
//!     drag), and the cell's position and velocity are READ BACK as the mean of
//!     the pins' (0xE553A0).
//!
//! The fit also stores the cell's angular speed (`+0x1D0`, 0xE5A2A0): the
//! signed angle between the old and the new rotation applied to (0, -1, 0),
//! in [0, 2 PI), over the frame's `dt` (`SoftBody::spin`). No code in the Cell
//! range of the image reads it back (a scan for `[reg + 0x1D0]` between
//! 0xE40000 and 0xEA0000 finds only its writer 0xE5A55F), so nothing consumes
//! it here either.
//!
//! The plane is the one of `cell_motion` (Bevy x, z over `UNIT`, the water's
//! x / y). ADAPTED: the shape's zero angle is the facing of heading 0 (retail
//! derives the facing from the matrix), `heading = -angle`.

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use crate::cell_data::{CellRecord, CellType};
use crate::cell_fluid::CellFluid;

/// Outline detail levels (0xE5A1A0): the quadtree depth.
pub const MAX_LEVEL: usize = 3;
/// Size thresholds of the levels: `>= 2` -> 3, `>= 1` -> 2, `>= 0.3` -> 1
/// (.rdata 0x1470EAC, 0x1453740, 0x13EB8BC).
pub const LEVEL_SIZES: [f32; 3] = [0.3, 1.0, 2.0];

/// Detail level of a cell of `size` retail world units (0xE5A1A0).
pub fn level_for_size(size: f32) -> usize {
    if size >= LEVEL_SIZES[2] {
        3
    } else if size >= LEVEL_SIZES[1] {
        2
    } else if size >= LEVEL_SIZES[0] {
        1
    } else {
        0
    }
}

/// Pins of a level: 4^level (the nodes of the quadtree at that depth).
pub fn pin_count(level: usize) -> usize {
    1 << (2 * level)
}

/// One quadtree node box of 0xE4D1B0 and the walk of 0xE4D5E0: the centres
/// of the nodes at depth `level` (the rest shape `q`, retail `.data 0x16AD6C0`
/// + level * 0x300), children (-,-) (+,-) (-,+) (+,+).
fn collect(level: usize, depth: usize, min: Vec2, max: Vec2, out: &mut Vec<Vec2>) {
    if depth == level {
        out.push((min + max) * 0.5);
        return;
    }
    let mid = (min + max) * 0.5;
    collect(level, depth + 1, min, mid, out);
    collect(level, depth + 1, Vec2::new(mid.x, min.y), Vec2::new(max.x, mid.y), out);
    collect(level, depth + 1, Vec2::new(min.x, mid.y), Vec2::new(mid.x, max.y), out);
    collect(level, depth + 1, mid, max, out);
}

/// The rest shape of a level: node centres of the unit square (-1..1)^2.
pub fn rest_shape(level: usize) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(pin_count(level));
    collect(level.min(MAX_LEVEL), 0, Vec2::splat(-1.0), Vec2::splat(1.0), &mut out);
    out
}

pub fn wrap_pi(a: f32) -> f32 {
    let a = (a + PI).rem_euclid(TAU) - PI;
    if a <= -PI {
        a + TAU
    } else {
        a
    }
}

/// Rotation by `angle` (counter-clockwise in the water's x / y plane).
fn rot(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// `OPENSPORE_NO_SOFTBODY=1` (diagnostics, read once): no cell gets pins, every
/// cell keeps the `FlowAt` drag (the behaviour before the soft bodies).
pub fn disabled() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("OPENSPORE_NO_SOFTBODY").is_some())
}

/// Which cells own outline pins: every foreground cell except cellType None
/// (never integrated) and Poison (0xE5A1A0, 0xE6D999: `+0xB4` 0 or 5).
pub fn has_outline(record: &CellRecord) -> bool {
    !matches!(record.cell_type, CellType::None | CellType::Poison)
}

/// Heading (`CellObject::heading`, facing (-sin h, -cos h) on the plane) of a
/// body angle. ADAPTED: the facing of angle 0 is (0, -1), the facing of heading
/// 0 (the matrix's local -Y axis in retail, 0xE5EB20).
pub fn heading_of_angle(angle: f32) -> f32 {
    wrap_pi(-angle)
}

pub fn angle_of_heading(heading: f32) -> f32 {
    wrap_pi(-heading)
}

/// 0xE5A2A0: the angular speed the fit writes to `+0x1D0`: the signed angle
/// (0x6994A0 about +z: `acos(dot)` when the cross product points along +z, else
/// `2 PI - acos(dot)`, `2 PI` = .data 0x16017A8 set by 0x12C1310 from
/// .rdata 0x152DE2C = 6.2831855) from the old rotation applied to (0, -1, 0)
/// to the new one, divided by `dt`. Equal vectors give 0.
pub fn retail_spin(old: f32, new: f32, dt: f32) -> f32 {
    let a = rot(Vec2::NEG_Y, old);
    let b = rot(Vec2::NEG_Y, new);
    if a == b {
        return 0.0;
    }
    let dot = a.dot(b).clamp(-1.0, 1.0);
    let cross = a.x * b.y - a.y * b.x;
    let angle = if cross > 0.0 { dot.acos() } else { TAU - dot.acos() };
    angle / dt
}

/// Result of the shape matching (0xE85D70 with beta = 0).
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    pub com: Vec2,
    /// The rotation of the polar decomposition (matrix +0x5C), `None` when
    /// `A` vanishes (retail then keeps the identity; here: keep the angle).
    pub angle: Option<f32>,
}

/// Centre of mass (0xE85370, unit weights: the mean) and the rotation of the
/// polar decomposition of `A = sum p_i (size q_i)^T` (0xE85170). In 2-D the
/// Jacobi steps solve it at once: the angle that turns `size q_i` onto `p_i`.
pub fn fit(cur: &[Vec2], rest: &[Vec2], size: f32) -> Fit {
    let n = cur.len().min(rest.len());
    let com = cur[..n].iter().copied().sum::<Vec2>() / n.max(1) as f32;
    let (mut dot, mut cross) = (0.0f32, 0.0f32);
    for i in 0..n {
        let p = cur[i] - com;
        let q = rest[i] * size;
        dot += q.x * p.x + q.y * p.y;
        cross += q.x * p.y - q.y * p.x;
    }
    let angle = if dot * dot + cross * cross > 0.0 { Some(cross.atan2(dot)) } else { None };
    Fit { com, angle }
}

/// Goal positions `com + R size q_i` (0xE85D70's output array).
pub fn goals(fit: &Fit, angle: f32, rest: &[Vec2], size: f32) -> Vec<Vec2> {
    rest.iter().map(|q| fit.com + rot(*q * size, angle)).collect()
}

/// The pins of one cell (retail cell fields +0x24C level, +0x250 count, +0x254
/// indices) and what the last integrate wrote into the cell.
#[derive(Clone, Debug)]
pub struct SoftBody {
    pub level: usize,
    pub pins: Vec<usize>,
    /// Rotation of the shape (transform matrix +0x5C as an angle).
    pub angle: f32,
    /// The fit's angular speed (`+0x1D0`, see `retail_spin`), rad/s, in [0, 2 PI / dt).
    pub spin: f32,
    /// Position and velocity the last integrate wrote to the cell (retail
    /// units): what lets later edits by other systems (contact impulses,
    /// events, world rescale) reach the pins.
    pub pos: Vec2,
    pub vel: Vec2,
}

/// All soft bodies of the stage, by cell entity (`CellGame::bodies`).
#[derive(Default)]
pub struct SoftBodies {
    pub map: HashMap<Entity, SoftBody>,
}

impl SoftBody {
    /// A cell gets its pins (0xE6D97B -> 0xE5CB60): one at every node centre of
    /// the level, `pos + size * R q`, with the cell's velocity. `None` (and
    /// nothing allocated) when the pool cannot hold them all. ADAPTED: retail's
    /// allocator returns -1 when the pool (1499) is exhausted and the caller
    /// goes on with that index; the cell here stays pin-less (the `FlowAt` drag).
    pub fn create(fluid: &mut CellFluid, level: usize, size: f32, pos: Vec2, vel: Vec2, angle: f32) -> Option<SoftBody> {
        if level == 0 || !fluid.ready() {
            return None;
        }
        let mut body = SoftBody { level, pins: Vec::new(), angle, spin: 0.0, pos, vel };
        for q in rest_shape(level) {
            match fluid.pin(pos + rot(q * size, angle), vel) {
                Some(i) => body.pins.push(i),
                None => {
                    body.release(fluid);
                    return None;
                }
            }
        }
        Some(body)
    }

    /// Give every pin back to the water (0xE4EC10).
    pub fn release(&mut self, fluid: &mut CellFluid) {
        for i in self.pins.drain(..) {
            fluid.release(i);
        }
    }

    /// 0xE553A0: the mean position and velocity of the pins.
    pub fn read_back(&self, fluid: &CellFluid) -> (Vec2, Vec2) {
        let n = self.pins.len().max(1) as f32;
        let (mut p, mut v) = (Vec2::ZERO, Vec2::ZERO);
        for &i in &self.pins {
            let (pi, vi) = fluid.particle(i);
            p += pi;
            v += vi;
        }
        (p / n, v / n)
    }

    /// The cell's velocity changed by `dv` (thrust impulse, 0xE560D0; the
    /// cell's acceleration times dt, 0xE7C080): every pin takes it.
    pub fn add_velocity(&self, fluid: &mut CellFluid, dv: Vec2) {
        if dv == Vec2::ZERO {
            return;
        }
        for &i in &self.pins {
            fluid.add_velocity(i, dv);
        }
    }

    /// The cell was moved by `d` by someone else (world rescale, recentre,
    /// teleports): the pins move with it.
    pub fn shift(&self, fluid: &mut CellFluid, d: Vec2) {
        if d == Vec2::ZERO {
            return;
        }
        for &i in &self.pins {
            let (p, v) = fluid.particle(i);
            fluid.set_particle(i, p + d, v);
        }
    }

    /// 0xE5E850: the cell's orientation changed by `delta`: every pin turns
    /// about the cell position `about` by it.
    pub fn rotate(&mut self, fluid: &mut CellFluid, about: Vec2, delta: f32) {
        if delta == 0.0 {
            return;
        }
        for &i in &self.pins {
            let (p, v) = fluid.particle(i);
            fluid.set_particle(i, about + rot(p - about, delta), v);
        }
        self.angle = wrap_pi(self.angle + delta);
    }

    /// 0xE5A430: shape matching. Writes the displacement that returns every
    /// pin to the rigid goal shape in the next water step and returns the new
    /// rotation of the cell and stores the angular speed over `dt` (`+0x1D0`).
    /// `fixed` (record +0xAC) skips the step.
    pub fn match_shape(&mut self, fluid: &mut CellFluid, size: f32, fixed: bool, dt: f32) -> f32 {
        if fixed || self.pins.len() < 2 {
            return self.angle;
        }
        let rest = rest_shape(self.level);
        let cur: Vec<Vec2> = self.pins.iter().map(|&i| fluid.particle(i).0).collect();
        let fit = fit(&cur, &rest, size);
        let angle = fit.angle.unwrap_or(self.angle);
        for (k, goal) in goals(&fit, angle, &rest, size).into_iter().enumerate() {
            fluid.displace(self.pins[k], goal - cur[k]);
        }
        self.spin = retail_spin(self.angle, angle, dt);
        self.angle = angle;
        angle
    }

    /// 0xE51A20: the detail level changed. Growing: each old pin hands its
    /// position and velocity to the four pins of every child node; shrinking:
    /// the pins below a node are replaced by one at their mean; from / to level
    /// 0 the pins are created / released. Returns false when the pool ran out
    /// (everything is released then).
    pub fn regrid(&mut self, fluid: &mut CellFluid, level: usize, size: f32, pos: Vec2, vel: Vec2) -> bool {
        if level == self.level {
            return true;
        }
        let old: Vec<(Vec2, Vec2)> = self.pins.iter().map(|&i| fluid.particle(i)).collect();
        let old_level = self.level;
        self.release(fluid);
        self.level = level;
        if level == 0 {
            return true;
        }
        let mut wanted: Vec<(Vec2, Vec2)> = Vec::with_capacity(pin_count(level));
        if old_level == 0 || old.is_empty() {
            // 0xE51A20 (old pin count 1, the "no pins" marker) allocates every new
            // pin at the cell's position `+0x4C` with its velocity `+0x90`.
            // ADAPTED: placing them on the shape keeps the first fit from
            // resetting the rotation (all pins at one point fit the identity).
            for q in rest_shape(level) {
                wanted.push((pos + rot(q * size, self.angle), vel));
            }
        } else if level > old_level {
            let per = pin_count(level - old_level);
            for (p, v) in &old {
                for _ in 0..per {
                    wanted.push((*p, *v));
                }
            }
        } else {
            let per = pin_count(old_level - level);
            for group in old.chunks(per) {
                let n = group.len() as f32;
                let p = group.iter().map(|g| g.0).sum::<Vec2>() / n;
                let v = group.iter().map(|g| g.1).sum::<Vec2>() / n;
                wanted.push((p, v));
            }
        }
        for (p, v) in wanted {
            match fluid.pin(p, v) {
                Some(i) => self.pins.push(i),
                None => {
                    self.release(fluid);
                    return false;
                }
            }
        }
        true
    }
}

impl SoftBodies {
    /// Before a cell moves: its pins follow edits other systems made to it
    /// since the last integrate (contact impulses, events, the world rescale),
    /// are created (0xE6D97B) or re-allocated when the detail level changed
    /// (0xE5A1A0 -> 0xE51A20), or are dropped when the cell has none any more.
    /// `pos` / `vel` / `heading` are the cell's, `size` its transform scale (all
    /// retail units); true when the cell has a soft body. `level` 0 (no outline,
    /// too small, or the cell is off screen) gives the pins back.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        fluid: &mut CellFluid,
        entity: Entity,
        level: usize,
        size: f32,
        pos: Vec2,
        vel: Vec2,
        heading: f32,
    ) -> bool {
        if level == 0 {
            self.remove(fluid, entity);
            return false;
        }
        if let Some(body) = self.map.get_mut(&entity) {
            body.shift(fluid, pos - body.pos);
            body.add_velocity(fluid, vel - body.vel);
            body.pos = pos;
            body.vel = vel;
            if body.level != level && !body.regrid(fluid, level, size, pos, vel) {
                self.map.remove(&entity);
                return false;
            }
            return true;
        }
        match SoftBody::create(fluid, level, size, pos, vel, angle_of_heading(heading)) {
            Some(body) => {
                self.map.insert(entity, body);
                true
            }
            None => false,
        }
    }

    /// A cell lost its outline (or died): give its pins back (0xE660B0).
    pub fn remove(&mut self, fluid: &mut CellFluid, entity: Entity) {
        if let Some(mut body) = self.map.remove(&entity) {
            body.release(fluid);
        }
    }

    /// Drop the bodies of cells that no longer take part (despawned, dead, or
    /// moved to the background layer).
    pub fn retain_live(&mut self, fluid: &mut CellFluid, live: &dyn Fn(Entity) -> bool) {
        let gone: Vec<Entity> = self.map.keys().copied().filter(|e| !live(*e)).collect();
        for e in gone {
            self.remove(fluid, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_fluid::{FluidInputs, STEP};

    fn quiet_fluid(seed: u64) -> CellFluid {
        let mut f = CellFluid::default();
        f.update(
            0.0,
            &FluidInputs { look: Vec2::ZERO, origin_abs: Vec2::ZERO, world_scale: 1.0, map: None, tile: 100.0, strength: 0.0, seed },
        );
        f
    }

    /// A fluid whose free particles are replaced by an empty region: pins far
    /// from everything (the particles are parked in the opposite corner).
    fn isolated_fluid() -> CellFluid {
        let mut f = CellFluid::with_particles(Vec2::new(-10.0, -7.5), &[]);
        // The 1500 parked particles sit at the box corner (-10, -7.5); the
        // tests work around (5, 5).
        f.update(
            0.0,
            &FluidInputs { look: Vec2::ZERO, origin_abs: Vec2::ZERO, world_scale: 1.0, map: None, tile: 100.0, strength: 0.0, seed: 1 },
        );
        f
    }

    #[test]
    fn levels_follow_the_size_thresholds() {
        assert_eq!(level_for_size(0.29), 0);
        assert_eq!(level_for_size(0.476), 1, "the avatar (0.476) has 4 pins");
        assert_eq!(level_for_size(1.0), 2);
        assert_eq!(level_for_size(1.99), 2);
        assert_eq!(level_for_size(2.0), 3);
        assert_eq!([1, 4, 16, 64], [0, 1, 2, 3].map(pin_count));
    }

    #[test]
    fn rest_shape_is_the_quadtree_node_centres() {
        let q1 = rest_shape(1);
        assert_eq!(q1, vec![Vec2::new(-0.5, -0.5), Vec2::new(0.5, -0.5), Vec2::new(-0.5, 0.5), Vec2::new(0.5, 0.5)]);
        let q2 = rest_shape(2);
        assert_eq!(q2.len(), 16);
        // Depth-first: the first four are the children of the (-,-) node.
        assert_eq!(q2[0], Vec2::new(-0.75, -0.75));
        assert_eq!(q2[1], Vec2::new(-0.25, -0.75));
        assert_eq!(q2[2], Vec2::new(-0.75, -0.25));
        assert_eq!(q2[3], Vec2::new(-0.25, -0.25));
        assert_eq!(q2[4], Vec2::new(0.25, -0.75));
        let q3 = rest_shape(3);
        assert_eq!(q3.len(), 64);
        assert!(q3.iter().all(|p| p.abs().max_element() < 1.0));
        assert!((q3.iter().copied().sum::<Vec2>()).length() < 1e-4, "symmetric about the origin");
    }

    #[test]
    fn the_fit_recovers_the_rotation_and_the_centre() {
        let rest = rest_shape(2);
        let size = 1.3;
        for angle in [-2.9f32, -0.7, 0.0, 0.3, 1.9, 3.0] {
            let cur: Vec<Vec2> = rest.iter().map(|q| Vec2::new(4.0, -2.0) + rot(*q * size, angle)).collect();
            let f = fit(&cur, &rest, size);
            assert!((f.com - Vec2::new(4.0, -2.0)).length() < 1e-4);
            let got = f.angle.unwrap();
            assert!(wrap_pi(got - angle).abs() < 1e-4, "{got} vs {angle}");
            // The goals are the pins themselves.
            for (g, c) in goals(&f, got, &rest, size).iter().zip(&cur) {
                assert!((*g - *c).length() < 1e-4);
            }
        }
        // Coincident pins: no rotation (retail: identity).
        assert_eq!(fit(&[Vec2::ONE; 4], &rest_shape(1), 1.0).angle, None);
    }

    #[test]
    fn a_noisy_shape_fits_the_best_rotation_and_displaces_toward_the_goal() {
        let mut f = isolated_fluid();
        let mut body = SoftBody::create(&mut f, 1, 1.0, Vec2::new(5.0, 5.0), Vec2::ZERO, 0.0).unwrap();
        assert_eq!(body.pins.len(), 4);
        // Push pin 0 out by 0.2 and turn the whole body 0.4 rad.
        let (p0, v0) = f.particle(body.pins[0]);
        f.set_particle(body.pins[0], p0 + Vec2::new(-0.2, -0.2), v0);
        body.rotate(&mut f, Vec2::new(5.0, 5.0), 0.4);
        let angle = body.match_shape(&mut f, 1.0, false, STEP);
        assert!(angle > 0.3 && angle < 0.5, "{angle}");
        // The displacement is goal - position for every pin whose offset is at
        // least 2^-8; applying one step lands the pins on the goal shape.
        let rest = rest_shape(1);
        let cur: Vec<Vec2> = body.pins.iter().map(|&i| f.particle(i).0).collect();
        let fit = fit(&cur, &rest, 1.0);
        let want = goals(&fit, angle, &rest, 1.0);
        f.step(0.0);
        for (k, &i) in body.pins.iter().enumerate() {
            let got = f.particle(i).0;
            assert!((got - want[k]).length() < 1e-3, "pin {k}: {got:?} vs {:?}", want[k]);
        }
    }

    #[test]
    fn rotating_the_pins_turns_the_fitted_angle() {
        let mut f = isolated_fluid();
        let mut body = SoftBody::create(&mut f, 2, 1.5, Vec2::new(5.0, 5.0), Vec2::ZERO, 0.25).unwrap();
        assert!((body.match_shape(&mut f, 1.5, false, STEP) - 0.25).abs() < 1e-4);
        body.rotate(&mut f, Vec2::new(5.0, 5.0), -0.5);
        let a = body.match_shape(&mut f, 1.5, false, STEP);
        assert!((a + 0.25).abs() < 1e-4, "{a}");
        // Fixed-orientation cells do not fit.
        let before = body.angle;
        assert_eq!(body.match_shape(&mut f, 1.5, true, STEP), before);
    }

    /// 0xE5A2A0 / 0x6994A0: the fit's angular speed is the counter-clockwise
    /// angle in [0, 2 PI) over dt: a turn of +0.1 rad in 0.1 s reads 1.0, a turn
    /// of -0.1 rad reads (2 PI - 0.1) / 0.1, no turn 0.
    #[test]
    fn the_fit_stores_the_angular_speed() {
        assert!((retail_spin(0.3, 0.4, 0.1) - 1.0).abs() < 1e-3);
        assert!((retail_spin(0.4, 0.3, 0.1) - (TAU - 0.1) / 0.1).abs() < 0.05);
        assert_eq!(retail_spin(0.7, 0.7, 0.1), 0.0);
        assert!((retail_spin(3.0, -3.0, 1.0) - (TAU - 6.0)).abs() < 1e-4, "across the +-PI seam");
        let mut f = isolated_fluid();
        let mut body = SoftBody::create(&mut f, 1, 1.0, Vec2::new(5.0, 5.0), Vec2::ZERO, 0.0).unwrap();
        // The pins are 0.2 rad round, the cell's matrix says 0.1 (the water
        // turned the pins since the last fit): the water's torque is 0.1 rad.
        body.rotate(&mut f, Vec2::new(5.0, 5.0), 0.2);
        body.angle = 0.1;
        body.match_shape(&mut f, 1.0, false, 0.1);
        assert!((body.spin - 1.0).abs() < 1e-2, "{}", body.spin);
        // Steering rotates the pins and the matrix together: no spin.
        body.rotate(&mut f, Vec2::new(5.0, 5.0), 0.3);
        body.match_shape(&mut f, 1.0, false, 0.1);
        // (Retail's quirk: matrices that differ only by rounding read as 0 or, when the
        // cross product comes out non-positive, as a full turn, 2 PI / dt.)
        assert!(body.spin.abs() < 1e-2 || (body.spin - TAU / 0.1).abs() < 0.5, "{}", body.spin);
        // A fixed-orientation cell keeps the last value.
        let kept = body.spin;
        body.match_shape(&mut f, 1.0, true, 0.1);
        assert_eq!(body.spin, kept);
    }

    /// 0xE78C00: a cell that leaves the screen loses its GFX object and its pins (level 0
    /// here), and gets them again when it returns.
    #[test]
    fn a_cell_gives_its_pins_back_when_it_leaves_the_screen_and_takes_them_when_it_returns() {
        let mut world = World::new();
        let cell = world.spawn_empty().id();
        let mut f = isolated_fluid();
        let mut bodies = SoftBodies::default();
        let at = Vec2::new(5.0, 5.0);
        assert!(bodies.prepare(&mut f, cell, 2, 1.0, at, Vec2::ZERO, 0.0));
        assert_eq!(f.pinned_count(), 16);
        assert!(!bodies.prepare(&mut f, cell, 0, 1.0, at, Vec2::ZERO, 0.0), "off screen: level 0");
        assert_eq!(f.pinned_count(), 0, "every pin went back to the water");
        assert!(bodies.map.is_empty());
        assert!(bodies.prepare(&mut f, cell, 2, 1.0, at, Vec2::new(1.0, 0.0), 0.0));
        assert_eq!(f.pinned_count(), 16);
        assert_eq!(bodies.map[&cell].read_back(&f).1, Vec2::new(1.0, 0.0), "back with the cell's velocity");
    }

    #[test]
    fn regrid_hands_down_and_averages() {
        let mut f = isolated_fluid();
        let mut body = SoftBody::create(&mut f, 1, 0.5, Vec2::new(5.0, 5.0), Vec2::new(1.0, 0.0), 0.0).unwrap();
        // Give the old pins distinct velocities.
        for (k, &i) in body.pins.iter().enumerate() {
            let (p, _) = f.particle(i);
            f.set_particle(i, p, Vec2::new(k as f32, 0.0));
        }
        assert!(body.regrid(&mut f, 2, 1.0, Vec2::new(5.0, 5.0), Vec2::ZERO));
        assert_eq!((body.level, body.pins.len()), (2, 16));
        // The four children of old pin 2 carry its velocity (2, 0).
        for k in 8..12 {
            assert_eq!(f.particle(body.pins[k]).1, Vec2::new(2.0, 0.0));
        }
        // Shrinking averages the groups of four: back to 4 pins at the means.
        assert!(body.regrid(&mut f, 1, 0.5, Vec2::new(5.0, 5.0), Vec2::ZERO));
        assert_eq!(body.pins.len(), 4);
        assert_eq!(f.particle(body.pins[3]).1, Vec2::new(3.0, 0.0));
        // To level 0: all pins are released; from 0 they are created on the shape.
        assert!(body.regrid(&mut f, 0, 0.1, Vec2::new(5.0, 5.0), Vec2::ZERO));
        assert!(body.pins.is_empty());
        assert!(body.regrid(&mut f, 1, 0.5, Vec2::new(5.0, 5.0), Vec2::new(0.5, 0.0)));
        assert_eq!(body.pins.len(), 4);
        let (mean, vel) = body.read_back(&f);
        assert!((mean - Vec2::new(5.0, 5.0)).length() < 1e-4 && (vel - Vec2::new(0.5, 0.0)).length() < 1e-6);
    }

    #[test]
    fn the_pool_running_out_releases_everything() {
        let mut f = isolated_fluid();
        // Fill the pool to 5 free places.
        let mut taken = Vec::new();
        while f.pin(Vec2::new(9.0, 7.0), Vec2::ZERO).map(|i| taken.push(i)).is_some() {
            if taken.len() == crate::cell_fluid::PARTICLES - 1 - 5 {
                break;
            }
        }
        assert!(SoftBody::create(&mut f, 2, 1.0, Vec2::ZERO, Vec2::ZERO, 0.0).is_none(), "16 pins do not fit in 5 places");
        // Nothing was kept: four pins still fit.
        assert!(SoftBody::create(&mut f, 1, 1.0, Vec2::ZERO, Vec2::ZERO, 0.0).is_some());
    }

    /// A swimming cell pushes the water: free particles around a moving soft
    /// body gain velocity along its direction of travel, against the same
    /// water without the cell.
    #[test]
    fn a_moving_cell_pushes_the_water() {
        let run = |with_cell: bool| {
            let mut f = quiet_fluid(11);
            let mut body = if with_cell {
                SoftBody::create(&mut f, 2, 1.0, Vec2::new(-3.0, 0.0), Vec2::new(3.0, 0.0), 0.0)
            } else {
                None
            };
            for _ in 0..60 {
                if let Some(b) = body.as_mut() {
                    b.match_shape(&mut f, 1.0, false, STEP);
                    // The cell keeps swimming at 3 units/s (thrust).
                    let (_, v) = b.read_back(&f);
                    b.add_velocity(&mut f, (Vec2::new(3.0, 0.0) - v) * STEP);
                }
                f.step(STEP);
            }
            // Free particles within 2.5 units of the cell's mean position.
            let centre = body.as_ref().map_or(Vec2::new(-3.0, 0.0) + Vec2::new(3.0, 0.0), |b| b.read_back(&f).0);
            let mut sum = Vec2::ZERO;
            let mut n = 0;
            for i in 0..crate::cell_fluid::PARTICLES {
                if f.is_pinned(i) {
                    continue;
                }
                let (p, v) = f.particle(i);
                if p.distance(centre) < 2.5 {
                    sum += v;
                    n += 1;
                }
            }
            (sum / n.max(1) as f32, n, body.map(|b| b.read_back(&f)))
        };
        let (with, n, cell) = run(true);
        let (without, _, _) = run(false);
        assert!(n > 5, "{n} particles around the cell");
        assert!(with.x > without.x + 0.05, "the wake follows the cell: {with:?} vs {without:?}");
        let (pos, vel) = cell.unwrap();
        assert!(vel.x > 0.5 && vel.x < 3.5, "the cell swims on: {vel:?}");
        assert!(pos.x > -3.0 + 0.5, "and moved: {pos:?}");
    }

    /// The retail rule for a still cell in a current: no `FlowAt` drag, the
    /// water's viscosity drags the pins along, so the cell's velocity grows
    /// from 0 toward the current's.
    #[test]
    fn a_still_cell_in_a_current_drifts_with_the_water() {
        let mut f = quiet_fluid(3);
        let current = Vec2::new(1.0, 0.0);
        // Every free particle moves with the current (and wraps with it).
        for i in 0..crate::cell_fluid::PARTICLES {
            let (p, _) = f.particle(i);
            f.set_particle(i, p, current);
        }
        let mut body = SoftBody::create(&mut f, 1, 0.6, Vec2::new(-4.0, 0.0), Vec2::ZERO, 0.0).unwrap();
        let mut speeds = Vec::new();
        for step in 0..180 {
            body.match_shape(&mut f, 0.6, false, STEP);
            f.step(STEP);
            if step % 60 == 59 {
                speeds.push(body.read_back(&f).1.x);
            }
        }
        assert!(speeds[0] > 0.02, "the water carries the cell: {speeds:?}");
        assert!(speeds[2] > speeds[0], "faster with time: {speeds:?}");
        assert!(speeds[2] < 1.2, "and not beyond the current: {speeds:?}");
    }
}
