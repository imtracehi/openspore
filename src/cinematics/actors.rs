//! Actor animation: `playAnimation` plays the real clip of the action.
//!
//! The scripts name an action id (`0x0541ba87` = `gen_brainup_admire`); the
//! TLSA action lists map it to ANIM clips (one clip per rig variant of the
//! creature kind; all variants share the timeline and the events). OpenSpore's
//! creatures have no retargeted skeleton player, so what an actor shows of a
//! clip is ORIGINAL data driven at the original timing:
//! * the clip's `root` channel (position and rotation keys, evaluated with the
//!   native curve code of `animation_sampling`) moves and leans the whole body
//!   on top of the actor's place (rear up, bounce, sway, bow);
//! * the clip's animation events fire at their times: `cinematic_continue`
//!   ends a `wait -forContinue` (retail: the animation event sends message
//!   0x445F729; the 66 s stick clip of `CRG2TRG` carries two), sounds and
//!   effects named by an event (`crg_to_tribe_musica`, `sfx_cin_c2t_a`,
//!   `vgen_brainup_admire`, `cinematic_creature_hitGroundWithStick_S`, ...)
//!   are started when an installed cue or effect has that name.
//!
//! ADAPTED: limb, head, eye and mouth channels are not shown (no retargeting
//! onto the player's own rig); the first rig variant of an action supplies the
//! root track; the position keys are metres of a nominal creature and are
//! scaled by the actor's size.

use std::collections::VecDeque;

use bevy::prelude::*;

use crate::animation::{Animation, Channel, Keys};
use crate::animation_sampling::{sample, RotationCurve};

/// Spore vector (x, y forward, z up) to Bevy (x, z, -y).
fn to_bevy(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0], v[2], -v[1])
}

/// The same basis change for a quaternion: the rotation axis is converted,
/// the angle stays.
fn quat_to_bevy(q: [f32; 4]) -> Quat {
    let q = Quat::from_xyzw(q[0], q[2], -q[1], q[3]);
    if q.length_squared() > 1e-8 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// One clip of a `playAnimation` list.
#[derive(Clone, Debug)]
pub struct ClipTrack {
    pub id: u32,
    pub seconds: f32,
    pub cyclic: bool,
    length_frames: f32,
    frame_length: f32,
    root: Option<Channel>,
    /// (seconds, event name), sorted by time; the `footstep` markers are left
    /// out (the actor's gait owns them).
    pub events: Vec<(f32, String)>,
    /// The clip holds its last pose instead of ending (the avatar's death).
    pub hold: bool,
    /// A pose made here, not read from a clip.
    fall: bool,
}

impl ClipTrack {
    /// A decoded clip.
    pub fn from_animation(id: u32, anim: Animation) -> Self {
        let mut events: Vec<(f32, String)> = Vec::new();
        for channel in &anim.channels {
            for component in &channel.components {
                let Keys::Info(keys) = &component.keys else { continue };
                for key in keys.iter().filter(|k| k.event_count > 0) {
                    let time = key.frame as f32 * anim.frame_length;
                    for event in anim.events.iter().skip(key.event_start as usize).take(key.event_count as usize) {
                        if event.name != "footstep" && !events.iter().any(|(t, n)| *t == time && *n == event.name) {
                            events.push((time, event.name.clone()));
                        }
                    }
                }
            }
        }
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let root = anim
            .channels
            .iter()
            .find(|c| c.name == "root" && c.components.iter().any(|k| matches!(k.keys, Keys::Position(_) | Keys::Rotation(_))))
            .cloned();
        Self {
            id,
            seconds: anim.duration_seconds().max(0.0),
            cyclic: anim.flags & 8 != 0,
            length_frames: anim.length_frames,
            frame_length: anim.frame_length,
            root,
            events,
            hold: false,
            fall: false,
        }
    }

    /// ADAPTED: the avatar's death pose. Retail's game code plays the
    /// creature's own die animation; OpenSpore's avatar has none, so the body
    /// topples onto its side and stays there.
    pub fn fall() -> Self {
        Self {
            id: 0,
            seconds: 0.9,
            cyclic: false,
            length_frames: 27.0,
            frame_length: 1.0 / 30.0,
            root: None,
            events: Vec::new(),
            hold: true,
            fall: true,
        }
    }

    /// A silent clip of a given length (an action whose clip cannot be read).
    pub fn timer(seconds: f32) -> Self {
        Self {
            id: 0,
            seconds,
            cyclic: false,
            length_frames: seconds * 30.0,
            frame_length: 1.0 / 30.0,
            root: None,
            events: Vec::new(),
            hold: false,
            fall: false,
        }
    }

    /// The root bone's offset at `t` seconds (Bevy axes, metres of a nominal
    /// creature), `None` for a clip without a root track.
    pub fn pose(&self, t: f32) -> Option<(Vec3, Quat)> {
        if self.fall {
            let u = (t / self.seconds).clamp(0.0, 1.0);
            let eased = u * u * (3.0 - 2.0 * u);
            return Some((Vec3::new(0.0, -0.1 * eased, 0.0), Quat::from_rotation_z(eased * std::f32::consts::FRAC_PI_2)));
        }
        let root = self.root.as_ref()?;
        let t = if self.cyclic && self.seconds > 0.0 { t.rem_euclid(self.seconds) } else { t.clamp(0.0, self.seconds) };
        let frame = (t / self.frame_length).clamp(0.0, self.length_frames);
        let s = sample(root, frame, self.length_frames, self.cyclic, RotationCurve::Spherical).ok()?;
        let position = s.position.map_or(Vec3::ZERO, |p| to_bevy(p.0));
        let rotation = s.rotation.map_or(Quat::IDENTITY, |r| quat_to_bevy(r.0));
        Some((position, rotation))
    }

    /// An event named `cinematic_continue` lies after `t`.
    pub fn continue_ahead(&self, t: f32) -> bool {
        self.events.iter().any(|(time, name)| *time > t && name == "cinematic_continue")
    }
}

/// The clip of an action id: the first rig variant of the first choice of the
/// TLSA action (the one with a root track), else the ANIM clip with that
/// instance id.
pub fn load_track(originals: &crate::Originals, id: u32) -> Option<ClipTrack> {
    let group = originals.action_lists.lists.iter().flat_map(|(_, list)| &list.groups).find(|g| g.id == id);
    let mut candidates: Vec<u32> = Vec::new();
    if let Some(group) = group {
        if let Some(choice) = group.choices.first() {
            candidates.extend(choice.animations.iter().map(|a| a.id));
        }
    }
    if candidates.is_empty() {
        candidates.push(id);
    }
    let mut best: Option<ClipTrack> = None;
    for clip in candidates {
        let Ok(key) = originals.animation_catalog.resolve(clip, None) else { continue };
        let Ok(bytes) = originals.animation_catalog.read(&originals.resource_index, key) else { continue };
        let Ok(anim) = crate::animation::parse(&bytes) else { continue };
        let track = ClipTrack::from_animation(id, anim);
        if track.seconds <= 0.0 {
            continue;
        }
        let has_root = track.root.is_some();
        if best.is_none() || (has_root && best.as_ref().is_some_and(|b| b.root.is_none())) {
            best = Some(track);
        }
        if has_root {
            break;
        }
    }
    best
}

/// One actor of a vignette: where it stands in the group's frame (Spore x
/// right, y forward), how it faces (degrees) and its action ids.
#[derive(Clone, Debug, PartialEq)]
pub struct VignetteActor {
    pub offset: [f32; 2],
    pub orientation: f32,
    pub animations: Vec<u32>,
}

/// A vignette definition (`Vignettes_Tribe!trg_build.prop`, a property list):
/// `vi_actor_sequences` picks one of the `vi_animations_<n>` action lists per
/// actor, `vi_actor_offsets` / `vi_actor_orientations` place it. ORIGINAL data.
pub fn load_vignette(originals: &crate::Originals, spec: &str) -> Option<Vec<VignetteActor>> {
    let (group, name) = spec.split_once('!')?;
    let name = name.strip_suffix(".prop").unwrap_or(name);
    let key = crate::dbpf::ResourceKey {
        group_id: crate::world_objects::name_id(group),
        instance_id: crate::world_objects::name_id(name),
        type_id: 0x00B1_B104,
    };
    let bytes = originals.resource_index.read(key).ok()?;
    let list = crate::prop::parse(&bytes).ok()?;
    // The properties are the registered ids (`reg_property.txt`), not name
    // hashes: vi_actor_sequences 0x04877BF5, vi_actor_offsets 0x04879D5A,
    // vi_actor_orientations 0x049A2146, vi_animations_<n> 0x04866690 + n.
    let get = |name: &str| {
        let id = match name {
            "vi_actor_sequences" => 0x0487_7BF5,
            "vi_actor_offsets" => 0x0487_9D5A,
            "vi_actor_orientations" => 0x049A_2146,
            _ => 0x0486_6690 + name.strip_prefix("vi_animations_")?.parse::<u32>().ok().filter(|n| *n < 12)?,
        };
        list.0.get(&id)
    };
    let be_u32 = |v: &Vec<u8>| v.get(..4).map(|b| u32::from_be_bytes(b.try_into().unwrap()));
    let sequences: Vec<u32> = get("vi_actor_sequences")?.values.iter().filter_map(be_u32).collect();
    let offsets: Vec<[f32; 2]> = get("vi_actor_offsets")
        .map(|p| {
            p.values
                .iter()
                .filter_map(|v| {
                    Some([f32::from_le_bytes(v.get(..4)?.try_into().ok()?), f32::from_le_bytes(v.get(4..8)?.try_into().ok()?)])
                })
                .collect()
        })
        .unwrap_or_default();
    let orientations: Vec<f32> = get("vi_actor_orientations")
        .map(|p| p.values.iter().filter_map(|v| v.get(..4).map(|b| f32::from_be_bytes(b.try_into().unwrap()))).collect())
        .unwrap_or_default();
    let actors: Vec<VignetteActor> = sequences
        .iter()
        .enumerate()
        .filter_map(|(i, seq)| {
            let animations: Vec<u32> = get(&format!("vi_animations_{seq}"))?.values.iter().filter_map(be_u32).collect();
            (!animations.is_empty()).then(|| VignetteActor {
                offset: offsets.get(i).copied().unwrap_or([0.0, 0.0]),
                orientation: orientations.get(i).copied().unwrap_or(0.0),
                animations,
            })
        })
        .collect();
    (!actors.is_empty()).then_some(actors)
}

/// An actor playing its clips in sequence.
pub struct ClipJob {
    pub entity: Entity,
    tracks: VecDeque<ClipTrack>,
    t: f32,
    fired: usize,
    /// Held until the last clip ends (`playAnimation` without `-noWait`).
    pub blocker: Option<u32>,
    pub done: bool,
    /// A vignette role: the list starts over when it ends.
    repeat: Option<Vec<ClipTrack>>,
    /// `-processAnimEventsUsingTarget`: where the clip's events play.
    pub event_target: Option<Vec3>,
}

impl ClipJob {
    pub fn new(entity: Entity, tracks: Vec<ClipTrack>, blocker: Option<u32>) -> Self {
        Self { entity, tracks: tracks.into(), t: 0.0, fired: 0, blocker, done: false, repeat: None, event_target: None }
    }

    /// A role that plays its clips again and again.
    pub fn looped(entity: Entity, tracks: Vec<ClipTrack>) -> Self {
        let mut job = Self::new(entity, tracks.clone(), None);
        job.repeat = Some(tracks);
        job
    }

    /// Advance by `dt`; the events that came due, in order.
    pub fn step(&mut self, dt: f32) -> Vec<String> {
        let mut out = Vec::new();
        if self.done {
            return out;
        }
        self.t += dt.max(0.0);
        // Several short clips can pass in one long frame.
        for _ in 0..16 {
            let Some(track) = self.tracks.front() else {
                self.done = true;
                break;
            };
            while let Some((time, name)) = track.events.get(self.fired) {
                if *time > self.t {
                    break;
                }
                out.push(name.clone());
                self.fired += 1;
            }
            if self.t < track.seconds {
                break;
            }
            if track.hold {
                self.t = track.seconds;
                break;
            }
            // A looping clip without a waiting script runs until replaced.
            if self.tracks.len() == 1 && track.cyclic && self.blocker.is_none() && track.seconds > 0.0 {
                self.t -= track.seconds;
                self.fired = 0;
                break;
            }
            self.t -= track.seconds;
            self.fired = 0;
            self.tracks.pop_front();
            if self.tracks.is_empty() {
                match &self.repeat {
                    Some(list) if list.iter().any(|t| t.seconds > 0.0) => self.tracks = list.clone().into(),
                    _ => {
                        self.done = true;
                        break;
                    }
                }
            }
        }
        out
    }

    /// The root offset now (`None` once finished or without a root track).
    pub fn pose(&self) -> Option<(Vec3, Quat)> {
        if self.done {
            return None;
        }
        self.tracks.front()?.pose(self.t)
    }

    /// A `cinematic_continue` event still lies ahead in this job.
    pub fn continue_ahead(&self) -> bool {
        if self.done {
            return false;
        }
        self.tracks.iter().enumerate().any(|(i, t)| t.continue_ahead(if i == 0 { self.t } else { -1.0 }))
    }

    /// The action id and time of the clip playing now (`creature_clips`
    /// poses the actor's limbs from it); none for a made-up pose or timer.
    pub fn current(&self) -> Option<(u32, f32)> {
        if self.done {
            return None;
        }
        let track = self.tracks.front()?;
        (track.id != 0 && !track.fall).then_some((track.id, self.t))
    }

    /// Seconds left of the whole list.
    pub fn remaining(&self) -> f32 {
        self.tracks.iter().enumerate().map(|(i, t)| if i == 0 { (t.seconds - self.t).max(0.0) } else { t.seconds }).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cinematics::{command::Command, runtime::Program, script};

    fn track(seconds: f32, cyclic: bool, events: &[(f32, &str)]) -> ClipTrack {
        ClipTrack {
            id: 1,
            seconds,
            cyclic,
            length_frames: seconds * 30.0,
            frame_length: 1.0 / 30.0,
            root: None,
            events: events.iter().map(|(t, n)| (*t, n.to_string())).collect(),
            hold: false,
            fall: false,
        }
    }

    #[test]
    fn a_job_fires_events_in_order_and_ends_with_its_last_clip() {
        let mut job = ClipJob::new(
            Entity::PLACEHOLDER,
            vec![track(1.0, false, &[(0.2, "a"), (0.9, "cinematic_continue")]), track(0.5, false, &[(0.1, "b")])],
            Some(7),
        );
        assert!(job.continue_ahead());
        assert_eq!(job.step(0.3), ["a"]);
        assert!(job.continue_ahead());
        assert_eq!(job.step(0.7), ["cinematic_continue"]);
        assert!(!job.continue_ahead());
        assert!(!job.done);
        assert_eq!(job.step(0.2), ["b"]);
        assert!(!job.done);
        job.step(0.5);
        assert!(job.done);
        assert!(job.pose().is_none());
    }

    #[test]
    fn a_looping_clip_without_a_waiting_script_never_ends() {
        let mut job = ClipJob::new(Entity::PLACEHOLDER, vec![track(1.0, true, &[(0.5, "x")])], None);
        let mut fired = 0;
        for _ in 0..50 {
            fired += job.step(0.1).len();
        }
        assert!(!job.done);
        assert!(fired >= 4, "{fired}");
        // The same clip with a blocker plays once.
        let mut once = ClipJob::new(Entity::PLACEHOLDER, vec![track(1.0, true, &[])], Some(1));
        once.step(1.2);
        assert!(once.done);
    }

    #[test]
    fn the_death_pose_topples_and_holds() {
        let mut job = ClipJob::new(Entity::PLACEHOLDER, vec![ClipTrack::fall()], None);
        let (_, start) = job.pose().expect("pose");
        assert!(start.angle_between(Quat::IDENTITY) < 1e-5);
        job.step(5.0);
        assert!(!job.done, "the pose is held");
        let (_, end) = job.pose().expect("pose");
        assert!((end.angle_between(Quat::IDENTITY) - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
    }

    #[test]
    fn axes_follow_the_spore_to_bevy_basis() {
        // Spore +Z (up) is Bevy +Y, Spore +Y (forward) is Bevy -Z.
        assert_eq!(to_bevy([0.0, 0.0, 1.0]), Vec3::Y);
        assert_eq!(to_bevy([0.0, 1.0, 0.0]), Vec3::NEG_Z);
        // A turn about Spore Z (up) is the same turn about Bevy Y.
        let q = quat_to_bevy(Quat::from_axis_angle(Vec3::Z, 1.0).to_array());
        let expected = Quat::from_axis_angle(Vec3::Y, 1.0);
        assert!(q.angle_between(expected) < 1e-5);
    }

    /// The installed clips of the scripted actions: what an actor shows (a
    /// root track) and the events that drive the cinematics.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_action_clips_have_root_tracks_and_the_continue_events() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = script::Library::from_index(&index);
        let program = Program::compile(&library);
        let mut originals = crate::Originals::default();
        originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        originals.action_lists = crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
        originals.resource_index = index;
        let mut ids = std::collections::BTreeSet::new();
        for state in program.states.values() {
            for command in &state.commands {
                if let Command::PlayAnimation(p) = command {
                    ids.extend(p.ids.iter().copied());
                }
            }
        }
        let swarm = crate::swarm_fx::SwarmLibrary::load(&originals.resource_index);
        let audio = crate::game_audio::AudioLibrary::from_index(&originals.resource_index);
        let mut loaded = 0;
        let mut rooted = 0;
        let mut names = std::collections::BTreeMap::<String, usize>::new();
        for id in &ids {
            let Some(track) = load_track(&originals, *id) else {
                println!("{id:08X}: no clip");
                continue;
            };
            loaded += 1;
            if track.root.is_some() {
                rooted += 1;
                // The pose is finite over the whole clip.
                for i in 0..=20 {
                    let (p, r) = track.pose(track.seconds * i as f32 / 20.0).expect("root pose");
                    assert!(p.is_finite() && r.is_finite() && p.length() < 10.0, "{id:08X} {p:?}");
                }
            }
            for (_, name) in &track.events {
                *names.entry(name.clone()).or_default() += 1;
            }
        }
        println!("{} ids, {loaded} clips, {rooted} with a root track", ids.len());
        let mut sound = 0;
        let mut effect = 0;
        for name in names.keys() {
            let s = audio.resolve(name).is_ok();
            let e = swarm.find(crate::world_objects::name_id(name)).is_some();
            println!("event {name}: sound {s} effect {e}");
            sound += s as usize;
            effect += e as usize;
        }
        println!("{} event names: {sound} sounds, {effect} effects", names.len());
        assert!(loaded + 3 >= ids.len(), "nearly every scripted action has a clip");
        assert!(rooted * 2 >= loaded, "most clips carry a root track");
        // The stick clip of CRG2TRG carries the two continue events of the script.
        let stick = load_track(&originals, 0x04A4_C879).expect("the stick action");
        let continues: Vec<f32> = stick.events.iter().filter(|(_, n)| n == "cinematic_continue").map(|(t, _)| *t).collect();
        println!("stick continue events at {continues:?} of {:.1} s", stick.seconds);
        assert_eq!(continues.len(), 2);
        assert!(continues[0] > 40.0 && continues[1] > 58.0 && continues[1] < stick.seconds);
    }

    /// The game behaviour clips that send `-forContinue` continues (brain-up,
    /// the alliance celebration) and the vignettes of the non-Space scripts
    /// are installed.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_behaviour_clips_and_stage_vignettes() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let mut originals = crate::Originals::default();
        originals.animation_catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        originals.action_lists = crate::animation_catalog::ActionLists::from_index(&index, &originals.animation_catalog);
        originals.resource_index = index;
        use crate::cinematics::host::{BRAIN_UP_CONTINUE_ANIMATION, CHIEF_SOCIAL_WIN, SOCIAL_WIN};
        for id in [BRAIN_UP_CONTINUE_ANIMATION, CHIEF_SOCIAL_WIN].into_iter().chain(SOCIAL_WIN) {
            let track = load_track(&originals, id).unwrap_or_else(|| panic!("clip {id:08X}"));
            let secs = crate::cinematics::host::animation_seconds(&originals, id);
            println!("{id:08X}: track {:.2} s, action {secs:?}", track.seconds);
            assert!(track.seconds > 0.5 && track.seconds < 30.0, "{id:08X}");
        }
        for spec in [
            "Vignettes_Tribe!trg_start.prop",
            "Vignettes_Tribe!baby_born.prop",
            "Vignettes_Tribe!trg_build.prop",
            "Vignettes_Tribe!trg_city.prop",
            "Vignettes_Tribe!t2c_vignette.prop",
            "Vignettes_Civ!open_vi.prop",
            "Vignettes_Civ!upgrade_air.prop",
            "Vignettes_Civ!upgrade_water.prop",
            "Vignettes_Civ!cvg_vi_death.prop",
            "Vignettes_Civ!c2s_vignette_part1.prop",
        ] {
            let places = load_vignette(&originals, spec).unwrap_or_else(|| panic!("{spec}"));
            let clips = places.iter().flat_map(|p| &p.animations).filter(|id| load_track(&originals, **id).is_some()).count();
            println!(
                "{spec}: {} places, {clips} readable clips; first at {:?} facing {}",
                places.len(),
                places[0].offset,
                places[0].orientation
            );
            assert!(clips > 0, "{spec}");
            for p in &places {
                assert!(p.offset.iter().all(|v| v.is_finite() && v.abs() < 100.0), "{spec} {:?}", p.offset);
                assert!(p.orientation.is_finite() && p.orientation.abs() <= 720.0, "{spec} {}", p.orientation);
            }
        }
    }

    /// Every string of the cinematic locale tables (the texts the scripts
    /// take from code are among them).
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn dump_cinematic_text_tables() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        for table in ["cinematics_trg", "cinematics_cvg", "cinematics_crg", "cinematics_spg", "cinematics_clg"] {
            let hash = crate::world_objects::name_id(table);
            let mut keys: Vec<_> = index
                .records
                .keys()
                .filter(|k| k.type_id == crate::spui::view::LOCALE_TABLE_TYPE && k.instance_id == hash)
                .copied()
                .collect();
            keys.sort_by_key(|k| k.group_id);
            println!("table {table}: {} resources", keys.len());
            let mut seen = std::collections::BTreeMap::new();
            for k in keys {
                if let Ok(bytes) = index.read(k) {
                    for (id, text) in crate::decode::parse_locale_table(&bytes) {
                        seen.entry(id).or_insert((k.group_id, text));
                    }
                }
            }
            for (id, (group, text)) in seen {
                println!("  {id:08X} [{group:08X}] {}", text.replace('\n', " | "));
            }
        }
    }
}
