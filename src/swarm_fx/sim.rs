//! Effect instance trees and their simulation.
//!
//! An [`EffectInstance`] mirrors one visual effect: one child per active block
//! (particle system, meta-particle system, nested effect, model, screen,
//! sequence). Particles live in Bevy world coordinates; rigid systems carry
//! their particles along when the effect moves.
//!
//! NATIVE: which blocks run (app flags, selection groups, LOD range and
//! scales; the LOD itself is picked per frame from the camera, see the LOD
//! helpers below and `notes\swarm-lod-notes.md`), the block transforms, every
//! authored number and curve, emission
//! modes (inject / maintain / rate with loop, cycles, delay, retrigger, hold,
//! kill), source shapes (box, round, ring/torus), loop boxes.
//! ADAPTED (native Swarm code not traced): curves are sampled at evenly spaced
//! keys over normalised life with linear interpolation; `vary` is a symmetric
//! relative variation (`x * (1 + vary * r)`, r in [-1, 1]); drag is
//! exponential; screw is turns per second about the effect Z axis; wiggles are
//! sinusoidal offsets; the transform scale scales particle sizes, positions,
//! velocities and forces (the authored effect-map tracers only fit their boxes
//! that way); world wind/gravity are not applied; preroll is simulated in at
//! most 120 steps; ribbons follow the effect's path parameter (static
//! ribbons) or the origin's history, shakes sum into `SwarmShake`.
//! Volumes (NATIVE, volume.rs) only carry their block frame and run until
//! stopped. Decals (decal.rs) carry their frame, age and three variation rolls;
//! their life rules are ADAPTED (see `DecalState`). Game models (game_model.rs)
//! carry their frame, modifiers and age; their life rule is ADAPTED (see
//! `GameModelState`).

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::format::*;
use super::{EffectRef, SwarmAppFlags, SwarmDiagnostics, SwarmEffect, SwarmIds, SwarmLibrary, SwarmMaps};

/// Spore effect space (Z up) to Bevy (Y up): `(x, y, z) -> (x, z, -y)`, the
/// same basis the model importers use, so Spore +Y faces Bevy -Z.
pub const AXES: Mat3 = Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y);

/// Retail effect parameter slots used by the Cell code (RE section 9).
pub const PARAM_INTENSITY: usize = 4;
pub const PARAM_COLOR: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: Vec3,
    /// Effect space -> world rotation (includes [`AXES`]).
    pub rot: Mat3,
    /// Uniform effect-space -> world scale.
    pub scale: f32,
}

impl Frame {
    pub fn new(origin: Vec3, rotation: Quat, scale: f32) -> Frame {
        Frame {
            origin,
            rot: Mat3::from_quat(rotation) * AXES,
            scale,
        }
    }
    pub fn point(&self, p: Vec3) -> Vec3 {
        self.origin + self.rot * (p * self.scale)
    }
    pub fn vector(&self, v: Vec3) -> Vec3 {
        self.rot * (v * self.scale)
    }
    pub fn inverse_point(&self, w: Vec3) -> Vec3 {
        self.rot.transpose() * (w - self.origin) / self.scale.max(1e-9)
    }
    /// Frame of a block: offset (bit 2), rotation (bit 1), scale (bit 0).
    pub fn child(&self, t: &BlockTransform) -> Frame {
        let offset = if t.flags & 4 != 0 {
            Vec3::from_array(t.offset)
        } else {
            Vec3::ZERO
        };
        let rotation = if t.flags & 2 != 0 {
            // Row-major `m[row][col]` applied to column vectors.
            Mat3::from_cols_array(&t.rotation).transpose()
        } else {
            Mat3::IDENTITY
        };
        let scale = if t.flags & 1 != 0 { t.scale } else { 1.0 };
        Frame {
            origin: self.point(offset),
            rot: self.rot * rotation,
            scale: self.scale * scale,
        }
    }
}

/// Multipliers inherited from parents (effect intensity, LOD scales,
/// meta-particle colour/alpha).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mods {
    pub alpha: f32,
    pub size: f32,
    pub emit: f32,
    pub color: Vec3,
}

impl Default for Mods {
    fn default() -> Self {
        Mods {
            alpha: 1.0,
            size: 1.0,
            emit: 1.0,
            color: Vec3::ONE,
        }
    }
}

/// PCG32 (deterministic per effect).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng(seed.wrapping_add(0x853C_49E6_748F_EA9B));
        r.next_u32();
        r
    }
    pub fn next_u32(&mut self) -> u32 {
        let old = self.0;
        self.0 = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        xorshifted.rotate_right((old >> 59) as u32)
    }
    /// [0, 1)
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
    /// [-1, 1)
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
    pub fn range(&mut self, r: [f32; 2]) -> f32 {
        r[0] + (r[1] - r[0]) * self.unit()
    }
    /// Probabilistic rounding so fractional counts average out.
    pub fn count(&mut self, x: f32) -> usize {
        if !(x > 0.0) {
            return 0;
        }
        let whole = x.floor();
        whole as usize + usize::from(self.unit() < x - whole)
    }
}

/// Evenly spaced keys over t in [0, 1], linear interpolation (ADAPTED).
pub fn curve(values: &[f32], t: f32, default: f32) -> f32 {
    match values.len() {
        0 => default,
        1 => values[0],
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (x as usize).min(n - 2);
            let f = x - i as f32;
            values[i] + (values[i + 1] - values[i]) * f
        }
    }
}

pub fn curve3(values: &[[f32; 3]], t: f32) -> Vec3 {
    match values.len() {
        0 => Vec3::ONE,
        1 => Vec3::from_array(values[0]),
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (x as usize).min(n - 2);
            Vec3::from_array(values[i]).lerp(Vec3::from_array(values[i + 1]), x - i as f32)
        }
    }
}

/// The emission settings shared by particle and meta-particle components.
#[derive(Clone, Copy, Debug)]
pub struct EmitParams<'a> {
    pub inject: bool,
    pub maintain: bool,
    pub hold: bool,
    pub kill: bool,
    pub rate: &'a [f32],
    pub rate_loop: f32,
    pub cycles: i32,
    pub delay: [f32; 2],
    pub retrigger: [f32; 2],
}

impl<'a> EmitParams<'a> {
    pub fn particles(p: &'a ParticleDef) -> Self {
        EmitParams {
            inject: p.has(pflag::INJECT),
            maintain: p.has(pflag::MAINTAIN),
            hold: p.has(pflag::RATE_HOLD),
            kill: p.has(pflag::RATE_KILL),
            rate: &p.rate,
            rate_loop: p.rate_loop,
            cycles: p.rate_curve_cycles,
            delay: p.emit_delay,
            retrigger: p.emit_retrigger,
        }
    }
    pub fn meta(p: &'a MetaParticleDef) -> Self {
        EmitParams {
            inject: p.has(mflag::INJECT),
            maintain: p.has(mflag::MAINTAIN),
            hold: p.has(mflag::RATE_HOLD),
            kill: p.has(mflag::RATE_KILL),
            rate: &p.rate,
            rate_loop: p.rate_loop,
            cycles: p.rate_curve_cycles,
            delay: p.emit_delay,
            retrigger: p.emit_retrigger,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Start,
    Delay(f32),
    Cycle,
    Retrigger(f32),
    Hold,
    Done,
}

/// Emission timeline: optional delay, `cycles` loops of `rate_loop` seconds
/// (0 = forever), then hold / kill / retrigger / done.
#[derive(Clone, Debug)]
pub struct Emitter {
    phase: Phase,
    cycle: i32,
    t: f32,
    carry: f32,
}

impl Default for Emitter {
    fn default() -> Self {
        Emitter {
            phase: Phase::Start,
            cycle: 0,
            t: 0.0,
            carry: 0.0,
        }
    }
}

/// What one emitter step asks for.
#[derive(Debug, Default, PartialEq)]
pub struct EmitStep {
    pub spawn: usize,
    pub kill_all: bool,
}

impl Emitter {
    pub fn done(&self) -> bool {
        self.phase == Phase::Done
    }

    fn begin_cycle(&mut self, p: &EmitParams, rng: &mut Rng, scale: f32, out: &mut EmitStep) {
        self.phase = Phase::Cycle;
        self.t = 0.0;
        if p.inject {
            out.spawn += rng.count(p.rate.first().copied().unwrap_or(1.0) * scale);
        }
    }

    fn continuous(&mut self, p: &EmitParams, rng: &mut Rng, dt: f32, alive: usize, scale: f32, t: f32, out: &mut EmitStep) {
        if p.maintain {
            let target = (p.rate.first().copied().unwrap_or(0.0) * scale).round().max(0.0) as usize;
            out.spawn += target.saturating_sub(alive + out.spawn);
        } else if !p.inject {
            self.carry += curve(p.rate, t, 0.0).max(0.0) * scale * dt;
            let n = self.carry.floor();
            self.carry -= n;
            out.spawn += n as usize;
        }
        let _ = rng;
    }

    pub fn step(&mut self, p: &EmitParams, rng: &mut Rng, dt: f32, alive: usize, scale: f32, stopped: bool) -> EmitStep {
        let mut out = EmitStep::default();
        if stopped {
            self.phase = Phase::Done;
            return out;
        }
        if self.phase == Phase::Start {
            if p.delay[0] >= 0.0 {
                self.phase = Phase::Delay(rng.range([p.delay[0], p.delay[1].max(p.delay[0])]));
            } else {
                self.cycle = 0;
                self.begin_cycle(p, rng, scale, &mut out);
            }
        }
        match self.phase {
            Phase::Delay(left) | Phase::Retrigger(left) => {
                let left = left - dt;
                if left <= 0.0 {
                    if matches!(self.phase, Phase::Delay(_)) {
                        self.cycle = 0;
                    } else {
                        self.cycle = 0;
                    }
                    self.begin_cycle(p, rng, scale, &mut out);
                } else {
                    self.phase = match self.phase {
                        Phase::Delay(_) => Phase::Delay(left),
                        _ => Phase::Retrigger(left),
                    };
                }
            }
            Phase::Cycle => {
                let length = p.rate_loop.max(1e-3);
                self.continuous(p, rng, dt, alive, scale, (self.t / length).min(1.0), &mut out);
                self.t += dt;
                while self.t >= length && self.phase == Phase::Cycle {
                    self.t -= length;
                    self.cycle += 1;
                    if p.cycles > 0 && self.cycle >= p.cycles && p.inject && p.hold {
                        // `inject N -hold T`: the burst repeats every T seconds
                        // (the authored particle lives slightly longer than T,
                        // e.g. the meteor rock of the Cell unlock pickups:
                        // life 0.5, hold 0.45, so it never blinks). ADAPTED
                        // reading of the flags; it was a single burst.
                        out.spawn += rng.count(p.rate.first().copied().unwrap_or(1.0) * scale);
                        self.cycle = 0;
                    } else if p.cycles > 0 && self.cycle >= p.cycles {
                        self.phase = if p.hold {
                            Phase::Hold
                        } else if p.kill {
                            out.kill_all = true;
                            Phase::Done
                        } else if p.retrigger[0] >= 0.0 {
                            Phase::Retrigger(rng.range([p.retrigger[0], p.retrigger[1].max(p.retrigger[0])]))
                        } else {
                            Phase::Done
                        };
                    } else if p.inject {
                        out.spawn += rng.count(p.rate.first().copied().unwrap_or(1.0) * scale);
                    }
                }
            }
            Phase::Hold => self.continuous(p, rng, dt, alive, scale, 1.0, &mut out),
            Phase::Start | Phase::Done => {}
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct Particle {
    pub pos: Vec3,
    pub vel: Vec3,
    pub age: f32,
    pub life: f32,
    /// Random draws in [-1, 1): size, aspect, rotation, alpha, colour (shared), wiggle phase.
    pub r: [f32; 6],
    /// Per-channel colour draws for `varyRGB`.
    pub rc: [f32; 3],
    /// Random frame offset (0..=255 range, used modulo the frame count).
    pub frame: u16,
}

/// Sample a point of the authored source volume (effect space).
fn source_point(volume: &[[f32; 3]; 2], torus_width: f32, round: bool, rng: &mut Rng) -> Vec3 {
    let min = Vec3::from_array(volume[0]);
    let max = Vec3::from_array(volume[1]);
    let center = (min + max) * 0.5;
    let half = (max - min) * 0.5;
    if torus_width >= 0.0 {
        // ring / torus: radius between (1 - width) and 1 of the X/Y extent.
        let a = rng.unit() * TAU;
        let r = 1.0 - torus_width.clamp(0.0, 1.0) * rng.unit();
        return center + Vec3::new(a.cos() * half.x * r, a.sin() * half.y * r, rng.signed() * half.z);
    }
    if round {
        for _ in 0..16 {
            let p = Vec3::new(rng.signed(), rng.signed(), rng.signed());
            if p.length_squared() <= 1.0 {
                return center + p * half;
            }
        }
        return center;
    }
    center + Vec3::new(rng.signed(), rng.signed(), rng.signed()) * half
}

fn emit_velocity(direction: &[[f32; 3]; 2], speed: [f32; 2], rng: &mut Rng) -> Vec3 {
    let a = Vec3::from_array(direction[0]);
    let b = Vec3::from_array(direction[1]);
    let d = a + (b - a) * Vec3::new(rng.unit(), rng.unit(), rng.unit());
    d.normalize_or_zero() * rng.range(speed)
}

/// One running particle component.
#[derive(Clone, Debug)]
pub struct ParticleSystem {
    pub dir: u16,
    pub index: u32,
    pub uid: u64,
    pub particles: Vec<Particle>,
    pub emitter: Emitter,
    pub frame: Option<Frame>,
    pub mods: Mods,
    pub rigid: bool,
    pub stopped: bool,
}

impl ParticleSystem {
    fn new(dir: u16, index: u32, uid: u64, rigid: bool) -> Self {
        ParticleSystem {
            dir,
            index,
            uid,
            particles: vec![],
            emitter: Emitter::default(),
            frame: None,
            mods: Mods::default(),
            rigid,
            stopped: false,
        }
    }

    pub fn finished(&self) -> bool {
        self.emitter.done() && self.particles.is_empty()
    }

    fn spawn(&mut self, def: &ParticleDef, maps: &SwarmMaps, rng: &mut Rng, frame: &Frame, age: f32) {
        let mut local = source_point(&def.emit_volume, def.torus_width, def.has(pflag::SOURCE_ROUND), rng);
        let mut pos = frame.point(local);
        if def.has(pflag::MAP_EMIT_HEIGHT_RANGE) && !def.map.emit.is_none() {
            // Emit only where the map height lies in the authored range;
            // without a registered map everything passes (ADAPTED).
            let mut tries = 0;
            while let Some(h) = maps.height(def.map.emit.instance, pos) {
                if (def.map.altitude_range[0]..=def.map.altitude_range[1]).contains(&h) {
                    break;
                }
                tries += 1;
                if tries > 4 {
                    return;
                }
                local = source_point(&def.emit_volume, def.torus_width, def.has(pflag::SOURCE_ROUND), rng);
                pos = frame.point(local);
            }
        }
        let vel = frame.vector(emit_velocity(&def.emit_direction, def.emit_speed, rng));
        let life = rng.range(def.life).max(1e-3);
        let mut p = Particle {
            pos,
            vel,
            age: 0.0,
            life,
            r: std::array::from_fn(|_| rng.signed()),
            rc: std::array::from_fn(|_| rng.signed()),
            frame: (rng.next_u32() & 0xFF) as u16,
        };
        if age > 0.0 {
            integrate(def, maps, frame, &mut p, age);
        }
        self.particles.push(p);
    }

    fn update(&mut self, def: &ParticleDef, maps: &SwarmMaps, rng: &mut Rng, dt: f32, frame: Frame, mods: Mods) {
        if self.rigid {
            if let Some(prev) = self.frame.filter(|prev| *prev != frame) {
                let rot = frame.rot * prev.rot.transpose();
                let k = frame.scale / prev.scale.max(1e-9);
                for p in &mut self.particles {
                    p.pos = frame.point(prev.inverse_point(p.pos));
                    p.vel = rot * p.vel * k;
                }
            }
        }
        self.frame = Some(frame);
        self.mods = mods;
        for p in &mut self.particles {
            integrate(def, maps, &frame, p, dt);
        }
        self.particles.retain(|p| p.age < p.life);
        let step = self.emitter.step(
            &EmitParams::particles(def),
            rng,
            dt,
            self.particles.len(),
            mods.emit,
            self.stopped,
        );
        if step.kill_all {
            self.particles.clear();
        }
        for _ in 0..step.spawn.min(20_000) {
            let age = rng.unit() * dt;
            self.spawn(def, maps, rng, &frame, age);
        }
    }
}

/// Advance one particle by `dt` seconds.
fn integrate(def: &ParticleDef, maps: &SwarmMaps, frame: &Frame, p: &mut Particle, dt: f32) {
    let t = (p.age / p.life).clamp(0.0, 1.0);
    let mut acc = frame.vector(Vec3::from_array(def.direction_forces));
    if def.radial_force != 0.0 {
        let center = frame.point(Vec3::from_array(def.radial_force_location));
        acc += (p.pos - center).normalize_or_zero() * def.radial_force * frame.scale;
    }
    if def.has(pflag::ATTRACTOR) {
        let origin = frame.point(Vec3::from_array(def.attractor_origin));
        let to = origin - p.pos;
        let distance = to.length();
        if distance < def.attractor.range * frame.scale && distance > 1e-6 {
            acc += to / distance * curve(&def.attractor.strength, t, 0.0) * frame.scale;
        }
        if def.attractor.kill_range > 0.0 && distance < def.attractor.kill_range * frame.scale {
            p.age = p.life;
        }
    }
    p.vel += acc * dt;
    if def.drag > 0.0 {
        p.vel *= (-def.drag * dt).exp();
    }
    let mut step = p.vel * dt;
    if def.has(pflag::MAP_ADVECT) {
        if let Some(flow) = maps.velocity(def.map.force.instance, p.pos) {
            let strength = if def.map.repulse_strength != 0.0 {
                def.map.repulse_strength
            } else {
                1.0
            };
            step += flow * strength * dt;
        }
    }
    p.pos += step;
    if def.screw_rate != 0.0 {
        let axis = (frame.rot * Vec3::Z).normalize_or_zero();
        let q = Quat::from_axis_angle(axis, def.screw_rate * TAU * dt);
        p.pos = frame.origin + q * (p.pos - frame.origin);
        p.vel = q * p.vel;
    }
    if def.has(pflag::LOOP_BOX) {
        p.pos = wrap_loop_box(frame, &def.emit_volume, p.pos);
    }
    p.age += dt;
}

/// Wrap a world position into the authored source box around the frame.
pub fn wrap_loop_box(frame: &Frame, volume: &[[f32; 3]; 2], pos: Vec3) -> Vec3 {
    let min = Vec3::from_array(volume[0]);
    let size = Vec3::from_array(volume[1]) - min;
    let mut local = frame.inverse_point(pos);
    for i in 0..3 {
        if size[i] > 1e-6 {
            local[i] = min[i] + (local[i] - min[i]).rem_euclid(size[i]);
        }
    }
    frame.point(local)
}

/// Normalised position inside the loop box per axis, for the edge fade.
pub fn loop_box_fade(frame: &Frame, volume: &[[f32; 3]; 2], alpha: &[f32], pos: Vec3) -> f32 {
    if alpha.is_empty() {
        return 1.0;
    }
    let min = Vec3::from_array(volume[0]);
    let size = Vec3::from_array(volume[1]) - min;
    let local = frame.inverse_point(pos);
    let mut fade = 1.0f32;
    // ADAPTED: the authored curve spans the box edge to edge on each
    // horizontal axis (the depth axis is left unfaded).
    for i in 0..2 {
        if size[i] > 1e-6 {
            fade = fade.min(curve(alpha, (local[i] - min[i]) / size[i], 1.0));
        }
    }
    fade.clamp(0.0, 1.0)
}

/// A meta particle: a moving, oriented copy of a whole effect.
pub struct MetaParticle {
    pub pos: Vec3,
    pub vel: Vec3,
    pub age: f32,
    pub life: f32,
    pub r: [f32; 6],
    pub child: Option<EffectInstance>,
}

pub struct MetaSystem {
    pub dir: u16,
    pub index: u32,
    pub particles: Vec<MetaParticle>,
    pub emitter: Emitter,
    pub frame: Option<Frame>,
    pub stopped: bool,
    pub rigid: bool,
}

impl MetaSystem {
    pub fn finished(&self) -> bool {
        self.emitter.done() && self.particles.is_empty()
    }
}

/// A model component (drawn by the render side from `frame` and `mods`).
pub struct ModelState {
    pub dir: u16,
    pub index: u32,
    pub uid: u64,
    pub frame: Option<Frame>,
    pub mods: Mods,
    pub age: f32,
}

/// A screen component (full-screen colour / background image).
pub struct ScreenState {
    pub dir: u16,
    pub index: u32,
    pub uid: u64,
    pub age: f32,
    pub mods: Mods,
    /// Game colour parameter (retail parameter 5 on `cell_background_color`).
    pub color: Option<Vec3>,
    pub stopped: bool,
}

impl ScreenState {
    fn finished(&self, def: &ScreenDef) -> bool {
        // Loop (bit 0) / sustain (bit 1) / hold (bit 3) keep it until stopped.
        self.stopped || (def.flags & 0b1011 == 0 && self.age >= def.delay + def.life.max(0.0))
    }
    /// Strength curve value over the screen effect's life.
    pub fn strength(&self, def: &ScreenDef) -> f32 {
        let life = def.life.max(1e-3);
        let t = (self.age - def.delay).max(0.0);
        let u = if def.flags & 1 != 0 {
            (t / life).fract()
        } else {
            (t / life).min(1.0)
        };
        curve(&def.strength, u, 1.0)
    }
    pub fn color(&self, def: &ScreenDef) -> Vec3 {
        let life = def.life.max(1e-3);
        let t = (self.age - def.delay).max(0.0);
        let u = if def.flags & 1 != 0 {
            (t / life).fract()
        } else {
            (t / life).min(1.0)
        };
        curve3(&def.color, u)
    }
}

/// A ribbon component: a strip through world-space points (the effect's
/// `path` parameter for static ribbons, else the trail of the effect origin).
pub struct RibbonState {
    pub dir: u16,
    pub index: u32,
    pub uid: u64,
    pub age: f32,
    pub life: f32,
    pub mods: Mods,
    pub frame: Option<Frame>,
    pub stopped: bool,
    /// Polyline, head first.
    pub points: Vec<Vec3>,
}

/// `ribbon` flag bits (SporeModder-FX `FLAGS_*`).
pub mod ribbon_flag {
    pub const STATIC: u32 = 0x1;
    pub const SUSTAIN: u32 = 0x100;
}

impl RibbonState {
    fn finished(&self, def: &RibbonDef) -> bool {
        self.stopped || (def.flags & ribbon_flag::SUSTAIN == 0 && self.age >= self.life)
    }
    /// Age over life, 0..1 (sustained ribbons loop their time curves).
    pub fn time(&self, def: &RibbonDef) -> f32 {
        let life = self.life.max(1e-3);
        if def.flags & ribbon_flag::SUSTAIN != 0 {
            (self.age / life).fract()
        } else {
            (self.age / life).min(1.0)
        }
    }
}

/// A shake component: shakes the camera (see shake.rs).
pub struct ShakeState {
    pub dir: u16,
    pub index: u32,
    pub age: f32,
    pub stopped: bool,
    pub phase: f32,
}

/// A running sequence script (see `update_sequence`).
pub struct SequenceState {
    pub dir: u16,
    pub index: u32,
    pub age: f32,
    /// Live effect instances: the entry in progress and the stopped ones still
    /// winding down (slots are reused, never removed).
    pub children: Vec<Option<EffectInstance>>,
    /// Next entry to begin.
    cursor: usize,
    /// The entry in progress.
    step: Option<SequenceStep>,
    /// The script ran past its last entry (and does not loop).
    done: bool,
}

/// One running `play` / `wait` entry.
struct SequenceStep {
    /// The `children` slot of the effect this entry plays (none for a `wait`).
    slot: Option<usize>,
    /// Seconds left (none = until the played effect ends by itself).
    left: Option<f32>,
}

/// A volume component (drawn by volume.rs from `frame`). Retail `IsRunning`
/// (0xFEB920) is its visible flag: it runs from start until stopped.
pub struct VolumeState {
    pub dir: u16,
    pub index: u32,
    pub uid: u64,
    pub frame: Option<Frame>,
    pub stopped: bool,
}

/// A decal component (drawn by decal.rs from `frame`, `age` and `rolls`).
/// ADAPTED life rules (the native decal instance was not traced): `loop`
/// (0) wraps its curves every `life` seconds and runs until stopped, `single`
/// (1) plays its curves once and finishes at `life`, `sustain` (2) plays them
/// once and holds the last value until stopped; paint decals (type 3) are a
/// one-off stamp that never keeps their effect alive (decal.rs keeps the
/// drawn mark after the effect has ended).
pub struct DecalState {
    pub dir: u16,
    pub index: u32,
    pub uid: u64,
    pub age: f32,
    /// Updates run so far (a paint stamp is shown for the first two).
    pub updates: u32,
    pub mods: Mods,
    pub frame: Option<Frame>,
    pub stopped: bool,
    /// Signed [-1, 1) rolls for the rotation, size and alpha `vary` values.
    pub rolls: [f32; 3],
}

impl DecalState {
    /// Whether the decal still keeps its effect running.
    pub fn running(&self, def: &DecalDef) -> bool {
        if self.stopped {
            return false;
        }
        if def.kind == decal_type::PAINT {
            return self.updates < 2;
        }
        def.life_type != 1 || self.age < def.life
    }
    /// Age over life, 0..1 (see the life rules above).
    pub fn time(&self, def: &DecalDef) -> f32 {
        let life = def.life.max(1e-3);
        if def.life_type == 0 {
            (self.age / life).fract()
        } else {
            (self.age / life).min(1.0)
        }
    }
}

/// A `gameModel` component (drawn by models.rs from `frame`, `mods` and `age`;
/// see game_model.rs). ADAPTED life rule (the native model instance was not
/// traced): a model without animations (held items, props) runs until
/// stopped; loop (mode 0) and sustain (mode 1) animations also run until
/// stopped; when every animation is `single` (mode 2) the model ends after
/// the longest animation length.
pub struct GameModelState {
    pub dir: u16,
    pub index: u32,
    pub uid: u64,
    pub frame: Option<Frame>,
    pub mods: Mods,
    pub age: f32,
    pub stopped: bool,
}

impl GameModelState {
    /// Whether the model still keeps its effect running.
    pub fn running(&self, def: &GameModelDef) -> bool {
        if self.stopped {
            return false;
        }
        if def.animations.is_empty() || def.animations.iter().any(|a| a.mode != 2) {
            return true;
        }
        let longest = def.animations.iter().map(|a| a.length_range[1]).fold(0.0, f32::max);
        self.age < longest
    }
}

pub enum ChildState {
    Particles(ParticleSystem),
    Meta(MetaSystem),
    Effect(Box<EffectInstance>),
    Model(ModelState),
    Screen(ScreenState),
    Sequence(SequenceState),
    Ribbon(RibbonState),
    Shake(ShakeState),
    Volume(VolumeState),
    Decal(DecalState),
    GameModel(GameModelState),
    /// Sound component (type 0x05): reports itself to [`SwarmSounds`] every
    /// update; never keeps the effect alive.
    Sound(SoundState),
    /// Not simulated: never blocks completion.
    Inert(u8),
}

/// A sound component of a running instance (see `sound.rs`).
pub struct SoundState {
    pub dir: u16,
    pub index: u32,
    pub uid: u64,
    pub age: f32,
    pub stopped: bool,
}

pub struct Child {
    pub block: u16,
    pub time_scale: f32,
    /// LOD (emit, size, alpha) scales of this block at the start / end of the
    /// current LOD interval (retail component parameter 0x101, 6 floats); the
    /// effect's LOD lerp blends them.
    pub lod: [f32; 3],
    pub lod_end: [f32; 3],
    /// A child whose block left the LOD range keeps running to its end with
    /// the lerp frozen at 0 or 1 (retail record flags 0x80 / 0x100).
    pub lod_fixed: Option<f32>,
    /// Stopped by an LOD change; removed once it has finished.
    pub retired: bool,
    pub finished: bool,
    pub ignore_length: bool,
    pub state: ChildState,
}

impl Child {
    /// (emit, size, alpha) scale for the effect's current LOD lerp.
    pub fn lod_scale(&self, lerp: f32) -> [f32; 3] {
        let t = self.lod_fixed.unwrap_or(lerp);
        std::array::from_fn(|k| self.lod[k] + (self.lod_end[k] - self.lod[k]) * t)
    }
}

/// The camera as the LOD evaluator sees it (retail `cGlobalParams` camera
/// position +0x8C and forward +0x68, the manager LOD distance scale +0x64 and
/// the world LOD level +0x80 / +0x88).
#[derive(Clone, Copy, Debug)]
pub struct LodView {
    /// Camera position and unit forward in world space; `None` = no distance
    /// LOD (every effect runs LOD 1).
    pub camera: Option<(Vec3, Vec3)>,
    /// Manager LOD distance scale (default 1).
    pub distance_scale: f32,
    /// World LOD level and its fraction: effects without `lodDistances` follow
    /// it. The quality setting that drives it is not modelled (ADAPTED: 1 / 0,
    /// the retail values after the world constructor).
    pub world_level: u8,
    pub world_lerp: f32,
}

impl LodView {
    pub const OFF: LodView = LodView {
        camera: None,
        distance_scale: 1.0,
        world_level: 1,
        world_lerp: 0.0,
    };
}

/// Retail LOD value of an effect from the camera (world update 0xA722DB):
/// plain distance, or with the extended-distance flag the weighted sum
/// `distance * w_d + (0.5 - 0.5 * cos(up, to camera)) * w_facing +
/// 0.5 * (1 + cos(camera forward, to camera)) * w_focus`.
/// `up` is the effect's world up (its Z axis), `to camera` runs effect -> camera.
pub fn lod_value(def: &VisualEffectDef, camera: Vec3, forward: Vec3, position: Vec3, up: Vec3) -> f32 {
    let v = camera - position;
    let d2 = v.length_squared();
    let d = d2.sqrt();
    if def.flags & eflag::EXTENDED_LOD_DISTANCES == 0 {
        return d;
    }
    let inv = 1.0 / (d + 1e-6);
    let [focus, facing, distance] = def.extended_lod_weights;
    distance * (d2 * inv) + facing * (0.5 - 0.5 * up.dot(v) * inv) + focus * 0.5 * (1.0 + forward.dot(v) * inv)
}

/// Retail LOD interval search (0xA724AF): LOD `i` (1-based) covers
/// `[distances[i - 1], distances[i])`; the last LOD (`count - 1`) is open
/// ended. Returns the LOD, its interval and the position inside it (0..=1).
pub fn lod_select(distances: &[f32], value: f32) -> (u8, [f32; 2], f32) {
    if distances.len() < 2 {
        return (1, [0.0, 0.0], 0.0);
    }
    let n = distances.len() - 1;
    let mut i = 1;
    while i < n && distances[i] <= value {
        i += 1;
    }
    let range = [distances[i - 1], distances[i]];
    (i.min(254) as u8, range, lod_lerp(range, value))
}

/// Position of `value` inside an LOD interval, clamped to 0..=1 (NaN -> 0 as
/// the retail check at 0xA72603).
pub fn lod_lerp(range: [f32; 2], value: f32) -> f32 {
    let t = (value - range[0]) / (range[1] - range[0]);
    if t.is_nan() {
        0.0
    } else {
        t.clamp(0.0, 1.0)
    }
}

/// Retail block rule (0xA71A4D): active for `lodBegin <= lod < lodEnd` (the
/// authored scale list has `lodEnd - lodBegin + 1` entries: the last one is
/// the value reached when the block leaves).
pub fn block_in_lod(block: &EffectBlock, lod: u8) -> bool {
    block.lod_begin <= lod && lod < block.lod_end
}

/// The (begin, end) scales of a block at `lod` (retail 0xA703A0).
pub fn block_scales(block: &EffectBlock, lod: u8) -> ([f32; 3], [f32; 3]) {
    let n = block.lod_scales.len();
    match n {
        0 => ([1.0; 3], [1.0; 3]),
        1 => (block.lod_scales[0], block.lod_scales[0]),
        _ => {
            let i = lod.saturating_sub(block.lod_begin) as usize;
            (block.lod_scales[i.min(n - 1)], block.lod_scales[(i + 1).min(n - 1)])
        }
    }
}

/// Stop one child (soft: emitters end, live particles finish).
fn stop_child(state: &mut ChildState) {
    match state {
        ChildState::Particles(s) => s.stopped = true,
        ChildState::Meta(m) => {
            m.stopped = true;
            for p in &mut m.particles {
                if let Some(c) = &mut p.child {
                    c.stop();
                }
            }
        }
        ChildState::Effect(e) => e.stop(),
        ChildState::Screen(s) => s.stopped = true,
        ChildState::Ribbon(r) => r.stopped = true,
        ChildState::Shake(sh) => sh.stopped = true,
        ChildState::Volume(v) => v.stopped = true,
        ChildState::Decal(d) => d.stopped = true,
        ChildState::GameModel(g) => g.stopped = true,
        ChildState::Sound(s) => s.stopped = true,
        ChildState::Sequence(s) => {
            for c in s.children.iter_mut().flatten() {
                c.stop();
            }
        }
        ChildState::Model(_) | ChildState::Inert(_) => {}
    }
}

/// Everything an update needs besides the instance itself.
pub struct SimContext<'a> {
    pub lib: &'a SwarmLibrary,
    pub maps: &'a SwarmMaps,
    pub app_flags: u32,
    pub ids: &'a mut u64,
    pub camera: Option<Vec3>,
    /// Camera basis as a Spore-to-world rotation for `cameraFacing` effects:
    /// Spore X = screen right, Y = into the screen, Z = screen up (ADAPTED).
    pub camera_rot: Option<Mat3>,
    pub params: [Option<Vec4>; 16],
    /// Effect `kParamPath` points (world space) for static ribbons.
    pub path: &'a [Vec3],
    /// Camera offset accumulated by this frame's shakes.
    pub shake: &'a mut Vec3,
    /// The root effect entity being updated (owner of the sounds below).
    pub entity: Entity,
    /// Sound components live in this step.
    pub sounds: &'a mut super::sound::SwarmSounds,
    /// Camera for the per-effect LOD (`LodView::OFF` = LOD 1 everywhere).
    pub lod: LodView,
}

impl SimContext<'_> {
    fn uid(&mut self) -> u64 {
        *self.ids += 1;
        *self.ids
    }
}

pub struct EffectInstance {
    pub effect: EffectRef,
    pub age: f32,
    pub children: Vec<Child>,
    pub rng: Rng,
    pub started: bool,
    pub stopping: bool,
    pub rigid: bool,
    pub preroll: bool,
    /// Current LOD (retail `mCurrentLOD`, 0 = none yet) and the position
    /// inside its interval (`mCurrentLODLerp`).
    pub lod: u8,
    pub lod_lerp: f32,
    /// Interval of the current LOD in LOD-value units (`mCurrentRange`).
    pub lod_range: [f32; 2],
    /// Selection-group picks (one block per group, rolled once at start).
    chosen: std::collections::HashMap<u16, usize>,
}

impl EffectInstance {
    pub fn new(effect: EffectRef, seed: u64, preroll: bool) -> Self {
        EffectInstance {
            effect,
            age: 0.0,
            children: vec![],
            rng: Rng::new(seed),
            started: false,
            stopping: false,
            rigid: false,
            preroll,
            lod: 0,
            lod_lerp: 0.0,
            lod_range: [f32::INFINITY, f32::NEG_INFINITY],
            chosen: Default::default(),
        }
    }

    fn start(&mut self, ctx: &mut SimContext, frame: &Frame) {
        self.started = true;
        let lib = ctx.lib;
        let def = &lib.dir(self.effect.dir).effects[self.effect.index as usize];
        self.rigid = def.flags & eflag::RIGID != 0;
        // Selection groups: one block per group, chosen by chance (u16 / 65535).
        let mut groups: std::collections::BTreeMap<u16, Vec<usize>> = Default::default();
        for (i, b) in def.blocks.iter().enumerate() {
            if b.selection_group != 0 {
                groups.entry(b.selection_group).or_default().push(i);
            }
        }
        for (g, members) in groups {
            let mut roll = self.rng.unit() * 65535.0;
            let mut pick = members[members.len() - 1];
            for &m in &members {
                let chance = def.blocks[m].selection_chance as f32;
                if roll < chance {
                    pick = m;
                    break;
                }
                roll -= chance;
            }
            self.chosen.insert(g, pick);
        }
        // The first LOD: from the camera when the effect has `lodDistances`,
        // else the world level (retail world update 0xA7261A).
        let (lod, lerp) = self.pick_lod(ctx, def, Some(frame));
        self.lod_lerp = lerp.1;
        if let Some(range) = lerp.0 {
            self.lod_range = range;
        }
        self.set_lod(ctx, lod);
    }

    /// The LOD the effect should run now: `(lod, (new range, lerp))`.
    /// `frame` is the effect's world frame (position and up for distance LOD).
    fn pick_lod(
        &mut self,
        ctx: &SimContext,
        def: &VisualEffectDef,
        frame: Option<&Frame>,
    ) -> (u8, (Option<[f32; 2]>, f32)) {
        let world = (ctx.lod.world_level.max(1), (None, ctx.lod.world_lerp));
        let (Some((camera, forward)), Some(frame)) = (ctx.lod.camera, frame) else {
            return world;
        };
        if def.lod_distances.is_empty() {
            return world;
        }
        let up = (frame.rot * Vec3::Z).normalize_or_zero();
        let value = ctx.lod.distance_scale * lod_value(def, camera, forward, frame.origin, up);
        // Still inside the current interval: only the position changes.
        if self.lod != 0 && value >= self.lod_range[0] && self.lod_range[1] > value {
            return (self.lod, (None, lod_lerp(self.lod_range, value)));
        }
        let (lod, range, lerp) = lod_select(&def.lod_distances, value);
        (lod, (Some(range), lerp))
    }

    /// Change the LOD (retail 0xA719A0): blocks that leave the new LOD are
    /// stopped (soft: they finish with the lerp frozen at the end they left
    /// through; a hard transition drops them), blocks that enter it start,
    /// blocks that stay get the scales of the new interval.
    fn set_lod(&mut self, ctx: &mut SimContext, new_lod: u8) {
        let lib = ctx.lib;
        let def = &lib.dir(self.effect.dir).effects[self.effect.index as usize];
        let hard = def.flags & (eflag::HARD_STOP | eflag::HARD_LOD_TRANSITION) != 0;
        let mut i = 0;
        while i < self.children.len() {
            let block = &def.blocks[self.children[i].block as usize];
            let child = &mut self.children[i];
            if block_in_lod(block, new_lod) && !child.retired {
                let (begin, end) = block_scales(block, new_lod);
                child.lod = begin;
                child.lod_end = end;
                i += 1;
                continue;
            }
            if !child.retired {
                stop_child(&mut child.state);
            }
            let running = !matches!(child.state, ChildState::Model(_) | ChildState::Inert(_));
            if hard || !running {
                self.children.remove(i);
                continue;
            }
            if !child.retired {
                child.retired = true;
                child.lod_fixed = Some(if self.lod_lerp < 0.5 { 0.0 } else { 1.0 });
            }
            i += 1;
        }
        for (i, b) in def.blocks.iter().enumerate() {
            if !block_in_lod(b, new_lod) || (self.lod != 0 && block_in_lod(b, self.lod)) {
                continue;
            }
            if b.app_flags_mask != 0 && (ctx.app_flags & b.app_flags_mask) != (b.app_flags & b.app_flags_mask) {
                continue;
            }
            if b.selection_group != 0 && self.chosen.get(&b.selection_group) != Some(&i) {
                continue;
            }
            let child = self.spawn_child(ctx, i, new_lod);
            self.children.push(child);
        }
        self.lod = new_lod;
    }

    /// Instantiate block `i` of the effect for `lod`.
    fn spawn_child(&mut self, ctx: &mut SimContext, i: usize, lod: u8) -> Child {
        let lib = ctx.lib;
        let def = &lib.dir(self.effect.dir).effects[self.effect.index as usize];
        let b = &def.blocks[i];
        let (lod_begin, lod_end) = block_scales(b, lod);
        let rigid = self.rigid || b.flags & bflag::RIGID != 0;
        let dir = self.effect.dir;
        let state = match b.kind {
            kind::PARTICLES if b.index >= 0 => {
                let uid = ctx.uid();
                ChildState::Particles(ParticleSystem::new(dir, b.index as u32, uid, rigid))
            }
            kind::META_PARTICLES if b.index >= 0 => ChildState::Meta(MetaSystem {
                dir,
                index: b.index as u32,
                particles: vec![],
                emitter: Emitter::default(),
                frame: None,
                stopped: false,
                rigid,
            }),
            kind::EFFECT => match lib.resolve_effect(dir, b.index) {
                // The parent's preroll steps the whole tree.
                Some(effect) => ChildState::Effect(Box::new(EffectInstance::new(
                    effect,
                    self.rng.next_u32() as u64,
                    false,
                ))),
                None => ChildState::Inert(b.kind),
            },
            kind::MODEL if b.index >= 0 => ChildState::Model(ModelState {
                dir,
                index: b.index as u32,
                uid: ctx.uid(),
                frame: None,
                mods: Mods::default(),
                age: 0.0,
            }),
            kind::SCREEN if b.index >= 0 => ChildState::Screen(ScreenState {
                dir,
                index: b.index as u32,
                uid: ctx.uid(),
                age: 0.0,
                mods: Mods::default(),
                color: None,
                stopped: false,
            }),
            kind::RIBBON if b.index >= 0 => {
                let def = &lib.dir(dir).ribbons[b.index as usize];
                ChildState::Ribbon(RibbonState {
                    dir,
                    index: b.index as u32,
                    uid: ctx.uid(),
                    age: 0.0,
                    life: self.rng.range(def.life).max(1e-3),
                    mods: Mods::default(),
                    frame: None,
                    stopped: false,
                    points: vec![],
                })
            }
            kind::SHAKE if b.index >= 0 => ChildState::Shake(ShakeState {
                dir,
                index: b.index as u32,
                age: 0.0,
                stopped: false,
                phase: self.rng.unit() * TAU,
            }),
            kind::VOLUME if b.index >= 0 => ChildState::Volume(VolumeState {
                dir,
                index: b.index as u32,
                uid: ctx.uid(),
                frame: None,
                stopped: false,
            }),
            kind::DECAL if b.index >= 0 => ChildState::Decal(DecalState {
                dir,
                index: b.index as u32,
                uid: ctx.uid(),
                age: 0.0,
                updates: 0,
                mods: Mods::default(),
                frame: None,
                stopped: false,
                rolls: [self.rng.signed(), self.rng.signed(), self.rng.signed()],
            }),
            kind::GAME_MODEL if b.index >= 0 => ChildState::GameModel(GameModelState {
                dir,
                index: b.index as u32,
                uid: ctx.uid(),
                frame: None,
                mods: Mods::default(),
                age: 0.0,
                stopped: false,
            }),
            kind::SOUND if b.index >= 0 && (b.index as usize) < lib.dir(dir).sounds.len() => {
                ChildState::Sound(SoundState { dir, index: b.index as u32, uid: ctx.uid(), age: 0.0, stopped: false })
            }
            kind::SEQUENCE if b.index >= 0 => {
                ChildState::Sequence(SequenceState {
                    dir,
                    index: b.index as u32,
                    age: 0.0,
                    children: vec![],
                    cursor: 0,
                    step: None,
                    done: false,
                })
            }
            other => ChildState::Inert(other),
        };
        Child {
            block: i as u16,
            time_scale: if b.time_scale > 0.0 { b.time_scale } else { 1.0 },
            lod: lod_begin,
            lod_end,
            lod_fixed: None,
            retired: false,
            finished: false,
            ignore_length: b.flags & bflag::IGNORE_LENGTH != 0,
            state,
        }
    }

    pub fn stop(&mut self) {
        self.stopping = true;
        for child in &mut self.children {
            stop_child(&mut child.state);
        }
    }

    /// Advance by `dt`; returns false once everything has finished.
    pub fn update(&mut self, ctx: &mut SimContext, dt: f32, mut frame: Frame, mods: Mods) -> bool {
        let lib = ctx.lib;
        let def = &lib.dir(self.effect.dir).effects[self.effect.index as usize];
        if def.flags & eflag::CAMERA_FACING != 0 {
            if let Some(rot) = ctx.camera_rot {
                frame.rot = rot;
            }
        }
        if !self.started {
            self.start(ctx, &frame);
            if self.preroll {
                self.run_preroll(ctx, frame, mods);
            }
        }
        self.age += dt;
        self.update_lod(ctx, def, &frame);
        let lerp = self.lod_lerp;
        let mut alive = false;
        for child in &mut self.children {
            let block = &def.blocks[child.block as usize];
            let mut cframe = frame.child(&block.transform);
            let cdt = dt * child.time_scale;
            let scale = child.lod_scale(lerp);
            let cmods = if matches!(child.state, ChildState::Effect(_)) {
                // A nested effect takes only the size scale, as a transform
                // scale about its block origin (retail 0xA84066 stores
                // data[1] / data[4]; the transform update 0xA82E03 applies it).
                cframe.scale *= scale[1];
                mods
            } else {
                Mods {
                    alpha: mods.alpha * scale[2],
                    size: mods.size * scale[1],
                    emit: mods.emit * scale[0],
                    color: mods.color,
                }
            };
            let running = match &mut child.state {
                ChildState::Particles(s) => {
                    let pdef = &lib.dir(s.dir).particles[s.index as usize];
                    s.stopped |= self.stopping;
                    s.update(pdef, ctx.maps, &mut self.rng, cdt, cframe, cmods);
                    !s.finished()
                }
                ChildState::Meta(m) => {
                    m.stopped |= self.stopping;
                    update_meta(m, ctx, &mut self.rng, cdt, cframe, cmods);
                    !m.finished()
                }
                ChildState::Effect(e) => {
                    if self.stopping && !e.stopping {
                        e.stop();
                    }
                    e.update(ctx, cdt, cframe, cmods)
                }
                ChildState::Model(m) => {
                    m.frame = Some(cframe);
                    m.mods = cmods;
                    m.age += cdt;
                    !self.stopping
                }
                ChildState::Screen(s) => {
                    s.age += cdt;
                    s.mods = cmods;
                    s.color = ctx.params[PARAM_COLOR].map(|c| c.truncate());
                    s.stopped |= self.stopping;
                    !s.finished(&lib.dir(s.dir).screens[s.index as usize])
                }
                ChildState::Ribbon(r) => {
                    let rdef = &lib.dir(r.dir).ribbons[r.index as usize];
                    r.age += cdt;
                    r.mods = cmods;
                    r.frame = Some(cframe);
                    r.stopped |= self.stopping;
                    update_ribbon_points(r, rdef, ctx.path, cframe);
                    !r.finished(rdef)
                }
                ChildState::Shake(sh) => {
                    let sdef = &lib.dir(sh.dir).shakes[sh.index as usize];
                    sh.age += cdt;
                    sh.stopped |= self.stopping;
                    let life = sdef.life.max(1e-3);
                    if sh.stopped || sh.age >= life {
                        false
                    } else {
                        let u = sh.age / life;
                        let freq = curve(&sdef.frequency, u, 10.0);
                        sh.phase += TAU * freq * cdt;
                        let fade = sdef.fade_time.max(1e-3);
                        let fade_in = ((life - sh.age) / fade).clamp(0.0, 1.0);
                        let mut amount = curve(&sdef.strength, u, 1.0) * fade_in * cmods.alpha;
                        if sdef.falloff > 0.0 {
                            if let Some(camera) = ctx.camera {
                                let mut d = cframe.origin - camera;
                                if sdef.flags & 1 == 0 {
                                    d.y = 0.0;
                                }
                                amount *= (1.0 - d.length() / (sdef.falloff * cframe.scale.max(1e-6))).clamp(0.0, 1.0);
                            }
                        }
                        // Table 0 shakes in all directions at the frequency,
                        // table 1 only up and down (screen Y).
                        let x = if sdef.table == 1 { 0.0 } else { sh.phase.sin() };
                        let y = (sh.phase * 1.37 + 1.0).sin() * sdef.aspect_ratio.max(0.0);
                        *ctx.shake += Vec3::new(x, y, 0.0) * amount;
                        true
                    }
                }
                ChildState::Sequence(seq) => update_sequence(seq, ctx, &mut self.rng, cdt, cframe, cmods, self.stopping),
                ChildState::Volume(v) => {
                    v.frame = Some(cframe);
                    v.stopped |= self.stopping;
                    !v.stopped
                }
                ChildState::Decal(d) => {
                    let ddef = &lib.dir(d.dir).decals[d.index as usize];
                    d.frame = Some(cframe);
                    d.mods = cmods;
                    d.age += cdt;
                    d.updates += 1;
                    d.stopped |= self.stopping;
                    d.running(ddef)
                }
                ChildState::GameModel(g) => {
                    let gdef = &lib.dir(g.dir).game_models[g.index as usize];
                    g.frame = Some(cframe);
                    g.mods = cmods;
                    g.age += cdt;
                    g.stopped |= self.stopping;
                    g.running(gdef)
                }
                ChildState::Sound(snd) => {
                    let sdef = &lib.dir(snd.dir).sounds[snd.index as usize];
                    snd.age += cdt;
                    snd.stopped |= self.stopping;
                    ctx.sounds.report(super::sound::LiveSound {
                        effect: ctx.entity,
                        voice: snd.uid,
                        flags: sdef.flags,
                        sound: sdef.sound.instance,
                        volume: if sdef.flags & sound_flag::VOLUME != 0 { sdef.volume } else { 1.0 },
                        period: if sdef.inv_sample > 0.0 { sdef.inv_sample } else { super::sound::DEFAULT_PERIOD },
                        origin: cframe.origin,
                        stopped: snd.stopped,
                    });
                    false
                }
                ChildState::Inert(_) => false,
            };
            if child.retired && !running {
                child.finished = true;
            }
            if running && !child.ignore_length {
                alive = true;
            }
        }
        if self.children.iter().any(|c| c.finished) {
            self.children.retain(|c| !c.finished);
        }
        if self.stopping && def.flags & eflag::HARD_STOP != 0 {
            return false;
        }
        alive
    }

    /// Per-frame LOD: pick it from the camera and switch the blocks when it
    /// changes (retail world update 0xA722C6..0xA725F8).
    fn update_lod(&mut self, ctx: &mut SimContext, def: &VisualEffectDef, frame: &Frame) {
        let (lod, (range, lerp)) = self.pick_lod(ctx, def, Some(frame));
        if lod != self.lod {
            self.set_lod(ctx, lod);
        }
        if let Some(range) = range {
            self.lod_range = range;
        }
        self.lod_lerp = lerp;
    }

    fn run_preroll(&mut self, ctx: &mut SimContext, frame: Frame, mods: Mods) {
        let lib = ctx.lib;
        let mut longest = 0f32;
        for child in &self.children {
            match &child.state {
                ChildState::Particles(s) => {
                    longest = longest.max(lib.dir(s.dir).particles[s.index as usize].preroll)
                }
                ChildState::Meta(m) => longest = longest.max(lib.dir(m.dir).metas[m.index as usize].preroll),
                _ => {}
            }
        }
        if longest <= 0.0 {
            return;
        }
        let steps = ((longest / 0.05).ceil() as usize).clamp(1, 120);
        let dt = longest / steps as f32;
        self.preroll = false;
        for _ in 0..steps {
            self.update(ctx, dt, frame, mods);
        }
        self.age = 0.0;
    }

    /// Visit every particle system in the tree with its definition.
    pub fn for_each_particles<'a>(&'a self, lib: &'a SwarmLibrary, f: &mut dyn FnMut(&'a ParticleSystem, &'a ParticleDef)) {
        for child in &self.children {
            match &child.state {
                ChildState::Particles(s) => f(s, &lib.dir(s.dir).particles[s.index as usize]),
                ChildState::Meta(m) => {
                    for p in &m.particles {
                        if let Some(c) = &p.child {
                            c.for_each_particles(lib, f);
                        }
                    }
                }
                ChildState::Effect(e) => e.for_each_particles(lib, f),
                ChildState::Sequence(s) => {
                    for c in s.children.iter().flatten() {
                        c.for_each_particles(lib, f);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn for_each_model<'a>(&'a self, lib: &'a SwarmLibrary, f: &mut dyn FnMut(&'a ModelState, &'a ModelDef)) {
        for child in &self.children {
            match &child.state {
                ChildState::Model(m) => f(m, &lib.dir(m.dir).models[m.index as usize]),
                ChildState::Meta(meta) => {
                    for p in &meta.particles {
                        if let Some(c) = &p.child {
                            c.for_each_model(lib, f);
                        }
                    }
                }
                ChildState::Effect(e) => e.for_each_model(lib, f),
                ChildState::Sequence(s) => {
                    for c in s.children.iter().flatten() {
                        c.for_each_model(lib, f);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn for_each_screen<'a>(&'a self, lib: &'a SwarmLibrary, f: &mut dyn FnMut(&'a ScreenState, &'a ScreenDef)) {
        for child in &self.children {
            match &child.state {
                ChildState::Screen(s) => f(s, &lib.dir(s.dir).screens[s.index as usize]),
                ChildState::Meta(meta) => {
                    for p in &meta.particles {
                        if let Some(c) = &p.child {
                            c.for_each_screen(lib, f);
                        }
                    }
                }
                ChildState::Effect(e) => e.for_each_screen(lib, f),
                ChildState::Sequence(s) => {
                    for c in s.children.iter().flatten() {
                        c.for_each_screen(lib, f);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn for_each_ribbon<'a>(&'a self, lib: &'a SwarmLibrary, f: &mut dyn FnMut(&'a RibbonState, &'a RibbonDef)) {
        for child in &self.children {
            match &child.state {
                ChildState::Ribbon(r) => f(r, &lib.dir(r.dir).ribbons[r.index as usize]),
                ChildState::Meta(meta) => {
                    for p in &meta.particles {
                        if let Some(c) = &p.child {
                            c.for_each_ribbon(lib, f);
                        }
                    }
                }
                ChildState::Effect(e) => e.for_each_ribbon(lib, f),
                ChildState::Sequence(s) => {
                    for c in s.children.iter().flatten() {
                        c.for_each_ribbon(lib, f);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn for_each_volume<'a>(&'a self, lib: &'a SwarmLibrary, f: &mut dyn FnMut(&'a VolumeState, &'a VolumeDef)) {
        for child in &self.children {
            match &child.state {
                ChildState::Volume(v) => f(v, &lib.dir(v.dir).volumes[v.index as usize]),
                ChildState::Meta(meta) => {
                    for p in &meta.particles {
                        if let Some(c) = &p.child {
                            c.for_each_volume(lib, f);
                        }
                    }
                }
                ChildState::Effect(e) => e.for_each_volume(lib, f),
                ChildState::Sequence(s) => {
                    for c in s.children.iter().flatten() {
                        c.for_each_volume(lib, f);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn for_each_decal<'a>(&'a self, lib: &'a SwarmLibrary, f: &mut dyn FnMut(&'a DecalState, &'a DecalDef)) {
        for child in &self.children {
            match &child.state {
                ChildState::Decal(d) => f(d, &lib.dir(d.dir).decals[d.index as usize]),
                ChildState::Meta(meta) => {
                    for p in &meta.particles {
                        if let Some(c) = &p.child {
                            c.for_each_decal(lib, f);
                        }
                    }
                }
                ChildState::Effect(e) => e.for_each_decal(lib, f),
                ChildState::Sequence(s) => {
                    for c in s.children.iter().flatten() {
                        c.for_each_decal(lib, f);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn for_each_game_model<'a>(
        &'a self,
        lib: &'a SwarmLibrary,
        f: &mut dyn FnMut(&'a GameModelState, &'a GameModelDef),
    ) {
        for child in &self.children {
            match &child.state {
                ChildState::GameModel(g) => f(g, &lib.dir(g.dir).game_models[g.index as usize]),
                ChildState::Meta(meta) => {
                    for p in &meta.particles {
                        if let Some(c) = &p.child {
                            c.for_each_game_model(lib, f);
                        }
                    }
                }
                ChildState::Effect(e) => e.for_each_game_model(lib, f),
                ChildState::Sequence(s) => {
                    for c in s.children.iter().flatten() {
                        c.for_each_game_model(lib, f);
                    }
                }
                _ => {}
            }
        }
    }

    /// Kinds of blocks that were skipped (for diagnostics).
    pub fn inert_kinds(&self, out: &mut Vec<u8>) {
        for child in &self.children {
            match &child.state {
                ChildState::Inert(k) => out.push(*k),
                ChildState::Effect(e) => e.inert_kinds(out),
                _ => {}
            }
        }
    }
}

fn update_meta(m: &mut MetaSystem, ctx: &mut SimContext, rng: &mut Rng, dt: f32, frame: Frame, mods: Mods) {
    let lib = ctx.lib;
    let def = &lib.dir(m.dir).metas[m.index as usize];
    if m.rigid {
        if let Some(prev) = m.frame.filter(|prev| *prev != frame) {
            for p in &mut m.particles {
                p.pos = frame.point(prev.inverse_point(p.pos));
            }
        }
    }
    m.frame = Some(frame);
    let component = (def.component.1 == 0)
        .then(|| lib.resolve_effect(m.dir, def.component.0))
        .flatten();
    for p in &mut m.particles {
        let t = (p.age / p.life).clamp(0.0, 1.0);
        let mut acc = frame.vector(Vec3::from_array(def.direction_forces));
        if def.radial_force != 0.0 {
            let c = frame.point(Vec3::from_array(def.radial_force_location));
            acc += (p.pos - c).normalize_or_zero() * def.radial_force * frame.scale;
        }
        p.vel += acc * dt;
        if def.drag > 0.0 {
            p.vel *= (-def.drag * dt).exp();
        }
        p.pos += p.vel * dt;
        if def.has(mflag::LOOP_BOX) {
            p.pos = wrap_loop_box(&frame, &def.emit_volume, p.pos);
        }
        p.age += dt;
        let turns = |values: &[f32], vary: f32, offset: f32, r: f32| {
            (curve(values, t, 0.0) + offset + vary * r) * TAU
        };
        let yaw = turns(&def.yaw, def.yaw_vary, def.yaw_offset, p.r[2]);
        let pitch = turns(&def.pitch, def.pitch_vary, def.pitch_offset, p.r[3]);
        let roll = turns(&def.roll, def.roll_vary, def.roll_offset, p.r[4]);
        let orient = Mat3::from_rotation_z(yaw) * Mat3::from_rotation_x(pitch) * Mat3::from_rotation_y(roll);
        let size = curve(&def.size, t, 1.0) * (1.0 + def.size_vary * p.r[0]) * mods.size;
        let alpha = (curve(&def.alpha, t, 1.0) * (1.0 + def.alpha_vary * p.r[1])).clamp(0.0, 1.0);
        let color = curve3(&def.color, t) + Vec3::from_array(def.color_vary) * p.r[5];
        let cframe = Frame {
            origin: p.pos,
            rot: frame.rot * orient,
            scale: frame.scale * size.max(0.0),
        };
        let cmods = Mods {
            alpha: mods.alpha * alpha,
            size: 1.0,
            emit: mods.emit,
            color: mods.color * color.max(Vec3::ZERO),
        };
        if p.age >= p.life {
            if let Some(c) = &mut p.child {
                if !c.stopping {
                    c.stop();
                }
            }
        }
        if let Some(c) = &mut p.child {
            if !c.update(ctx, dt, cframe, cmods) {
                p.child = None;
            }
        }
    }
    m.particles.retain(|p| p.age < p.life || p.child.is_some());
    let alive = m.particles.iter().filter(|p| p.age < p.life).count();
    let step = m.emitter.step(&EmitParams::meta(def), rng, dt, alive, mods.emit, m.stopped);
    if step.kill_all {
        for p in &mut m.particles {
            if let Some(c) = &mut p.child {
                c.stop();
            }
            p.age = p.life;
        }
    }
    for _ in 0..step.spawn.min(256) {
        let local = source_point(&def.emit_volume, def.torus_width, def.has(mflag::SOURCE_ROUND), rng);
        m.particles.push(MetaParticle {
            pos: frame.point(local),
            vel: frame.vector(emit_velocity(&def.emit_direction, def.emit_speed, rng)),
            age: 0.0,
            life: rng.range(def.life).max(1e-3),
            r: std::array::from_fn(|_| rng.signed()),
            child: component.map(|e| EffectInstance::new(e, rng.next_u32() as u64, false)),
        });
    }
}

/// Static ribbons follow the effect's path parameter (a straight line along
/// the effect's +Y axis without one); trailing ribbons keep the origin's
/// recent positions, one per segment length.
fn update_ribbon_points(r: &mut RibbonState, def: &RibbonDef, path: &[Vec3], frame: Frame) {
    let count = def.segment_count.clamp(1, 256) as usize;
    let seg = def.segment_length.max(1e-4) * frame.scale;
    if def.flags & ribbon_flag::STATIC != 0 {
        r.points.clear();
        if path.len() >= 2 {
            r.points.extend_from_slice(path);
        } else {
            r.points.push(frame.origin);
            r.points.push(frame.point(Vec3::new(0.0, def.segment_length * count as f32, 0.0)));
        }
        return;
    }
    if r.points.is_empty() {
        r.points.push(frame.origin);
        r.points.push(frame.origin);
    }
    r.points[0] = frame.origin;
    if r.points.len() >= 2 && r.points[0].distance(r.points[1]) > seg {
        r.points.insert(0, frame.origin);
    }
    r.points.truncate(count + 1);
}

/// Run a sequence (effect component type 4).
///
/// Retail data (VERIFIED, installed effdirs): the entries are a script played
/// in order, not independent start times. `wait t` entries carry no effect,
/// `play E t` plays E for t seconds (`play E` without a time plays it until it
/// ends by itself), e.g. the editor beacons loop 1.5 s per effect, `g2c_comet`
/// chains its six stages (the effects carry the stage game messages in order)
/// and the lamp blinkers read `wait 1.85, play 0.5, wait 0.3, play 0.5, ...`.
/// The old player started every entry at its time value and stopped it at the
/// same value, so every timed entry was cut off the frame it began (the comet
/// never appeared).
/// ADAPTED (the native sequence instance was not located in the code image):
/// a `[a, b]` range is a random duration; the effect is stopped softly when its
/// time is up and the next entry begins at once (`overlap` is the default);
/// `hardStop` / `noOverlap` (4) removes the stopped effect immediately;
/// `noStop` / `overlap` (8) never stops it; `hardStart` (2) has no visible
/// effect here; `loop` (1) restarts the script after the last entry.
fn update_sequence(
    seq: &mut SequenceState,
    ctx: &mut SimContext,
    rng: &mut Rng,
    dt: f32,
    frame: Frame,
    mods: Mods,
    stopping: bool,
) -> bool {
    let lib = ctx.lib;
    let def = &lib.dir(seq.dir).sequences[seq.index as usize];
    seq.age += dt;
    if stopping {
        seq.done = true;
        seq.step = None;
    }
    // Advance the script by `dt`, starting the entries that come due.
    let mut budget = dt;
    let mut guard = 0;
    while !seq.done {
        guard += 1;
        if guard > 64 {
            break;
        }
        if seq.step.is_none() {
            if seq.cursor >= def.instances.len() {
                if def.flags & seqflag::LOOP != 0 && !def.instances.is_empty() {
                    seq.cursor = 0;
                } else {
                    seq.done = true;
                    break;
                }
            }
            let (range, effect) = def.instances[seq.cursor];
            let duration = |rng: &mut Rng| (range[0] >= 0.0).then(|| rng.range([range[0], range[1].max(range[0])]));
            let step = match lib.resolve_effect(seq.dir, effect) {
                Some(e) => {
                    let instance = EffectInstance::new(e, rng.next_u32() as u64, false);
                    let slot = match seq.children.iter().position(Option::is_none) {
                        Some(i) => {
                            seq.children[i] = Some(instance);
                            i
                        }
                        None => {
                            seq.children.push(Some(instance));
                            seq.children.len() - 1
                        }
                    };
                    SequenceStep { slot: Some(slot), left: duration(rng) }
                }
                // `wait` (no effect) or an effect that is not installed.
                None => SequenceStep { slot: None, left: Some(duration(rng).unwrap_or(0.0)) },
            };
            seq.step = Some(step);
        }
        let step = seq.step.as_mut().unwrap();
        let Some(left) = step.left else { break };
        if left > budget {
            step.left = Some(left - budget);
            break;
        }
        // The entry's time is up.
        budget -= left;
        let slot = step.slot;
        seq.step = None;
        seq.cursor += 1;
        if let Some(i) = slot {
            if def.flags & seqflag::NO_STOP == 0 {
                if def.flags & seqflag::HARD_STOP != 0 {
                    seq.children[i] = None;
                } else if let Some(c) = &mut seq.children[i] {
                    c.stop();
                }
            }
        }
    }
    // Run the instances; an entry that plays "until it ends" is over once its
    // effect has finished.
    let mut alive = !seq.done;
    for i in 0..seq.children.len() {
        let Some(c) = &mut seq.children[i] else { continue };
        if stopping && !c.stopping {
            c.stop();
        }
        if c.update(ctx, dt, frame, mods) {
            alive = true;
        } else {
            seq.children[i] = None;
            if let Some(step) = seq.step.as_mut() {
                if step.slot == Some(i) {
                    if step.left.is_none() {
                        seq.step = None;
                        seq.cursor += 1;
                    } else {
                        // Timed entry whose effect ended early: keep waiting.
                        step.slot = None;
                    }
                }
            }
        }
    }
    alive
}

/// Resolve the world frame of an effect entity.
fn effect_frame(effect: &SwarmEffect, parents: &Query<&GlobalTransform>) -> Option<Frame> {
    match effect.attach_to {
        Some(entity) => {
            let g = parents.get(entity).ok()?;
            let (scale, rotation, translation) = g.to_scale_rotation_translation();
            let rot = rotation * effect.rotation;
            Some(Frame::new(
                translation + rotation * (effect.position * scale),
                rot,
                effect.scale * scale.max_element().max(1e-6),
            ))
        }
        None => Some(Frame::new(effect.position, effect.rotation, effect.scale)),
    }
}

/// `OPENSPORE_SWARM_LOD=0` runs every effect at LOD 1 (the behaviour before
/// the retail LOD evaluator).
fn lod_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("OPENSPORE_SWARM_LOD").map_or(true, |v| v != "0"))
}

/// Advance every effect; despawn the finished ones.
#[allow(clippy::too_many_arguments)]
pub fn simulate(
    mut commands: Commands,
    time: Res<Time>,
    lib: Res<SwarmLibrary>,
    maps: Res<SwarmMaps>,
    flags: Res<SwarmAppFlags>,
    mut ids: ResMut<SwarmIds>,
    mut diagnostics: ResMut<SwarmDiagnostics>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    editor: Option<Res<crate::editor::EditorState>>,
    parents: Query<&GlobalTransform>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut effects: Query<(Entity, &mut SwarmEffect)>,
    mut shake: ResMut<super::shake::SwarmShake>,
    mut sounds: ResMut<super::sound::SwarmSounds>,
) {
    if !lib.loaded {
        return;
    }
    sounds.live.clear();
    let mut shake_sum = Vec3::ZERO;
    let paused = pause.is_some_and(|p| p.open) || editor.is_some_and(|e| e.open);
    let dt = if paused { 0.0 } else { time.delta_secs().min(0.1) };
    let camera_rot = camera
        .iter()
        .next()
        .map(|g| Mat3::from_cols(g.right().as_vec3(), g.forward().as_vec3(), g.up().as_vec3()));
    let lod_camera = camera
        .iter()
        .next()
        .map(|g| (g.translation(), g.forward().as_vec3().normalize_or_zero()));
    let camera = camera.iter().next().map(|g| g.translation());
    // ADAPTED: the retail world LOD / manager distance scale come from the
    // graphics quality settings; both stay at their constructor values.
    let lod_on = lod_enabled();
    for (entity, mut effect) in &mut effects {
        if !effect.alive {
            continue;
        }
        let Some(frame) = effect_frame(&effect, &parents) else {
            // The attached entity is gone: let the effect wind down in place.
            if let Some(instance) = &mut effect.instance {
                if !instance.stopping {
                    instance.stop();
                }
            } else {
                commands.entity(entity).despawn();
                continue;
            }
            effect.attach_to = None;
            continue;
        };
        if effect.instance.is_none() {
            let Some(found) = lib.find(effect.effect) else {
                if diagnostics.missing.insert(effect.effect) {
                    warn!("Swarm effect {:08x} is not installed", effect.effect);
                }
                effect.alive = false;
                if effect.despawn_when_done {
                    commands.entity(entity).despawn();
                }
                continue;
            };
            diagnostics.started += 1;
            let seed = (effect.effect as u64) << 32 | (diagnostics.started & 0xFFFF_FFFF);
            effect.instance = Some(EffectInstance::new(found, seed, effect.preroll));
        }
        let mods = Mods {
            alpha: effect.alpha * effect.params[PARAM_INTENSITY].map_or(1.0, |p| p.x),
            size: 1.0,
            emit: effect.emit,
            color: effect.tint,
        };
        let stop = effect.stop;
        let hard = effect.hard_stop;
        let params = effect.params;
        let (tag, id) = (effect.tag, effect.effect);
        let path = effect.path.clone();
        let layer = effect.layer;
        let mut ctx = SimContext {
            lib: &lib,
            maps: &maps,
            app_flags: flags.0,
            ids: &mut ids.0,
            camera,
            camera_rot,
            params,
            path: &path,
            shake: &mut shake_sum,
            entity,
            sounds: &mut sounds,
            // Effects drawn through another camera (background worlds) keep
            // LOD 1: the main camera is not their viewer.
            lod: LodView {
                camera: if lod_on && layer == 0 { lod_camera } else { None },
                ..LodView::OFF
            },
        };
        let instance = effect.instance.as_mut().unwrap();
        if stop && !instance.stopping {
            instance.stop();
        }
        let alive = !hard && instance.update(&mut ctx, dt, frame, mods);
        if std::env::var_os("OPENSPORE_FX_DEBUG").is_some() && tag == 0 {
            let mut counts = vec![];
            instance.for_each_particles(&lib, &mut |s, _| {
                counts.push((s.index, s.particles.len(), s.frame.map(|f| (f.origin, f.scale))))
            });
            let mut inert = vec![];
            instance.inert_kinds(&mut inert);
            let mut models = vec![];
            instance.for_each_game_model(&lib, &mut |g, d| models.push((d.model.instance, g.frame.map(|f| f.origin))));
            info!(
                "fx {id:08x} age {:.2} alive {alive} lod {} lerp {:.2} systems {counts:?} inert {inert:?} game models {models:?}",
                instance.age, instance.lod, instance.lod_lerp
            );
        }
        if !alive {
            effect.alive = false;
            effect.instance = None;
            if effect.despawn_when_done {
                commands.entity(entity).despawn();
            }
        }
    }
    shake.offset = if paused { Vec3::ZERO } else { shake_sum };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(rate: &[f32]) -> EmitParams<'_> {
        EmitParams {
            inject: false,
            maintain: false,
            hold: false,
            kill: false,
            rate,
            rate_loop: 1.0,
            cycles: 1,
            delay: [-1.0, -1.0],
            retrigger: [-1.0, -1.0],
        }
    }

    #[test]
    fn curves_interpolate_evenly_spaced_keys() {
        assert_eq!(curve(&[], 0.3, 7.0), 7.0);
        assert_eq!(curve(&[2.0], 0.9, 7.0), 2.0);
        assert!((curve(&[0.0, 1.6, 2.8, 3.6, 1.0], 0.5, 0.0) - 2.8).abs() < 1e-6);
        assert!((curve(&[0.0, 1.0], 0.25, 0.0) - 0.25).abs() < 1e-6);
        assert_eq!(curve(&[0.0, 1.0], 2.0, 0.0), 1.0);
    }

    #[test]
    fn inject_emits_once_then_finishes() {
        let rate = [6.0];
        let mut p = params(&rate);
        p.inject = true;
        p.rate_loop = 0.01;
        let mut e = Emitter::default();
        let mut rng = Rng::new(1);
        assert_eq!(e.step(&p, &mut rng, 1.0 / 60.0, 0, 1.0, false).spawn, 6);
        assert!(e.done());
        assert_eq!(e.step(&p, &mut rng, 1.0 / 60.0, 6, 1.0, false).spawn, 0);
    }

    #[test]
    fn looping_inject_repeats_every_loop() {
        let rate = [1.0];
        let mut p = params(&rate);
        p.inject = true;
        p.rate_loop = 0.3;
        p.cycles = 0;
        let mut e = Emitter::default();
        let mut rng = Rng::new(2);
        let total: usize = (0..60).map(|_| e.step(&p, &mut rng, 0.05, 0, 1.0, false).spawn).sum();
        // 3 s at one ring per 0.3 s: the first at t=0 plus ten loops.
        assert!((10..=11).contains(&total), "{total}");
        assert!(!e.done());
    }

    /// `inject 1 -hold 0.45` (the Cell unlock rock, particle life 0.5): the
    /// burst repeats every loop, so the particle overlaps its successor.
    #[test]
    fn inject_with_hold_repeats_forever() {
        let rate = [1.0];
        let mut p = params(&rate);
        p.inject = true;
        p.hold = true;
        p.rate_loop = 0.45;
        p.cycles = 1;
        let mut e = Emitter::default();
        let mut rng = Rng::new(7);
        let total: usize = (0..60).map(|_| e.step(&p, &mut rng, 0.05, 0, 1.0, false).spawn).sum();
        assert!((6..=8).contains(&total), "{total}");
        assert!(!e.done());
    }

    #[test]
    fn rate_curve_emits_its_integral() {
        let rate = [100.0];
        let p = params(&rate);
        let mut e = Emitter::default();
        let mut rng = Rng::new(3);
        let total: usize = (0..120).map(|_| e.step(&p, &mut rng, 1.0 / 60.0, 0, 1.0, false).spawn).sum();
        assert!((98..=101).contains(&total), "{total}");
        assert!(e.done());
    }

    #[test]
    fn maintain_tops_up_to_the_count() {
        let rate = [20.0];
        let mut p = params(&rate);
        p.maintain = true;
        p.cycles = 0;
        let mut e = Emitter::default();
        let mut rng = Rng::new(4);
        assert_eq!(e.step(&p, &mut rng, 0.1, 0, 1.0, false).spawn, 20);
        assert_eq!(e.step(&p, &mut rng, 0.1, 17, 1.0, false).spawn, 3);
        assert_eq!(e.step(&p, &mut rng, 0.1, 17, 0.5, false).spawn, 0);
    }

    #[test]
    fn delay_and_retrigger_gate_emission() {
        let rate = [2.0];
        let mut p = params(&rate);
        p.inject = true;
        p.rate_loop = 0.1;
        p.delay = [0.5, 0.5];
        p.retrigger = [1.0, 1.0];
        let mut e = Emitter::default();
        let mut rng = Rng::new(5);
        let mut times = vec![];
        for i in 0..100 {
            if e.step(&p, &mut rng, 0.05, 0, 1.0, false).spawn > 0 {
                times.push(i as f32 * 0.05);
            }
        }
        assert!(times.len() >= 3, "{times:?}");
        assert!((times[0] - 0.45).abs() < 0.06, "{times:?}");
        assert!((times[1] - times[0] - 1.1).abs() < 0.11, "{times:?}");
    }

    #[test]
    fn static_ribbons_follow_the_path_and_trails_keep_the_origin_history() {
        let def = RibbonDef {
            flags: ribbon_flag::STATIC,
            segment_count: 10,
            segment_length: 0.5,
            ..Default::default()
        };
        let mut r = RibbonState {
            dir: 0,
            index: 0,
            uid: 1,
            age: 0.0,
            life: 1.0,
            mods: Mods::default(),
            frame: None,
            stopped: false,
            points: vec![],
        };
        let f = Frame::new(Vec3::new(1.0, 0.0, 2.0), Quat::IDENTITY, 2.0);
        // Without a path: a straight line of segment_count * segment_length along +Y.
        update_ribbon_points(&mut r, &def, &[], f);
        assert_eq!(r.points.len(), 2);
        assert!(r.points[1].abs_diff_eq(f.point(Vec3::new(0.0, 5.0, 0.0)), 1e-5));
        // With a path: exactly the path.
        let path = [Vec3::ZERO, Vec3::X, Vec3::new(2.0, 0.0, 1.0)];
        update_ribbon_points(&mut r, &def, &path, f);
        assert_eq!(r.points, path.to_vec());
        // A trailing ribbon adds a point every segment length and keeps the head.
        let trail = RibbonDef {
            flags: 0,
            segment_count: 3,
            segment_length: 1.0,
            ..Default::default()
        };
        let mut r2 = RibbonState { points: vec![], ..r };
        for i in 0..20 {
            update_ribbon_points(&mut r2, &trail, &[], Frame::new(Vec3::new(i as f32 * 0.6, 0.0, 0.0), Quat::IDENTITY, 1.0));
        }
        assert!(r2.points.len() <= 4);
        assert!(r2.points[0].abs_diff_eq(Vec3::new(19.0 * 0.6, 0.0, 0.0), 1e-5));
        assert!(r2.points[0].distance(r2.points[1]) <= 1.2);
    }

    #[test]
    fn frames_map_spore_axes_and_block_offsets() {
        let f = Frame::new(Vec3::new(1.0, 0.0, 0.0), Quat::IDENTITY, 2.0);
        // Spore +Z (up) is Bevy +Y; Spore +Y is Bevy -Z.
        assert!(f.point(Vec3::Z).abs_diff_eq(Vec3::new(1.0, 2.0, 0.0), 1e-6));
        assert!(f.point(Vec3::Y).abs_diff_eq(Vec3::new(1.0, 0.0, -2.0), 1e-6));
        let block = BlockTransform {
            flags: 4,
            scale: 1.0,
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            offset: [0.0, 0.0, -5.0],
        };
        let c = f.child(&block);
        assert!(c.origin.abs_diff_eq(Vec3::new(1.0, -10.0, 0.0), 1e-6));
        assert!(f.inverse_point(f.point(Vec3::new(0.3, -2.0, 4.0))).abs_diff_eq(Vec3::new(0.3, -2.0, 4.0), 1e-5));
    }

    #[test]
    fn loop_box_wraps_into_the_source_box() {
        let f = Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0);
        let volume = [[-15.0, -12.0, -2.0], [15.0, 12.0, 2.0]];
        let wrapped = wrap_loop_box(&f, &volume, f.point(Vec3::new(20.0, -13.0, 0.0)));
        let local = f.inverse_point(wrapped);
        assert!(local.abs_diff_eq(Vec3::new(-10.0, 11.0, 0.0), 1e-4), "{local}");
        let fade = loop_box_fade(&f, &volume, &[0.0, 1.0, 1.0, 0.0], f.point(Vec3::ZERO));
        assert!((fade - 1.0).abs() < 1e-6);
        let edge = loop_box_fade(&f, &volume, &[0.0, 1.0, 1.0, 0.0], f.point(Vec3::new(14.9, 0.0, 0.0)));
        assert!(edge < 0.05, "{edge}");
    }

    // ---- LOD evaluator (retail world update 0xA722C6, switch 0xA719A0) ----

    fn lod_def(flags: u32, weights: [f32; 3], distances: &[f32], blocks: Vec<EffectBlock>) -> VisualEffectDef {
        VisualEffectDef {
            flags,
            app_flags_mask: 0,
            notify: 0,
            screen_size_range: [0.0; 2],
            cursor_active_distance: 0.0,
            cursor_button: 0,
            lod_distances: distances.to_vec(),
            extended_lod_weights: weights,
            seed: 0,
            blocks,
        }
    }

    fn volume_block(index: i32, lod: (u8, u8), scales: &[[f32; 3]]) -> EffectBlock {
        EffectBlock {
            kind: kind::VOLUME,
            flags: 0,
            transform: BlockTransform {
                flags: 0,
                scale: 1.0,
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                offset: [0.0; 3],
            },
            lod_begin: lod.0,
            lod_end: lod.1,
            lod_scales: scales.to_vec(),
            emit_scale: [1.0; 2],
            size_scale: [1.0; 2],
            alpha_scale: [1.0; 2],
            app_flags: 0,
            app_flags_mask: 0,
            selection_group: 0,
            selection_chance: 0,
            time_scale: 1.0,
            index,
        }
    }

    #[test]
    fn lod_select_picks_the_interval_and_the_position_in_it() {
        // civ_selection_vehicle: lodDistances 0 .25 .45 .5 .75 (4 LODs).
        let d = [0.0, 0.25, 0.45, 0.5, 0.75];
        let (lod, range, lerp) = lod_select(&d, 0.07);
        assert_eq!((lod, range), (1, [0.0, 0.25]));
        assert!((lerp - 0.28).abs() < 1e-5);
        assert_eq!(lod_select(&d, 0.25).0, 2);
        assert_eq!(lod_select(&d, 0.47).0, 3);
        let (lod, range, lerp) = lod_select(&d, 0.6);
        assert_eq!((lod, range), (4, [0.5, 0.75]));
        assert!((lerp - 0.4).abs() < 1e-5);
        // Past the last distance the last LOD stays at lerp 1.
        assert_eq!(lod_select(&d, 9.0), (4, [0.5, 0.75], 1.0));
        // Two distances: one LOD whose lerp runs across them (1 .. 100).
        assert_eq!(lod_select(&[1.0, 100.0], 0.0), (1, [1.0, 100.0], 0.0));
        assert_eq!(lod_select(&[1.0, 100.0], 500.0), (1, [1.0, 100.0], 1.0));
        // Equal distances divide by zero: NaN -> 0 (0xA72603).
        assert_eq!(lod_lerp([3.0, 3.0], 3.0), 0.0);
    }

    #[test]
    fn lod_value_follows_the_extended_weights() {
        let cam = Vec3::new(0.0, 10.0, 0.0);
        let up = Vec3::Y;
        let plain = lod_def(0, [0.0; 3], &[0.0, 1.0], vec![]);
        assert!((lod_value(&plain, cam, Vec3::NEG_Y, Vec3::ZERO, up) - 10.0).abs() < 1e-5);
        // Facing: camera on the effect's up axis -> 0, at the horizon -> 0.5, below -> 1.
        let facing = lod_def(eflag::EXTENDED_LOD_DISTANCES, [0.0, 1.0, 0.0], &[0.0, 1.0], vec![]);
        assert!(lod_value(&facing, cam, Vec3::NEG_Y, Vec3::ZERO, up).abs() < 1e-4);
        let side = Vec3::new(10.0, 0.0, 0.0);
        assert!((lod_value(&facing, side, Vec3::NEG_X, Vec3::ZERO, up) - 0.5).abs() < 1e-4);
        assert!((lod_value(&facing, -cam, Vec3::Y, Vec3::ZERO, up) - 1.0).abs() < 1e-4);
        // Focus: 0 when the camera looks straight at the effect, 1 when it looks away.
        let focus = lod_def(eflag::EXTENDED_LOD_DISTANCES, [1.0, 0.0, 0.0], &[0.0, 1.0], vec![]);
        assert!(lod_value(&focus, cam, Vec3::NEG_Y, Vec3::ZERO, up).abs() < 1e-4);
        assert!((lod_value(&focus, cam, Vec3::Y, Vec3::ZERO, up) - 1.0).abs() < 1e-4);
        // Distance weight scales the distance.
        let dist = lod_def(eflag::EXTENDED_LOD_DISTANCES, [0.0, 0.0, 0.01], &[0.0, 1.0], vec![]);
        assert!((lod_value(&dist, cam, Vec3::NEG_Y, Vec3::ZERO, up) - 0.1).abs() < 1e-4);
    }

    #[test]
    fn block_range_excludes_its_end_and_scales_pair_up() {
        // `lodRange 1 4` with four scale entries: active at LODs 1..3, the
        // last entry is what the block reaches when it leaves.
        let scales = [[1.0, 1.0, 0.05], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]];
        let b = volume_block(0, (1, 4), &scales);
        assert!(block_in_lod(&b, 1) && block_in_lod(&b, 3));
        assert!(!block_in_lod(&b, 4) && !block_in_lod(&b, 0));
        assert_eq!(block_scales(&b, 1), (scales[0], scales[1]));
        assert_eq!(block_scales(&b, 3), (scales[2], scales[3]));
        assert_eq!(block_scales(&volume_block(0, (1, 255), &[]), 2), ([1.0; 3], [1.0; 3]));
        assert_eq!(block_scales(&volume_block(0, (1, 255), &scales[..1]), 2), (scales[0], scales[0]));
    }

    /// A two-block effect with `lodDistances 0 10 20 30`: block 0 for LODs 1..2,
    /// block 1 for LODs 2..3. The camera distance walks through the LODs.
    #[test]
    fn instance_switches_blocks_with_the_camera_distance() {
        let fade = [[1.0, 1.0, 1.0], [1.0, 1.0, 0.5], [1.0, 1.0, 0.0]];
        let mut dir = EffectDirectory::default();
        dir.effects.push(lod_def(
            0,
            [0.0; 3],
            &[0.0, 10.0, 20.0, 30.0],
            vec![volume_block(0, (1, 3), &fade), volume_block(1, (2, 4), &fade)],
        ));
        let lib = SwarmLibrary { dirs: vec![dir], loaded: true, ..Default::default() };
        let maps = SwarmMaps::default();
        let mut ids = 0u64;
        let mut shake = Vec3::ZERO;
        let mut sounds = super::super::sound::SwarmSounds::default();
        let mut instance = EffectInstance::new(EffectRef { dir: 0, index: 0 }, 1, false);
        let mut at = |instance: &mut EffectInstance, distance: f32| {
            let mut ctx = SimContext {
                lib: &lib,
                maps: &maps,
                app_flags: 0,
                ids: &mut ids,
                camera: None,
                camera_rot: None,
                params: [None; 16],
                path: &[],
                shake: &mut shake,
                entity: Entity::PLACEHOLDER,
                sounds: &mut sounds,
                lod: LodView {
                    camera: Some((Vec3::new(0.0, 0.0, distance), Vec3::NEG_Z)),
                    ..LodView::OFF
                },
            };
            instance.update(&mut ctx, 0.05, Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0), Mods::default());
            instance.children.iter().map(|c| c.block).collect::<Vec<_>>()
        };
        // LOD 1 (0..10): only block 0, at lerp 0.5.
        assert_eq!(at(&mut instance, 5.0), vec![0]);
        assert_eq!((instance.lod, instance.lod_lerp), (1, 0.5));
        // LOD 2 (10..20): block 0 stays, block 1 starts.
        assert_eq!(at(&mut instance, 15.0), vec![0, 1]);
        assert_eq!(instance.lod, 2);
        let scale = instance.children[0].lod_scale(instance.lod_lerp);
        assert!((scale[2] - 0.25).abs() < 1e-5, "{scale:?}");
        // LOD 3 (20..30): block 0 leaves (its volume stops at once), block 1 stays.
        assert_eq!(at(&mut instance, 25.0), vec![1]);
        assert_eq!(instance.lod, 3);
        // Back to LOD 1: block 1 leaves, block 0 returns.
        assert_eq!(at(&mut instance, 1.0), vec![0]);
        // No camera: LOD 1 whatever the distance.
        let mut flat = EffectInstance::new(EffectRef { dir: 0, index: 0 }, 2, false);
        let mut ctx = SimContext {
            lib: &lib,
            maps: &maps,
            app_flags: 0,
            ids: &mut ids,
            camera: None,
            camera_rot: None,
            params: [None; 16],
            path: &[],
            shake: &mut shake,
            entity: Entity::PLACEHOLDER,
            sounds: &mut sounds,
            lod: LodView::OFF,
        };
        flat.update(&mut ctx, 0.05, Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0), Mods::default());
        assert_eq!(flat.lod, 1);
    }

    // ---- sequences: an ordered play / wait script ----

    /// Effect 0 = a volume (runs until stopped), effect 1 = an effect that ends
    /// at once (an inert block), effect 2 = the sequence block `seq`.
    fn sequence_lib(seq: SequenceDef) -> SwarmLibrary {
        let mut dir = EffectDirectory::default();
        dir.effects.push(lod_def(0, [0.0; 3], &[], vec![volume_block(0, (1, 255), &[])]));
        let mut inert = volume_block(0, (1, 255), &[]);
        inert.kind = kind::SOUND;
        inert.index = 99;
        dir.effects.push(lod_def(0, [0.0; 3], &[], vec![inert]));
        let mut block = volume_block(0, (1, 255), &[]);
        block.kind = kind::SEQUENCE;
        dir.effects.push(lod_def(0, [0.0; 3], &[], vec![block]));
        dir.sequences.push(seq);
        SwarmLibrary { dirs: vec![dir], loaded: true, ..Default::default() }
    }

    /// Step the sequence effect (index 2) in 0.05 s ticks; per tick (alive, live instances).
    fn run_sequence(lib: &SwarmLibrary, seconds: f32) -> Vec<(bool, usize)> {
        let maps = SwarmMaps::default();
        let mut ids = 0u64;
        let mut shake = Vec3::ZERO;
        let mut sounds = super::super::sound::SwarmSounds::default();
        let mut instance = EffectInstance::new(EffectRef { dir: 0, index: 2 }, 7, false);
        let mut out = vec![];
        for _ in 0..(seconds / 0.05).round() as usize {
            let mut ctx = SimContext {
                lib,
                maps: &maps,
                app_flags: 0,
                ids: &mut ids,
                camera: None,
                camera_rot: None,
                params: [None; 16],
                path: &[],
                shake: &mut shake,
                entity: Entity::PLACEHOLDER,
                sounds: &mut sounds,
                lod: LodView::OFF,
            };
            let alive = instance.update(&mut ctx, 0.05, Frame::new(Vec3::ZERO, Quat::IDENTITY, 1.0), Mods::default());
            let live = instance
                .children
                .iter()
                .map(|c| match &c.state {
                    ChildState::Sequence(s) => s.children.iter().flatten().count(),
                    _ => 0,
                })
                .sum();
            out.push((alive, live));
        }
        out
    }

    /// `wait .5, play V 1, wait .25, play V 1` (the shape of `g2c_comet`'s chain and
    /// the lamp blinkers): each entry begins when the one before it ends.
    #[test]
    fn sequence_plays_its_entries_in_order() {
        let lib = sequence_lib(SequenceDef {
            instances: vec![([0.5, 0.5], -1), ([1.0, 1.0], 0), ([0.25, 0.25], -1), ([1.0, 1.0], 0)],
            flags: 0,
        });
        let ticks = run_sequence(&lib, 3.5);
        let at = |t: f32| ticks[(t / 0.05).round() as usize - 1];
        // Waiting: the sequence is alive but nothing plays yet.
        assert_eq!(at(0.3), (true, 0));
        // First play (0.5 .. 1.5).
        assert_eq!(at(1.0), (true, 1));
        // The second wait (1.5 .. 1.75): the first play was stopped and finished.
        assert_eq!(at(1.65), (true, 0));
        // Second play (1.75 .. 2.75).
        assert_eq!(at(2.2), (true, 1));
        // The script is over and nothing lingers: the sequence ends.
        assert_eq!(at(3.4), (false, 0));
    }

    /// `play E` without a time plays until E ends by itself, then the next entry
    /// begins; `loop` starts the script again.
    #[test]
    fn sequence_waits_for_untimed_plays_and_loops() {
        let lib = sequence_lib(SequenceDef { instances: vec![([-1.0, -1.0], 1), ([0.3, 0.3], 0)], flags: 0 });
        let ticks = run_sequence(&lib, 1.0);
        // The inert effect ends at once, then the volume plays for 0.3 s.
        assert!(ticks.iter().take(3).any(|t| t.1 == 1), "{ticks:?}");
        assert_eq!(ticks[19], (false, 0), "{ticks:?}");
        let looped = sequence_lib(SequenceDef { instances: vec![([0.3, 0.3], 0)], flags: seqflag::LOOP });
        let ticks = run_sequence(&looped, 3.0);
        assert!(ticks.iter().all(|t| t.0), "a looping sequence never ends");
        assert!(ticks.iter().filter(|t| t.1 == 1).count() > 40, "{ticks:?}");
    }

    /// `noStop` (overlap) never stops the played effect, `hardStop` and the default
    /// both end a volume when its time is up.
    #[test]
    fn sequence_no_stop_keeps_the_effect_running() {
        let stop = sequence_lib(SequenceDef { instances: vec![([0.5, 0.5], 0)], flags: seqflag::HARD_STOP });
        let keep = sequence_lib(SequenceDef { instances: vec![([0.5, 0.5], 0)], flags: seqflag::NO_STOP });
        let a = run_sequence(&stop, 1.0);
        let b = run_sequence(&keep, 1.0);
        assert_eq!(a[5], (true, 1));
        assert_eq!(a[19], (false, 0), "{a:?}");
        assert_eq!(b[19], (true, 1), "{b:?}");
    }
}
