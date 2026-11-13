//! Retail planet geometry queries (pure Rust, no ECS). Everything here is in
//! planet-local Spore space: the planet centre is the origin, directions are
//! the cube-map axes of `cTerrainMapSet` (Z is Spore's "up" axis but the planet
//! has no preferred up: up = normalize(position)).
//!
//! Original code (march2017 addresses, `notes/planet-re-notes.md` 1B-1D):
//! face/uv mapping `cTerrainMapSet::GetHeight` 0xF92760, bilinear sample with
//! neighbour-face wrap 0xF8D5C0, `IsInWater` 0xB7E530, `ToSurface` 0xB81780,
//! `GetOrientation` 0xB7F3A0 -> 0xF9C610, `GetGravity` 0xB7E5E0, height-range
//! quadtree `cHeightRanges` (0x555 nodes per face) allocated in 0xF929F0.
use bevy::math::{Mat3, Vec3};

/// Height samples per cube-face edge (`planet` prop planetHeightMapRes 0x200).
pub const FACE_RES: usize = 512;
/// Planet radius of every installed terrain script (terrainScriptPlanetInfo.z,
/// cTerrainMapSet default 500).
pub const PLANET_RADIUS: f32 = 500.0;
/// cTerrainMapSet mBeachLevel (normalised height above the water level).
pub const BEACH_LEVEL: f32 = 0.025;
/// `GetGravity` (.rdata 0x156B5D4), along -up.
pub const GRAVITY: f32 = 9.8;
/// `planet_settings~!planet` kMinCliffGradient / kMaxCliffGradient (degrees).
pub const MIN_CLIFF_GRADIENT_DEG: f32 = 32.0;
pub const MAX_CLIFF_GRADIENT_DEG: f32 = 45.0;
/// Height-range quadtree depth: levels 0..=5 (1+4+16+64+256+1024 = 0x555).
pub const RANGE_LEVELS: usize = 6;

/// `cTerrainStateMgr::Initialize` 0xFC0480 with kMinMaxAlt 10, kMidMaxAlt 100,
/// kMaxMaxAlt 400 (`planet_settings~!planet`): 100 at R = 500.
pub fn altitude_range(radius: f32) -> f32 {
    let f = radius * 0.002;
    if f < 1.0 {
        10.0 + (100.0 - 10.0) * f
    } else {
        100.0 + (400.0 - 100.0) * (f - 1.0)
    }
}

/// Direction -> (face, u, v), exactly as 0xF92760 (u from the signed normal
/// component, v from its absolute value).
pub fn face_uv(p: Vec3) -> (usize, f32, f32) {
    let (ax, ay, az) = (p.x.abs(), p.y.abs(), p.z.abs());
    if ax <= az && ay <= az {
        if az == 0.0 {
            return (0, 0.5, 0.5);
        }
        (
            if p.z >= 0.0 { 0 } else { 1 },
            (p.x / p.z + 1.0) * 0.5,
            (p.y / az + 1.0) * 0.5,
        )
    } else if ay < ax {
        (
            if p.x >= 0.0 { 2 } else { 3 },
            (p.y / p.x + 1.0) * 0.5,
            (p.z / ax + 1.0) * 0.5,
        )
    } else {
        (
            if p.y >= 0.0 { 4 } else { 5 },
            (p.z / p.y + 1.0) * 0.5,
            (p.x / ay + 1.0) * 0.5,
        )
    }
}

/// Inverse of [`face_uv`]: the cube point (normal component +-1) of face/uv.
/// Matches the quad builder's axis table .rdata 0x1491054 ({0,1,2},{1,2,0},
/// {2,0,1}: u axis, v axis, normal axis; odd faces negate u and the normal).
pub fn face_point(face: usize, u: f32, v: f32) -> Vec3 {
    let (s, t) = (2.0 * u - 1.0, 2.0 * v - 1.0);
    match face {
        0 => Vec3::new(s, t, 1.0),
        1 => Vec3::new(-s, t, -1.0),
        2 => Vec3::new(1.0, s, t),
        3 => Vec3::new(-1.0, -s, t),
        4 => Vec3::new(t, 1.0, s),
        _ => Vec3::new(t, -1.0, -s),
    }
}

/// Unit direction of a face texel centre.
pub fn texel_dir(face: usize, row: usize, col: usize, res: usize) -> Vec3 {
    face_point(
        face,
        (col as f32 + 0.5) / res as f32,
        (row as f32 + 0.5) / res as f32,
    )
    .normalize()
}

/// The 6 x N x N 16-bit height cube (`cTerrainMap16`); texel index =
/// (face * N + row) * N + col with row from v and col from u (0xF8D5C0).
#[derive(Clone, PartialEq)]
pub struct HeightCube {
    res: usize,
    raw: Vec<u16>,
}

impl std::fmt::Debug for HeightCube {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HeightCube({}x{}x6)", self.res, self.res)
    }
}

/// Raw texel value -> normalised height h = (raw - 32768) / 32768, [-1, 1).
#[inline]
pub fn raw_to_h(raw: u16) -> f32 {
    (raw as f32 - 32768.0) * (1.0 / 32768.0)
}
#[inline]
pub fn h_to_raw(h: f32) -> u16 {
    (h * 32768.0 + 32768.0).round().clamp(0.0, 65535.0) as u16
}

impl HeightCube {
    pub fn filled(res: usize, h: f32) -> Self {
        Self {
            res,
            raw: vec![h_to_raw(h); 6 * res * res],
        }
    }
    pub fn from_raw(res: usize, raw: Vec<u16>) -> Option<Self> {
        (raw.len() == 6 * res * res && res >= 2).then_some(Self { res, raw })
    }
    pub fn res(&self) -> usize {
        self.res
    }
    pub fn raw(&self) -> &[u16] {
        &self.raw
    }
    pub fn raw_mut(&mut self) -> &mut [u16] {
        &mut self.raw
    }
    #[inline]
    pub fn index(&self, face: usize, row: usize, col: usize) -> usize {
        (face * self.res + row) * self.res + col
    }
    #[inline]
    pub fn texel(&self, face: usize, row: usize, col: usize) -> f32 {
        raw_to_h(self.raw[self.index(face, row, col)])
    }

    /// Texel lookup that remaps one-axis overruns onto the neighbouring face
    /// (0x684C60). `None` for a texel off the face on both axes (the corner
    /// case 0xF8D5C0 fills with the mean of the other three).
    fn wrapped(&self, face: usize, row: i64, col: i64) -> Option<f32> {
        let n = self.res as i64;
        let row_off = row < 0 || row >= n;
        let col_off = col < 0 || col >= n;
        if !row_off && !col_off {
            return Some(self.texel(face, row as usize, col as usize));
        }
        if row_off && col_off {
            return None;
        }
        let p = face_point(
            face,
            (col as f32 + 0.5) / n as f32,
            (row as f32 + 0.5) / n as f32,
        );
        let (f2, u2, v2) = face_uv(p);
        let c2 = ((u2 * n as f32).floor() as i64).clamp(0, n - 1) as usize;
        let r2 = ((v2 * n as f32).floor() as i64).clamp(0, n - 1) as usize;
        Some(self.texel(f2, r2, c2))
    }

    /// Bilinear sample at (u * N - 0.5, v * N - 0.5), as 0xF8D5C0.
    pub fn sample(&self, face: usize, u: f32, v: f32) -> f32 {
        let n = self.res as f32;
        let x = u * n - 0.5;
        let y = v * n - 0.5;
        let (c0, r0) = (x.floor(), y.floor());
        let (fu, fv) = (x - c0, y - r0);
        let (c0, r0) = (c0 as i64, r0 as i64);
        let n = self.res as i64;
        let inside = c0 >= 0 && r0 >= 0 && c0 + 1 < n && r0 + 1 < n;
        let (t00, t01, t10, t11) = if inside {
            let i = self.index(face, r0 as usize, c0 as usize);
            let s = self.res;
            (
                raw_to_h(self.raw[i]),
                raw_to_h(self.raw[i + 1]),
                raw_to_h(self.raw[i + s]),
                raw_to_h(self.raw[i + s + 1]),
            )
        } else {
            let t = [
                self.wrapped(face, r0, c0),
                self.wrapped(face, r0, c0 + 1),
                self.wrapped(face, r0 + 1, c0),
                self.wrapped(face, r0 + 1, c0 + 1),
            ];
            let known: Vec<f32> = t.iter().flatten().copied().collect();
            let mean = if known.is_empty() {
                0.0
            } else {
                known.iter().sum::<f32>() / known.len() as f32
            };
            let g = |o: Option<f32>| o.unwrap_or(mean);
            (g(t[0]), g(t[1]), g(t[2]), g(t[3]))
        };
        // ((1-fu) t10 + fu t11) fv + (fu t01 + (1-fu) t00)(1 - fv)
        ((1.0 - fu) * t10 + fu * t11) * fv + (fu * t01 + (1.0 - fu) * t00) * (1.0 - fv)
    }

    pub fn sample_dir(&self, dir: Vec3) -> f32 {
        let (f, u, v) = face_uv(dir);
        self.sample(f, u, v)
    }
}

/// Per-face min/max quadtree over normalised heights (cHeightRanges layout:
/// level L has 2^L x 2^L nodes; each node also covers a one-texel apron so
/// bilinear values inside the node stay within its range).
#[derive(Clone, Debug, Default)]
pub struct HeightRanges {
    /// levels[L][face][y * 2^L + x] = (min, max)
    levels: Vec<Vec<Vec<(f32, f32)>>>,
}

impl HeightRanges {
    pub fn build(cube: &HeightCube) -> Self {
        let res = cube.res();
        let leaf_count = 1usize << (RANGE_LEVELS - 1);
        let block = (res / leaf_count).max(1);
        let mut levels = vec![Vec::new(); RANGE_LEVELS];
        let mut leaves = vec![vec![(f32::MAX, f32::MIN); leaf_count * leaf_count]; 6];
        for (face, out) in leaves.iter_mut().enumerate() {
            for by in 0..leaf_count {
                for bx in 0..leaf_count {
                    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                    let r0 = (by * block) as i64 - 1;
                    let c0 = (bx * block) as i64 - 1;
                    for r in r0..=r0 + block as i64 + 1 {
                        for c in c0..=c0 + block as i64 + 1 {
                            if let Some(h) = cube.wrapped(face, r, c) {
                                lo = lo.min(h);
                                hi = hi.max(h);
                            }
                        }
                    }
                    out[by * leaf_count + bx] = (lo, hi);
                }
            }
        }
        levels[RANGE_LEVELS - 1] = leaves;
        for level in (0..RANGE_LEVELS - 1).rev() {
            let n = 1usize << level;
            let child = &levels[level + 1];
            let mut faces = vec![vec![(0.0, 0.0); n * n]; 6];
            for (face, out) in faces.iter_mut().enumerate() {
                for y in 0..n {
                    for x in 0..n {
                        let mut range = (f32::MAX, f32::MIN);
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let c = child[face][(y * 2 + dy) * n * 2 + x * 2 + dx];
                            range = (range.0.min(c.0), range.1.max(c.1));
                        }
                        out[y * n + x] = range;
                    }
                }
            }
            levels[level] = faces;
        }
        Self { levels }
    }

    /// (min, max) normalised height of the node at `level` containing (u, v).
    pub fn node(&self, level: usize, face: usize, u: f32, v: f32) -> (f32, f32) {
        let n = 1usize << level;
        let x = ((u * n as f32) as usize).min(n - 1);
        let y = ((v * n as f32) as usize).min(n - 1);
        self.levels[level][face][y * n + x]
    }
    /// Node range by integer coordinates.
    pub fn node_at(&self, level: usize, face: usize, x: usize, y: usize) -> (f32, f32) {
        let n = 1usize << level;
        self.levels[level][face][y * n + x]
    }
    pub fn global(&self) -> (f32, f32) {
        self.levels[0]
            .iter()
            .fold((f32::MAX, f32::MIN), |r, f| (r.0.min(f[0].0), r.1.max(f[0].1)))
    }
}

/// One planet: height cube plus the cTerrainMapSet scalars.
#[derive(Clone, Debug)]
pub struct Planet {
    pub cube: HeightCube,
    pub radius: f32,
    pub altitude_range: f32,
    /// mWaterLevel on the h scale (2 * WTA.x - 1; 0 for every installed planet).
    pub water_level: f32,
    pub ranges: HeightRanges,
}

impl Planet {
    pub fn new(cube: HeightCube, radius: f32, water_level: f32) -> Self {
        let ranges = HeightRanges::build(&cube);
        Self {
            cube,
            radius,
            altitude_range: altitude_range(radius),
            water_level,
            ranges,
        }
    }
    /// A featureless ocean-level ball (tests, headless fallbacks).
    pub fn flat(h: f32) -> Self {
        Self::new(HeightCube::filled(8, h), PLANET_RADIUS, 0.0)
    }

    /// Normalised height at a direction (bilinear, exact retail sample).
    pub fn h_at(&self, p: Vec3) -> f32 {
        self.cube.sample_dir(p)
    }
    /// `GetHeight`: surface radius R + altRange * h along `p`.
    pub fn height(&self, p: Vec3) -> f32 {
        self.radius + self.altitude_range * self.h_at(p)
    }
    pub fn water_radius(&self) -> f32 {
        self.radius + self.altitude_range * self.water_level
    }
    pub fn is_in_water(&self, p: Vec3) -> bool {
        self.height(p) < self.water_radius()
    }
    /// `ToSurface`: onto the ground, or onto the water surface over the sea.
    pub fn to_surface(&self, p: Vec3) -> Vec3 {
        let r = self.height(p).max(self.water_radius());
        p * (1.0 / (p.length_squared() + 1e-8).sqrt()) * r
    }
    /// Ground (never water) surface point along `p`.
    pub fn ground_point(&self, p: Vec3) -> Vec3 {
        p.normalize_or(Vec3::Z) * self.height(p)
    }
    pub fn up(&self, p: Vec3) -> Vec3 {
        p.normalize_or(Vec3::Z)
    }
    /// `GetGravity` outside the editors: -9.8 along up.
    pub fn gravity(&self, p: Vec3) -> Vec3 {
        -self.up(p) * GRAVITY
    }
    /// Retail gameplay distance: straight-line (chord) distance.
    pub fn distance(&self, a: Vec3, b: Vec3) -> f32 {
        a.distance(b)
    }
    /// Analytic ground normal from central differences of the height field
    /// (about one texel apart).
    pub fn ground_normal(&self, p: Vec3) -> Vec3 {
        let up = self.up(p);
        let (t, b) = tangent_basis(up);
        let step = self.radius * std::f32::consts::FRAC_PI_2 / self.cube.res() as f32;
        let at = |o: Vec3| self.height(up * self.radius + o);
        let dx = (at(t * step) - at(-t * step)) / (2.0 * step);
        let dy = (at(b * step) - at(-b * step)) / (2.0 * step);
        (up - t * dx - b * dy).normalize()
    }

    /// `GetOrientation(pos, dir)`: rows X = normalize(dir x up),
    /// Y = normalize(up x X), Z = up; objects face +Y with +Z up. Returned as
    /// a matrix whose columns are X, Y, Z (local -> planet). Identity for a
    /// degenerate position or direction (|v|^2 < 2^-16), as 0xF9C610.
    pub fn orientation(&self, pos: Vec3, dir: Vec3) -> Mat3 {
        const EPS: f32 = 1.525_878_9e-5;
        if pos.length_squared() < EPS || dir.length_squared() < EPS {
            return Mat3::IDENTITY;
        }
        let up = pos.normalize();
        let mut x = dir.cross(up);
        if x.length() < EPS {
            x = up.any_orthonormal_vector();
        } else {
            x = x.normalize();
        }
        let y = up.cross(x);
        if y.length() < EPS {
            return Mat3::IDENTITY;
        }
        Mat3::from_cols(x, y.normalize(), up)
    }

    /// Ray cast against the height field (water ignored). Returns the hit
    /// distance along `dir` (normalised internally) up to `max_t`. Steps are
    /// skipped with the min/max quadtree; the crossing is refined by
    /// bisection on the exact bilinear surface.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_t: f32) -> Option<f32> {
        let d = dir.normalize_or_zero();
        if d == Vec3::ZERO {
            return None;
        }
        let (_, gmax) = self.ranges.global();
        let r_outer = self.radius + self.altitude_range * gmax + 0.01;
        let (t0, t1) = sphere_span(origin, d, r_outer)?;
        let mut t = t0.max(0.0);
        let end = t1.min(max_t);
        let above = |t: f32| {
            let q = origin + d * t;
            q.length() - self.height(q)
        };
        if above(t) <= 0.0 {
            return Some(t);
        }
        let min_step = 0.2_f32;
        while t < end {
            let q = origin + d * t;
            let r = q.length();
            let (face, u, v) = face_uv(q);
            let mut step = min_step;
            for level in 0..RANGE_LEVELS {
                let n = (1usize << level) as f32;
                let (_, hmax) = self.ranges.node(level, face, u, v);
                let rmax = self.radius + self.altitude_range * hmax;
                if r <= rmax + 1e-3 {
                    continue;
                }
                // Stay inside the node footprint: distance to its uv edge,
                // converted to an angle (d(theta)/du >= 1 on a cube face)
                // with a safety factor, then to arc length above rmax.
                let (fx, fy) = ((u * n).fract(), (v * n).fract());
                let edge_uv = fx.min(1.0 - fx).min(fy).min(1.0 - fy) / n;
                let footprint = edge_uv * 0.5 * rmax;
                // ... and above the node's maximum radius.
                let descend = sphere_span(q, d, rmax).map_or(f32::MAX, |(a, _)| a.max(0.0));
                let candidate = footprint.min(descend);
                if candidate > step {
                    step = candidate;
                }
            }
            let next = (t + step).min(end);
            if above(next) <= 0.0 {
                let (mut lo, mut hi) = (t, next);
                for _ in 0..24 {
                    let mid = 0.5 * (lo + hi);
                    if above(mid) <= 0.0 {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(hi);
            }
            if next >= end {
                break;
            }
            t = next;
        }
        None
    }
}

/// Where the planet sits in a Bevy stage (planet-re 4C): the centre at
/// (0, -R, 0) and the planet rotated so the stage origin direction is world
/// +Y. Spore planet-local space -> Bevy world is a proper rotation (both are
/// right-handed), so terrain is never mirrored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanetFrame {
    pub centre: Vec3,
    /// planet-local -> world rotation.
    pub rotation: bevy::math::Quat,
}

impl PlanetFrame {
    pub fn new(radius: f32, origin_dir_local: Vec3) -> Self {
        Self {
            centre: Vec3::new(0.0, -radius, 0.0),
            rotation: bevy::math::Quat::from_rotation_arc(
                origin_dir_local.normalize_or(Vec3::Y),
                Vec3::Y,
            ),
        }
    }
    pub fn to_world(&self, local: Vec3) -> Vec3 {
        self.centre + self.rotation * local
    }
    pub fn to_local(&self, world: Vec3) -> Vec3 {
        self.rotation.inverse() * (world - self.centre)
    }
    pub fn dir_to_world(&self, local: Vec3) -> Vec3 {
        self.rotation * local
    }
    pub fn dir_to_local(&self, world: Vec3) -> Vec3 {
        self.rotation.inverse() * world
    }
    /// Local up at a world position.
    pub fn up(&self, world: Vec3) -> Vec3 {
        (world - self.centre).normalize_or(Vec3::Y)
    }
}

/// Facade height queries since start (diagnostics: `OPENSPORE_PLANET_TRACE`).
pub static FACADE_QUERIES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A planet placed in a stage.
#[derive(Clone, Debug)]
pub struct StagePlanet {
    pub planet: Planet,
    pub frame: PlanetFrame,
}

impl StagePlanet {
    /// World y of the `to_surface` surface (ground, or water over the sea)
    /// on the vertical line through world (x, z): the flat-world facade.
    pub fn surface_y(&self, x: f32, z: f32) -> f32 {
        self.vertical_y(x, z, true)
    }
    /// World y of the ground (sea floor under water) on the vertical line.
    pub fn ground_y(&self, x: f32, z: f32) -> f32 {
        self.vertical_y(x, z, false)
    }
    fn radius_at(&self, world_dir: Vec3, water: bool) -> f32 {
        let local = self.frame.dir_to_local(world_dir);
        let h = self.planet.height(local);
        if water {
            h.max(self.planet.water_radius())
        } else {
            h
        }
    }
    fn vertical_y(&self, x: f32, z: f32, water: bool) -> f32 {
        FACADE_QUERIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let c = self.frame.centre;
        let (dx, dz) = (x - c.x, z - c.z);
        let d2 = dx * dx + dz * dz;
        let mut r = self.radius_at(Vec3::new(dx, (self.planet.radius.powi(2) - d2).max(0.0).sqrt(), dz), water);
        for _ in 0..3 {
            let dy = (r * r - d2).max(0.0).sqrt();
            r = self.radius_at(Vec3::new(dx, dy, dz), water);
        }
        c.y + (r * r - d2).max(0.0).sqrt()
    }
    /// `to_surface` of a world position (world space).
    pub fn to_surface_world(&self, world: Vec3) -> Vec3 {
        self.frame
            .to_world(self.planet.to_surface(self.frame.to_local(world)))
    }
    pub fn is_in_water_world(&self, world: Vec3) -> bool {
        self.planet.is_in_water(self.frame.to_local(world))
    }
    /// World ground normal (analytic, from the height field).
    pub fn ground_normal_world(&self, world: Vec3) -> Vec3 {
        self.frame
            .dir_to_world(self.planet.ground_normal(self.frame.to_local(world)))
    }
}

/// ADAPTED stage origin (retail placement of the player nest is not traced,
/// planet-re 4F-3; planet-creature 4E step 4): the Creature stage starts at a
/// land point near the sea (the creature walks out of the water, and herds
/// drink at the shore), a few units above the sea on gentle ground with land
/// all around the nest out to 40 units (the Tribal and Civ stages, which
/// share the origin, still lay out their sites at island scale), the nearest
/// water about 55 units away, and as much land as possible within the herds'
/// OriginDistance bands (35-350 units) so the retail population fits on it.
/// Scores a deterministic Fibonacci set of directions; the seed only rotates
/// the set. Without any candidate passing the nest-pad tests the landiest
/// spot wins.
pub fn choose_stage_origin(planet: &Planet, seed: u64) -> Vec3 {
    const CANDIDATES: usize = 3000;
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    let spin = (seed % 997) as f32 * 0.618;
    let hw = planet.water_level;
    let mut best = (f32::MIN, Vec3::Y);
    let mut fallback = (f32::MIN, Vec3::Y);
    for i in 0..CANDIDATES {
        let y = 1.0 - (i as f32 + 0.5) / CANDIDATES as f32 * 2.0;
        let rr = (1.0 - y * y).max(0.0).sqrt();
        let a = i as f32 * golden + spin;
        let dir = Vec3::new(a.cos() * rr, y, a.sin() * rr);
        let h0 = planet.h_at(dir) - hw;
        if h0 <= 0.0 {
            continue;
        }
        let (t, b) = tangent_basis(dir);
        let at = |dist: f32, ang: f32| {
            let theta = dist / planet.radius;
            let tangent = t * ang.cos() + b * ang.sin();
            planet.h_at(dir * theta.cos() + tangent * theta.sin()) - hw
        };
        // The nest pad: land all around out to 40 units, gentle ground.
        let mut pad = 0.0;
        let mut rough = 0.0f32;
        for ring in [8.0, 16.0, 24.0, 32.0, 40.0] {
            for k in 0..12 {
                let h = at(ring, k as f32 / 12.0 * std::f32::consts::TAU);
                if h > 0.004 {
                    pad += 1.0;
                }
                rough = rough.max((h - h0).abs() * planet.altitude_range);
            }
        }
        let pad = pad / 60.0;
        if pad > fallback.0 {
            fallback = (pad, dir);
        }
        if !(0.02..=0.15).contains(&h0) || pad < 0.95 {
            continue;
        }
        // The nearest water along sixteen bearings (45-120 units).
        let mut nearest = f32::MAX;
        for k in 0..16 {
            let ang = k as f32 / 16.0 * std::f32::consts::TAU;
            let mut d = 45.0;
            while d <= 120.0 && d < nearest {
                if at(d, ang) < 0.0 {
                    nearest = d;
                    break;
                }
                d += 5.0;
            }
        }
        // Land in the herd bands.
        let mut band = 0.0;
        for ring in [50.0, 100.0, 150.0, 200.0, 250.0, 300.0, 350.0] {
            for k in 0..16 {
                if at(ring, (k as f32 + 0.5) / 16.0 * std::f32::consts::TAU) > 0.004 {
                    band += 1.0;
                }
            }
        }
        let band = band / 112.0;
        let shore = if nearest == f32::MAX { 2.0 } else { (nearest - 55.0).abs() / 40.0 };
        let score = band * 4.0 - shore - rough * 0.12 - h0 * 4.0;
        if score > best.0 {
            best = (score, dir);
        }
    }
    if best.0 == f32::MIN {
        fallback.1
    } else {
        best.1
    }
}

/// Entry/exit distances of a ray with a centred sphere (exit > 0 required).
fn sphere_span(o: Vec3, d: Vec3, r: f32) -> Option<(f32, f32)> {
    let b = o.dot(d);
    let c = o.length_squared() - r * r;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let (t0, t1) = (-b - s, -b + s);
    (t1 > 0.0).then_some((t0, t1))
}

/// Any right-handed tangent basis (t, b) with t x b = up.
pub fn tangent_basis(up: Vec3) -> (Vec3, Vec3) {
    let t = up.any_orthonormal_vector();
    let b = up.cross(t);
    (t, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_cube(res: usize) -> HeightCube {
        // h varies with the texel direction so wrap and bilinear are testable.
        let mut cube = HeightCube::filled(res, 0.0);
        for face in 0..6 {
            for row in 0..res {
                for col in 0..res {
                    let d = texel_dir(face, row, col, res);
                    let i = cube.index(face, row, col);
                    cube.raw_mut()[i] = h_to_raw(d.x * 0.5 + d.y * 0.25 - d.z * 0.125);
                }
            }
        }
        cube
    }

    #[test]
    fn altitude_range_matches_planet_settings() {
        assert!((altitude_range(500.0) - 100.0).abs() < 1e-4);
        assert!((altitude_range(250.0) - 55.0).abs() < 1e-4);
        assert!((altitude_range(1000.0) - 400.0).abs() < 1e-3);
    }

    #[test]
    fn face_mapping_round_trips_everywhere() {
        for face in 0..6 {
            for &(u, v) in &[(0.5, 0.5), (0.01, 0.99), (0.99, 0.02), (0.3, 0.7), (0.75, 0.25)] {
                let p = face_point(face, u, v) * 3.7;
                let (f2, u2, v2) = face_uv(p);
                assert_eq!(f2, face, "face {face} uv ({u},{v})");
                assert!((u2 - u).abs() < 1e-5 && (v2 - v).abs() < 1e-5);
            }
        }
        // Face centres are the six axes in retail order.
        let axes = [Vec3::Z, -Vec3::Z, Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y];
        for (face, axis) in axes.iter().enumerate() {
            assert_eq!(face_uv(*axis).0, face);
            assert!((face_point(face, 0.5, 0.5) - *axis).length() < 1e-6);
        }
    }

    #[test]
    fn face_frames_are_right_handed_like_the_axis_table() {
        // du x dv points outward on even faces and inward on odd faces
        // (odd faces negate u and the normal, keeping the table's winding).
        for face in 0..6 {
            let p = face_point(face, 0.5, 0.5);
            let du = face_point(face, 0.6, 0.5) - p;
            let dv = face_point(face, 0.5, 0.6) - p;
            let outward = du.cross(dv).dot(p);
            assert!(outward.abs() > 1e-6);
            if face % 2 == 0 {
                assert!(outward > 0.0, "face {face}");
            }
        }
    }

    #[test]
    fn sample_matches_hand_bilinear_inside_a_face() {
        let cube = ramp_cube(16);
        let (face, u, v) = (2usize, 0.37f32, 0.61f32);
        let x = u * 16.0 - 0.5;
        let y = v * 16.0 - 0.5;
        let (c, r) = (x.floor() as usize, y.floor() as usize);
        let (fu, fv) = (x.fract(), y.fract());
        let t = |r: usize, c: usize| cube.texel(face, r, c);
        let expect = (t(r, c) * (1.0 - fu) + t(r, c + 1) * fu) * (1.0 - fv)
            + (t(r + 1, c) * (1.0 - fu) + t(r + 1, c + 1) * fu) * fv;
        assert!((cube.sample(face, u, v) - expect).abs() < 1e-6);
        // Texel centres return the texel exactly.
        let centre = cube.sample(face, (5.0 + 0.5) / 16.0, (9.0 + 0.5) / 16.0);
        assert!((centre - t(9, 5)).abs() < 1e-6);
    }

    #[test]
    fn sample_is_continuous_across_face_edges_and_corners() {
        let cube = ramp_cube(64);
        // Walk across every face edge: the sampled value must not jump.
        let mut worst = 0.0f32;
        for i in 0..400 {
            let a = i as f32 / 400.0 * std::f32::consts::TAU;
            for &tilt in &[0.0f32, 0.4, -0.7] {
                let p0 = Vec3::new(a.cos(), tilt, a.sin()).normalize();
                let p1 = Vec3::new((a + 0.002).cos(), tilt, (a + 0.002).sin()).normalize();
                worst = worst.max((cube.sample_dir(p0) - cube.sample_dir(p1)).abs());
            }
        }
        assert!(worst < 0.03, "seam jump {worst}");
        // Exact cube corner: no NaN, close to the analytic value.
        let corner = Vec3::new(1.0, 1.0, 1.0).normalize();
        let h = cube.sample_dir(corner);
        let expect = corner.x * 0.5 + corner.y * 0.25 - corner.z * 0.125;
        assert!(h.is_finite() && (h - expect).abs() < 0.03, "{h} vs {expect}");
    }

    #[test]
    fn heights_follow_the_retail_scale() {
        let cube = HeightCube::filled(8, 0.25);
        let planet = Planet::new(cube, PLANET_RADIUS, 0.0);
        let p = Vec3::new(3.0, -4.0, 12.0);
        assert!((planet.height(p) - 525.0).abs() < 0.01);
        assert!((planet.water_radius() - 500.0).abs() < 1e-4);
        assert!(!planet.is_in_water(p));
        let s = planet.to_surface(p);
        assert!((s.length() - 525.0).abs() < 0.01);
        assert!(s.normalize().dot(p.normalize()) > 0.99999);
        // Over the sea ToSurface stands on the water sphere.
        let sea = Planet::new(HeightCube::filled(8, -0.3), PLANET_RADIUS, 0.0);
        assert!(sea.is_in_water(p));
        assert!((sea.to_surface(p).length() - 500.0).abs() < 0.01);
        assert!((sea.ground_point(p).length() - 470.0).abs() < 0.01);
        assert!((sea.gravity(p).length() - 9.8).abs() < 1e-4);
        assert!(sea.gravity(p).dot(p) < 0.0);
        let a = Vec3::new(0.0, 0.0, 500.0);
        let b = Vec3::new(0.0, 500.0, 0.0);
        assert!((sea.distance(a, b) - 500.0 * 2f32.sqrt()).abs() < 0.01);
    }

    #[test]
    fn orientation_frames_are_orthonormal_with_up_last() {
        let planet = Planet::flat(0.0);
        for (pos, dir) in [
            (Vec3::new(10.0, 490.0, 30.0), Vec3::X),
            (Vec3::new(-300.0, 100.0, 380.0), Vec3::new(0.2, 0.9, -0.1)),
            (Vec3::new(0.0, 0.0, 500.0), Vec3::Z), // dir parallel to up
        ] {
            let m = planet.orientation(pos, dir);
            let (x, y, z) = (m.x_axis, m.y_axis, m.z_axis);
            for v in [x, y, z] {
                assert!((v.length() - 1.0).abs() < 1e-4);
            }
            assert!(x.dot(y).abs() < 1e-4 && y.dot(z).abs() < 1e-4 && x.dot(z).abs() < 1e-4);
            assert!((z - pos.normalize()).length() < 1e-4);
            assert!((x.cross(y) - z).length() < 1e-4, "right-handed");
            // Forward (+Y) is the tangent projection of dir when possible.
            let tangent = dir - z * dir.dot(z);
            if tangent.length() > 1e-3 {
                assert!(y.dot(tangent.normalize()) > 0.999);
            }
        }
        assert_eq!(planet.orientation(Vec3::ZERO, Vec3::X), Mat3::IDENTITY);
    }

    #[test]
    fn height_ranges_bound_every_sample() {
        let cube = ramp_cube(64);
        let ranges = HeightRanges::build(&cube);
        for i in 0..2000 {
            let a = i as f32 * 0.731;
            let p = Vec3::new(a.sin() * 1.3, (a * 0.37).cos(), (a * 1.91).sin() - 0.2);
            let (face, u, v) = face_uv(p);
            let h = cube.sample(face, u, v);
            for level in 0..RANGE_LEVELS {
                let (lo, hi) = ranges.node(level, face, u, v);
                assert!(h >= lo - 1e-5 && h <= hi + 1e-5, "level {level}");
            }
        }
    }

    #[test]
    fn stage_frame_puts_the_origin_on_top_and_the_facade_matches_the_sphere() {
        let planet = Planet::new(ramp_cube(64), PLANET_RADIUS, 0.0);
        let origin = Vec3::new(0.3, -0.2, 0.9).normalize();
        let frame = PlanetFrame::new(planet.radius, origin);
        assert!((frame.dir_to_world(origin) - Vec3::Y).length() < 1e-5);
        assert!((frame.to_local(frame.to_world(Vec3::new(1.0, 2.0, 3.0))) - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-3);
        let stage = StagePlanet { planet, frame };
        // At the origin the facade height is the surface radius - R.
        let y0 = stage.surface_y(0.0, 0.0);
        let expect = stage.planet.height(origin).max(stage.planet.water_radius()) - PLANET_RADIUS;
        assert!((y0 - expect).abs() < 1e-3, "{y0} vs {expect}");
        // Off the origin the facade point lies on the sphere surface.
        for (x, z) in [(20.0, -10.0), (-35.0, 30.0), (5.0, 44.0)] {
            let y = stage.surface_y(x, z);
            let p = Vec3::new(x, y, z);
            let local = stage.frame.to_local(p);
            let r = stage.planet.to_surface(local).length();
            assert!((local.length() - r).abs() < 0.01, "({x},{z}): {} vs {r}", local.length());
            // to_surface of the facade point is itself (idempotent grounding).
            assert!((stage.to_surface_world(p) - p).length() < 0.01);
        }
    }

    #[test]
    fn stage_origin_is_land_with_land_around_it() {
        // An island: land in a cap around +X, sea elsewhere.
        let mut cube = HeightCube::filled(64, -0.3);
        for face in 0..6 {
            for row in 0..64 {
                for col in 0..64 {
                    let d = texel_dir(face, row, col, 64);
                    let i = cube.index(face, row, col);
                    let h = (d.x - 0.8) * 0.6;
                    cube.raw_mut()[i] = h_to_raw(h.max(-0.3));
                }
            }
        }
        let planet = Planet::new(cube, PLANET_RADIUS, 0.0);
        let o = choose_stage_origin(&planet, 7);
        let h = planet.h_at(o);
        assert!(h > 0.0, "origin on land, h {h}");
        assert!(o.x > 0.8, "inside the island {o}");
    }

    #[test]
    fn raycast_hits_the_surface_from_above_and_from_space() {
        let planet = Planet::new(ramp_cube(64), PLANET_RADIUS, 0.0);
        // Straight down from orbit.
        let origin = Vec3::new(0.0, 0.0, 2000.0);
        let t = planet.raycast(origin, -Vec3::Z, 5000.0).unwrap();
        let hit = origin - Vec3::Z * t;
        assert!((hit.length() - planet.height(hit)).abs() < 0.01);
        // Grazing ray near the ground.
        let up = Vec3::new(0.3, 0.8, 0.5).normalize();
        let ground = up * planet.height(up);
        let (tangent, _) = tangent_basis(up);
        let from = ground + up * 3.0 + tangent * 2.0;
        let dir = (ground - from).normalize();
        let t = planet.raycast(from, dir, 100.0).unwrap();
        let hit = from + dir * t;
        assert!((hit.length() - planet.height(hit)).abs() < 0.01);
        // Pointing away misses.
        assert!(planet.raycast(origin, Vec3::Z, 5000.0).is_none());
    }
}
