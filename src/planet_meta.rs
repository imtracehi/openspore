//! Swarm `metaParticles` as the planet terrain generator plays them
//! (`notes/planet-meta-notes.md`; addresses ModAPI march2017, VERIFIED unless
//! marked). The component (factory 0xAA04F0, instance 0x268 bytes, vtable
//! 0x1458BC4: Start 0xA9B1C0, Stop 0xAA0A50, Update 0xAA1560, SetTransforms
//! 0xA9BAD0, SetFloatParams 0xA9C9A0) emits particles that each carry a
//! child visual effect; the child's brushes stamp along the particle path.
//! This module holds the particle state and the pure maths (surfaces, frames,
//! random walk, the particle transform); `planet_gen` runs the world loop.
use crate::swarm_fx::format::{MetaParticleDef, RandomWalk};
use bevy::math::Vec3;

/// 2*pi: `_DAT_01678FFC` / `DAT_01677174` (static initialisers 0x130F8F0,
/// 0x1309E90). Pitch / roll / yaw curves are in revolutions.
pub const TAU: f32 = std::f32::consts::TAU;
/// Integrator sub-step cap (DAT_01458BB4 = 1/15 s).
pub const MAX_SUBSTEP: f32 = 0.066_666_67;
/// Flags that select the sub-stepped integrator 0xA9D590 (DAT_01679034 =
/// 0x02903000: surface, map collide, random walk, attractor, tractor).
pub const FORCES_MASK: u64 = 0x0290_3000;
/// Surface flags that pin the emission point (DAT_01565430 = 0xE).
pub const PIN_MASK: u32 = 0xE;
/// The built-in sphere surface (registered by 0xA6DC00, vtable 0x14559E8):
/// basis point 0 = centre, point 1 .x = radius.
pub const SURFACE_SPHERE: u32 = 0xCE9F_4D94;

/// `MetaParticleEffect` flag bits (SporeModder-FX FLAGBIT_*; bit 32+ = the
/// first flags word, retail desc+0xC).
pub mod bit {
    pub const INJECT: u32 = 0;
    pub const MAINTAIN: u32 = 1;
    pub const EMIT_BASE: u32 = 3;
    pub const SOURCE_ROUND: u32 = 4;
    pub const SOURCE_SCALE: u32 = 11;
    pub const SURFACE: u32 = 12;
    pub const RANDOM_WALK: u32 = 20;
    pub const RANDOM_WALK_WAIT: u32 = 21;
    pub const RATE_KILL: u32 = 30;
    pub const RATE_HOLD: u32 = 31;
    pub const ALIGN_SURFACE: u32 = 34;
}

/// A local frame: rows are the local axes in world space (Swarm Transform).
pub type Rows = [Vec3; 3];
pub const IDENTITY_ROWS: Rows = [Vec3::X, Vec3::Y, Vec3::Z];

#[inline]
pub fn apply_rows(r: &Rows, v: Vec3) -> Vec3 {
    r[0] * v.x + r[1] * v.y + r[2] * v.z
}

/// The rows of `a` expressed in frame `b` (row-vector product a * b).
#[inline]
pub fn mul_rows(a: &Rows, b: &Rows) -> Rows {
    [apply_rows(b, a[0]), apply_rows(b, a[1]), apply_rows(b, a[2])]
}

/// 0x7D1B90: rotate the rows about local Z (row0' = row0 c + row1 s).
pub fn rot_z(r: &mut Rows, a: f32) {
    let (s, c) = a.sin_cos();
    let (r0, r1) = (r[0], r[1]);
    r[0] = r0 * c + r1 * s;
    r[1] = r1 * c - r0 * s;
}

/// 0x6B9000: about local Y (row2' = row2 c + row0 s).
pub fn rot_y(r: &mut Rows, a: f32) {
    let (s, c) = a.sin_cos();
    let (r2, r0) = (r[2], r[0]);
    r[2] = r2 * c + r0 * s;
    r[0] = r0 * c - r2 * s;
}

/// 0x5A2D70: about local X (row1' = row1 c + row2 s).
pub fn rot_x(r: &mut Rows, a: f32) {
    let (s, c) = a.sin_cos();
    let (r1, r2) = (r[1], r[2]);
    r[1] = r1 * c + r2 * s;
    r[2] = r2 * c - r1 * s;
}

/// 0xA81980: row0 = d.
pub fn align_x(d: Vec3) -> Rows {
    let len = (d.y * d.y + d.x * d.x).sqrt();
    if len <= 0.0 {
        return [d, Vec3::X, Vec3::Y];
    }
    let r1 = Vec3::new(-d.y / len, d.x / len, 0.0);
    [d, r1, d.cross(r1)]
}

/// 0xA81850: row1 = d.
pub fn align_y(d: Vec3) -> Rows {
    let len = (d.x * d.x + d.y * d.y).sqrt();
    if len <= 0.0 {
        return [Vec3::Y, d, Vec3::X];
    }
    let r0 = Vec3::new(d.y / len, -d.x / len, 0.0);
    [r0, d, r0.cross(d)]
}

/// 0xA81AE0: row2 = d.
pub fn align_z(d: Vec3) -> Rows {
    if d.y.abs() < d.x.abs() {
        let inv = 1.0 / (d.z * d.z + d.x * d.x).sqrt();
        let r0 = Vec3::new(d.z * inv, 0.0, -d.x * inv);
        [r0, d.cross(r0), d]
    } else {
        let inv = 1.0 / (d.z * d.z + d.y * d.y).sqrt();
        let r1 = Vec3::new(0.0, d.z * inv, -d.y * inv);
        [r1.cross(d), r1, d]
    }
}

/// Minimal rotation taking unit `from` onto unit `to`, applied to `v`
/// (the quaternion built from the bisector in 0xA7EB00).
pub fn rotate_between(from: Vec3, to: Vec3, v: Vec3) -> Vec3 {
    let axis = from.cross(to);
    let s = axis.length();
    let c = from.dot(to);
    if s < 1e-9 {
        return if c >= 0.0 { v } else { -v };
    }
    let k = axis / s;
    // Rodrigues.
    v * c + k.cross(v) * s + k * k.dot(v) * (1.0 - c)
}

/// Swarm's table sin/cos (0xA78C60): k = round(a * 16 / 2pi), x = a - k pi/8,
/// second-order expansion around the table entry. INFERRED: the table
/// (DAT_01677840, runtime-filled) holds (sin, cos) of k pi / 8.
pub fn table_sincos(a: f32) -> (f32, f32) {
    let k = (a * (16.0 / TAU)).round_ties_even();
    let x = a - k * (std::f32::consts::PI / 8.0);
    let base = (k as i64).rem_euclid(16) as f32 * (std::f32::consts::PI / 8.0);
    let (t0, t1) = base.sin_cos();
    let s = (t1 - t0 * x * 0.5) * x + t0;
    let c = t1 - (t1 * x * 0.5 + t0) * x;
    (s, c)
}

/// The heading approximation of 0xA9DD20 (atan2(y, x) by a cubic), in
/// revolutions.
pub fn approx_heading(x: f32, y: f32) -> f32 {
    let ay = y.abs() + 1e-10;
    let ax = x.abs();
    let f = (ay - ax) / (ax + ay);
    let mut a = (0.9817 - f * f * 0.1963) * f;
    let q = std::f32::consts::FRAC_PI_4;
    a = if x < 0.0 { q * 3.0 - a } else { a + q };
    if y < 0.0 {
        a = -a;
    }
    a / TAU
}

/// Swarm curve lookup ((n - 1) * t, truncated index, linear blend).
pub fn curve(values: &[f32], t: f32) -> f32 {
    match values.len() {
        0 => 1.0,
        1 => values[0],
        n => {
            let x = (n - 1) as f32 * t.clamp(0.0, 1.0);
            let i = (x as usize).min(n - 1);
            let f = x - i as f32;
            if f <= 0.0 || i + 1 >= n {
                values[i]
            } else {
                values[i] + (values[i + 1] - values[i]) * f
            }
        }
    }
}

/// The surfaces a meta component uses. Only the built-in sphere is
/// emulated; other surface maps (terrain height surfaces registered by
/// 0xF9F9B0 for live planets) are not used by the installed terrain scripts.
#[derive(Clone, Copy, Debug)]
pub struct Sphere {
    pub flags: u32,
    pub centre: Vec3,
    pub radius: f32,
    pub pin_offset: f32,
}

impl Sphere {
    /// 0xA7DEE0: the state transform is identity unless the surface is in
    /// source space (0x40) or world space (0x80, = the rigid transform, which
    /// is identity for terrain effects). ADAPTED: source-space spheres (not
    /// used by the installed scripts) are placed through the source frame.
    pub fn from_def(s: &crate::swarm_fx::format::Surface, source: &crate::planet_gen::Frame) -> Option<Self> {
        if s.map.instance != SURFACE_SPHERE {
            return None;
        }
        let p = |i: usize| s.points.get(i).copied().unwrap_or([0.0; 3]);
        let mut centre = Vec3::from_array(p(0));
        let mut radius = p(1)[0];
        if s.flags & 0x40 != 0 {
            centre = source.point(centre);
            radius *= source.scale;
        }
        Some(Self {
            flags: s.flags & 0x3FFF,
            centre,
            radius,
            pin_offset: s.pin_offset,
        })
    }

    /// FindClosestSurfacePoint 0xA7E310: radial projection (z biased by 1e-6).
    pub fn closest(&self, p: Vec3) -> Option<(Vec3, Vec3)> {
        let mut d = p - self.centre;
        d.z += 1e-6;
        let len = d.length();
        if len < 1e-6 {
            return None;
        }
        let n = d / len;
        Some((self.centre + n * self.radius, n))
    }
}

/// Random walk state (6 floats per walk at particle+0x4C / +0x64).
#[derive(Clone, Copy, Debug, Default)]
pub struct Walk {
    pub timer: f32,
    /// Vertical strength ([1]).
    pub vert: f32,
    /// Horizontal strength ([2]).
    pub horiz: f32,
    pub a3: f32,
    pub a4: f32,
    pub curve_index: usize,
}

/// Random-number source used by the meta code (the global Math RNG).
pub trait Rand {
    fn float(&mut self) -> f32;
    /// 0x7CDC40 / 0x7CDC90 per lane: lo + r (hi - lo), r >= hi -> hi, < lo -> lo.
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        let r = self.float() * (hi - lo) + lo;
        if r < hi {
            if r < lo {
                lo
            } else {
                r
            }
        } else {
            hi
        }
    }
    /// 0xA78F00: uniform in [-v, v].
    fn sym(&mut self, v: f32) -> f32 {
        let lo = -v;
        self.float() * (v - lo) + lo
    }
}

impl Walk {
    /// 0xA79020: next walk target.
    pub fn retarget(&mut self, def: &RandomWalk, rng: &mut impl Rand) {
        self.timer += rng.range(def.time[0], def.time[1]);
        let s = rng.range(def.strength[0], def.strength[1]);
        if def.mix >= 0.5 {
            self.horiz = (1.0 - def.mix) * s * 2.0;
            self.vert = s;
        } else {
            self.horiz = s;
            self.vert = def.mix * s * 2.0;
        }
        if def.mix < 1.0 {
            let r = rng.sym(def.turn_range + 1e-5);
            self.a3 += TAU * r;
            self.a3 += if r >= 0.0 { def.turn_offset * TAU } else { -def.turn_offset * TAU };
        }
        if def.mix > 0.0 {
            let r = rng.sym(def.turn_range + 1e-5);
            self.a4 += TAU * r;
            self.a4 += if r >= 0.0 { def.turn_offset * TAU } else { -def.turn_offset * TAU };
        }
        let n = def.turn_offset_curve.len();
        if n > 0 && self.curve_index < n {
            self.a3 += def.turn_offset_curve[self.curve_index] * TAU;
            if self.a3 > TAU {
                self.a3 -= TAU;
            }
            if self.a3 < -TAU {
                self.a3 += TAU;
            }
            self.curve_index += 1;
            if self.curve_index >= n {
                self.curve_index = match def.loop_type {
                    0 => 0,
                    1 => n - 1,
                    _ => n,
                };
            }
        }
    }

    /// 0xA79280: add the walk force (local frame) to `f`.
    pub fn apply(&self, f: &mut Vec3) {
        if self.vert == 0.0 {
            if self.horiz != 0.0 {
                let (s, c) = table_sincos(self.a3);
                f.x += c * self.horiz;
                f.y += s * self.horiz;
            }
        } else if self.horiz == 0.0 {
            f.z += table_sincos(self.a4).0 * self.vert;
        } else {
            let (s4, c4) = table_sincos(self.a4);
            let (s3, c3) = table_sincos(self.a3);
            f.x += c3 * c4 * self.horiz;
            f.y += s3 * c4 * self.horiz;
            f.z += s4 * self.vert;
        }
    }

    /// 0xA793B0 (per particle at emission): angles from the preferred
    /// direction (0xA78F50), then a first target or a synced start.
    pub fn init(def: &RandomWalk, preferred: Vec3, wait: bool, rng: &mut impl Rand) -> Self {
        let (a3, a4) = if preferred.x * preferred.x + preferred.y * preferred.y >= 1e-6 {
            (preferred.y.atan2(preferred.x), preferred.z.clamp(-1.0, 1.0).asin())
        } else if preferred.z < 0.0 {
            (0.0, -std::f32::consts::FRAC_PI_2)
        } else {
            (0.0, std::f32::consts::FRAC_PI_2)
        };
        let mut w = Walk {
            a3,
            a4,
            ..Default::default()
        };
        if !wait {
            w.timer = 0.0;
            w.retarget(def, rng);
        } else {
            w.timer = rng.range(def.time[0], def.time[1]);
            let s = rng.range(def.strength[0], def.strength[1]);
            if def.mix < 0.5 {
                w.horiz = s;
                w.vert = def.mix * s * 2.0;
            } else {
                w.horiz = (1.0 - def.mix) * s * 2.0;
                w.vert = s;
            }
        }
        w
    }
}

/// One live particle (0xA0-byte record; offsets in the notes).
#[derive(Clone, Debug)]
pub struct Particle {
    pub age: f32,
    pub lifetime: f32,
    pub pos: Vec3,
    pub vel: Vec3,
    pub size_vary: f32,
    pub alpha_vary: f32,
    /// Heading in revolutions (+0x48; -999 for the velocity-aligned modes).
    pub heading: f32,
    pub walks: [Walk; 2],
    /// Child effect (world slot) carried by the particle.
    pub child: Option<usize>,
    /// Surface alignment frame (the +0x80 quaternion as rows).
    pub frame: Rows,
    pub pitch_vary: f32,
    pub roll_vary: f32,
    pub yaw_vary: f32,
}

/// Emission / life state of one metaParticles component.
#[derive(Debug)]
pub struct MetaState {
    pub running: bool,
    pub stopped: bool,
    pub rate_clock: f64,
    pub loop_count: u32,
    pub rate_speed: f64,
    pub emit_acc: f32,
    pub delay: f32,
    pub clock: f64,
    pub first_update: bool,
    /// Scales (params 0..4 and 0x101; all 1 for terrain effects).
    pub alpha_scale: f32,
    pub force: Vec3,
    pub particles: Vec<Particle>,
    /// Children whose particle died while they still ran (+0x250).
    pub orphans: Vec<usize>,
    pub surfaces: Vec<Sphere>,
}

impl MetaState {
    pub fn new(def: &MetaParticleDef) -> Self {
        Self {
            running: false,
            stopped: false,
            rate_clock: 0.0,
            loop_count: 0,
            // 0xAA0AF0: 1 / max(rateLoop, 1e-6).
            rate_speed: 1.0 / f64::from(def.rate_loop.max(1e-6)),
            emit_acc: 0.0,
            delay: 0.0,
            clock: 0.0,
            first_update: false,
            alpha_scale: 1.0,
            force: Vec3::ZERO,
            particles: Vec::new(),
            orphans: Vec::new(),
            surfaces: Vec::new(),
        }
    }
}

/// The child transform of a particle (0xA9CD50): rows from the align mode
/// (heading about Z, or an axis along the velocity), the surface frame, then
/// the roll / pitch / yaw curves; offset = position, scale = `size`.
pub fn particle_rows(def: &MetaParticleDef, p: &Particle, age_frac: f32) -> Rows {
    let align_surface = def.has(bit::ALIGN_SURFACE);
    let mut rows = IDENTITY_ROWS;
    if !(2..=4).contains(&def.align_mode) {
        rot_z(&mut rows, p.heading * TAU);
    } else {
        let len = p.vel.length();
        let mut d = if len <= 0.0001 {
            let mut d = p.vel * 10000.0;
            let i = def.align_mode as usize - 2;
            d[i] += 1.0 - len * 10000.0;
            d
        } else {
            p.vel / len
        };
        if align_surface {
            d = Vec3::new(d.dot(p.frame[0]), d.dot(p.frame[1]), d.dot(p.frame[2]));
        }
        rows = match def.align_mode {
            2 => align_x(d),
            3 => align_y(d),
            _ => align_z(d),
        };
    }
    if align_surface {
        rows = mul_rows(&rows, &p.frame);
    }
    rot_y(&mut rows, (p.roll_vary * curve(&def.roll, age_frac) + def.roll_offset) * TAU);
    rot_x(&mut rows, (p.pitch_vary * curve(&def.pitch, age_frac) + def.pitch_offset) * TAU);
    rot_z(&mut rows, (p.yaw_vary * curve(&def.yaw, age_frac) + def.yaw_offset) * TAU);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sincos_tracks_the_exact_values() {
        for i in -40..40 {
            let a = i as f32 * 0.37;
            let (s, c) = table_sincos(a);
            assert!((s - a.sin()).abs() < 0.01 && (c - a.cos()).abs() < 0.01, "{a}");
        }
        assert!((approx_heading(1.0, 0.0)).abs() < 0.01);
        assert!((approx_heading(0.0, 1.0) - 0.25).abs() < 0.01);
        assert!((approx_heading(-1.0, 0.0) - 0.5).abs() < 0.01);
    }

    #[test]
    fn frames_and_rotations_stay_orthonormal() {
        for d in [Vec3::new(0.3, -0.4, 0.86), Vec3::new(-0.9, 0.1, 0.2), Vec3::Z] {
            let d = d.normalize();
            for (rows, axis) in [(align_x(d), 0), (align_y(d), 1), (align_z(d), 2)] {
                assert!((rows[axis] - d).length() < 1e-5);
                for i in 0..3 {
                    assert!((rows[i].length() - 1.0).abs() < 1e-4, "{rows:?}");
                    assert!(rows[i].dot(rows[(i + 1) % 3]).abs() < 1e-4);
                }
            }
        }
        let mut r = IDENTITY_ROWS;
        rot_z(&mut r, std::f32::consts::FRAC_PI_2);
        assert!((r[0] - Vec3::Y).length() < 1e-6);
        let v = rotate_between(Vec3::X, Vec3::Y, Vec3::X);
        assert!((v - Vec3::Y).length() < 1e-6);
        let s = Sphere {
            flags: 4,
            centre: Vec3::ZERO,
            radius: 500.0,
            pin_offset: 0.0,
        };
        let (p, n) = s.closest(Vec3::new(0.0, 900.0, 0.0)).unwrap();
        assert!((p - Vec3::new(0.0, 500.0, 0.0)).length() < 1e-2 && (n - Vec3::Y).length() < 1e-4);
    }
}
