//! Animation-event sounds of the creatures, tribe members, citizens and
//! cinematic actors that play retail clips (`creature_clips`).
//!
//! RETAIL (`notes\audio-sweep-notes.md` A1-A3, march2017 addresses):
//! * A clip's info keys carry events (SporeModder-FX `AnimationEvent`, type =
//!   `flags & 0x20007`): `sound` (0, id = FNV of the soundProp), `sound2`
//!   (6, sets the game audio parameter named by the event), `footstep` (7).
//!   The animation player hands them to the anim sound player 0x9A4FA0
//!   (event name, creature, block index, position): names starting with `v`
//!   are voices, `foot...` names go to the footstep resolver, the rest play
//!   as 3D one-shots at the creature.
//! * Footsteps (type 7, 0x9A6910): the footfall effect `CR_footfall_creature`
//!   and the sound `foot_step` resolved by the audio system's `footsteps`
//!   table (0xA3A190) to `foot_step_<foot>_<l|r>_<surface>` with gain / pitch
//!   / high-pass from the `footcurve*` tuning (0xA18D10, 0xA3ABF0).
//!
//! OpenSpore: [`clip_cues`] reads a clip's events once; [`fire`] plays the
//! ones a clip passed since the last frame through `cell_audio::NamedSounds`
//! (soundProp gain, polyphony, 3D roll-off by the listener distance).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::animation::{Animation, Keys};

/// SporeModder-FX `AnimationEvent.FLAG_TYPE_MASK` and the types used here.
pub const EVENT_TYPE_MASK: u32 = 0x2_0007;
pub const EVENT_SOUND: u32 = 0;
pub const EVENT_SOUND2: u32 = 6;
pub const EVENT_FOOTSTEP: u32 = 7;
/// `AnimationEvent.FLAG_DISABLED`: the event is in the clip but off.
pub const EVENT_DISABLED: u32 = 0x8;

/// What a clip event asks of the audio.
#[derive(Clone, Debug, PartialEq)]
pub enum Cue {
    /// Play a sound (soundProp or SNR id; the event's name when it has one).
    Sound { id: u32, name: String },
    /// `sound2`: set the game audio parameter `name` (FNV `id`) to `value`.
    Param { id: u32, name: String, value: f32 },
    /// A foot touches the ground: the foot block chosen by the selector.
    Footstep { capability: String, select_flags: u32 },
}

/// One audio event of a clip at `time` seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipCue {
    pub time: f32,
    pub cue: Cue,
    /// Event `maxSqrDist` (retail units squared; 0 = unlimited).
    pub max_sqr_distance: f32,
    /// A disabled `sfx_trg_com_*` hit sound: `(rank, count)` among the
    /// disabled hit sounds at the same instant; one of them plays per blow
    /// ([`hit_variant`]). `None` for ordinary events.
    pub pick: Option<(u8, u8)>,
}

/// The audio events of a clip, by time.
///
/// RETAIL data: the attack clips of the tool abilities carry their hit sounds as
/// events with `FLAG_DISABLED` set (`spear_stab`: `sfx_trg_com_sp_lo`, `_med`,
/// `_hi`, `_miss` at the blow; `axe_attack`: `sfx_trg_com_axe_lo/med/hi`;
/// `torch_poke`: `sfx_trg_com_club_lo/med/hi`; the chieftain's
/// `chief_attack_01`: `sfx_trg_com_club_hi`). The four spear sounds are
/// alternatives, not a chord (played together they were a loud stack on every
/// stab). ADAPTED: the retail rule that chooses among them (the damage of the
/// blow, INFERRED) is not decoded: one of the non-miss variants of the instant
/// plays per blow, picked by [`hit_variant`]; `_miss` is left out (a thrown
/// spear's own miss is `tribal_weapon_audio`). Disabled events of other names
/// play as before: the Cell clips mark `sfx_cell_eat` disabled too, and the
/// Cell stage plays it.
pub fn clip_cues(anim: &Animation) -> Vec<ClipCue> {
    let mut out = vec![];
    for channel in &anim.channels {
        for component in &channel.components {
            let Keys::Info(keys) = &component.keys else { continue };
            for k in keys {
                for event in anim.events.iter().skip(k.event_start as usize).take(k.event_count as usize) {
                    let kind = event.flags & EVENT_TYPE_MASK;
                    let name = event.name.trim_end_matches('\0').to_string();
                    // The disabled `sfx_trg_com_*` hit sounds are alternatives (below).
                    let hit_set = event.flags & EVENT_DISABLED != 0 && kind == EVENT_SOUND && name.starts_with("sfx_trg_com_");
                    if hit_set && name.ends_with("_miss") {
                        continue;
                    }
                    let cue = match kind {
                        EVENT_SOUND if event.id != 0 => Cue::Sound { id: event.id, name },
                        EVENT_SOUND2 if !name.is_empty() => {
                            let value = if event.flags & 0x300 == 0x200 {
                                f32::from_bits(event.parameter0)
                            } else {
                                event.parameter0 as i32 as f32
                            };
                            Cue::Param { id: crate::world_objects::name_id(&name), name, value }
                        }
                        EVENT_FOOTSTEP => Cue::Footstep {
                            capability: event.selectors[0].capability.trim_end_matches('\0').to_string(),
                            select_flags: event.selectors[0].select_flags,
                        },
                        _ => continue,
                    };
                    out.push(ClipCue {
                        time: k.frame as f32 * anim.frame_length,
                        cue,
                        max_sqr_distance: event.max_squared_distance,
                        pick: hit_set.then_some((0, 1)),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    // Rank the disabled hit sounds of each instant.
    let mut i = 0;
    while i < out.len() {
        if out[i].pick.is_none() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < out.len() && out[j].pick.is_some() && (out[j].time - out[i].time).abs() < 1e-3 {
            j += 1;
        }
        let count = (j - i) as u8;
        for (rank, cue) in out[i..j].iter_mut().enumerate() {
            cue.pick = Some((rank as u8, count));
        }
        i = j;
    }
    out
}

/// Pure: whether the disabled hit sound ranked `rank` of `count` plays for this
/// blow: one variant per (actor, clip, instant), the same for every variant of
/// the instant so exactly one plays.
pub fn hit_variant(actor: u64, clip: u32, time: f32, rank: u8, count: u8) -> bool {
    let h = actor.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (clip as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ ((time * 1000.0) as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    let h = (h ^ (h >> 29)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((h >> 32) % count.max(1) as u64) as u8 == rank
}

static CUES: OnceLock<Mutex<HashMap<u32, Arc<Vec<ClipCue>>>>> = OnceLock::new();

/// The audio events of a playing clip (its `Clip::anim`), cached by the
/// clip's id.
pub fn cues_of(clip: &crate::creature_clips::Clip) -> Arc<Vec<ClipCue>> {
    let map = CUES.get_or_init(Default::default);
    if let Some(c) = map.lock().unwrap_or_else(|e| e.into_inner()).get(&clip.id) {
        return c.clone();
    }
    let cues = Arc::new(clip_cues(&clip.anim));
    map.lock().unwrap_or_else(|e| e.into_inner()).insert(clip.id, cues.clone());
    cues
}

/// Pure: whether an event at `at` lies in the clip time passed this frame,
/// `(from, to]` (`from` < 0: the clip started, from its first frame); a
/// looping clip that wrapped also covers its start.
pub fn due(from: f32, to: f32, length: f32, looped: bool, at: f32) -> bool {
    if from < 0.0 {
        return at <= to + 1e-4;
    }
    if !looped || length <= 0.0 {
        return at > from && at <= to;
    }
    let (a, b) = (from.rem_euclid(length), to.rem_euclid(length));
    let laps = ((to / length).floor() - (from / length).floor()) as i64;
    match laps {
        0 => at > a && at <= b,
        1 => at > a || at <= b,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Footsteps
// ---------------------------------------------------------------------------

/// `foottype` (0xDD94FB65) values of the installed movement parts and the
/// foot names of the `foot_step_<foot>_<l|r>_<surface>` soundProps. VERIFIED:
/// `spike` = FNV("spike") (insect feet); INFERRED by the parts that carry
/// them (the other five values are not FNV of the names): sucker feet `web`,
/// default `paw`, dino `bird`, primate `bare`, hoof `hoof`.
pub const FOOT_TYPES: [(u32, &str); 6] = [
    (0x002A_E8E9, "spike"),
    (0x4666_1A85, "web"),
    (0x4A77_D123, "paw"),
    (0x5C0D_4A2C, "bird"),
    (0x76BE_6FDD, "bare"),
    (0xBBA0_E895, "hoof"),
];
/// `foottype` property of a part PROP.
pub const P_FOOTTYPE: u32 = 0xDD94_FB65;

/// Pure: the surface name of a footstep. ADAPTED: retail asks the terrain's
/// `surfacetype` resolver (0xA3A190) for the material under the foot; here
/// water depth picks `wade` / `splash`, the land `dirt` (the clips' own
/// stomp default, `sfx_foot_stomp_dirt`), the beach `sand`, steep or high
/// ground `rock`, ground among the flora `grass`.
pub fn surface(water_depth: f32, height_above_sea: f32, slope: f32, near_plants: bool) -> &'static str {
    if water_depth > 0.6 {
        "splash"
    } else if water_depth > 0.0 {
        "wade"
    } else if height_above_sea < 1.5 {
        "sand"
    } else if slope > 0.7 || height_above_sea > 60.0 {
        "rock"
    } else if near_plants {
        "grass"
    } else {
        "dirt"
    }
}

/// Pure: a piecewise-linear `footcurve*` value (x, y pairs; clamped ends).
pub fn curve(points: &[f32], x: f32) -> f32 {
    let pairs: Vec<(f32, f32)> = points.chunks_exact(2).map(|c| (c[0], c[1])).collect();
    match pairs.as_slice() {
        [] => 1.0,
        [(_, y)] => *y,
        _ => {
            if x <= pairs[0].0 {
                return pairs[0].1;
            }
            for w in pairs.windows(2) {
                let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                if x <= x1 {
                    let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 1.0 };
                    return y0 + (y1 - y0) * t;
                }
            }
            pairs[pairs.len() - 1].1
        }
    }
}

/// The `footsteps` soundProp's curves (VERIFIED data; inputs per 0xA3ABF0:
/// gain = numlegs x vel x mass x footsize x len, pitch = mass x footsize).
pub const NUMLEGS_TO_GAIN: [f32; 14] = [0.0, 1.0, 1.0, 1.0, 2.0, 1.0, 3.0, 0.9, 4.0, 0.8, 5.0, 0.7, 6.0, 0.6];
pub const VEL_TO_GAIN: [f32; 6] = [0.0, 0.3, 1.0, 0.5, 2.0, 1.0];

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

use bevy::prelude::*;

/// A creature playing a clip this frame.
struct Source {
    entity: Entity,
    clip: Arc<crate::creature_clips::Clip>,
    time: f32,
    looped: bool,
    pos: Vec3,
    player: bool,
    /// The player's species (avatar, tribe members, citizens, cinematic
    /// copies): its own mouth and feet.
    self_species: bool,
}

fn source_of(entity: Entity, clips: &crate::creature_clips::ClipPlayer, pos: Vec3, player: bool, self_species: bool) -> Option<Source> {
    let top = clips.current()?;
    Some(Source { entity, clip: top.bound.clip.clone(), time: top.time, looped: top.looped, pos, player, self_species })
}

#[derive(Default)]
pub struct Cursors {
    seen: HashMap<Entity, (u32, f32)>,
    /// The player's foot name (`foottype` of its first foot part) and mouth
    /// code (its mouth's `mouthtype`), by part count.
    player_foot: Option<(usize, &'static str)>,
    player_mouth: Option<(usize, &'static str)>,
    /// Per creature: the foot of its next footstep (left / right in turn).
    step_side: HashMap<Entity, bool>,
}

/// `mouthtype` (0x0E40C402) of the mouth parts: FNV of a three-letter code
/// (`ROA` radial omnivore 1, `JHD` jaw herbivore 4, `MCA` mandible
/// carnivore 1 ..., VERIFIED by the name registry); the voice soundProps
/// exist per mouth as `<event name><Mo>XCr` with the code's first two
/// letters (`vlib_idle_shrtRoXCr`, every voice name x the ten codes Ao Bc Bh
/// Fc Jc Jh Mc Po Ro Sh). INFERRED: the audio system's `mouthtype` resolver
/// (0xA3A190) appends it to `v...` voice events.
pub const P_MOUTHTYPE: u32 = 0x0E40_C402;
pub const MOUTH_CODES: [&str; 10] = ["Ao", "Bc", "Bh", "Fc", "Jc", "Jh", "Mc", "Po", "Ro", "Sh"];
/// ADAPTED: a creature whose mouth is not known (the voice set with the
/// most variants).
pub const DEFAULT_MOUTH: &str = "Ro";

/// Pure: the voice code of a `mouthtype` value (the two letters of the
/// three-letter code it hashes).
pub fn mouth_code(mouthtype: u32) -> Option<&'static str> {
    for code in MOUTH_CODES {
        let up = code.to_uppercase();
        for last in b'A'..=b'Z' {
            let name = format!("{up}{}", last as char);
            if crate::pd::fnv(&name) == mouthtype {
                return Some(code);
            }
        }
    }
    None
}

/// ADAPTED: other species' mouths are not read; each creature keeps one of
/// the ten voice sets.
fn npc_mouth(entity: Entity) -> &'static str {
    MOUTH_CODES[((entity.to_bits() as u32).wrapping_mul(2_654_435_761) >> 16) as usize % MOUTH_CODES.len()]
}

/// The voice code of the player's creature (its first mouth part with a
/// `mouthtype`).
pub(crate) fn player_mouth(originals: &crate::Originals, run: &crate::game::RunProgress) -> &'static str {
    for p in &run.parts {
        let key = crate::dbpf::ResourceKey {
            type_id: crate::asset_catalog::PROP,
            group_id: crate::asset_catalog::CREATURE_PARTS,
            instance_id: p.definition_id.unwrap_or(p.instance),
        };
        let Ok(props) = originals.resource_index.properties(key) else { continue };
        let Some(v) = props.0.get(&P_MOUTHTYPE).and_then(|v| v.values.first()) else { continue };
        let Some(id) = v.get(..4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes) else { continue };
        if let Some(code) = mouth_code(id) {
            return code;
        }
    }
    DEFAULT_MOUTH
}

/// `foot_step` resolved (retail `footsteps` resolver): the creature's foot,
/// the next side, the surface under it.
fn footstep_name(foot: &str, sides: &mut HashMap<Entity, bool>, entity: Entity, land: Option<&crate::terrain::LandSurface>, pos: Vec3) -> &'static str {
    // ADAPTED: the footstep events carry no foot selector; sides alternate.
    let right = sides.entry(entity).or_insert(false);
    *right = !*right;
    let side = if *right { "r" } else { "l" };
    let (depth, above, slope) = land.map_or((0.0, 10.0, 0.0), |l| terrain_at(l, pos));
    intern(format!("foot_step_{foot}_{side}_{}", surface(depth, above, slope, false)))
}

/// The first installed sound (soundProp or SNR) among the candidates.
fn pick<const N: usize>(library: &Option<Arc<crate::game_audio::AudioLibrary>>, names: [&'static str; N]) -> Option<&'static str> {
    let lib = library.as_ref()?;
    names.into_iter().find(|n| {
        let id = crate::world_objects::name_id(n);
        lib.contains(crate::game_audio::TYPE_SOUND_PROP, id) || lib.contains(crate::game_audio::TYPE_SNR, id)
    })
}

/// The same footstep on dirt (every foot has its dirt set).
fn fallback_dirt(name: &'static str) -> &'static str {
    match name.rsplit_once('_') {
        Some((head, _)) => intern(format!("{head}_dirt")),
        None => name,
    }
}

/// ADAPTED: creatures farther than this from the camera play no clip
/// sounds (retail skips voices beyond animation LOD 5, 0x9A4FA0).
pub const CLIP_AUDIO_RANGE: f32 = 250.0;

/// The foot name of the player's creature (its movement parts' `foottype`).
fn player_foot(originals: &crate::Originals, run: &crate::game::RunProgress) -> &'static str {
    for p in &run.parts {
        let key = crate::dbpf::ResourceKey {
            type_id: crate::asset_catalog::PROP,
            group_id: crate::asset_catalog::CREATURE_PARTS,
            instance_id: p.definition_id.unwrap_or(p.instance),
        };
        let Ok(props) = originals.resource_index.properties(key) else { continue };
        let Some(v) = props.0.get(&P_FOOTTYPE).and_then(|v| v.values.first()) else { continue };
        let Some(id) = v.get(..4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes) else { continue };
        if let Some((_, name)) = FOOT_TYPES.iter().find(|(h, _)| *h == id) {
            return name;
        }
    }
    "paw"
}

/// Play the audio events the clips passed since the last frame: sounds and
/// footsteps through `NamedSounds` with the distance to the camera,
/// `sound2` parameters into the Pd engine.
#[allow(clippy::too_many_arguments)]
pub fn fire(
    time: Res<Time<Real>>,
    originals: Option<Res<crate::Originals>>,
    run: Option<Res<crate::game::RunProgress>>,
    drive: Option<Res<crate::creature_clips::DriveState>>,
    walkers: Query<(Entity, &GlobalTransform, &crate::locomotion_preview::AdaptedWalk, Has<crate::game::Player>)>,
    crowd: Query<(Entity, &GlobalTransform, &crate::outfit_crowd::CrowdActor)>,
    actors: Query<(Entity, &GlobalTransform), With<crate::cinematics::CinematicActor>>,
    scene: Option<Res<crate::cinematics::CinematicScene>>,
    players: Query<&GlobalTransform, With<crate::game::Player>>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    land: Option<Res<crate::terrain::LandSurface>>,
    audio: Res<crate::game_audio::GameAudio>,
    mut sounds: ResMut<crate::cell_audio::NamedSounds>,
    mut pd: Option<ResMut<crate::pd_audio::PdMusic>>,
    mut state: Local<Cursors>,
) {
    let Some(originals) = originals else { return };
    let library = audio.library();
    let mut sources: Vec<Source> = vec![];
    if let Some((entity, clips)) = drive.as_ref().and_then(|d| d.avatar_clips()) {
        if let Ok(tf) = players.get(entity) {
            sources.extend(source_of(entity, clips, tf.translation(), true, true));
        }
    }
    for (entity, tf, walk, player) in &walkers {
        sources.extend(source_of(entity, walk.clip_player(), tf.translation(), player, player));
    }
    for (entity, tf, actor) in &crowd {
        sources.extend(source_of(entity, actor.clip_player(), tf.translation(), false, true));
    }
    if let Some(scene) = &scene {
        for (entity, tf) in &actors {
            let Some((id, t)) = scene.actor_clip(entity) else { continue };
            let Some(clip) = crate::creature_clips::clip(&originals, id) else { continue };
            let looped = clip.cyclic;
            sources.push(Source { entity, clip, time: t, looped, pos: tf.translation(), player: false, self_species: true });
        }
    }
    let listener = camera.single().ok().map(|c| c.translation());
    let now = time.elapsed_secs_f64();
    if let Some(run) = &run {
        if state.player_foot.is_none_or(|(n, _)| n != run.parts.len()) {
            state.player_foot = Some((run.parts.len(), player_foot(&originals, run)));
            state.player_mouth = Some((run.parts.len(), player_mouth(&originals, run)));
        }
    }
    let foot = state.player_foot.map_or("paw", |(_, f)| f);
    let mut seen = std::collections::HashSet::new();
    for s in sources {
        if !seen.insert(s.entity) {
            continue;
        }
        let previous = state.seen.insert(s.entity, (s.clip.id, s.time));
        let from = match previous {
            Some((id, t)) if id == s.clip.id && s.time + 1e-4 >= t => t,
            _ => -1.0,
        };
        let cues = cues_of(&s.clip);
        if debug() && previous.is_none_or(|(id, _)| id != s.clip.id) {
            eprintln!("anim audio: {:?}{} plays {:08X} ({} audio events)", s.entity, if s.player { " (player)" } else { "" }, s.clip.id, cues.len());
        }
        if cues.is_empty() {
            continue;
        }
        let distance = listener.map(|l| l.distance(s.pos));
        if !s.player && distance.is_some_and(|d| d > CLIP_AUDIO_RANGE) {
            continue;
        }
        for c in cues.iter() {
            if !due(from, s.time, s.clip.seconds, s.looped, c.time) {
                continue;
            }
            if c.max_sqr_distance > 0.0 && distance.is_some_and(|d| d * d > c.max_sqr_distance) {
                continue;
            }
            if let Some((rank, count)) = c.pick {
                if !hit_variant(s.entity.to_bits(), s.clip.id, c.time, rank, count) {
                    continue;
                }
            }
            match &c.cue {
                Cue::Param { id, value, .. } => {
                    if let Some(pd) = pd.as_mut() {
                        pd.set_audio_param_hash(*id, *value);
                    }
                }
                Cue::Sound { id, name } => {
                    let mouth = if s.self_species { state.player_mouth.map_or(DEFAULT_MOUTH, |(_, m)| m) } else { npc_mouth(s.entity) };
                    let (label, id, source) = if name == "foot_step" {
                        let n = footstep_name(if s.self_species { foot } else { "paw" }, &mut state.step_side, s.entity, land.as_deref(), s.pos);
                        let Some(n) = pick(&library, [n, fallback_dirt(n)]) else { continue };
                        (n, crate::world_objects::name_id(n), "footstep")
                    } else if name.starts_with('v') {
                        // A voice: the creature's mouth bank (`<name><Mouth>XCr`), else the
                        // citizen bank of its voice class (`<name><Mam|Bir|Ins>AIn`), else the
                        // plain name (`voice_banks`); names not installed stay silent.
                        let Some(n) = crate::voice_banks::pick(&library, name, mouth, intern) else { continue };
                        (n, crate::world_objects::name_id(n), "voice")
                    } else {
                        (intern(if name.is_empty() { format!("#{id:08X}") } else { name.clone() }), *id, "animation")
                    };
                    let mut play = crate::cell_audio::Play::named(label, source);
                    play.id = id;
                    play.distance = distance;
                    // The Settings > Sound "Voices" slider (`Bus::Voice`).
                    if source == "voice" {
                        play.bus = crate::game_audio::Bus::Voice;
                    }
                    sounds.play(play, now, 0.0);
                }
                Cue::Footstep { .. } => {
                    let label = footstep_name(if s.self_species { foot } else { "paw" }, &mut state.step_side, s.entity, land.as_deref(), s.pos);
                    let Some(label) = pick(&library, [label, fallback_dirt(label)]) else { continue };
                    let mut play = crate::cell_audio::Play::named(label, "footstep");
                    play.distance = distance;
                    // ADAPTED: two legs at a walk (the `footsteps` curves'
                    // other inputs, mass / foot size / leg length, at 1).
                    play.gain = curve(&NUMLEGS_TO_GAIN, 2.0) * curve(&VEL_TO_GAIN, 2.0);
                    sounds.play(play, now, 0.0);
                }
            }
        }
    }
    state.seen.retain(|e, _| seen.contains(e));
    state.step_side.retain(|e, _| seen.contains(e));
}

/// (water depth, height above the sea, slope) under a point of the stage.
fn terrain_at(land: &crate::terrain::LandSurface, p: Vec3) -> (f32, f32, f32) {
    let Some(sp) = land.planet() else { return (0.0, 10.0, 0.0) };
    let local = sp.frame.to_local(p);
    let above = sp.planet.height(local) - sp.planet.water_radius();
    let up = local.normalize_or_zero();
    let n = sp.planet.ground_normal(local);
    // 0 on flat ground, about 0.9 at 40 degrees.
    let slope = (1.0 - n.dot(up).clamp(-1.0, 1.0)) * 3.0;
    ((-above).max(0.0), above, slope)
}

/// `OPENSPORE_ANIM_AUDIO_DEBUG=1`: log every clip change with its audio
/// event count (sounds played are logged by `OPENSPORE_CELL_AUDIO_DEBUG`).
fn debug() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("OPENSPORE_ANIM_AUDIO_DEBUG").is_some())
}

/// Sound names live as long as the program (`Play::name` is `&'static`).
pub(crate) fn intern(name: String) -> &'static str {
    static NAMES: OnceLock<Mutex<std::collections::HashSet<&'static str>>> = OnceLock::new();
    let set = NAMES.get_or_init(Default::default);
    let mut set = set.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = set.get(name.as_str()) {
        return s;
    }
    let s: &'static str = Box::leak(name.into_boxed_str());
    set.insert(s);
    s
}

pub struct AnimAudioPlugin;

impl Plugin for AnimAudioPlugin {
    fn build(&self, app: &mut App) {
        use crate::stages::GameStage;
        // Creature: `creature_audio` runs the decoder and the player of
        // `NamedSounds` (a queued sound starts this frame or the next); the
        // other stages get them here.
        app.add_systems(
            Update,
            fire.run_if(in_state(GameStage::Creature)).run_if(resource_exists::<crate::game::RunProgress>),
        )
        .add_systems(
            Update,
            (crate::cell_audio::pump_decoder, fire, crate::cell_audio::play_sounds)
                .chain()
                .run_if(|s: Option<Res<State<GameStage>>>| {
                    s.is_some_and(|s| matches!(s.get(), GameStage::Tribal | GameStage::Civilization | GameStage::Space))
                })
                .run_if(resource_exists::<crate::game::RunProgress>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_disabled_hit_sound_plays_per_blow() {
        let mut counts = [0u32; 3];
        for actor in 0..300u64 {
            let playing: Vec<u8> = (0..3).filter(|r| hit_variant(actor, 0x056D_2CF4, 0.47, *r, 3)).collect();
            assert_eq!(playing.len(), 1, "actor {actor}: {playing:?}");
            counts[playing[0] as usize] += 1;
        }
        assert!(counts.iter().all(|c| *c > 50), "every variant is heard: {counts:?}");
        assert!(hit_variant(5, 1, 0.5, 0, 1), "a lone variant always plays");
    }

    #[test]
    fn footstep_curves_and_surfaces() {
        assert_eq!(curve(&NUMLEGS_TO_GAIN, 2.0), 1.0);
        assert!((curve(&NUMLEGS_TO_GAIN, 4.0) - 0.8).abs() < 1e-6);
        assert!((curve(&VEL_TO_GAIN, 1.5) - 0.75).abs() < 1e-6);
        assert_eq!(curve(&VEL_TO_GAIN, 9.0), 1.0);
        assert_eq!(surface(1.0, -1.0, 0.0, false), "splash");
        assert_eq!(surface(0.2, -0.2, 0.0, false), "wade");
        assert_eq!(surface(0.0, 0.5, 0.0, false), "sand");
        assert_eq!(surface(0.0, 10.0, 1.0, false), "rock");
        assert_eq!(surface(0.0, 10.0, 0.1, true), "grass");
        assert_eq!(surface(0.0, 10.0, 0.1, false), "dirt");
        assert_eq!(crate::pd::fnv("spike"), FOOT_TYPES[0].0);
    }

    #[test]
    fn mouth_codes_from_the_mouthtype_hashes() {
        // ce_mouth_jaw_herbivore_04 (JHD), radial omnivore 1 (ROA), the cell
        // jaw / mandible carnivore 1 (MCA).
        assert_eq!(mouth_code(663_108_769), Some("Jh"));
        assert_eq!(mouth_code(0x4072_DC09), Some("Ro"));
        assert_eq!(mouth_code(1_014_894_446), Some("Mc"));
        assert_eq!(mouth_code(1), None);
    }

    #[test]
    fn due_covers_the_passed_clip_time() {
        assert!(due(-1.0, 0.0, 1.0, false, 0.0), "a new clip fires its first-frame events");
        assert!(due(0.2, 0.3, 1.0, false, 0.25));
        assert!(!due(0.2, 0.3, 1.0, false, 0.2));
        // A loop that wrapped: the end and the start.
        assert!(due(0.9, 1.1, 1.0, true, 0.95));
        assert!(due(0.9, 1.1, 1.0, true, 0.05));
        assert!(!due(0.9, 1.1, 1.0, true, 0.5));
    }

    /// Census of the audio events of every installed clip (types, names,
    /// footstep selectors): `notes\audio-sweep-notes.md` A1.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_clip_audio_census() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
        let mut names: std::collections::BTreeMap<String, usize> = Default::default();
        let mut feet: std::collections::BTreeMap<String, usize> = Default::default();
        let mut clips = 0;
        let mut with_steps = 0;
        let keys: Vec<_> = index.records.keys().filter(|k| k.type_id == crate::decode::TYPE_ANIM).copied().collect();
        for key in keys {
            let Ok(anim) = index.read(key).map_err(|e| e.to_string()).and_then(|b| crate::animation::parse(&b)) else { continue };
            clips += 1;
            for e in &anim.events {
                *kinds.entry(format!("{:X}", e.flags & EVENT_TYPE_MASK)).or_default() += 1;
                if e.flags & EVENT_TYPE_MASK == EVENT_FOOTSTEP && clips % 97 == 0 {
                    eprintln!(
                        "footstep event of {:08X}: name {:?} id {:08X} flags {:08X} p0 {:08X} p1 {:08X} selectors {:?}",
                        key.instance_id, e.name, e.id, e.flags, e.parameter0, e.parameter1, e.selectors
                    );
                }
            }
            let cues = clip_cues(&anim);
            if cues.iter().any(|c| matches!(c.cue, Cue::Footstep { .. })) {
                with_steps += 1;
            }
            for c in cues {
                match c.cue {
                    Cue::Sound { name, id } => *names.entry(if name.is_empty() { format!("#{id:08X}") } else { name }).or_default() += 1,
                    Cue::Param { name, value, .. } => *names.entry(format!("sound2 {name}={value}")).or_default() += 1,
                    Cue::Footstep { capability, select_flags } => *feet.entry(format!("{capability} {select_flags:X}")).or_default() += 1,
                }
            }
        }
        eprintln!("{clips} clips, {with_steps} with footstep events; event types {kinds:?}");
        eprintln!("footstep selectors {feet:?}");
        let mut top: Vec<_> = names.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        for (n, c) in top.iter().take(400) {
            eprintln!("  {c:5} {n}");
        }
        assert!(clips > 1000);
    }
}
