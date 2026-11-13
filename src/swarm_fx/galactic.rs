//! The galactic sprite programs of the Swarm particle renderer (helper `swarm-galactic`,
//! `notes/swarm-galactic-notes.md`): the vertex stages of the standard shaders
//! `galacticSpinTilt` 0x70000120, `galacticStar` 0x70000121 and `galacticZpoleTilt` 0x70000122
//! (ids of the patched shader pack the game loads: `PatchData.package` renumbers the standard ids
//! from 0x70000087 on; `Spore_Graphics.package` has the same programs as 0x7000011A..C), ported
//! instruction by instruction from the disassembly in `space-shaders/dumps/galactic_sm3.txt`.
//!
//! Materials (VERIFIED: the compiled states of `0x40212001!3` of `PatchData.package` name the
//! program, blend SRCALPHA / ONE, ZWRITE 0, ZENABLE 1, linear filters, default (wrap) addressing):
//! `galacticSpinTiltAdditive_material` 0x188566C8 -> 0x70000120, `galacticZpoleTiltAdditive_material`
//! 0xBF4F930A -> 0x70000122, `galacticStar_material` 0x05BC752F -> 0x70000121. Installed users (census,
//! `scripts/census_galactic.py`): the Space effect directory 0xC1195706 (`sg_galaxy`: the protoplanetary
//! disks `protoplanetary_top_3x3` 0xA4641C96 (Spin) / `protoplanetary_side_3x3` 0x36A1D684 (Zpole) and the
//! black-hole vortex point `blackhole_vortex_top_3x3` 0xB7744FB6 (Spin)) and the editor effect
//! directory 0xBAFC5CA1 (`galacticStar` x3 on `SG_star_16pack`: 0xC1AC036D, 0x34B785C7, 0x4247CD5B).
//!
//! The pixel stage of all three is the same four instructions (`texld` the strip at the two tile
//! coordinates, `lrp` by the frame blend, times the vertex colour): the draw side is
//! `galactic_draw.rs` / `shaders/swarm_galactic.wesl`.
//!
//! Vertex inputs of the Swarm particle (D3D vertex declaration of the dump): v0 = position, v1.w = half
//! the sprite size, v2 = colour (rgb) and alpha, v3 = (corner uv, two per-particle randoms in [0, 1]);
//! shader data: `customParams` 0x206 = c0..c15 (the material's register block), `gameInfo` 0x248 (x = game
//! time), `screenInfo` 0x204, the matrices (modelToClip 6, worldToClip 12, worldToCamera 14, modelToCamera
//! 7, cameraToClip 18). The camera space of these programs is D3D's (x right, y up, z forward).
//!
//! Evidence of the port: `galactic_golden.rs` holds reference outputs of the three programs produced by
//! `scripts/vs3.py` (a vs_3_0 interpreter that runs the disassembly itself) for random cameras, particles
//! and the installed material registers; `tests::match_the_shader_interpreter` compares every output.
//!
//! ADAPTED (marked where used): the identity model matrix (particles are built in world space), the
//! handedness of the camera frame taken from the engine camera (it mirrors the spin direction at most),
//! the clamp of the tilt dot product to [-1, 1] (retail reads a neighbouring constant register when
//! rounding takes it just outside), the two vertex randoms of a simulated particle. (`screenInfo` = (width, height, 1 / width,
//! 1 / height) is VERIFIED, 0x7124C0: `Camera::from_frame`.)

use bevy::math::{Vec2, Vec3, Vec4};

use super::format::{MaterialDef, MaterialValue};

/// `galacticSpinTiltAdditive_material` (the name is not in `reg_file.txt`; the FNV-1 hash matches).
pub const MATERIAL_SPIN_ADDITIVE: u32 = 0x1885_66C8;
/// `galacticZpoleTiltAdditive_material`.
pub const MATERIAL_ZPOLE_ADDITIVE: u32 = 0xBF4F_930A;
/// `galacticStar_material`.
pub const MATERIAL_STAR: u32 = 0x05BC_752F;

/// The three galactic vertex programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Program {
    /// 0x70000120: a plane at a random tilt that spins about its normal.
    SpinTilt,
    /// 0x70000121: a pixel-sized twinkling billboard.
    Star,
    /// 0x70000122: a plane built from a random pole direction and the view direction.
    ZpoleTilt,
}

impl Program {
    /// The program a Swarm effect material (the shader id an effect directory's material names) runs.
    pub fn from_material(shader: u32) -> Option<Program> {
        match shader {
            MATERIAL_SPIN_ADDITIVE => Some(Program::SpinTilt),
            MATERIAL_ZPOLE_ADDITIVE => Some(Program::ZpoleTilt),
            MATERIAL_STAR => Some(Program::Star),
            _ => None,
        }
    }

    /// Standard shader id in the patched pack.
    pub fn shader_id(self) -> u32 {
        match self {
            Program::SpinTilt => 0x7000_0120,
            Program::Star => 0x7000_0121,
            Program::ZpoleTilt => 0x7000_0122,
        }
    }
}

/// The material's `customParams` block c0..c15 (64 floats, register * 4 + lane); unset registers are 0.
#[derive(Clone, Debug, PartialEq)]
pub struct Registers(pub [f32; 64]);

impl Default for Registers {
    fn default() -> Self {
        Registers([0.0; 64])
    }
}

impl Registers {
    /// Property ids 0x10000 + 16 * (register * 4 + lane) of the material (floats and float arrays, which
    /// continue into the following lanes: `c7.y = [3, -0.1, 3]` fills c7.y, c7.z, c7.w).
    pub fn from_material(material: &MaterialDef) -> Registers {
        let mut regs = [0.0f32; 64];
        for p in &material.properties {
            let id = p.name.instance;
            if id < 0x10000 || id & 0xF != 0 {
                continue;
            }
            let lane = ((id - 0x10000) >> 4) as usize;
            match &p.value {
                MaterialValue::Float(v) if lane < 64 => regs[lane] = *v,
                MaterialValue::Floats(values) => {
                    for (i, v) in values.iter().enumerate() {
                        if lane + i < 64 {
                            regs[lane + i] = *v;
                        }
                    }
                }
                _ => {}
            }
        }
        Registers(regs)
    }

    fn c(&self, register: usize, lane: usize) -> f32 {
        self.0[register * 4 + lane]
    }

    /// Tile columns / rows of the strip (c8.xy) and its frame rate (c8.z).
    pub fn tiles(&self) -> (f32, f32, f32) {
        (self.c(8, 0), self.c(8, 1), self.c(8, 2))
    }
}

/// The camera as the programs see it: worldToCamera (3 x 4, x right / y up / z forward), cameraToClip
/// (a D3D projection, 4 x 4), `screenInfo.zw` and the game time. The model matrix is the identity.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub camera_from_world: [Vec4; 3],
    pub clip_from_camera: [Vec4; 4],
    pub pixel_size: Vec2,
    pub time: f32,
}

impl Camera {
    /// From the engine camera's frame vectors (a right-handed engine frame; ADAPTED: the camera space is
    /// (right, up, forward) as D3D's) and a perspective projection with the retail clip planes.
    pub fn from_frame(
        eye: Vec3,
        right: Vec3,
        up: Vec3,
        forward: Vec3,
        fov_y: f32,
        aspect: f32,
        near: f32,
        far: f32,
        viewport: Vec2,
        time: f32,
    ) -> Camera {
        let row = |axis: Vec3| axis.extend(-axis.dot(eye));
        let fy = 1.0 / (fov_y * 0.5).tan();
        let fx = fy / aspect;
        let q = far / (far - near);
        Camera {
            camera_from_world: [row(right), row(up), row(forward)],
            clip_from_camera: [
                Vec4::new(fx, 0.0, 0.0, 0.0),
                Vec4::new(0.0, fy, 0.0, 0.0),
                Vec4::new(0.0, 0.0, q, -near * q),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
            ],
            // VERIFIED (0x7124C0, the renderer's per-frame setup): `SetShaderData(0x204)` takes the four floats
            // (width, height, 1 / width, 1 / height) of `IRenderer::GetScreenInfo()` (each reciprocal 0 for a
            // 0 size); the `galacticStar` stage reads .zw. The size is the render target's: the viewport here.
            pixel_size: Vec2::new(1.0 / viewport.x.max(1.0), 1.0 / viewport.y.max(1.0)),
            time,
        }
    }

    fn to_camera_point(&self, p: Vec3) -> Vec3 {
        let p = p.extend(1.0);
        Vec3::new(self.camera_from_world[0].dot(p), self.camera_from_world[1].dot(p), self.camera_from_world[2].dot(p))
    }

    fn to_camera_vector(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.camera_from_world[0].truncate().dot(v),
            self.camera_from_world[1].truncate().dot(v),
            self.camera_from_world[2].truncate().dot(v),
        )
    }

    /// A camera-space vector in world space (the rotation part of worldToCamera is orthonormal).
    pub fn to_world_vector(&self, v: Vec3) -> Vec3 {
        self.camera_from_world[0].truncate() * v.x
            + self.camera_from_world[1].truncate() * v.y
            + self.camera_from_world[2].truncate() * v.z
    }

    fn clip_point(&self, c: Vec3) -> Vec4 {
        let c = c.extend(1.0);
        Vec4::new(
            self.clip_from_camera[0].dot(c),
            self.clip_from_camera[1].dot(c),
            self.clip_from_camera[2].dot(c),
            self.clip_from_camera[3].dot(c),
        )
    }

    /// dp3 of the cameraToClip rows with a camera-space vector (the linear part).
    fn clip_vector(&self, v: Vec3) -> Vec4 {
        Vec4::new(
            self.clip_from_camera[0].truncate().dot(v),
            self.clip_from_camera[1].truncate().dot(v),
            self.clip_from_camera[2].truncate().dot(v),
            self.clip_from_camera[3].truncate().dot(v),
        )
    }
}

/// One sprite corner's vertex inputs (v0 = `center`, v1.w = `half_size`, v2 = `color`, v3 = `corner`, `rand`).
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub center: Vec3,
    pub half_size: f32,
    /// Colour and alpha (v2).
    pub color: Vec4,
    /// v3.xy: (0, 0) top left, (1, 1) bottom right.
    pub corner: Vec2,
    /// v3.zw, per particle in [0, 1].
    pub rand: Vec2,
}

/// The vertex stage's outputs.
#[derive(Clone, Copy, Debug)]
pub struct Output {
    /// oPos (clip space).
    pub position: Vec4,
    /// oT1.xy and oT1.zw: the two strip tile coordinates the pixel stage blends.
    pub uv_a: Vec2,
    pub uv_b: Vec2,
    /// oT2.x: the blend factor between them.
    pub blend: f32,
    /// oT3: colour (rgb) and alpha (a tilt-faded alpha for the plane programs, which can be negative: the
    /// additive blend then subtracts).
    pub color: Vec4,
    /// The corner's displacement from the particle centre in world space, so a sprite can be built as a
    /// world-space mesh: the plane programs' offset, and for the billboard the camera-plane displacement
    /// that gives the same clip-space offset.
    pub world_offset: Vec3,
}

// --- D3D instruction helpers ------------------------------------------------------------------------

fn frc(x: f32) -> f32 {
    x - x.floor()
}

fn rcp(x: f32) -> f32 {
    1.0 / x
}

/// `rsq` of the reference rasterizer: 1 / sqrt(|x|).
fn rsq(x: f32) -> f32 {
    1.0 / x.abs().sqrt()
}

/// `nrm`: a zero vector stays zero.
fn nrm(v: Vec3) -> Vec3 {
    let l = v.length();
    if l > 0.0 {
        v / l
    } else {
        Vec3::ZERO
    }
}

fn sat(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// `lrp`: s0 * (s1 - s2) + s2.
fn lrp(s0: f32, s1: f32, s2: f32) -> f32 {
    s0 * (s1 - s2) + s2
}

// The `def` constants of the programs (the dump prints six digits).
const TWO_PI: f32 = 6.28319;
const MINUS_PI: f32 = -3.14159;
const INV_TWO_PI: f32 = 0.159155;

/// The animate-tiles code shared by all three programs: two strip cells (frame `t` and `t + 1`, `t` from the
/// particle's random, the game time and the strip's rate) as texture coordinates of a `cols` x `rows` tile
/// atlas (v flipped) and the blend between them.
fn animate_tiles(regs: &Registers, time: f32, corner: Vec2, rand_w: f32) -> (Vec2, Vec2, f32) {
    let (cols, rows, rate) = regs.tiles();
    let n = cols * rows + 1.0;
    let scaled = n * rand_w;
    let start = scaled - frc(scaled);
    let t = rand_w * (rows + cols) + (time * rate + start);
    let blend = frc(t);
    let tf = t - blend;
    let inv_cols = rcp(cols);
    let inv_rows = rcp(rows);
    // The cell of a frame number with the shader's fmod emulation (`sge` picks the sign of the divisor).
    let cell = |frame: f32| -> (f32, f32) {
        let q = frame * inv_cols;
        let s = if frame * cols >= -(frame * cols) { cols } else { -cols };
        let s2 = if q * rows >= -(q * rows) { rows } else { -rows };
        let col_f = s * frc(frame * rcp(s));
        let row_f = s2 * frc(q * rcp(s2));
        (col_f - frc(col_f), row_f - frc(row_f))
    };
    let (c0, r0) = cell(tf);
    let (c1, r1) = cell(tf + 1.0);
    let a = Vec2::new(inv_cols * (c0 + corner.x), inv_rows * (r0 + corner.y));
    let b = Vec2::new(inv_cols * (c1 + corner.x), inv_rows * (r1 + corner.y));
    (Vec2::new(a.x, 1.0 - a.y), Vec2::new(b.x, 1.0 - b.y), blend)
}

/// The 3-entry tilt table c7.yzw at the dot product `d` (-1, 0, 1 -> entry 0, 1, 2): the shader splits `d + 1`
/// into floor / ceil (`mova` rounds to the nearest integer, an index into the identity rows c16..c18) and
/// interpolates with `frc(d)`.
fn tilt_table(regs: &Registers, d: f32) -> f32 {
    // ADAPTED: |d| beyond 1 by rounding reads a neighbouring constant register in retail.
    let d = d.clamp(-1.0, 1.0);
    let a = d + 1.0;
    let fr = frc(d);
    let ceil = a + frc(-a);
    let floor = a - fr;
    let entry = |index: f32| -> f32 {
        match (index + 0.5).floor() as i32 {
            0 => regs.c(7, 1),
            1 => regs.c(7, 2),
            2 => regs.c(7, 3),
            _ => 0.0, // c19 = (0, 0, 0, 1): dp3 of the table with it
        }
    };
    lrp(fr, entry(ceil), entry(floor))
}

/// The distance fade of the plane programs: the clip depth `z` against the four knots c1.xyzw, alpha
/// c2.xyzw, linearly between the two knots around it; 0 outside the knot range (the selector vectors the
/// shader builds with `slt` are all zero there).
fn knot_fade(regs: &Registers, z: f32) -> f32 {
    let knot = |i: usize| regs.c(1, i);
    let alpha = |i: usize| regs.c(2, i);
    let s: [f32; 4] = std::array::from_fn(|i| if knot(i) < z { 1.0 } else { 0.0 });
    let n: [f32; 4] = std::array::from_fn(|i| 1.0 - s[i]);
    // r2 = r0 * r3.yzww (the lower knot), r0' = r0.xxyz * r3 (the upper knot).
    let lower = [s[0] * n[1], s[1] * n[2], s[2] * n[3], s[3] * n[3]];
    let upper = [s[0] * n[0], s[0] * n[1], s[1] * n[2], s[2] * n[3]];
    let pick = |sel: &[f32; 4], f: &dyn Fn(usize) -> f32| (0..4).map(|i| sel[i] * f(i)).sum::<f32>();
    let (k_lo, a_lo) = (pick(&lower, &knot), pick(&lower, &alpha));
    let (k_hi, a_hi) = (pick(&upper, &knot), pick(&upper, &alpha));
    let t = sat((z - k_lo) * rcp(k_hi - k_lo));
    // 0 * inf (z == knot == 0 outside the range) is NaN in IEEE, 0 after the saturate of the shader.
    let t = if t.is_nan() { 0.0 } else { t };
    lrp(t, a_hi, a_lo)
}

/// The tilt-table alpha: `min(|f|, 1) * sign(f)`.
fn tilt_alpha(f: f32) -> f32 {
    let sign = (if f > 0.0 { 1.0 } else { 0.0 }) - (if f < 0.0 { 1.0 } else { 0.0 });
    f.abs().min(1.0) * sign
}

/// The shared tail of `galacticSpinTilt` / `galacticZpoleTilt`: `offset` is the corner's camera-space
/// displacement and `d` the cosine between the view direction to the corner and the sprite plane's normal.
fn tilt_tail(regs: &Registers, cam: &Camera, v: &Vertex, cam_center: Vec3, offset: Vec3, d: f32) -> Output {
    let f = tilt_table(regs, d);
    let tilt = tilt_alpha(f);
    let clip_c = cam.clip_point(cam_center);
    let position = cam.clip_vector(offset) + clip_c;
    let fade = knot_fade(regs, position.z);
    let (uv_a, uv_b, blend) = animate_tiles(regs, cam.time, v.corner, v.rand.y);
    Output {
        position,
        uv_a,
        uv_b,
        blend,
        color: Vec4::new(v.color.x, v.color.y, v.color.z, tilt * (fade * v.color.w)),
        world_offset: cam.to_world_vector(offset),
    }
}

/// `galacticSpinTilt` (0x70000120).
fn spin_tilt(regs: &Registers, cam: &Camera, v: &Vertex) -> Output {
    // The spin angle: a rate of c15.x + 2 (rand.w - 0.5) c15.y turns per second.
    let rate = 2.0 * ((v.rand.y - 0.5) * regs.c(15, 1)) + regs.c(15, 0);
    let turns = rate * cam.time * INV_TWO_PI + 0.5;
    let angle = frc(turns) * TWO_PI + MINUS_PI;
    let (sin, cos) = angle.sin_cos();
    let spin = Vec3::new(cos, -sin, 0.0);
    let spin = spin * rsq(spin.dot(spin));
    let size = v.half_size + v.half_size;
    let corner = Vec2::new(v.corner.x - 0.5, 0.5 - v.corner.y) * size;
    // The random tilt: two sines (rand.zw mapped to [-1, 1], scaled by c15.z).
    let t = Vec2::new(v.rand.x * 2.0 - 1.0, v.rand.y * 2.0 - 1.0);
    let (ta, tb) = (t.y * regs.c(15, 2), t.x * regs.c(15, 2));
    // The corner in the spinning plane.
    let r = Vec2::new(spin.x * corner.x + sin * corner.y, spin.y * corner.x + cos * corner.y);
    let sb = (1.0 - tb * tb).abs().sqrt();
    let sa = (1.0 - ta * ta).abs().sqrt();
    // The tilt matrix applied to (r, 0): the plane's offset in model (= world) space, and its normal.
    let offset = Vec3::new(r.x * sa, r.x * (ta * tb) + r.y * sb, r.x * (ta * sb) - r.y * tb);
    let normal = Vec3::new(-ta, tb * sa, sb * sa);
    let cam_center = cam.to_camera_point(v.center);
    let cam_offset = cam.to_camera_vector(offset);
    let view = nrm(cam_center + cam_offset);
    let n = nrm(cam.to_camera_vector(normal));
    tilt_tail(regs, cam, v, cam_center, cam_offset, view.dot(n))
}

/// `galacticZpoleTilt` (0x70000122).
fn zpole_tilt(regs: &Registers, cam: &Camera, v: &Vertex) -> Output {
    // A random pole direction (unit by construction) in model space, rotated to camera space.
    let (rz, rw) = (v.rand.x * 2.0 - 1.0, v.rand.y * 2.0 - 1.0);
    let p = rw * regs.c(15, 2);
    let q = rz * regs.c(15, 2);
    let sq = (1.0 - q * q).abs().sqrt();
    let sp = (1.0 - p * p).abs().sqrt();
    let pole = Vec3::new(-p, q * sp, sp * sq);
    let pole_cam = nrm(cam.to_camera_vector(pole));
    let cam_center = cam.to_camera_point(v.center);
    let view = nrm(cam_center);
    // The plane holds the pole and the tangent around it as seen from the camera.
    let cross = pole_cam.cross(view);
    let u = cross * rsq(cross.dot(cross));
    let size = v.half_size + v.half_size;
    let (cx, cy) = (size * (v.corner.x - 0.5), size * (0.5 - v.corner.y));
    let offset = u * cx + pole_cam * cy;
    let normal = nrm(u.cross(pole_cam));
    let to_corner = nrm(cam_center + offset);
    tilt_tail(regs, cam, v, cam_center, offset, to_corner.dot(normal))
}

/// `galacticStar` (0x70000121): a pixel-sized sprite whose size and colour fade with the clip depth and whose
/// brightness twinkles.
fn star(regs: &Registers, cam: &Camera, v: &Vertex) -> Output {
    let size = v.half_size + v.half_size;
    let corner = Vec2::new(v.corner.x - 0.5, 0.5 - v.corner.y);
    let cam_center = cam.to_camera_point(v.center);
    let clip = cam.clip_point(cam_center);
    // The size in pixels fades from 0.01 c13.z to 0.01 c13.w between the clip depths c13.x and c13.y.
    let (c13x, c13y) = (regs.c(13, 0), regs.c(13, 1));
    let t = sat((clip.z - c13x) * rcp(c13y - c13x));
    let pixels = lrp(t, 0.01 * regs.c(13, 3), 0.01 * regs.c(13, 2));
    let ndc_offset = corner * size * pixels * cam.pixel_size;
    let position = Vec4::new(ndc_offset.x * clip.w + clip.x, ndc_offset.y * clip.w + clip.y, clip.z, clip.w);
    // The colour fades between c14.z and c14.w over the depths c14.x .. c14.y; the twinkle is |sin| of a
    // phase running at c15.x per second, raised to c15.y and mixed toward 1 by c15.z.
    let (c14x, c14y) = (regs.c(14, 0), regs.c(14, 1));
    let t2 = sat((clip.z - c14x) * rcp(c14y - c14x));
    let color_scale = lrp(t2, regs.c(14, 3), regs.c(14, 2));
    let phase0 = (v.rand.y + v.rand.x) * 10.0;
    let phase = regs.c(15, 0) * cam.time + phase0;
    let angle = frc(phase * INV_TWO_PI + 0.5) * TWO_PI + MINUS_PI;
    let s = angle.sin();
    let twinkle_pow = (s.abs() + 1.0e-8).powf(regs.c(15, 1));
    let twinkle = 1.0 - (1.0 - twinkle_pow) * (1.0 - regs.c(15, 2));
    let (uv_a, uv_b, blend) = animate_tiles(regs, cam.time, v.corner, v.rand.y);
    // The camera-plane displacement that moves the clip position by `ndc_offset`.
    let (fx, fy) = (cam.clip_from_camera[0].x, cam.clip_from_camera[1].y);
    let world_offset =
        cam.to_world_vector(Vec3::new(ndc_offset.x * clip.w / fx, ndc_offset.y * clip.w / fy, 0.0));
    Output { position, uv_a, uv_b, blend, color: v.color * (color_scale * twinkle), world_offset }
}

/// The vertex stage of `program` for one sprite corner.
pub fn vertex(program: Program, regs: &Registers, cam: &Camera, v: &Vertex) -> Output {
    match program {
        Program::SpinTilt => spin_tilt(regs, cam, v),
        Program::Star => star(regs, cam, v),
        Program::ZpoleTilt => zpole_tilt(regs, cam, v),
    }
}

/// The four corners of a sprite in the order (0, 0), (1, 0), (0, 1), (1, 1).
pub fn sprite(program: Program, regs: &Registers, cam: &Camera, center: Vec3, half_size: f32, color: Vec4, rand: Vec2) -> [Output; 4] {
    std::array::from_fn(|i| {
        let corner = Vec2::new((i & 1) as f32, (i >> 1) as f32);
        vertex(program, regs, cam, &Vertex { center, half_size, color, corner, rand })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm_fx::galactic_golden::GOLDEN;

    fn registers(case: &crate::swarm_fx::galactic_golden::Golden) -> Registers {
        let mut r = Registers::default();
        for (i, v) in case.regs {
            r.0[*i] = *v;
        }
        r
    }

    fn rows<const N: usize>(values: &[f32]) -> [Vec4; N] {
        std::array::from_fn(|i| Vec4::new(values[i * 4], values[i * 4 + 1], values[i * 4 + 2], values[i * 4 + 3]))
    }

    fn close(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance * (1.0 + b.abs())
    }

    /// Every output of the three programs equals the interpreter's (the disassembly executed on f32
    /// registers): clip position, both tile coordinates, the blend and the colour.
    #[test]
    fn match_the_shader_interpreter() {
        assert!(GOLDEN.len() >= 50);
        let mut per_program = [0usize; 3];
        for (n, case) in GOLDEN.iter().enumerate() {
            let program = match case.program {
                0 => Program::SpinTilt,
                1 => Program::Star,
                _ => Program::ZpoleTilt,
            };
            per_program[case.program as usize] += 1;
            let cam = Camera {
                camera_from_world: rows::<3>(&case.camera_from_world),
                clip_from_camera: rows::<4>(&case.clip_from_camera),
                pixel_size: Vec2::from_array(case.pixel_size),
                time: case.time,
            };
            let v = Vertex {
                center: Vec3::from_array(case.center),
                half_size: case.half_size,
                color: Vec4::from_array(case.color),
                corner: Vec2::from_array(case.corner),
                rand: Vec2::from_array(case.rand),
            };
            let out = vertex(program, &registers(case), &cam, &v);
            let tol = 2.0e-3;
            let want_pos = Vec4::from_array(case.position);
            let pos_ok = (0..4).all(|i| close(out.position[i], want_pos[i], tol));
            let uv_ok = close(out.uv_a.x, case.uv_a[0], tol)
                && close(out.uv_a.y, case.uv_a[1], tol)
                && close(out.uv_b.x, case.uv_b[0], tol)
                && close(out.uv_b.y, case.uv_b[1], tol)
                && close(out.blend, case.blend, tol);
            let want_color = Vec4::from_array(case.out_color);
            let color_ok = (0..4).all(|i| close(out.color[i], want_color[i], tol));
            assert!(
                pos_ok && uv_ok && color_ok,
                "case {n} ({program:?}): position {:?} want {:?}; uv {:?} {:?} blend {} want {:?} {:?} {}; colour {:?} want {:?}",
                out.position,
                want_pos,
                out.uv_a,
                out.uv_b,
                out.blend,
                case.uv_a,
                case.uv_b,
                case.blend,
                out.color,
                want_color
            );
        }
        assert!(per_program.iter().all(|n| *n >= 12), "{per_program:?}");
    }

    /// The world-space offset reproduces the clip-space offset of the plane programs (the mesh path of the
    /// renderer: the engine projects `center + world_offset`).
    #[test]
    fn world_offset_moves_the_clip_position_like_the_shader() {
        for case in GOLDEN.iter() {
            let program = match case.program {
                0 => Program::SpinTilt,
                1 => Program::Star,
                _ => Program::ZpoleTilt,
            };
            let cam = Camera {
                camera_from_world: rows::<3>(&case.camera_from_world),
                clip_from_camera: rows::<4>(&case.clip_from_camera),
                pixel_size: Vec2::from_array(case.pixel_size),
                time: case.time,
            };
            let v = Vertex {
                center: Vec3::from_array(case.center),
                half_size: case.half_size,
                color: Vec4::from_array(case.color),
                corner: Vec2::from_array(case.corner),
                rand: Vec2::from_array(case.rand),
            };
            let out = vertex(program, &registers(case), &cam, &v);
            let moved = cam.clip_point(cam.to_camera_point(v.center + out.world_offset));
            for i in 0..4 {
                assert!(
                    close(moved[i], out.position[i], 2.0e-3),
                    "{program:?} component {i}: {:?} vs {:?}",
                    moved,
                    out.position
                );
            }
        }
    }

    fn test_camera(time: f32) -> Camera {
        // Looking down -z from (0, 0, -10) in D3D space: right +x, up +y, forward +z.
        Camera::from_frame(Vec3::new(0.0, 0.0, -10.0), Vec3::X, Vec3::Y, Vec3::Z, 1.0, 1.4, 0.05, 10_000.0, Vec2::new(1280.0, 900.0), time)
    }

    fn disk_registers() -> Registers {
        // `protoplanetary_top_3x3` (0xA4641C96): the installed material registers.
        let mut r = Registers::default();
        for (i, v) in [(4, 0.0), (5, 25.0), (6, 40.0), (7, 100.0), (8, 1.0), (9, 1.0), (10, 1.0), (11, 0.0)] {
            r.0[i] = v;
        }
        r.0[29] = 3.0;
        r.0[30] = -0.1;
        r.0[31] = 3.0;
        r.0[32] = 3.0;
        r.0[33] = 3.0;
        r.0[34] = 0.7;
        r.0[60] = -0.1;
        r.0[61] = 0.1;
        r.0[62] = 1.0;
        r
    }

    #[test]
    fn the_distance_fade_follows_the_knots() {
        let r = disk_registers();
        // alpha 1 up to the third knot (40), falling to 0 at 100, 0 beyond and behind the camera.
        assert!((knot_fade(&r, 10.0) - 1.0).abs() < 1e-6);
        assert!((knot_fade(&r, 39.0) - 1.0).abs() < 1e-6);
        assert!((knot_fade(&r, 70.0) - 0.5).abs() < 1e-6);
        assert_eq!(knot_fade(&r, 150.0), 0.0);
        assert_eq!(knot_fade(&r, -3.0), 0.0);
    }

    #[test]
    fn the_tilt_table_is_piecewise_linear_through_its_three_entries() {
        let r = disk_registers();
        // c7.yzw = (3, -0.1, 3) at d = -1, 0, 1.
        assert!((tilt_table(&r, -1.0) - 3.0).abs() < 1e-5);
        assert!((tilt_table(&r, 0.0) + 0.1).abs() < 1e-5);
        assert!((tilt_table(&r, 1.0) - 3.0).abs() < 1e-5);
        assert!((tilt_table(&r, 0.5) - 1.45).abs() < 1e-5);
        assert!((tilt_table(&r, -0.5) - 1.45).abs() < 1e-5);
        // The alpha is the table clamped to [-1, 1] (a face-on disk is opaque, an edge-on one slightly negative).
        assert_eq!(tilt_alpha(3.0), 1.0);
        assert!((tilt_alpha(-0.1) + 0.1).abs() < 1e-6);
        assert_eq!(tilt_alpha(0.0), 0.0);
    }

    /// A tilted plane sprite keeps its size (the offset basis is orthonormal): the corner offsets of one
    /// particle span the authored size, whatever the tilt and spin.
    #[test]
    fn spin_tilt_offsets_keep_the_authored_size() {
        let r = disk_registers();
        for (time, rand) in [(0.0, Vec2::new(0.2, 0.7)), (13.0, Vec2::new(0.9, 0.1)), (101.5, Vec2::new(0.5, 0.5))] {
            let cam = test_camera(time);
            let s = sprite(Program::SpinTilt, &r, &cam, Vec3::new(1.0, 2.0, 3.0), 0.5, Vec4::ONE, rand);
            let width = (s[1].world_offset - s[0].world_offset).length();
            let height = (s[2].world_offset - s[0].world_offset).length();
            assert!((width - 1.0).abs() < 1e-3, "width {width}");
            assert!((height - 1.0).abs() < 1e-3, "height {height}");
            assert!(s[0].world_offset.dot(s[3].world_offset) < 0.0, "opposite corners");
        }
    }

    #[test]
    fn tile_coordinates_stay_inside_the_atlas_and_the_blend_is_a_fraction() {
        let r = disk_registers();
        for i in 0..200 {
            let time = i as f32 * 0.37;
            let rand = Vec2::new((i as f32 * 0.123).fract(), (i as f32 * 0.377).fract());
            for corner in [Vec2::ZERO, Vec2::ONE] {
                let (a, b, blend) = animate_tiles(&r, time, corner, rand.y);
                for uv in [a, b] {
                    assert!((0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y), "{uv:?}");
                }
                assert!((0.0..1.0).contains(&blend));
            }
        }
    }

    #[test]
    fn the_materials_map_to_their_programs() {
        assert_eq!(Program::from_material(crate::world_objects::name_id("galacticZpoleTiltAdditive_material")), Some(Program::ZpoleTilt));
        assert_eq!(Program::from_material(crate::world_objects::name_id("galacticSpinTiltAdditive_material")), Some(Program::SpinTilt));
        assert_eq!(Program::from_material(crate::world_objects::name_id("galacticStar_material")), Some(Program::Star));
        assert_eq!(Program::from_material(crate::world_objects::name_id("effectAnimMaterial")), None);
        assert_eq!(Program::SpinTilt.shader_id(), 0x7000_0120);
        assert_eq!(Program::Star.shader_id(), 0x7000_0121);
        assert_eq!(Program::ZpoleTilt.shader_id(), 0x7000_0122);
    }
}
