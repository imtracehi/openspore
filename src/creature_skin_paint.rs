//! Skin paint (Skinner): the installed `.pfx` brush scripts and a CPU painter
//! onto the 512x512 skin paint texture (`notes/skin-re-notes.md` section 2).
//!
//! VERIFIED data: a creature's paint is three brush effects (base, coat,
//! detail), plain-text Swarm scripts `skinpaint_<layer>_<name>` (type `pfx`
//! 0x497767B9, group `gameEffects_pfx~` 0xEA5118B1) built from
//! `SPSkinPaintClear` (base fill), `SPSkinPaintSettings` (material),
//! `SPSkinPaintDistribute` (where stamps go), `SPSkinPaintParticle` (one brush
//! stamp, its channels and `modifier` blocks), `SPSkinPaintFlood` (per-texel
//! wash), `sequence` / `play`, `define ... enddef` macros with `create`.
//! Keyword semantics follow SporeModder-FX `effects/Skinpaint*Effect.java`
//! (channels, modifier sources, `-set -add -mult -div`, `-range`, `-wrap`,
//! `-clamp2`, `-vary`). Brush textures `<brush>__diffuse` / `__specBump` are
//! RW4 rasters in group 0x406A2100 (diffuse: white RGB + mask alpha; specBump:
//! R spec, B bump, A mask). Pipeline (cSkinPainterPipeline 0x51AFE0): layers
//! base -> coat -> detail, diffuse / spec / bump passes with the alpha, add,
//! subtract, multiply, screen blends, colour dilate 2 x 8, bump -> normal with
//! height `skinpaintBumpHeight / 3` = 5/3.
//!
//! ADAPTED (the retail particle simulation runs in the Swarm effects world and
//! is not traced): stamps are painted in 3D (every texel knows its surface
//! point, a stamp covers the texels within its square on the tangent plane),
//! distribute samples are area-stratified, crawling particles move at
//! [`MOVE_SPEED`] along / around the spine or bone, the Swarm random generator
//! is a SplitMix64, identity-colour stamps (tribal identity) are not shown,
//! `-back` / `-belly` split at 90 degrees from the back, no ambient occlusion.
//! A baked skin (`paint_on` with `project`, the Cell body and the Cell parts'
//! charts) places its particles with retail's triangle-order algorithm and
//! retail's random generators instead (`Painter::samples_mesh`,
//! `cSkinpaintDistributeEffect::ApplyEffect` 0x5314B0, `RetailRng`, VERIFIED).

// The test-vector loaders and the records' limb data serve the tests and the
// later limb / Test Drive work.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;

use bevy::math::Vec3;

use crate::creature_skin::{chain_param, chain_tangent, Primitives, Unwrapped, PAINT_SIZE};

/// Brush script resources.
pub const PFX_TYPE: u32 = 0x4977_67B9;
pub const PFX_GROUP: u32 = 0xEA51_18B1;
/// Brush textures (RW4 raster).
pub const BRUSH_GROUP: u32 = 0x406A_2100;
pub const RASTER_TYPE: u32 = 0x2F4E_681B;
/// `skinpaintBumpHeight` (5) / 3 of `skpBumpToNormal`.
pub const BUMP_TO_NORMAL: f32 = 5.0 / 3.0;
/// Colour dilate passes x steps (pipeline stages 7 / 8).
pub const DILATE_STEPS: usize = 16;
/// Surface speed of crawling brush particles (units / s). VERIFIED for the Cell
/// paint: the simulation job (0x5248B0 -> 0x520300) advances every particle
/// 0.02 units per 1/50 s tick (`0x15DE718`, set to 0.02 by the static
/// initialiser 0x125DC20) and `life` seconds are `trunc(life * 50)` ticks, i.e.
/// 1.0 unit per second of life. Creatures keep the same figure (ADAPTED there).
pub const MOVE_SPEED: f32 = 1.0;
/// One simulation tick of the Skinner particle job: 0.02 units.
pub const TICK_STEP: f32 = 0.02;
/// Default seeds of the startup creatures (`B_00.crt` `skineffectseed1..3`).
pub const DEFAULT_SEEDS: [u32; 3] = [4660, 6_435_736, 6_435_736];

// ---------------------------------------------------------------------------
// Script model
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Blend {
    #[default]
    Alpha,
    Add,
    Subtract,
    Multiply,
    Screen,
}

/// `diffuseColor`: one of the creature's three colours, the identity colour or
/// an explicit colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorRef {
    Layer(usize),
    Identity,
    Rgb([f32; 3]),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Op {
    #[default]
    Set,
    Add,
    Mult,
    Div,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RangeFlag {
    /// Applies only while the source is inside the range.
    #[default]
    Inside,
    Open,
    Clamp,
    Wrap,
    Mirror,
}

/// `LIST_MODIFIER_CHANNELS` (lower case).
pub const CHANNELS: [&str; 19] = [
    "size", "aspect", "rotation", "spacing", "killswitch", "hairradius", "alpha", "diffusehue", "diffusesat", "diffuseval",
    "diffusealpha", "bumpscale", "bumpalpha", "specularscale", "specularalpha", "uservar1", "uservar2", "uservar3", "uservar4",
];

pub mod ch {
    pub const SIZE: usize = 0;
    pub const ASPECT: usize = 1;
    pub const ROTATION: usize = 2;
    pub const SPACING: usize = 3;
    pub const KILL: usize = 4;
    pub const ALPHA: usize = 6;
    pub const HUE: usize = 7;
    pub const SAT: usize = 8;
    pub const VAL: usize = 9;
    pub const DIFFUSE_ALPHA: usize = 10;
    pub const BUMP_SCALE: usize = 11;
    pub const BUMP_ALPHA: usize = 12;
    pub const SPEC_SCALE: usize = 13;
    pub const SPEC_ALPHA: usize = 14;
}

/// Channel values before any variable (ADAPTED where the defaults are not
/// dumped: size 1, alphas / scales 1, particle spacing 0.05).
const CHANNEL_BASE: [f32; 19] = [1.0, 1.0, 0.0, 0.05, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0];

fn channel(name: &str) -> Option<usize> {
    let n = name.to_ascii_lowercase();
    match n.as_str() {
        "rotate" => Some(ch::ROTATION),
        "varyhue" => Some(ch::HUE),
        "varysat" => Some(ch::SAT),
        "varyval" => Some(ch::VAL),
        _ => CHANNELS.iter().position(|c| *c == n),
    }
}

/// `LIST_MODIFIER_TYPES` (+ a channel as source).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Age,
    Random,
    WorldAngle(Vec3),
    WorldPos(Vec3),
    WorldDist(Vec3),
    BoneAngle,
    BonePos,
    BoneOffset,
    TorsoPos,
    LimbPos,
    LimbType,
    Region,
    PaintMask,
    Channel(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub channel: usize,
    pub op: Op,
    pub values: Vec<f32>,
    pub vary: f32,
    /// Index into `Particle::modifiers`; `None` = constant or curve over age.
    pub modifier: Option<usize>,
    pub range: (f32, f32),
    pub flag: RangeFlag,
    /// A curve over the particle's age (several values or `-vary`).
    pub curve: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionKind {
    Dir,
    Attract,
    AroundBone,
    AlongBone,
    AroundSpine,
    AlongSpine,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub kind: MotionKind,
    pub vector: Vec3,
    pub reverse: bool,
}

/// `SPSkinPaintParticle` (also the channel set of `SPSkinPaintFlood`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Particle {
    pub brushes: Vec<String>,
    pub life: (f32, f32),
    pub delay: (f32, f32),
    pub color: Option<ColorRef>,
    pub diffuse_blend: Blend,
    pub specular_blend: Blend,
    pub bump_blend: Blend,
    pub align: Option<Motion>,
    pub attract: Option<Motion>,
    /// `attractForce` (description +0x54): the fraction of the way the travel
    /// direction turns towards the attract target on every 1/50 s tick
    /// (binary Cell particles; 0 = none).
    pub attract_force: f32,
    pub init_dir: Option<Motion>,
    pub chain: Option<String>,
    pub inherit: Vec<usize>,
    pub modifiers: Vec<Source>,
    pub vars: Vec<Variable>,
}

pub mod region {
    pub const TORSO: u32 = 1;
    pub const LIMBS: u32 = 2;
    pub const PARTS: u32 = 4;
    pub const BACK: u32 = 8;
    pub const BELLY: u32 = 0x10;
    pub const JOINTS: u32 = 0x20;
}

/// `SPSkinPaintDistribute`.
#[derive(Clone, Debug, PartialEq)]
pub struct Distribute {
    pub particle: String,
    pub spacing: f32,
    pub ordered: bool,
    pub limit: Option<usize>,
    /// `region` flags (None = everywhere).
    pub region: Option<u32>,
    pub body_range: (f32, f32),
    pub inverse: bool,
    /// `particleSelect` (binary Skinpaint components, the Cell brushes): the
    /// candidate particles with their probabilities; replaces `particle` when
    /// not empty. `select_all` stamps every candidate (each with its
    /// probability), otherwise one is drawn per point.
    pub select: Vec<(String, f32)>,
    pub select_all: bool,
    /// `region -back v` / `-belly v`: cosine thresholds of the normal's angle
    /// from the chain's up (`cos(v pi / 2)` / `-cos(v pi / 2)`); `None` keeps the
    /// 90 degree split.
    pub back: Option<f32>,
    pub belly: Option<f32>,
    /// `-centerOnly`: only the centre line of the back / belly region.
    pub center_only: bool,
    /// `cover` flag of the `spacing` line (description byte +0x3A): the
    /// exclusion around a placed point does not spread over triangles whose
    /// normals turn more than ~72 degrees away (baked-mesh placement only).
    pub cover: bool,
}

impl Default for Distribute {
    fn default() -> Self {
        Self {
            particle: String::new(),
            spacing: 0.2,
            ordered: false,
            limit: None,
            region: None,
            body_range: (0.0, 1.0),
            inverse: false,
            select: Vec::new(),
            select_all: false,
            back: None,
            belly: None,
            center_only: false,
            cover: false,
        }
    }
}

/// `SPSkinPaintClear`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clear {
    pub diffuse: Option<ColorRef>,
    pub spec: Option<f32>,
    pub bump: Option<f32>,
}

/// `SPSkinPaintSettings` (material of the painted skin).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub exponent: f32,
    pub phong: f32,
    pub gloss: f32,
}

impl Default for Settings {
    /// `cSkinPainter` defaults: gloss 0.5, phong 1, spec exponent 20.
    fn default() -> Self {
        Self { exponent: 20.0, phong: 1.0, gloss: 0.5 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Distribute(String),
    Flood(String),
    Clear(Clear),
    Settings { exponent: Option<f32>, phong: Option<f32>, gloss: Option<f32> },
    Sequence(String),
    Effect(String),
}

/// One parsed `.pfx` script.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    pub particles: HashMap<String, Particle>,
    pub floods: HashMap<String, Particle>,
    pub distributes: HashMap<String, Distribute>,
    pub effects: HashMap<String, Vec<Item>>,
    pub sequences: HashMap<String, Vec<String>>,
    /// Effects in definition order.
    pub effect_order: Vec<String>,
    /// Keywords this parser does not interpret (diagnostics).
    pub unknown: Vec<String>,
}

impl Script {
    /// The entry effect: the one named like the resource, else the last.
    pub fn entry(&self, instance: u32) -> Option<&str> {
        self.effect_order
            .iter()
            .find(|n| crate::world_objects::name_id(n) == instance)
            .or_else(|| self.effect_order.last())
            .map(String::as_str)
    }

    /// Brush names referenced by the particles.
    pub fn brushes(&self) -> Vec<String> {
        let mut out: Vec<String> = self.particles.values().flat_map(|p| p.brushes.iter().cloned()).collect();
        out.sort();
        out.dedup();
        out
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Split a line into tokens; `( ... )` vectors stay one token.
fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && depth <= 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_option(t: &str) -> bool {
    t.len() > 1 && t.starts_with('-') && t[1..].starts_with(|c: char| c.is_ascii_alphabetic())
}

/// Positional arguments and `-option args...` of one line (after the keyword).
struct Line {
    args: Vec<String>,
    options: Vec<(String, Vec<String>)>,
}

impl Line {
    fn new(toks: &[String]) -> Self {
        let mut args = Vec::new();
        let mut options: Vec<(String, Vec<String>)> = Vec::new();
        for t in toks {
            if is_option(t) {
                options.push((t[1..].to_ascii_lowercase(), Vec::new()));
            } else if let Some(o) = options.last_mut() {
                o.1.push(t.clone());
            } else {
                args.push(t.clone());
            }
        }
        Self { args, options }
    }
    fn has(&self, name: &str) -> bool {
        self.options.iter().any(|o| o.0 == name)
    }
    fn opt(&self, name: &str) -> Option<&[String]> {
        self.options.iter().find(|o| o.0 == name).map(|o| o.1.as_slice())
    }
    fn opt_f(&self, name: &str, i: usize) -> Option<f32> {
        self.opt(name).and_then(|a| a.get(i)).and_then(|s| num(s))
    }
}

fn num(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok()
}

fn vector(s: &str) -> Option<Vec3> {
    let v: Vec<f32> = s.trim_matches(|c| c == '(' || c == ')').split(',').filter_map(num).collect();
    (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
}

fn color_ref(s: &str) -> Option<ColorRef> {
    let l = s.to_ascii_lowercase();
    match l.as_str() {
        "color1" => Some(ColorRef::Layer(0)),
        "color2" => Some(ColorRef::Layer(1)),
        "color3" => Some(ColorRef::Layer(2)),
        "identity" => Some(ColorRef::Identity),
        _ => vector(s).map(|v| ColorRef::Rgb(v.to_array())),
    }
}

fn blend(s: &str) -> Blend {
    match s.to_ascii_lowercase().as_str() {
        "add" => Blend::Add,
        "subtract" => Blend::Subtract,
        "multiply" => Blend::Multiply,
        "screen" => Blend::Screen,
        _ => Blend::Alpha,
    }
}

fn motion(line: &Line) -> Option<Motion> {
    let first = line.args.first()?;
    let kind = match first.as_str() {
        "dir" | "move" | "inherit" => MotionKind::Dir,
        "attract" => MotionKind::Attract,
        "aroundBone" => MotionKind::AroundBone,
        "alongBone" => MotionKind::AlongBone,
        "aroundSpine" => MotionKind::AroundSpine,
        "alongSpine" => MotionKind::AlongSpine,
        v => return vector(v).map(|vector| Motion { kind: MotionKind::Attract, vector, reverse: line.has("reverse") }),
    };
    Some(Motion { kind, vector: Vec3::ZERO, reverse: line.has("reverse") })
}

fn range_flag(line: &Line) -> RangeFlag {
    if line.has("wrap") || line.has("wrap2") {
        RangeFlag::Wrap
    } else if line.has("clamp") || line.has("clamp2") {
        RangeFlag::Clamp
    } else if line.has("mirror") || line.has("mirror2") {
        RangeFlag::Mirror
    } else if line.has("open") {
        RangeFlag::Open
    } else {
        RangeFlag::Inside
    }
}

/// A channel line inside a particle (`in_modifier` = index of the open block).
fn variable(c: usize, line: &Line, in_modifier: Option<usize>) -> Option<Variable> {
    let op_opt: Option<&str> = ["set", "add", "mult", "div"].into_iter().find(|o| line.has(o));
    let op = match op_opt {
        Some("add") => Op::Add,
        Some("mult") => Op::Mult,
        Some("div") => Op::Div,
        _ => Op::Set,
    };
    let mut values: Vec<f32> = op_opt.and_then(|o| line.opt(o)).map(|a| a.iter().filter_map(|s| num(s)).collect()).unwrap_or_default();
    if values.is_empty() {
        values = line.args.iter().filter_map(|s| num(s)).collect();
    }
    if values.is_empty() {
        return None;
    }
    let vary = line.opt_f("vary", 0).unwrap_or(0.0);
    match in_modifier {
        Some(m) => Some(Variable {
            channel: c,
            op,
            values,
            vary,
            modifier: Some(m),
            range: (line.opt_f("range", 0).unwrap_or(0.0), line.opt_f("range", 1).unwrap_or(1.0)),
            flag: range_flag(line),
            curve: false,
        }),
        None => {
            let curve = line.args.len() > 1 || line.has("vary") || line.has("offset");
            // SporeModder-FX: a curve multiplies by default (`-add` adds); a
            // single value sets unless an operation is named.
            let op = if curve && op_opt.is_none() { Op::Mult } else { op };
            Some(Variable { channel: c, op, values, vary, modifier: None, range: (0.0, 1.0), flag: RangeFlag::Clamp, curve })
        }
    }
}

fn range_pair(line: &Line, default: (f32, f32)) -> (f32, f32) {
    let a = line.args.iter().filter_map(|s| num(s)).collect::<Vec<_>>();
    match a.len() {
        0 => default,
        1 => {
            let vary = line.opt_f("vary", 0).unwrap_or(0.0);
            (a[0] * (1.0 - vary), a[0] * (1.0 + vary))
        }
        _ => (a[0], a[1]),
    }
}

#[derive(PartialEq)]
enum Block {
    Particle(String, bool),
    Distribute(String),
    Effect(String),
    Sequence(String),
    Skip,
}

/// Parse a `.pfx` brush script.
pub fn parse(text: &str) -> Script {
    let mut script = Script::default();
    // Macros first (`define name() ... enddef`), expanded by `create name()`.
    let mut macros: HashMap<String, Vec<String>> = HashMap::new();
    let mut lines: Vec<String> = Vec::new();
    let mut defining: Option<(String, Vec<String>)> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let toks = tokens(line);
        let first = toks[0].as_str();
        let macro_name = || toks.get(1).map(|n| n.trim_end_matches("()").to_string()).unwrap_or_default();
        if let Some((name, body)) = defining.as_mut() {
            if first == "enddef" {
                macros.insert(std::mem::take(name), std::mem::take(body));
                defining = None;
            } else {
                body.push(line.to_string());
            }
        } else if first == "define" {
            defining = Some((macro_name(), Vec::new()));
        } else if first == "create" {
            lines.extend(macros.get(&macro_name()).cloned().unwrap_or_default());
        } else {
            lines.push(line.to_string());
        }
    }
    let mut block: Option<Block> = None;
    let mut particle = Particle::default();
    let mut dist = Distribute::default();
    let mut items: Vec<Item> = Vec::new();
    let mut plays: Vec<String> = Vec::new();
    let mut modifier: Option<usize> = None;
    let mut nested_skip = 0usize;
    for line in &lines {
        let toks = tokens(line);
        let key = toks[0].as_str();
        let rest = Line::new(&toks[1..]);
        let name = toks.get(1).cloned().unwrap_or_default();
        let Some(b) = block.as_ref() else {
            block = match key {
                "SPSkinPaintParticle" => Some(Block::Particle(name, false)),
                "SPSkinPaintFlood" => Some(Block::Particle(name, true)),
                "SPSkinPaintDistribute" => Some(Block::Distribute(name)),
                "effect" => Some(Block::Effect(name)),
                "sequence" => Some(Block::Sequence(name)),
                "collection" | "endCollection" | "if" | "endif" | "set" | "seti" | "setb" | "include" | "sinclude" | "trace" | "export" | "import" | "seed" | "model" => None,
                k => {
                    script.unknown.push(k.to_string());
                    None
                }
            };
            particle = Particle::default();
            dist = Distribute::default();
            items.clear();
            plays.clear();
            modifier = None;
            nested_skip = 0;
            continue;
        };
        if nested_skip > 0 {
            if key == "end" {
                nested_skip -= 1;
            }
            continue;
        }
        if key == "end" {
            if modifier.is_some() {
                modifier = None;
                continue;
            }
            match block.take().unwrap() {
                Block::Particle(n, false) => {
                    script.particles.insert(n, std::mem::take(&mut particle));
                }
                Block::Particle(n, true) => {
                    script.floods.insert(n, std::mem::take(&mut particle));
                }
                Block::Distribute(n) => {
                    script.distributes.insert(n, std::mem::take(&mut dist));
                }
                Block::Effect(n) => {
                    script.effect_order.push(n.clone());
                    script.effects.insert(n, std::mem::take(&mut items));
                }
                Block::Sequence(n) => {
                    script.sequences.insert(n, std::mem::take(&mut plays));
                }
                Block::Skip => {}
            }
            continue;
        }
        match b {
            Block::Particle(..) => match key {
                "brush" => particle.brushes = rest.args.clone(),
                "life" => particle.life = range_pair(&rest, (0.0, 0.0)),
                "delay" => particle.delay = range_pair(&rest, (0.0, 0.0)),
                "diffuseColor" | "color" => particle.color = rest.args.first().and_then(|s| color_ref(s)).or(rest.has("identity").then_some(ColorRef::Identity)),
                "diffuseBlend" => particle.diffuse_blend = blend(rest.args.first().map_or("", |s| s)),
                "specularBlend" => particle.specular_blend = blend(rest.args.first().map_or("", |s| s)),
                "bumpBlend" => particle.bump_blend = blend(rest.args.first().map_or("", |s| s)),
                "align" => particle.align = motion(&rest),
                "attract" | "restrict" => particle.attract = motion(&rest),
                "initDir" => particle.init_dir = motion(&rest),
                "chain" => particle.chain = rest.args.first().cloned(),
                "inherit" => particle.inherit = rest.args.iter().filter_map(|a| channel(a)).collect(),
                "modifier" => {
                    let src = rest.args.first().map(String::as_str).unwrap_or("");
                    let axis = rest.args.get(1).and_then(|s| vector(s)).unwrap_or(Vec3::Z);
                    let source = match src {
                        "age" => Source::Age,
                        "random" => Source::Random,
                        "worldAngle" => Source::WorldAngle(axis.normalize_or(Vec3::Z)),
                        "worldPos" => Source::WorldPos(axis / (axis.x + axis.y + axis.z + 1e-8)),
                        "worldDist" => Source::WorldDist(axis / (axis.length_squared() + 1e-8)),
                        "boneAngle" => Source::BoneAngle,
                        "bonePos" => Source::BonePos,
                        "boneOffset" => Source::BoneOffset,
                        "torsoPos" => Source::TorsoPos,
                        "limbPos" => Source::LimbPos,
                        "limbType" => Source::LimbType,
                        "region" => Source::Region,
                        "paintMask" => Source::PaintMask,
                        other => match channel(other) {
                            Some(c) => Source::Channel(c),
                            None => {
                                script.unknown.push(format!("modifier {other}"));
                                Source::PaintMask
                            }
                        },
                    };
                    particle.modifiers.push(source);
                    modifier = Some(particle.modifiers.len() - 1);
                }
                "compareNormal" | "comparePosition" | "curve" | "material" | "texture" | "trace" | "set" | "seti" | "setb" | "if" | "endif" => {}
                k => match channel(k) {
                    Some(c) => {
                        if let Some(v) = variable(c, &rest, modifier) {
                            particle.vars.push(v);
                        }
                    }
                    None => script.unknown.push(k.to_string()),
                },
            },
            Block::Distribute(_) => match key {
                "particle" => dist.particle = name,
                "spacing" => {
                    dist.spacing = rest.args.first().and_then(|s| num(s)).unwrap_or(0.2);
                    dist.ordered = rest.has("ordered");
                }
                "limit" => dist.limit = rest.args.first().and_then(|s| num(s)).map(|v| v as usize),
                "region" => {
                    let mut r = 0;
                    for (flag, bit) in [
                        ("torso", region::TORSO),
                        ("limbs", region::LIMBS),
                        ("parts", region::PARTS),
                        ("back", region::BACK),
                        ("belly", region::BELLY),
                        ("joints", region::JOINTS),
                    ] {
                        if rest.has(flag) {
                            r |= bit;
                        }
                    }
                    if let (Some(a), Some(b)) = (rest.opt_f("bodyrange", 0), rest.opt_f("bodyrange", 1)) {
                        dist.body_range = (a, b);
                    }
                    dist.inverse = rest.has("inverse");
                    dist.region = Some(r);
                }
                "particleSelect" => nested_skip += 1,
                _ => {}
            },
            Block::Effect(_) => match key {
                "SPSkinPaintDistribute" => items.push(Item::Distribute(name)),
                "SPSkinPaintFlood" => items.push(Item::Flood(name)),
                "SPSkinPaintClear" => items.push(Item::Clear(Clear {
                    diffuse: rest.opt("diffuse").or(rest.opt("rgb")).and_then(|a| a.first()).and_then(|s| color_ref(s)),
                    spec: rest.opt_f("spec", 0),
                    bump: rest.opt_f("bump", 0),
                })),
                "SPSkinPaintSettings" => items.push(Item::Settings {
                    exponent: rest.opt_f("exponent", 0),
                    phong: rest.opt_f("phong", 0),
                    gloss: rest.opt_f("gloss", 0),
                }),
                "sequence" => items.push(Item::Sequence(name)),
                "effect" => items.push(Item::Effect(name)),
                _ => {}
            },
            Block::Sequence(_) => {
                if key == "play" {
                    plays.push(name);
                }
            }
            Block::Skip => {}
        }
    }
    script
}

// ---------------------------------------------------------------------------
// Texel map: every painted texel's surface point
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub pos: Vec3,
    pub normal: Vec3,
    /// 0 torso, 1 limbs (`modifier region`).
    pub region: u8,
    pub chain: u16,
    /// Position along the chain, 0 (head / body end) .. 1.
    pub along: f32,
    /// Angle around the chain, degrees 0..360 (0 = back).
    pub angle: f32,
    pub texel: u32,
}

/// The skin surface behind the paint texture (texel -> point) with a uniform
/// grid for stamp lookups.
#[derive(Clone, Debug, Default)]
pub struct TexelMap {
    pub size: u32,
    pub samples: Vec<Sample>,
    /// Sample per texel (`u32::MAX` = gutter).
    pub texel: Vec<u32>,
    min: Vec3,
    cell: f32,
    dims: [usize; 3],
    cells: Vec<(u32, u32)>,
    order: Vec<u32>,
    /// Triangles (positions, region) for distribute sampling.
    triangles: Vec<([Vec3; 3], [Vec3; 3], u16)>,
    bounds: (Vec3, Vec3),
    /// Baked skin only (the Cell body): the vertebra index of every triangle,
    /// in the order the triangles were given to [`texel_map`] (retail keeps it
    /// in bits 8..15 of the per-triangle flags, `(flags >> 8) & 0xFF`, set by
    /// the classifier 0x5241F0 from the triangle's first corner), and the
    /// number of vertebrae. Empty: the `range` of a distribute falls back to
    /// the position along the chain.
    pub vertebra: Vec<u8>,
    pub vertebrae: u32,
    /// The map is a part's chart of the atlas (the Cell parts): every triangle
    /// carries the retail part flag 4 instead of the vertebra flags 0x19.
    pub parts: bool,
}

const GRID_CELL: f32 = 0.04;

/// Rasterize the unwrapped skin into the texel map.
pub fn texel_map(prims: &Primitives, un: &Unwrapped) -> TexelMap {
    let size = PAINT_SIZE;
    let mut map = TexelMap { size, texel: vec![u32::MAX; (size * size) as usize], ..Default::default() };
    let s = size as f32;
    for t in &un.triangles {
        let [a, b, c] = t.map(|i| i as usize);
        let uv = [un.uvs[a], un.uvs[b], un.uvs[c]].map(|u| (u[0] * s, u[1] * s));
        let chain = un.chart[a].chain;
        map.triangles.push(([un.positions[a], un.positions[b], un.positions[c]], [un.normals[a], un.normals[b], un.normals[c]], chain as u16));
        let x0 = uv.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let x1 = uv.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil().min(s - 1.0) as i32;
        let y0 = uv.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let y1 = uv.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(s - 1.0) as i32;
        let area = (uv[1].0 - uv[0].0) * (uv[2].1 - uv[0].1) - (uv[2].0 - uv[0].0) * (uv[1].1 - uv[0].1);
        if area.abs() < 1e-9 {
            continue;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((uv[1].0 - px) * (uv[2].1 - py) - (uv[2].0 - px) * (uv[1].1 - py)) / area;
                let w1 = ((uv[2].0 - px) * (uv[0].1 - py) - (uv[0].0 - px) * (uv[2].1 - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -0.02 || w1 < -0.02 || w2 < -0.02 {
                    continue;
                }
                let texel = (y as u32 * size + x as u32) as usize;
                if map.texel[texel] != u32::MAX {
                    continue;
                }
                let pos = un.positions[a] * w0 + un.positions[b] * w1 + un.positions[c] * w2;
                let normal = (un.normals[a] * w0 + un.normals[b] * w1 + un.normals[c] * w2).normalize_or(Vec3::Z);
                let ch = &prims.chains[chain];
                let p = chain_param(ch, pos);
                map.texel[texel] = map.samples.len() as u32;
                map.samples.push(Sample {
                    pos,
                    normal,
                    region: ch.limb as u8,
                    chain: chain as u16,
                    along: if p.length > 0.0 { (p.s / p.length).clamp(0.0, 1.0) } else { 0.0 },
                    angle: p.theta.to_degrees().rem_euclid(360.0),
                    texel: texel as u32,
                });
            }
        }
    }
    // Uniform grid over the samples.
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for s in &map.samples {
        min = min.min(s.pos);
        max = max.max(s.pos);
    }
    if map.samples.is_empty() {
        return map;
    }
    map.bounds = (min, max);
    map.min = min - Vec3::splat(1e-3);
    map.cell = GRID_CELL;
    let d = ((max - map.min) / GRID_CELL).floor() + Vec3::ONE;
    map.dims = [d.x as usize, d.y as usize, d.z as usize];
    let n = map.dims[0] * map.dims[1] * map.dims[2];
    let mut keyed: Vec<(u32, u32)> = map.samples.iter().enumerate().map(|(i, s)| (map.cell_index(s.pos) as u32, i as u32)).collect();
    keyed.sort_unstable();
    map.cells = vec![(0, 0); n];
    map.order = keyed.iter().map(|k| k.1).collect();
    let mut i = 0;
    while i < keyed.len() {
        let c = keyed[i].0;
        let start = i;
        while i < keyed.len() && keyed[i].0 == c {
            i += 1;
        }
        map.cells[c as usize] = (start as u32, (i - start) as u32);
    }
    map
}

impl TexelMap {
    /// The surface sample nearest to `p` (position, normal), searching a
    /// growing neighbourhood; `p` itself when the map is empty. Puts a
    /// crawling particle back on a baked mesh (`paint_on`).
    pub fn nearest(&self, p: Vec3) -> (Vec3, Vec3) {
        let mut r = GRID_CELL;
        for _ in 0..6 {
            let mut best: Option<(f32, &Sample)> = None;
            self.near(p, r, |i| {
                let s = &self.samples[i as usize];
                let d = (s.pos - p).length_squared();
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, s));
                }
            });
            if let Some((_, s)) = best {
                return (s.pos, s.normal);
            }
            r *= 2.0;
        }
        (p, Vec3::Z)
    }

    fn coords(&self, p: Vec3) -> [usize; 3] {
        let c = ((p - self.min) / self.cell).floor();
        [
            (c.x.max(0.0) as usize).min(self.dims[0] - 1),
            (c.y.max(0.0) as usize).min(self.dims[1] - 1),
            (c.z.max(0.0) as usize).min(self.dims[2] - 1),
        ]
    }
    fn cell_index(&self, p: Vec3) -> usize {
        let c = self.coords(p);
        (c[0] * self.dims[1] + c[1]) * self.dims[2] + c[2]
    }
    /// Samples within `r` of `p`.
    fn near(&self, p: Vec3, r: f32, mut f: impl FnMut(u32)) {
        if self.samples.is_empty() {
            return;
        }
        let lo = self.coords(p - Vec3::splat(r));
        let hi = self.coords(p + Vec3::splat(r));
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    let (start, count) = self.cells[(x * self.dims[1] + y) * self.dims[2] + z];
                    for &i in &self.order[start as usize..(start + count) as usize] {
                        f(i);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Painter
// ---------------------------------------------------------------------------

/// A decoded brush texture (RGBA8).
#[derive(Clone, Debug, Default)]
pub struct BrushTex {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl BrushTex {
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        if self.width == 0 || self.height == 0 {
            return [1.0; 4];
        }
        let x = (u * self.width as f32 - 0.5).clamp(0.0, self.width as f32 - 1.0);
        let y = (v * self.height as f32 - 0.5).clamp(0.0, self.height as f32 - 1.0);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let px = |x: u32, y: u32, c: usize| self.rgba[((y * self.width + x) * 4) as usize + c] as f32 / 255.0;
        std::array::from_fn(|c| {
            let a = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
            let b = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
            a * (1.0 - fy) + b * fy
        })
    }
}

/// `<brush>__diffuse` and `<brush>__specBump`.
#[derive(Clone, Debug, Default)]
pub struct Brush {
    pub diffuse: Option<BrushTex>,
    pub spec_bump: Option<BrushTex>,
}

/// What to paint: the three layer scripts (with their resource instance ids),
/// colours and seeds.
pub struct PaintJob<'a> {
    pub scripts: [Option<(&'a Script, u32)>; 3],
    pub colors: [[f32; 3]; 3],
    pub seeds: [u32; 3],
    pub brushes: &'a HashMap<String, Brush>,
}

/// The painted textures (`cSkinPainter` tex0 / tex2) and material settings.
#[derive(Clone, Debug, Default)]
pub struct Painted {
    pub size: u32,
    /// sRGB diffuse, alpha 255.
    pub diffuse: Vec<u8>,
    /// Tangent-space normal map from the bump layer.
    pub normal: Vec<u8>,
    /// Mean specular level.
    pub spec: f32,
    /// Specular level per texel (0..255, dilated like the colour): the alpha
    /// of the retail spec / bump map (`specNMap.a`).
    pub spec_map: Vec<u8>,
    /// Bump height per texel (0..255, 128 = flat; the blue channel of the
    /// retail spec / bump map `tex1`, the input of `skpBumpToNormal`).
    pub bump_map: Vec<u8>,
    pub settings: Settings,
    pub stamps: usize,
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Retail's `Math::RandomNumberGenerator`: one 32-bit state word with three
/// draws (VERIFIED from the code image, march2017 addresses). The Cell paint
/// (`project` set) uses it where the retail code does: the placement shuffle
/// (`RandomInt` on a local object), the `particleSelect` draws and the
/// particles' seeds (the global object 0x16778DC), and the draws of each
/// particle (`cSkinpaintInfo`'s own object, seeded `seed ^ 0x129C8901`).
/// ADAPTED: retail seeds the global object from `rdtsc` (and a zero state
/// from `rdtsc` too), so its sequence cannot be reproduced; a fixed seed
/// replaces it so a style paints the same way twice.
#[derive(Clone, Copy, Debug)]
struct RetailRng(u32);

impl RetailRng {
    /// `RandomUInt` 0x936020: `state = state * 0x41C64E6D + 0x3039` over a
    /// 64-bit product (the state keeps the low 32 bits); the result is bits
    /// 16..47 of that product.
    fn uint(&mut self) -> u32 {
        let p = self.0 as u64 * 0x41C6_4E6D + 0x3039;
        self.0 = p as u32;
        (p >> 16) as u32
    }
    /// `RandomInt(n)` 0xA68F70: `uint()` scaled into `0..n` (high word of the
    /// 64-bit product with `n`).
    fn int(&mut self, n: u32) -> u32 {
        ((self.uint() as u64 * n as u64) >> 32) as u32
    }
    /// `RandomFloat` 0x936040: `state *= 0x278DDE6D` (wrapping), value =
    /// `(int)state * 2^-32 + 0.5`, 0 when it reaches 1.
    fn float(&mut self) -> f32 {
        if self.0 == 0 {
            self.0 = 1;
        }
        self.0 = self.0.wrapping_mul(0x278D_DE6D);
        let v = self.0 as i32 as f32 * (1.0 / 4_294_967_296.0) + 0.5;
        if v < 1.0 {
            v
        } else {
            0.0
        }
    }
}

/// The two kinds of generator a [`Painter`] hands to its particle draws.
enum Draws {
    /// The layer's SplitMix64 (creatures, flood).
    Layer,
    /// A retail `cSkinpaintInfo` generator (Cell paint).
    Retail(RetailRng),
}

impl Draws {
    fn next(&mut self, layer: &mut Rng) -> f32 {
        match self {
            Draws::Layer => layer.next(),
            Draws::Retail(r) => r.float(),
        }
    }
}

/// Per-sample working layers.
struct Canvas {
    diffuse: Vec<[f32; 3]>,
    spec: Vec<f32>,
    bump: Vec<f32>,
    settings: Settings,
    stamps: usize,
}

fn apply_blend(mode: Blend, dst: f32, src: f32, a: f32) -> f32 {
    let a = a.clamp(0.0, 1.0);
    let v = match mode {
        Blend::Alpha => dst + (src - dst) * a,
        Blend::Add => dst + src * a,
        Blend::Subtract => dst - src * a,
        Blend::Multiply => dst * (1.0 + (src - 1.0) * a),
        Blend::Screen => 1.0 - (1.0 - dst) * (1.0 - src * a),
    };
    v.clamp(0.0, 1.0)
}

fn rgb_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let d = max - min;
    let h = if d <= 1e-6 {
        0.0
    } else if max == c[0] {
        60.0 * ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if max == c[1] {
        60.0 * ((c[2] - c[0]) / d + 2.0)
    } else {
        60.0 * ((c[0] - c[1]) / d + 4.0)
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max]
}

fn hsv_to_rgb(h: [f32; 3]) -> [f32; 3] {
    let (hue, s, v) = (h[0].rem_euclid(360.0), h[1].clamp(0.0, 1.0), h[2].clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

/// Where a channel evaluation happens.
#[derive(Clone, Copy)]
struct Ctx {
    pos: Vec3,
    normal: Vec3,
    region: u8,
    along: f32,
    angle: f32,
    age: f32,
}

/// Channel values of `p` at `ctx` (`randoms`: one per modifier block, then
/// one per curve variable, fixed per particle).
fn channels(p: &Particle, ctx: &Ctx, randoms: &[f32], bounds: (Vec3, Vec3)) -> [f32; 19] {
    let mut c = CHANNEL_BASE;
    let extent = (bounds.1 - bounds.0).max(Vec3::splat(1e-4));
    let mut curve_i = p.modifiers.len();
    for v in &p.vars {
        let value = match v.modifier {
            None if v.curve => {
                let n = v.values.len();
                let x = if n > 1 {
                    let f = ctx.age.clamp(0.0, 1.0) * (n - 1) as f32;
                    let i = (f.floor() as usize).min(n - 2);
                    v.values[i] + (v.values[i + 1] - v.values[i]) * (f - i as f32)
                } else {
                    v.values[0]
                };
                let r = randoms.get(curve_i).copied().unwrap_or(0.5);
                curve_i += 1;
                x * (1.0 + v.vary * (2.0 * r - 1.0))
            }
            None => v.values[0],
            Some(m) => {
                let x = match p.modifiers[m] {
                    Source::Age => ctx.age,
                    Source::Random => randoms.get(m).copied().unwrap_or(0.5),
                    Source::WorldAngle(axis) => ctx.normal.dot(axis).clamp(-1.0, 1.0).acos().to_degrees(),
                    Source::WorldPos(axis) => ((ctx.pos - bounds.0) / extent).dot(axis),
                    Source::WorldDist(axis) => ctx.pos.dot(axis),
                    Source::BoneAngle => ctx.angle,
                    Source::BonePos | Source::TorsoPos => ctx.along,
                    Source::LimbPos => {
                        if ctx.region == 1 {
                            ctx.along
                        } else {
                            0.0
                        }
                    }
                    Source::Region => ctx.region as f32,
                    Source::Channel(k) => c[k],
                    Source::BoneOffset | Source::LimbType | Source::PaintMask => 0.0,
                };
                let (lo, hi) = v.range;
                let span = hi - lo;
                let mut u = if span.abs() > 1e-9 { (x - lo) / span } else { 0.0 };
                match v.flag {
                    RangeFlag::Inside => {
                        if x < lo - 1e-6 || x > hi + 1e-6 {
                            continue;
                        }
                    }
                    RangeFlag::Open => {}
                    RangeFlag::Clamp => u = u.clamp(0.0, 1.0),
                    RangeFlag::Wrap => u = u.rem_euclid(1.0),
                    RangeFlag::Mirror => u = 1.0 - ((u * 0.5).rem_euclid(1.0) * 2.0 - 1.0).abs(),
                }
                let a = v.values[0];
                let b = *v.values.get(1).unwrap_or(&a);
                a + (b - a) * u
            }
        };
        let k = v.channel;
        c[k] = match v.op {
            Op::Set => value,
            Op::Add => c[k] + value,
            Op::Mult => c[k] * value,
            Op::Div => {
                if value != 0.0 {
                    c[k] / value
                } else {
                    c[k]
                }
            }
        };
    }
    c
}

struct Stamp {
    time: f32,
    order: u32,
    pos: Vec3,
    normal: Vec3,
    tangent: Vec3,
    half: (f32, f32),
    color: Option<[f32; 3]>,
    alphas: [f32; 3],
    spec: f32,
    bump: f32,
    blends: [Blend; 3],
    brush: Option<String>,
}

struct Painter<'a> {
    prims: &'a Primitives,
    map: &'a TexelMap,
    job: &'a PaintJob<'a>,
    canvas: Canvas,
    /// Moves a crawling particle back onto the surface (position, normal);
    /// `None` = the implicit field of `prims` (creature skins). A skin that
    /// is a baked mesh (the Cell body) passes its nearest-texel lookup.
    project: Option<&'a dyn Fn(Vec3) -> (Vec3, Vec3)>,
    /// Edge neighbours of every triangle of `map` (`u32::MAX` = none), welded by
    /// position; only built for a baked mesh (`project` set), where the
    /// retail triangle-order placement runs (`samples_mesh`).
    adjacency: Vec<[u32; 3]>,
    /// State of retail's global `RandomNumberGenerator` (0x16778DC), used by
    /// the baked-mesh placement (`project` set): see [`RetailRng`].
    global: std::cell::Cell<u32>,
}

/// Edge-neighbour table of the texel map's triangles, welding vertices that
/// share a position (a baked mesh splits them along UV seams; the retail
/// skin mesh has one position stream, `mesh + 0x94`).
fn weld_adjacency(tris: &[([Vec3; 3], [Vec3; 3], u16)]) -> Vec<[u32; 3]> {
    let key = |p: Vec3| ((p.x * 1.0e4).round() as i32, (p.y * 1.0e4).round() as i32, (p.z * 1.0e4).round() as i32);
    let mut edges: HashMap<((i32, i32, i32), (i32, i32, i32)), Vec<u32>> = HashMap::new();
    for (t, tri) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (key(tri.0[k]), key(tri.0[(k + 1) % 3]));
            edges.entry(if a <= b { (a, b) } else { (b, a) }).or_default().push(t as u32);
        }
    }
    tris.iter()
        .enumerate()
        .map(|(t, tri)| {
            let mut out = [u32::MAX; 3];
            for k in 0..3 {
                let (a, b) = (key(tri.0[k]), key(tri.0[(k + 1) % 3]));
                if let Some(list) = edges.get(&if a <= b { (a, b) } else { (b, a) }) {
                    out[k] = list.iter().copied().find(|o| *o != t as u32).unwrap_or(u32::MAX);
                }
            }
            out
        })
        .collect()
}

impl Painter<'_> {
    /// `RandomUInt` on the global generator (retail 0x16778DC).
    fn global_uint(&self) -> u32 {
        let mut r = RetailRng(self.global.get());
        let v = r.uint();
        self.global.set(r.0);
        v
    }

    /// `RandomFloat` on the global generator.
    fn global_float(&self) -> f32 {
        let mut r = RetailRng(self.global.get());
        let v = r.float();
        self.global.set(r.0);
        v
    }

    fn resolve(&self, c: Option<ColorRef>) -> Option<[f32; 3]> {
        match c? {
            ColorRef::Layer(i) => Some(self.job.colors[i.min(2)]),
            ColorRef::Rgb(v) => Some(v),
            ColorRef::Identity => None,
        }
    }

    fn run_effect(&mut self, script: &Script, name: &str, rng: &mut Rng, depth: usize) {
        if depth > 8 {
            return;
        }
        let Some(items) = script.effects.get(name) else { return };
        for item in items {
            match item {
                Item::Clear(c) => {
                    let color = self.resolve(c.diffuse);
                    for i in 0..self.map.samples.len() {
                        if let Some(col) = color {
                            self.canvas.diffuse[i] = col;
                        }
                        if let Some(s) = c.spec {
                            self.canvas.spec[i] = s;
                        }
                        if let Some(b) = c.bump {
                            self.canvas.bump[i] = b;
                        }
                    }
                }
                Item::Settings { exponent, phong, gloss } => {
                    let s = &mut self.canvas.settings;
                    s.exponent = exponent.unwrap_or(s.exponent);
                    s.phong = phong.unwrap_or(s.phong);
                    s.gloss = gloss.unwrap_or(s.gloss);
                }
                Item::Distribute(d) => {
                    if let Some(d) = script.distributes.get(d) {
                        self.distribute(script, d, rng);
                    }
                }
                Item::Flood(f) => {
                    if let Some(p) = script.floods.get(f) {
                        self.flood(p, rng);
                    }
                }
                Item::Sequence(s) => {
                    for e in script.sequences.get(s).cloned().unwrap_or_default() {
                        self.run_effect(script, &e, rng, depth + 1);
                    }
                }
                Item::Effect(e) => self.run_effect(script, e, rng, depth + 1),
            }
        }
    }

    fn flood(&mut self, p: &Particle, rng: &mut Rng) {
        let randoms: Vec<f32> = (0..p.modifiers.len() + p.vars.len()).map(|_| rng.next()).collect();
        let base = self.resolve(p.color);
        for i in 0..self.map.samples.len() {
            let s = self.map.samples[i];
            let ctx = Ctx { pos: s.pos, normal: s.normal, region: s.region, along: s.along, angle: s.angle, age: 0.0 };
            let c = channels(p, &ctx, &randoms, self.map.bounds);
            if let Some(col) = base {
                let hsv = rgb_to_hsv(col);
                let col = hsv_to_rgb([hsv[0] + c[ch::HUE], hsv[1] * c[ch::SAT], hsv[2] * c[ch::VAL]]);
                let a = c[ch::ALPHA] * c[ch::DIFFUSE_ALPHA];
                let d = &mut self.canvas.diffuse[i];
                for k in 0..3 {
                    d[k] = apply_blend(p.diffuse_blend, d[k], col[k], a);
                }
            }
        }
    }

    /// Retail placement of `cSkinpaintDistributeEffect::ApplyEffect`
    /// (0x5314B0, march2017) on a triangle mesh; used for a baked skin (the
    /// Cell body). VERIFIED reading of the code:
    /// * the triangles are visited in index order (`ordered`, description byte
    ///   +0x3B) or after a partial Fisher-Yates shuffle of the first half
    ///   (`i < n / 2`, `j = i + RandomInt(n - i)`);
    /// * a visited, unused triangle puts one particle at its centroid
    ///   (barycentric 1/3, 1/3); `-centerOnly` instead takes the point of the
    ///   triangle plane above (0, y, z) of the centroid when the centroid is
    ///   within 0.1 of the x = 0 plane, requires it to lie inside the triangle
    ///   and within 0.01 of x = 0;
    /// * `region` (desc +0x24) is a mask against the per-triangle flags of the
    ///   skin classifier (0x5241F0: vertebra triangles 0x19 = torso | back |
    ///   belly, limb triangles 0x22 = limbs | joints): `back` / `belly` bits
    ///   are decided by the dot of the point's direction from the spine with the
    ///   spine's up against the thresholds (desc +0x28 / +0x2C), the `range`
    ///   (+0x30 / +0x34) compares the vertebra index with `numVertebrae * range`,
    ///   `inverse` (+0x38) flips the verdict; no region (-1) accepts everything;
    /// * after a placement (`limit`, +0x20, counts placements whether or not a
    ///   particle is selected) every triangle reachable over shared edges whose
    ///   centroid lies within `spacing` (+0x1C) of the point is marked used;
    ///   with `cover` (+0x3A) a triangle takes part only while all three of
    ///   its corner normals have a dot above 0.31 with the placed triangle's
    ///   summed normal.
    /// The shuffle and the particle draws use retail's generators
    /// ([`RetailRng`], VERIFIED); `range` compares the triangle's vertebra
    /// index (`TexelMap::vertebra`) as the code does. ADAPTED: the spine
    /// direction / up of back and belly come from the body's vertebra polyline
    /// (the retail value sums the vertebrae around the point), the global
    /// generator's state is a fixed seed.
    fn samples_mesh(&self, d: &Distribute) -> Vec<(Vec3, Vec3, u16)> {
        let tris = &self.map.triangles;
        let n = tris.len();
        let mut order: Vec<u32> = (0..n as u32).collect();
        // The local generator (0x531567): seeded with `RandomUInt` of the global
        // object xor 0x3098A4A3 (a seed of -1 or 0 would read `rdtsc`).
        let mut local = RetailRng(self.global_uint() ^ 0x3098_A4A3);
        if !d.ordered {
            for i in 0..n / 2 {
                let j = (i + local.int((n - i) as u32) as usize).min(n - 1);
                order.swap(i, j);
            }
        }
        let spacing2 = d.spacing * d.spacing;
        let (back_cos, belly_cos) = (d.back.unwrap_or(0.0), d.belly.unwrap_or(0.0));
        let mut used = vec![false; n];
        let mut seen = vec![0u32; n];
        let mut round = 0u32;
        let mut stack: Vec<u32> = Vec::new();
        let mut out = Vec::new();
        for &t in &order {
            let t = t as usize;
            if d.limit.is_some_and(|l| out.len() >= l) || out.len() >= 60_000 {
                break;
            }
            if used[t] {
                continue;
            }
            let (p, nrm, chain) = tris[t];
            let ch = &self.prims.chains[(chain as usize).min(self.prims.chains.len() - 1)];
            let (mut u, mut v) = (1.0 / 3.0, 1.0 / 3.0);
            let mut pos = (p[0] + p[1] + p[2]) / 3.0;
            if d.center_only {
                if pos.x.abs() >= 0.1 {
                    continue;
                }
                let (e0, e1) = (p[1] - p[0], p[2] - p[0]);
                let face = e0.cross(e1).normalize_or_zero();
                let q = Vec3::new(0.0, pos.y, pos.z);
                let q = q + face * (pos - q).dot(face);
                let denom = e0.cross(e1).dot(face);
                if denom.abs() < 1e-12 {
                    continue;
                }
                let w = q - p[0];
                u = w.cross(e1).dot(face) / denom;
                v = e0.cross(w).dot(face) / denom;
                if u < 0.0 || v < 0.0 || u + v > 1.0 {
                    continue;
                }
                pos = p[0] + e0 * u + e1 * v;
                if pos.x.abs() > 0.01 {
                    continue;
                }
            }
            if let Some(mask) = d.region {
                let flags: u32 = if self.map.parts {
                    region::PARTS
                } else if ch.limb {
                    0x22
                } else {
                    0x19
                };
                let mut masked = flags & mask;
                let mut inc = false;
                if masked != 0 {
                    if masked & (region::BACK | region::BELLY) != 0 {
                        masked &= !region::TORSO;
                    }
                    if masked & 7 != 0 {
                        inc = true;
                    }
                    if !inc && masked & (region::BACK | region::BELLY) != 0 {
                        let dot = chain_param(ch, pos).theta.cos();
                        if masked & region::BACK != 0 {
                            inc = dot >= back_cos;
                        }
                        if !inc && masked & region::BELLY != 0 {
                            inc = dot <= belly_cos;
                        }
                    }
                    if inc && (d.body_range.0 > 0.0 || d.body_range.1 < 1.0) {
                        if let (Some(v), true) = (self.map.vertebra.get(t), self.map.vertebrae > 0) {
                            // VERIFIED (0x5323B8): `numVertebrae * range[0] <= vertebra <=
                            // numVertebrae * range[1]` with the vertebra index of the
                            // triangle's flags.
                            let (v, count) = (*v as f32, self.map.vertebrae as f32);
                            inc = v >= count * d.body_range.0 && v <= count * d.body_range.1;
                        } else {
                            let cp = chain_param(ch, pos);
                            let along = if cp.length > 0.0 { cp.s / cp.length } else { 0.0 };
                            inc = along >= d.body_range.0 && along <= d.body_range.1;
                        }
                    }
                }
                if inc == d.inverse {
                    continue;
                }
            }
            out.push((pos, (nrm[0] * (1.0 - u - v) + nrm[1] * u + nrm[2] * v).normalize_or(Vec3::Z), chain));
            // Exclusion: flood over shared edges through the triangles whose
            // centroid is within `spacing` of the placed point.
            let n0 = (nrm[0] + nrm[1] + nrm[2]).normalize_or_zero();
            round += 1;
            stack.clear();
            stack.push(t as u32);
            while let Some(c) = stack.pop() {
                let c = c as usize;
                if seen[c] == round {
                    continue;
                }
                let (cp, cn, _) = tris[c];
                if ((cp[0] + cp[1] + cp[2]) / 3.0 - pos).length_squared() >= spacing2 {
                    continue;
                }
                if d.cover && cn.iter().any(|k| k.dot(n0) <= 0.31) {
                    continue;
                }
                used[c] = true;
                seen[c] = round;
                stack.extend(self.adjacency[c].iter().copied().filter(|a| *a != u32::MAX));
            }
        }
        out
    }

    /// Area-stratified sample points over the selected region.
    fn samples(&self, d: &Distribute, rng: &mut Rng) -> Vec<(Vec3, Vec3, u16)> {
        if self.project.is_some() && !self.adjacency.is_empty() {
            return self.samples_mesh(d);
        }
        let want = d.region.unwrap_or(region::TORSO | region::LIMBS | region::PARTS);
        let mut out = Vec::new();
        let cell = (d.spacing.max(0.005)).powi(2);
        let mut carry = rng.next() * cell;
        for (p, n, chain) in &self.map.triangles {
            let ch = &self.prims.chains[*chain as usize];
            let mut ok = if ch.limb { want & region::LIMBS != 0 } else { want & region::TORSO != 0 };
            let centroid = (p[0] + p[1] + p[2]) / 3.0;
            if ok && want & (region::BACK | region::BELLY) != 0 {
                let angle = chain_param(ch, centroid).theta.abs().to_degrees();
                // `-back v` keeps the normals within `acos(cos(v pi / 2))` of
                // up, `-belly v` those at least `acos(-cos(v pi / 2))` from it
                // (retail stores the cosines; the default is the 90 degree split).
                let back = d.back.map_or(90.0, |c| c.clamp(-1.0, 1.0).acos().to_degrees());
                let belly = d.belly.map_or(90.0, |c| c.clamp(-1.0, 1.0).acos().to_degrees());
                ok = (want & region::BACK != 0 && angle <= back) || (want & region::BELLY != 0 && angle >= belly);
            }
            if ok && d.body_range != (0.0, 1.0) {
                let cp = chain_param(ch, centroid);
                let along = if cp.length > 0.0 { cp.s / cp.length } else { 0.0 };
                ok = along >= d.body_range.0 && along <= d.body_range.1;
            }
            if ok == d.inverse {
                continue;
            }
            carry += (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5;
            while carry >= cell {
                carry -= cell;
                let (mut u, mut v) = (rng.next(), rng.next());
                if u + v > 1.0 {
                    u = 1.0 - u;
                    v = 1.0 - v;
                }
                let pos = p[0] + (p[1] - p[0]) * u + (p[2] - p[0]) * v;
                let nrm = (n[0] * (1.0 - u - v) + n[1] * u + n[2] * v).normalize_or(Vec3::Z);
                if d.center_only {
                    // ADAPTED: `-centerOnly` keeps a band of +-12 degrees around
                    // the centre line of the back (angle 0) / belly (180).
                    let angle = chain_param(ch, pos).theta.abs().to_degrees();
                    if angle.min((180.0 - angle).abs()) > 12.0 {
                        continue;
                    }
                }
                out.push((pos, nrm, *chain));
                if d.limit.is_some_and(|l| out.len() >= l) || out.len() >= 60_000 {
                    return out;
                }
            }
        }
        if d.ordered {
            let key = |p: &(Vec3, Vec3, u16)| {
                let cp = chain_param(&self.prims.chains[p.2 as usize], p.0);
                (p.2, cp.s, cp.theta)
            };
            out.sort_by(|a, b| {
                let (ka, kb) = (key(a), key(b));
                ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1)).then(ka.2.total_cmp(&kb.2))
            });
        } else {
            for i in (1..out.len()).rev() {
                let j = ((rng.next() * (i + 1) as f32) as usize).min(i);
                out.swap(i, j);
            }
        }
        out
    }

    fn motion_dir(&self, m: &Motion, chain: usize, pos: Vec3, normal: Vec3, current: Vec3) -> Vec3 {
        let spine = &self.prims.chains[0];
        let own = &self.prims.chains[chain.min(self.prims.chains.len() - 1)];
        let tangent_plane = |v: Vec3| (v - normal * v.dot(normal)).normalize_or(Vec3::ZERO);
        let d = match m.kind {
            MotionKind::AroundSpine => normal.cross(chain_tangent(spine, pos)),
            MotionKind::AroundBone => normal.cross(chain_tangent(own, pos)),
            MotionKind::AlongSpine => chain_tangent(spine, pos),
            MotionKind::AlongBone => chain_tangent(own, pos),
            MotionKind::Attract => m.vector,
            MotionKind::Dir => current,
        };
        let d = tangent_plane(d);
        if m.reverse {
            -d
        } else {
            d
        }
    }

    fn ctx(&self, pos: Vec3, normal: Vec3, chain: usize, age: f32) -> Ctx {
        let ch = &self.prims.chains[chain.min(self.prims.chains.len() - 1)];
        let p = chain_param(ch, pos);
        Ctx {
            pos,
            normal,
            region: ch.limb as u8,
            along: if p.length > 0.0 { (p.s / p.length).clamp(0.0, 1.0) } else { 0.0 },
            angle: p.theta.to_degrees().rem_euclid(360.0),
            age,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_stamp(&self, p: &Particle, c: &[f32; 19], pos: Vec3, normal: Vec3, dir: Vec3, chain: usize, time: f32, order: u32, draws: &mut Draws, rng: &mut Rng) -> Option<Stamp> {
        if c[ch::KILL] >= 1.0 || c[ch::ALPHA] <= 0.0 || c[ch::SIZE] <= 0.0 {
            return None;
        }
        // Brush orientation: the alignment (motion / chain axes) turned by the
        // rotation channel around the normal.
        let tangent_plane = |v: Vec3| (v - normal * v.dot(normal)).normalize_or(Vec3::ZERO);
        let mut t0 = match &p.align {
            Some(m) => self.motion_dir(m, chain, pos, normal, dir),
            None => Vec3::ZERO,
        };
        if t0 == Vec3::ZERO {
            t0 = tangent_plane(chain_tangent(&self.prims.chains[chain.min(self.prims.chains.len() - 1)], pos));
        }
        if t0 == Vec3::ZERO {
            t0 = tangent_plane(Vec3::X);
        }
        if t0 == Vec3::ZERO {
            t0 = tangent_plane(Vec3::Y);
        }
        let b0 = normal.cross(t0);
        let r = c[ch::ROTATION].to_radians();
        let tangent = t0 * r.cos() + b0 * r.sin();
        let color = self.resolve(p.color).map(|col| {
            let hsv = rgb_to_hsv(col);
            hsv_to_rgb([hsv[0] + c[ch::HUE], hsv[1] * c[ch::SAT], hsv[2] * c[ch::VAL]])
        });
        let brush = if p.brushes.len() > 1 {
            let i = ((draws.next(rng) * p.brushes.len() as f32) as usize).min(p.brushes.len() - 1);
            p.brushes.get(i).cloned()
        } else {
            p.brushes.first().cloned()
        };
        Some(Stamp {
            time,
            order,
            pos,
            normal,
            tangent,
            half: (c[ch::SIZE] * c[ch::ASPECT].max(0.01) * 0.5, c[ch::SIZE] * 0.5),
            color,
            alphas: [c[ch::ALPHA] * c[ch::DIFFUSE_ALPHA], c[ch::ALPHA] * c[ch::SPEC_ALPHA], c[ch::ALPHA] * c[ch::BUMP_ALPHA]],
            spec: c[ch::SPEC_SCALE],
            bump: c[ch::BUMP_SCALE],
            blends: [p.diffuse_blend, p.specular_blend, p.bump_blend],
            brush,
        })
    }

    fn distribute(&mut self, script: &Script, d: &Distribute, rng: &mut Rng) {
        // Candidate particles: the one `particle`, or the `particleSelect` list.
        let candidates: Vec<(&Particle, f32)> = if d.select.is_empty() {
            script.particles.get(&d.particle).map(|p| (p, 1.0)).into_iter().collect()
        } else {
            d.select.iter().filter_map(|(n, prob)| script.particles.get(n).map(|p| (p, *prob))).collect()
        };
        if candidates.is_empty() {
            return;
        }
        let points = self.samples(d, rng);
        let mut stamps: Vec<Stamp> = Vec::new();
        let mut order = 0u32;
        for (pos0, n0, chain) in points {
            let chain = chain as usize;
            // Which particles stamp at this point: all of them (each with its
            // probability) or one drawn by the probabilities (ADAPTED: a total
            // below 1 leaves some points empty).
            let baked = self.project.is_some();
            let chosen: Vec<&Particle> = if d.select.is_empty() {
                vec![candidates[0].0]
            } else if d.select_all && baked {
                // Retail (0x5324FF): every entry draws on the global generator
                // and is used when its probability is at least the draw.
                candidates.iter().filter(|(_, prob)| *prob >= self.global_float()).map(|(p, _)| *p).collect()
            } else if d.select_all {
                candidates.iter().filter(|(_, prob)| rng.next() < *prob).map(|(p, _)| *p).collect()
            } else if baked {
                // Retail (0x53255A): one global draw per point, the first entry whose
                // probability is at least the draw; none when no entry reaches it.
                let r = self.global_float();
                candidates.iter().find(|(_, prob)| *prob >= r).map(|(p, _)| vec![*p]).unwrap_or_default()
            } else {
                let mut r = rng.next();
                candidates
                    .iter()
                    .find(|(_, prob)| {
                        r -= prob;
                        r < 0.0
                    })
                    .map(|(p, _)| vec![*p])
                    .unwrap_or_default()
            };
            for p in chosen {
                // Retail (0x5326F9): the particle's seed is `RandomUInt` of the global
                // generator; its own draws come from a generator seeded `seed ^
                // 0x129C8901` (`cSkinpaintInfo::Init`, 0x523910). ADAPTED: the order of
                // the draws below is this painter's, not the info's.
                let mut draws = if baked { Draws::Retail(RetailRng(self.global_uint() ^ 0x129C_8901)) } else { Draws::Layer };
                let randoms: Vec<f32> = (0..p.modifiers.len() + p.vars.len()).map(|_| draws.next(rng)).collect();
                let delay = p.delay.0 + (p.delay.1 - p.delay.0) * draws.next(rng);
                let life = (p.life.0 + (p.life.1 - p.life.0) * draws.next(rng)).max(0.0);
                let moving = life > 0.0 && (p.attract.is_some() || p.init_dir.is_some() || p.align.is_some_and(|a| a.kind != MotionKind::Dir));
                let (mut pos, mut normal) = (pos0, n0);
                let angle = draws.next(rng) * std::f32::consts::TAU;
                let start_dir = {
                    let t = (Vec3::X - normal * normal.x).normalize_or(Vec3::Y);
                    let b = normal.cross(t);
                    t * angle.cos() + b * angle.sin()
                };
                let mut dir = match &p.init_dir {
                    Some(m) => self.motion_dir(m, chain, pos, normal, start_dir),
                    None => start_dir,
                };
                let total = if moving { life * MOVE_SPEED } else { 0.0 };
                let mut travelled = 0.0;
                loop {
                    let age = if total > 0.0 { (travelled / total).min(1.0) } else { 0.0 };
                    let ctx = self.ctx(pos, normal, chain, age);
                    let c = channels(p, &ctx, &randoms, self.map.bounds);
                    if c[ch::KILL] >= 1.0 {
                        break;
                    }
                    if let Some(s) = self.make_stamp(p, &c, pos, normal, dir, chain, delay + age * life, order, &mut draws, rng) {
                        order += 1;
                        stamps.push(s);
                        // A chained particle stamps where its parent does,
                        // inheriting the listed channels.
                        if let Some(child) = p.chain.as_ref().and_then(|n| script.particles.get(n)) {
                            let child_randoms: Vec<f32> = (0..child.modifiers.len() + child.vars.len()).map(|_| draws.next(rng)).collect();
                            let mut cc = channels(child, &ctx, &child_randoms, self.map.bounds);
                            for &k in &child.inherit {
                                cc[k] = c[k];
                            }
                            if let Some(s) = self.make_stamp(child, &cc, pos, normal, dir, chain, delay + age * life, order, &mut draws, rng) {
                                order += 1;
                                stamps.push(s);
                            }
                        }
                    }
                    if total <= 0.0 || travelled >= total {
                        break;
                    }
                    let step = c[ch::SPACING].max(0.005);
                    if self.project.is_some() {
                        // Baked mesh (Cell), retail 0x520300: the direction is set once at
                        // birth (`initDir`) and afterwards only the attract force turns it,
                        // by `attractForce` towards the attract target on every 1/50 s tick
                        // (0.02 units); the particle dies when the target lies more than
                        // ~134 degrees (dot < -0.7) behind it.
                        if let Some(m) = p.attract.as_ref().filter(|_| p.attract_force > 0.0) {
                            let nd = self.motion_dir(m, chain, pos, normal, dir);
                            if nd != Vec3::ZERO {
                                if p.init_dir.is_none() && dir.dot(nd) < -0.7 {
                                    break;
                                }
                                let k = 1.0 - (1.0 - p.attract_force.min(1.0)).powf(step / TICK_STEP);
                                dir = (dir + (nd - dir) * k).normalize_or(nd);
                            }
                        }
                    } else if let Some(m) = p.attract.as_ref().or(p.init_dir.as_ref()).or(p.align.as_ref()) {
                        let nd = self.motion_dir(m, chain, pos, normal, dir);
                        if nd != Vec3::ZERO {
                            dir = nd;
                        }
                    }
                    let (np, nn) = match self.project {
                        Some(f) => f(pos + dir * step),
                        None => self.prims.field.project(pos + dir * step),
                    };
                    pos = np;
                    normal = nn;
                    travelled += step;
                    if stamps.len() > 400_000 {
                        break;
                    }
                }
            }
        }
        stamps.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.order.cmp(&b.order)));
        for s in &stamps {
            self.apply_stamp(s);
        }
        self.canvas.stamps += stamps.len();
    }

    fn apply_stamp(&mut self, s: &Stamp) {
        let brush = s.brush.as_ref().and_then(|b| self.job.brushes.get(b));
        let diffuse = brush.and_then(|b| b.diffuse.as_ref());
        let spec_bump = brush.and_then(|b| b.spec_bump.as_ref());
        let bitangent = s.normal.cross(s.tangent);
        let (hx, hy) = s.half;
        let radius = (hx * hx + hy * hy).sqrt();
        let map = self.map;
        let canvas = &mut self.canvas;
        map.near(s.pos, radius, |i| {
            let smp = &map.samples[i as usize];
            if smp.normal.dot(s.normal) < 0.2 {
                return;
            }
            let d = smp.pos - s.pos;
            if d.length_squared() > radius * radius {
                return;
            }
            let u = d.dot(s.tangent) / (2.0 * hx) + 0.5;
            let v = d.dot(bitangent) / (2.0 * hy) + 0.5;
            if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                return;
            }
            let bd = match diffuse {
                Some(t) => t.sample(u, v),
                None => {
                    let r2 = (2.0 * u - 1.0).powi(2) + (2.0 * v - 1.0).powi(2);
                    [1.0, 1.0, 1.0, (1.0 - r2).max(0.0)]
                }
            };
            let sb = spec_bump.map_or([1.0, 1.0, 0.5, bd[3]], |t| t.sample(u, v));
            let i = i as usize;
            if let Some(col) = s.color {
                let a = bd[3] * s.alphas[0];
                if a > 0.0 {
                    let dst = &mut canvas.diffuse[i];
                    for k in 0..3 {
                        dst[k] = apply_blend(s.blends[0], dst[k], col[k] * bd[k], a);
                    }
                }
            }
            let a = sb[3] * s.alphas[1];
            if a > 0.0 {
                canvas.spec[i] = apply_blend(s.blends[1], canvas.spec[i], s.spec * sb[0], a);
            }
            let a = sb[3] * s.alphas[2];
            if a > 0.0 {
                canvas.bump[i] = apply_blend(s.blends[2], canvas.bump[i], s.bump * sb[2], a);
            }
        });
    }
}

/// Dilate covered texels into the gutters (pipeline stages 7 / 8).
fn dilate<const N: usize>(size: u32, data: &mut [[f32; N]], covered: &mut [bool], steps: usize) {
    let s = size as i32;
    for _ in 0..steps {
        let snapshot = covered.to_vec();
        let mut changed = false;
        for y in 0..s {
            for x in 0..s {
                let i = (y * s + x) as usize;
                if snapshot[i] {
                    continue;
                }
                let mut sum = [0.0; N];
                let mut n = 0.0;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= s || ny >= s {
                        continue;
                    }
                    let j = (ny * s + nx) as usize;
                    if snapshot[j] {
                        for k in 0..N {
                            sum[k] += data[j][k];
                        }
                        n += 1.0;
                    }
                }
                if n > 0.0 {
                    data[i] = sum.map(|v| v / n);
                    covered[i] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Paint a skin: run the three layer scripts onto the texel map, dilate, and
/// derive the normal map from the bump layer.
pub fn paint(prims: &Primitives, map: &TexelMap, job: &PaintJob) -> Painted {
    paint_on(prims, map, job, None)
}

/// [`paint`] on a skin whose surface is not the implicit field of `prims`:
/// `project` puts a crawling particle back on it (position, normal).
pub fn paint_on(prims: &Primitives, map: &TexelMap, job: &PaintJob, project: Option<&dyn Fn(Vec3) -> (Vec3, Vec3)>) -> Painted {
    let n = map.samples.len();
    let mut painter = Painter {
        prims,
        map,
        job,
        canvas: Canvas { diffuse: vec![job.colors[0]; n], spec: vec![0.05; n], bump: vec![0.5; n], settings: Settings::default(), stamps: 0 },
        project,
        adjacency: if project.is_some() { weld_adjacency(&map.triangles) } else { Vec::new() },
        global: std::cell::Cell::new(job.seeds[0] ^ 0x1D2C_3B4A),
    };
    for (layer, script) in job.scripts.iter().enumerate() {
        let Some((script, instance)) = script else { continue };
        let Some(entry) = script.entry(*instance) else { continue };
        let mut rng = Rng::new(((job.seeds[layer] as u64) << 32) ^ *instance as u64);
        // ADAPTED: retail's global generator state is `rdtsc`-seeded; the layer's seed
        // and the script's id stand in for it (reproducible, differs per style).
        painter.global.set(job.seeds[layer] ^ *instance ^ 0x1D2C_3B4A);
        painter.run_effect(script, entry, &mut rng, 0);
    }
    let size = map.size;
    let texels = (size * size) as usize;
    // r, g, b, bump and the specular level (the dilate fills them together).
    let mut colour = vec![[0.0f32; 5]; texels];
    let mut covered = vec![false; texels];
    let mut spec_sum = 0.0;
    for (i, s) in map.samples.iter().enumerate() {
        let t = s.texel as usize;
        let d = painter.canvas.diffuse[i];
        colour[t] = [d[0], d[1], d[2], painter.canvas.bump[i], painter.canvas.spec[i]];
        covered[t] = true;
        spec_sum += painter.canvas.spec[i];
    }
    dilate(size, &mut colour, &mut covered, DILATE_STEPS);
    let fill = job.colors[0];
    let mut diffuse = vec![0u8; texels * 4];
    for (i, c) in colour.iter().enumerate() {
        let c = if covered[i] { [c[0], c[1], c[2]] } else { fill };
        for k in 0..3 {
            diffuse[i * 4 + k] = (c[k].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        diffuse[i * 4 + 3] = 255;
    }
    // Bump -> normal (`skpBumpToNormal`, height 5/3 per texel step).
    let s = size as i32;
    let height = |x: i32, y: i32| {
        let i = (y.clamp(0, s - 1) * s + x.clamp(0, s - 1)) as usize;
        if covered[i] {
            colour[i][3]
        } else {
            0.5
        }
    };
    let mut normal = vec![0u8; texels * 4];
    for y in 0..s {
        for x in 0..s {
            let dx = (height(x + 1, y) - height(x - 1, y)) * 0.5 * BUMP_TO_NORMAL;
            let dy = (height(x, y + 1) - height(x, y - 1)) * 0.5 * BUMP_TO_NORMAL;
            let nrm = Vec3::new(-dx, -dy, 1.0).normalize();
            let i = (y * s + x) as usize * 4;
            normal[i] = ((nrm.x * 0.5 + 0.5) * 255.0).round() as u8;
            normal[i + 1] = ((nrm.y * 0.5 + 0.5) * 255.0).round() as u8;
            normal[i + 2] = ((nrm.z * 0.5 + 0.5) * 255.0).round() as u8;
            normal[i + 3] = 255;
        }
    }
    let spec_map = colour
        .iter()
        .zip(&covered)
        .map(|(c, ok)| ((if *ok { c[4] } else { 0.05 }).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    let bump_map = colour
        .iter()
        .zip(&covered)
        .map(|(c, ok)| ((if *ok { c[3] } else { 0.5 }).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    Painted {
        size,
        diffuse,
        normal,
        spec: if n > 0 { spec_sum / n as f32 } else { 0.05 },
        spec_map,
        bump_map,
        settings: painter.canvas.settings,
        stamps: painter.canvas.stamps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature_skin::{build, order_records, unwrap, BlockKind, Chain, ChartId, SkinRecord, RES_HIGH_QUALITY};

    /// An `n x n` grid of quads over [-0.5, 0.5]^2 (z = 0, normal +Z) as the
    /// texel map of a baked mesh, with a spine along +Y below it.
    fn flat_grid(n: usize) -> (Primitives, TexelMap) {
        let mut un = Unwrapped {
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            source: vec![],
            triangles: vec![],
            chart: vec![],
            charts: vec![ChartId { chain: 0, side: 0 }],
            texel_density: 0.0,
        };
        for j in 0..=n {
            for i in 0..=n {
                let (fx, fy) = (i as f32 / n as f32, j as f32 / n as f32);
                un.positions.push(Vec3::new(fx - 0.5, fy - 0.5, 0.0));
                un.normals.push(Vec3::Z);
                un.uvs.push([0.1 + 0.8 * fx, 0.1 + 0.8 * fy]);
                un.source.push(un.source.len() as u32);
                un.chart.push(ChartId { chain: 0, side: 0 });
            }
        }
        for j in 0..n {
            for i in 0..n {
                let a = (j * (n + 1) + i) as u32;
                let b = a + n as u32 + 1;
                un.triangles.push([a, b, a + 1]);
                un.triangles.push([a + 1, b, b + 1]);
            }
        }
        let chain = Chain { points: vec![Vec3::new(0.0, -0.5, -0.1), Vec3::new(0.0, 0.5, -0.1)], radii: vec![0.1, 0.1], limb: false, up: Vec3::Z };
        let prims = Primitives { chains: vec![chain], ..Default::default() };
        let map = texel_map(&prims, &un);
        (prims, map)
    }

    fn placed(d: &Distribute, seed: u64) -> Vec<(Vec3, Vec3, u16)> {
        let (prims, map) = flat_grid(24);
        let job = PaintJob { scripts: [None, None, None], colors: [[1.0; 3]; 3], seeds: [1; 3], brushes: &HashMap::new() };
        let project = |p: Vec3| (p, Vec3::Z);
        let painter = Painter {
            prims: &prims,
            map: &map,
            job: &job,
            canvas: Canvas { diffuse: vec![], spec: vec![], bump: vec![], settings: Settings::default(), stamps: 0 },
            project: Some(&project),
            adjacency: weld_adjacency(&map.triangles),
            global: std::cell::Cell::new(seed as u32),
        };
        painter.samples_mesh(d)
    }

    #[test]
    fn retail_generators_follow_the_code_image() {
        // 0x936020: state * 0x41C64E6D + 0x3039 (64-bit), result = bits 16..47.
        let mut r = RetailRng(0x1234_5678);
        assert_eq!(r.uint(), 0x6403_0B71);
        assert_eq!(r.0, 0x0B71_9151);
        // 0xA68F70: the same draw scaled by n (high word of the product).
        assert_eq!(RetailRng(0x1234_5678).int(100), 39);
        // 0x936040: state * 0x278DDE6D, (int)state * 2^-32 + 0.5.
        let mut f = RetailRng(0x1234_5678);
        assert!((f.float() - 0.544_386).abs() < 1e-5);
        assert_eq!(f.0, 0x0B5C_E118);
        // Every draw stays inside the unit interval.
        let mut g = RetailRng(7);
        assert!((0..1000).all(|_| (0.0..1.0).contains(&g.float())));
    }

    #[test]
    fn part_charts_take_only_part_and_unfiltered_distributes() {
        // A part chart carries flag 4: region -1 stamps it, a parts mask stamps it, torso /
        // back / limb masks do not (retail 0x5323..: `flags & region`), `inverse` flips.
        let (prims, mut map) = flat_grid(24);
        map.parts = true;
        let job = PaintJob { scripts: [None, None, None], colors: [[1.0; 3]; 3], seeds: [1; 3], brushes: &HashMap::new() };
        let project = |p: Vec3| (p, Vec3::Z);
        let painter = Painter {
            prims: &prims,
            map: &map,
            job: &job,
            canvas: Canvas { diffuse: vec![], spec: vec![], bump: vec![], settings: Settings::default(), stamps: 0 },
            project: Some(&project),
            adjacency: weld_adjacency(&map.triangles),
            global: std::cell::Cell::new(9),
        };
        let count = |d: Distribute| painter.samples_mesh(&d).len();
        assert!(count(Distribute { spacing: 0.1, ..Default::default() }) > 40);
        assert!(count(Distribute { spacing: 0.1, region: Some(region::PARTS), ..Default::default() }) > 40);
        assert_eq!(count(Distribute { spacing: 0.1, region: Some(region::TORSO | region::LIMBS), ..Default::default() }), 0);
        assert_eq!(count(Distribute { spacing: 0.1, region: Some(region::BACK | region::BELLY), back: Some(0.0), belly: Some(0.0), ..Default::default() }), 0);
        assert!(count(Distribute { spacing: 0.1, region: Some(region::TORSO), inverse: true, ..Default::default() }) > 40);
    }

    #[test]
    fn range_compares_the_vertebra_index_of_the_triangle() {
        // Retail 0x5323B8: `numVertebrae * range[0] <= vertebra <= numVertebrae * range[1]`.
        let (prims, mut map) = flat_grid(24);
        let tri_count = map.triangles.len();
        // Left half of the grid = vertebra 0..2, right half = 3..5 (by centroid x).
        map.vertebrae = 6;
        map.vertebra = map
            .triangles
            .iter()
            .map(|t| (((t.0[0].x + t.0[1].x + t.0[2].x) / 3.0 + 0.5) * 5.999).floor() as u8)
            .collect();
        assert_eq!(map.vertebra.len(), tri_count);
        let job = PaintJob { scripts: [None, None, None], colors: [[1.0; 3]; 3], seeds: [1; 3], brushes: &HashMap::new() };
        let project = |p: Vec3| (p, Vec3::Z);
        let painter = Painter {
            prims: &prims,
            map: &map,
            job: &job,
            canvas: Canvas { diffuse: vec![], spec: vec![], bump: vec![], settings: Settings::default(), stamps: 0 },
            project: Some(&project),
            adjacency: weld_adjacency(&map.triangles),
            global: std::cell::Cell::new(5),
        };
        let d = Distribute { spacing: 0.08, region: Some(region::TORSO), body_range: (0.5, 1.0), ..Default::default() };
        let right = painter.samples_mesh(&d);
        assert!(right.len() > 10, "{}", right.len());
        // vertebra >= 3 -> centroids at x >= 0 (a triangle of vertebra 3 may start just below 0).
        assert!(right.iter().all(|p| p.0.x > -0.1), "{:?}", right.iter().map(|p| p.0.x).fold(f32::MAX, f32::min));
    }

    #[test]
    fn retail_placement_keeps_the_spacing_and_fills_the_surface() {
        let d = Distribute { spacing: 0.1, ..Default::default() };
        let pts = placed(&d, 7);
        assert!(pts.len() > 40, "{}", pts.len());
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                assert!((a.0 - b.0).length() >= 0.1 - 1e-4, "{:?} {:?}", a.0, b.0);
            }
        }
        // Reproducible per seed, different between seeds.
        assert_eq!(pts.iter().map(|p| p.0.to_array()).collect::<Vec<_>>(), placed(&d, 7).iter().map(|p| p.0.to_array()).collect::<Vec<_>>());
        assert_ne!(pts[0].0, placed(&d, 8)[0].0);
    }

    #[test]
    fn retail_placement_is_ordered_limited_and_centred_on_demand() {
        // `ordered` starts at the first triangle (centroid of (0,0), (0,1/24), (1/24,0) of the grid).
        let ordered = placed(&Distribute { spacing: 0.1, ordered: true, ..Default::default() }, 3);
        let first = ordered[0].0;
        assert!((first - Vec3::new(-0.5 + 1.0 / 72.0, -0.5 + 1.0 / 72.0, 0.0)).length() < 1e-4, "{first:?}");
        // `limit` counts placements.
        assert_eq!(placed(&Distribute { spacing: 0.1, limit: Some(5), ..Default::default() }, 3).len(), 5);
        // `-centerOnly`: every point sits on the x = 0 plane, one per row of triangles.
        let centre = placed(&Distribute { spacing: 0.04, center_only: true, ..Default::default() }, 3);
        assert!(!centre.is_empty());
        assert!(centre.iter().all(|p| p.0.x.abs() <= 0.01), "{:?}", centre.iter().map(|p| p.0.x).collect::<Vec<_>>());
    }

    #[test]
    fn retail_placement_filters_by_region_and_inverse() {
        let torso = Distribute { spacing: 0.1, region: Some(region::TORSO), ..Default::default() };
        assert!(placed(&torso, 1).len() > 40);
        // A parts-only or limbs-only region puts nothing on a vertebra skin;
        // `inverse` flips the verdict.
        let parts = Distribute { spacing: 0.1, region: Some(region::PARTS), ..Default::default() };
        assert!(placed(&parts, 1).is_empty());
        assert!(placed(&Distribute { inverse: true, ..parts }, 1).len() > 40);
        // Back keeps the points above the spine (theta within 90 degrees of up); the
        // flat grid lies above its spine, so back keeps all and belly none.
        let back = Distribute { spacing: 0.1, region: Some(region::TORSO | region::BACK), back: Some(0.0), ..Default::default() };
        assert!(placed(&back, 1).len() > 40);
        let belly = Distribute { spacing: 0.1, region: Some(region::TORSO | region::BELLY), belly: Some(0.0), ..Default::default() };
        assert!(placed(&belly, 1).is_empty());
    }

    /// A script in the installed syntax (written for the test, the shape of
    /// `skinpaint_base_blankMatte`).
    const BLANK_MATTE: &str = "# test brush
SPSkinPaintParticle skinpaint_base_blankMatte_base_brush
  brush skn_brush_blankTooth
  alpha 1
  size 0.22 -vary 0.2
  life 0
  delay 0 1
  diffuseColor color1
  diffuseAlpha 0.65 -vary 0.1
  diffuseBlend alpha
  specularAlpha 0.95 -vary 0.1
  specularScale 0.03 -vary 0.1
  bumpAlpha 0.1 -vary 0.1
  bumpScale 1 -vary 0.1
  modifier random
    diffuseVal -mult 0.98 1.02 -range 0 0.1 -wrap
    diffuseHue -add -3 3 -range 0 0.333 -wrap
    rotate -add 0 360 -range 0 0.5 -wrap
  end
end
SPSkinPaintDistribute skinpaint_base_blankMatte_pattern_dist
  particle skinpaint_base_blankMatte_base_brush
  spacing 0.05
end
effect skinpaint_base_blankMatte_pattern
  SPSkinPaintDistribute skinpaint_base_blankMatte_pattern_dist
end
effect skinpaint_base_blankMatte_clear
  SPSkinPaintClear -diffuse color1 -spec 0.05 -bump .5
  SPSkinPaintSettings -exponent 40 -phong 1 -gloss .4
end
sequence skinpaint_base_blankMatte_seq
  play skinpaint_base_blankMatte_clear
  play skinpaint_base_blankMatte_pattern
end
effect skinpaint_base_blankMatte
  sequence skinpaint_base_blankMatte_seq
end
";

    fn body() -> (Primitives, Unwrapped) {
        let mut blocks = Vec::new();
        for i in 0..5 {
            blocks.push(SkinRecord {
                kind: BlockKind::Vertebra,
                flags: crate::creature_skin::flag::USES_SKIN,
                scale: [0.6, 0.9, 1.1, 0.9, 0.6][i],
                position: Vec3::new(0.0, -0.3 + i as f32 * 0.15, 0.4),
                parent: i as i16 - 1,
                ..Default::default()
            });
        }
        let records = order_records(blocks);
        let (prims, mesh) = build(&records, RES_HIGH_QUALITY).unwrap();
        let un = unwrap(&prims, &mesh);
        (prims, un)
    }

    #[test]
    fn parses_blank_matte() {
        let s = parse(BLANK_MATTE);
        assert!(s.unknown.is_empty(), "{:?}", s.unknown);
        assert_eq!(s.entry(crate::world_objects::name_id("skinpaint_base_blankMatte")), Some("skinpaint_base_blankMatte"));
        let p = &s.particles["skinpaint_base_blankMatte_base_brush"];
        assert_eq!(p.brushes, vec!["skn_brush_blankTooth".to_string()]);
        assert_eq!(p.color, Some(ColorRef::Layer(0)));
        assert_eq!(p.delay, (0.0, 1.0));
        assert_eq!(p.modifiers, vec![Source::Random]);
        let size = p.vars.iter().find(|v| v.channel == ch::SIZE).unwrap();
        assert!(size.curve && size.op == Op::Mult && size.values == vec![0.22] && (size.vary - 0.2).abs() < 1e-6);
        let hue = p.vars.iter().find(|v| v.channel == ch::HUE).unwrap();
        assert_eq!((hue.op, hue.values.clone(), hue.range, hue.flag, hue.modifier), (Op::Add, vec![-3.0, 3.0], (0.0, 0.333), RangeFlag::Wrap, Some(0)));
        let rot = p.vars.iter().find(|v| v.channel == ch::ROTATION).unwrap();
        assert_eq!(rot.values, vec![0.0, 360.0]);
        assert_eq!(s.distributes["skinpaint_base_blankMatte_pattern_dist"].spacing, 0.05);
        let clear = &s.effects["skinpaint_base_blankMatte_clear"];
        assert_eq!(clear[0], Item::Clear(Clear { diffuse: Some(ColorRef::Layer(0)), spec: Some(0.05), bump: Some(0.5) }));
        assert_eq!(clear[1], Item::Settings { exponent: Some(40.0), phong: Some(1.0), gloss: Some(0.4) });
        assert_eq!(s.sequences["skinpaint_base_blankMatte_seq"].len(), 2);
    }

    #[test]
    fn macros_regions_and_channel_math() {
        let text = "define m()\n  brush dog_fur\n  modifier region\n    alpha -set 0 0 -range 1 1\n  end\nenddef\n\
SPSkinPaintParticle p\n  create m()\n  life 0.5 -vary 0.25\n  attract aroundSpine -reverse\n  size 0.1 0.2 -vary 0\n  diffuseVal -mult 1.2\nend\n\
SPSkinPaintDistribute d\n  particle p\n  spacing 0.08 -ordered\n  region -torso\nend\n\
effect e\n  SPSkinPaintDistribute d\nend\n";
        let s = parse(text);
        let p = &s.particles["p"];
        assert_eq!(p.brushes, vec!["dog_fur".to_string()]);
        assert_eq!(p.life, (0.375, 0.625));
        assert_eq!(p.attract, Some(Motion { kind: MotionKind::AroundSpine, vector: Vec3::ZERO, reverse: true }));
        let d = &s.distributes["d"];
        assert!(d.ordered && d.region == Some(region::TORSO) && d.spacing == 0.08);
        let ctx = |region: u8, age: f32| Ctx { pos: Vec3::ZERO, normal: Vec3::Z, region, along: 0.0, angle: 0.0, age };
        let r = [0.5; 8];
        let torso = channels(p, &ctx(0, 0.5), &r, (Vec3::ZERO, Vec3::ONE));
        assert_eq!(torso[ch::ALPHA], 1.0);
        assert!((torso[ch::SIZE] - 0.15).abs() < 1e-6);
        assert!((torso[ch::VAL] - 1.2).abs() < 1e-6);
        let limb = channels(p, &ctx(1, 0.0), &r, (Vec3::ZERO, Vec3::ONE));
        assert_eq!(limb[ch::ALPHA], 0.0);
    }

    #[test]
    fn paints_blank_matte_deterministically() {
        let (prims, un) = body();
        let map = texel_map(&prims, &un);
        assert!(map.samples.len() > 20_000, "{} texels", map.samples.len());
        let script = parse(BLANK_MATTE);
        let brushes = HashMap::new();
        let instance = crate::world_objects::name_id("skinpaint_base_blankMatte");
        let job = PaintJob { scripts: [Some((&script, instance)), None, None], colors: [[0.6, 0.3, 0.2], [1.0; 3], [1.0; 3]], seeds: DEFAULT_SEEDS, brushes: &brushes };
        let a = paint(&prims, &map, &job);
        assert!(a.stamps > 200, "{} stamps", a.stamps);
        assert_eq!(a.diffuse.len(), 512 * 512 * 4);
        assert_eq!(a.settings, Settings { exponent: 40.0, phong: 1.0, gloss: 0.4 });
        // Colour 1 with small HSV jitter on every painted texel.
        let mut n = 0;
        let mut varied = false;
        for s in &map.samples {
            let i = s.texel as usize * 4;
            let c = [a.diffuse[i], a.diffuse[i + 1], a.diffuse[i + 2]];
            assert!((c[0] as i32 - 153).abs() < 40 && (c[1] as i32 - 77).abs() < 40 && (c[2] as i32 - 51).abs() < 40, "{c:?}");
            varied |= c != [153, 77, 51];
            n += 1;
        }
        assert!(n > 0 && varied);
        let b = paint(&prims, &map, &job);
        assert_eq!(a.diffuse, b.diffuse);
        // Another seed paints differently.
        let job2 = PaintJob { seeds: [1, 2, 3], ..job };
        assert_ne!(paint(&prims, &map, &job2).diffuse, a.diffuse);
    }

    #[test]
    fn hsv_round_trip() {
        for c in [[0.2, 0.4, 0.6], [1.0, 0.0, 0.0], [0.5, 0.5, 0.5], [0.1, 0.9, 0.3]] {
            let back = hsv_to_rgb(rgb_to_hsv(c));
            for k in 0..3 {
                assert!((back[k] - c[k]).abs() < 1e-4, "{c:?} {back:?}");
            }
        }
    }

    /// Every installed brush script parses without unknown keywords that
    /// change the picture (bounded: the ~120 `pfx` texts of one group).
    #[test]
    fn installed_skinpaint_scripts_parse() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let keys: Vec<_> = index.records.keys().filter(|k| k.group_id == PFX_GROUP && k.type_id == PFX_TYPE).copied().collect();
        let mut parsed = 0;
        let mut entries = 0;
        for key in keys {
            let Ok(bytes) = index.read(key) else { continue };
            let text = String::from_utf8_lossy(&bytes);
            if !text.contains("SPSkinPaint") {
                continue;
            }
            let s = parse(&text);
            parsed += 1;
            if s.entry(key.instance_id).is_some() {
                entries += 1;
            }
            let bad: Vec<_> = s.unknown.iter().filter(|u| !u.starts_with("skinpaint_")).collect();
            assert!(bad.is_empty(), "{:08X}: {bad:?}", key.instance_id);
        }
        assert!(parsed == 0 || parsed > 90, "{parsed}");
        assert_eq!(parsed, entries);
    }
}
