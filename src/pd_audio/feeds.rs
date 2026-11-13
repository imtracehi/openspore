//! The game -> patch inputs of the Creature and Tribe mix modes, set at the
//! moments the retail game sets them (`notes\pd-music-re-notes.md` 6).
//!
//! RETAIL (march2017 addresses, VERIFIED unless marked):
//! * Every `0x5CA970` set reaches the patch: the EAPd input manager
//!   (vtable 0x1455760, float set 0xA68630 -> 0xA64900) queues a task
//!   (callback 0xA64830) for the `gfparam` listeners with no old-value test,
//!   so a parameter set every frame outputs every frame.
//! * The Creature audio update (0xD41F23..0xD42524) sets 14 parameters every
//!   frame ([`CREATURE_PER_FRAME`]); the stance (`ui_social` / `ui_fight`) is
//!   set when it changes (SetAbilityMode 0xD2E55F / 0xD2E56E); the social
//!   meter update (0xD9B9C0) sets `npcsocialbar` / `socialbar` every update
//!   while its bars are shown; the befriend success sets `socialsuccess` 1
//!   (0xD9BDC5 in 0xD9BBD0).
//! * The parameter providers (vtables 0x14872F0..0x1487EC8, one class per
//!   name: slot 6 caches FNV(name), slot 10 updates, slot 13 = 0xEA53B0
//!   stores the value and marks it dirty only when it changed) feed
//!   `timeofday` (0xEA5890: local day fraction x day length, from the camera
//!   position), `planetzoom` (0xEA5B90: camera distance to the planet
//!   surface), `cameraspeed` (0xEA7840: camera displacement since the last
//!   update), and in the Tribe game mode (0x1654C02) `tribecount` /
//!   `tribedistance` / `npctribecount` / `npctribedistance` (0xEA8C20: the
//!   player tribe's member count and distance to the camera, the same for
//!   the NPC tribe nearest the camera).
//! * Tribe social (cTribeSocial..., update 0xDA2F30, sendgame handler
//!   0xDA1F20, round evaluation 0xDA0100): when the troupe stands in place
//!   (state 3 -> 4) the game sets `tribe_social_x/y/z`, then the member
//!   counts per instrument: maracas -> id 0x40F3A3B0 = FNV("shaker_start ")
//!   (a trailing space: the patch's `gfparam shaker_start` never hears it, so
//!   the shaker backing players never start in retail), didgeridoos ->
//!   `didger_start`, horns -> `flute_start`. The patch answers
//!   `trg_social_length` (rounds, 5) and `trg_round_length` (4);
//!   `trg_social_thought_bubble` (just before each beat) makes the village
//!   show the instrument it wants; `trg_social_beat` (just after) counts a
//!   round and the troupe plays. Each evaluation sets `shaker_solo` /
//!   `didger_solo` / `flute_solo` to the number of members that played that
//!   instrument and, when at least one played the wanted one,
//!   `trg_social_success` 1; the end sets `trg_social_stop` 1 (0xD9FE60).
//! * The event inputs ([`CREATURE_EVENT_INPUTS`], `creature-sound2`): the meteor
//!   strike's start / stop set `crg_meteors` 1 / 0 (0xD51862 / 0xD50932), the
//!   weather manager's storm message sets `rain_density` 1 when the camera
//!   enters a local storm and 0 when it leaves (0xFC6BCD; the `crg_rain` patch
//!   restarts a 3 s delay on every 1, so it is sent on change only), a
//!   completed mission sets `missioncomplete` 1 (0xC2E745, every time), the UFO
//!   abduction event sets `crg_ufo` 1 at its start (0xD4D1A1 / 0xD4FABC) and 0
//!   at its stop (0xD4FAD0; `creature_events::ufo` writes both into `MixEvents`,
//!   the `mix` path below sends every change). `mixmode_simulationpause` is
//!   `pd_audio::feed_simulation_pause`.
//! * The Tribal event inputs (`tribal-sound2`): a completed goal card
//!   (`CompleteGoal` 0xB708A0 -> the mission completion 0xC2E700) sets
//!   `missioncomplete` 1 (0xC2E745), which `tribe_drums3` reads; the social
//!   game's stop routine (0xD9FE60, run when the ended song's bar is hidden,
//!   or at once when the chieftain walks away) sets `trg_social_stop` 1 and
//!   `trg_social_stop_on_beat` 0 (0xD9FF12 / 0xD9FF5B), and the walk-away handler
//!   (0xDA34BE, while rounds remain) sets `trg_social_stop_on_beat` 1 first
//!   ([`song_end_inputs`]).
//! * Never set by any code or data (no immediate, no string, no animation
//!   or cinematic event): `TribeLoopEvent`, `tribe_attack_start`,
//!   `tribe_attack_fight`, `zulu_attack`, `triberecruitevent`, `shaker_start`,
//!   `fight_hit`, `intendtofight`, `babygrowup`, `predator_moment(B)`,
//!   `creature_social_state`: they stay unset here too. `TribeLongEvent`,
//!   `TribeShortEvent`, `trg_to_civ_start` come from the cinematic scripts
//!   (`mixevent`, forwarded by `feed_stage_params`).
//!
//! ADAPTED (OpenSpore's own rules where retail systems differ): the Creature
//! social encounter has one meter, so both bars read its friendship;
//! `socialsuccess` fires when the friendship fills; `timeofday` uses the day
//! length `defaultDayLength` (960, INFERRED: the patch windows 220..310 and
//! 760..850 then fall at 5.5..7.75 h and 19..21.25 h); the Tribal
//! performance keeps OpenSpore's call/answer notes, but while
//! `music_tribe_social` runs its beats close the answer windows (a note not
//! answered by the next `trg_social_beat` is missed) and the solos /
//! success follow each answer.

use std::collections::HashMap;

use bevy::prelude::*;

use super::{PdMusic, PdSendGame};
use crate::creature_music::CreatureAudioParams;
use crate::game::RunProgress;
use crate::stages::GameStage;
use crate::tribal_tools::ToolKind;

/// The parameters the Creature audio update sets every frame
/// (0xD41F23..0xD42524).
pub const CREATURE_PER_FRAME: [&str; 14] = [
    "creature_health",
    "creature_health_low",
    "creature_health_critical",
    "creature_hunger",
    "nearest_predator",
    "nearest_epic_predator",
    "brain_level",
    "distance_to_migration_point",
    "ispregnant",
    "isbaby",
    "instealth",
    "infight",
    "creature_social_level",
    "ismating",
];

/// The inputs set when an event starts or ends, not every frame: sent on change
/// only (the initial 0 is not sent: retail never sets them before the event).
pub const CREATURE_EVENT_INPUTS: [&str; 2] = ["crg_meteors", "rain_density"];

/// `defaultDayLength` (GlobalTuning): the `timeofday` provider multiplies
/// the local day fraction by the day length.
pub const DAY_LENGTH: f32 = crate::creature_ecology::DEFAULT_DAY_LENGTH;

/// The id the Tribe social code sets with the maracas count (0x40F3A3B0).
pub const SHAKER_START_RETAIL: &str = "shaker_start ";

/// Pure: the `timeofday` parameter for a clock in hours.
pub fn timeofday_param(hours: f32) -> f32 {
    hours.rem_euclid(24.0) / 24.0 * DAY_LENGTH
}

/// Pure: (didgeridoo, horn, maraca) members among the tools carried.
pub fn instrument_counts<'a>(tools: impl IntoIterator<Item = &'a ToolKind>) -> (u32, u32, u32) {
    let mut c = (0, 0, 0);
    for t in tools {
        match t {
            ToolKind::Aerophones => c.0 += 1,
            ToolKind::WoodenHorns => c.1 += 1,
            ToolKind::Maracas => c.2 += 1,
            _ => {}
        }
    }
    c
}

/// Pure: the solo parameter of an OpenSpore answer (3 horn, 4 maracas,
/// 5 didgeridoo).
pub fn solo_param(answer: u32) -> Option<&'static str> {
    match answer {
        3 => Some("flute_solo"),
        4 => Some("shaker_solo"),
        5 => Some("didger_solo"),
        _ => None,
    }
}

/// Pure: the inputs the end of a song sets, in order. The stop routine 0xD9FE60
/// (it hides the bar and ends the game object) sets `trg_social_stop` to 1.0
/// (0xD9FF12) and `trg_social_stop_on_beat` to 0.0 (0xD9FF5B); when the
/// chieftain walks away while rounds remain (`round < length`, 0xDA34B1) the
/// handler sets `trg_social_stop_on_beat` to 1.0 (0xDA34BE) before.
pub fn song_end_inputs(left_with_rounds_remaining: bool) -> Vec<(&'static str, f32)> {
    let mut out = Vec::new();
    if left_with_rounds_remaining {
        out.push(("trg_social_stop_on_beat", 1.0));
    }
    out.push(("trg_social_stop", 1.0));
    out.push(("trg_social_stop_on_beat", 0.0));
    out
}

/// `terrainScriptAmbientSoundEffects` (planet theme property, inherited by
/// the terrain scripts; AllPlanets: `planet_ambience_default`).
pub const P_AMBIENT_SOUND_EFFECTS: u32 = 0x0405_39D4;
/// The effect directory whose `sound` components the ambient sound effects
/// play (`planet_ambience_rivers` -> soundProp `ambience_rivers` ...).
pub const AMBIENCE_EFFDIR: crate::dbpf::ResourceKey =
    crate::dbpf::ResourceKey { type_id: 0xEA51_18B0, group_id: 0x4045_0200, instance_id: 0xC119_5706 };
/// AllPlanets (the root of every planet theme chain).
pub const ALL_PLANETS: crate::dbpf::ResourceKey =
    crate::dbpf::ResourceKey { type_id: crate::asset_catalog::PROP, group_id: 0x4284_0100, instance_id: 0x6E66_0BEB };

/// Pure: effect name -> the sound ids of its `sound` blocks, from an effect
/// directory (EFFDIR v4: typed component sections, effects, exports; the
/// layout SporeModder-FX reads, as `planet_data` / `swarm_fx` do).
pub fn effect_sounds(data: &[u8]) -> Result<HashMap<u32, Vec<u32>>, String> {
    struct Rd<'a>(&'a [u8], usize);
    impl Rd<'_> {
        fn take(&mut self, n: usize) -> Result<&[u8], String> {
            let s = self.0.get(self.1..self.1 + n).ok_or("effdir truncated")?;
            self.1 += n;
            Ok(s)
        }
        fn u8(&mut self) -> Result<u8, String> {
            Ok(self.take(1)?[0])
        }
        fn u16(&mut self) -> Result<u16, String> {
            Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
        }
        fn u32(&mut self) -> Result<u32, String> {
            Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
        }
    }
    let mut r = Rd(data, 0);
    if r.u16()? != 4 {
        return Err("unsupported effdir version".into());
    }
    let mut sounds: Vec<u32> = vec![];
    for _pass in 0..2 {
        loop {
            let kind = r.u16()? as i16;
            if kind == -1 {
                break;
            }
            r.u16()?;
            let size = r.u32()? as usize;
            let start = r.1;
            if kind == 5 && sounds.is_empty() {
                // flags, soundID (group, instance), track, invSample, length, volume
                let n = r.u32()? as usize;
                for _ in 0..n.min(1 << 16) {
                    let c = r.take(28)?;
                    sounds.push(u32::from_be_bytes(c[8..12].try_into().unwrap()));
                }
            }
            r.1 = start + size;
        }
    }
    r.u16()?;
    let count = r.u32()? as usize;
    if count > 1 << 20 {
        return Err("implausible effect count".into());
    }
    let mut effects: Vec<Vec<u32>> = Vec::with_capacity(count);
    for _ in 0..count {
        r.take(4 + 4 + 4 + 8 + 4 + 1)?;
        let lods = r.u32()? as usize;
        r.take(4 * lods.min(1 << 12) + 12 + 4)?;
        let blocks = r.u32()? as usize;
        let mut snd = vec![];
        for _ in 0..blocks.min(1 << 12) {
            let kind = r.u8()?;
            r.take(4 + 2 + 4 + 36 + 12 + 2)?;
            let n = r.u32()? as usize;
            r.take(12 * n.min(64) + 24 + 8 + 4 + 4)?;
            let index = r.u32()? as i32;
            if kind == 5 {
                if let Some(s) = usize::try_from(index).ok().and_then(|i| sounds.get(i)) {
                    snd.push(*s);
                }
            }
        }
        effects.push(snd);
    }
    let mut out = HashMap::new();
    loop {
        let index = r.u32()? as i32;
        if index == -1 {
            break;
        }
        r.u32()?;
        let name = r.u32()?;
        if let Some(s) = usize::try_from(index).ok().and_then(|i| effects.get(i)).filter(|s| !s.is_empty()) {
            out.entry(name).or_insert_with(|| s.clone());
        }
    }
    Ok(out)
}

/// The planet's ambient sound effects as sound ids: the terrain script's
/// `terrainScriptAmbientSoundEffects` (through its theme chain), or
/// AllPlanets' without a stage planet.
pub fn planet_ambience_sounds(index: &crate::asset_catalog::ResourceIndex, terrain_script: Option<u32>) -> Vec<u32> {
    let key = terrain_script
        .and_then(|instance| {
            [crate::planet_data::TERRAIN_SCRIPTS_GROUP, crate::planet_data::TERRAIN_SCRIPTS_FALLBACK_GROUP]
                .into_iter()
                .map(|group_id| crate::dbpf::ResourceKey { type_id: crate::asset_catalog::PROP, group_id, instance_id: instance })
                .find(|k| index.records.contains_key(k))
        })
        .unwrap_or(ALL_PLANETS);
    let effects: Vec<u32> = index
        .properties(key)
        .ok()
        .and_then(|p| {
            p.0.get(&P_AMBIENT_SOUND_EFFECTS).map(|p| {
                p.values.iter().filter_map(|v| Some(u32::from_le_bytes(v.get(..4)?.try_into().ok()?))).collect()
            })
        })
        .unwrap_or_default();
    if effects.is_empty() {
        return vec![];
    }
    let Ok(dir) = index.read(AMBIENCE_EFFDIR).map_err(|e| e.to_string()).and_then(|d| effect_sounds(&d)) else {
        return vec![];
    };
    effects.iter().filter_map(|e| dir.get(e)).flatten().copied().collect()
}

/// Start the planet's ambient sound effects in a context, once.
fn start_planet_ambience(
    pd: &mut PdMusic,
    context: &'static str,
    started: &mut bool,
    originals: Option<&crate::Originals>,
    cache: Option<&crate::terrain::PlanetCache>,
) {
    if *started {
        return;
    }
    let Some(originals) = originals else { return };
    *started = true;
    let sounds = planet_ambience_sounds(&originals.resource_index, cache.map(|c| c.script.terrain_script));
    for id in sounds {
        pd.start_mode_sound(id, "planet_ambience", context);
    }
}

/// Values the providers last sent (they send on change only).
#[derive(Default)]
struct Providers(HashMap<&'static str, f32>);

impl Providers {
    fn set(&mut self, pd: &mut PdMusic, name: &'static str, value: f32) {
        if self.0.get(name) != Some(&value) {
            self.0.insert(name, value);
            pd.set_audio_param(name, value);
            if super::debug() && name != "planetzoom" && name != "cameraspeed" && name != "spacezoom" {
                pd.note(format!("provider {name} = {value}"));
            }
        }
    }
}

/// Camera height above the ground under it (`planetzoom`).
fn camera_altitude(camera: Vec3, land: Option<&crate::terrain::LandSurface>) -> f32 {
    let ground = land.map_or(0.0, |l| l.ground_height(camera.x, camera.z));
    (camera.y - ground).max(0.0)
}

/// `layer_water`'s `Radius` (soundProp `layer_water`, VERIFIED data).
pub const WATER_LAYER_RADIUS: f32 = 40.0;

/// The camera's target on the ground (its look ray against the ground
/// height under the camera) and the camera's distance to it.
pub fn camera_target(eye: Vec3, forward: Vec3, land: Option<&crate::terrain::LandSurface>) -> (Vec3, f32) {
    let ground = land.map_or(0.0, |l| l.ground_height(eye.x, eye.z));
    let t = if forward.y < -0.05 { ((eye.y - ground) / -forward.y).clamp(0.0, 2000.0) } else { 30.0 };
    let target = eye + forward * t;
    (target, t)
}

/// Pure: plants of the planet flora within `radius` (+ ADAPTED plant radius
/// 1) of a planet-local point (`plantcount`, provider 0xEA5A30 -> 0xB79FA0:
/// the plants within the camera's distance of its target; retail also skips
/// plants of size class 3 and up, OpenSpore's flora has no size class).
pub fn plant_count(plan: &crate::planet_flora::FloraPlan, local: Vec3, radius: f32) -> usize {
    let r2 = (radius + 1.0) * (radius + 1.0);
    plan.groves
        .iter()
        .filter(|g| g.centre.distance_squared(local) < (radius + 40.0) * (radius + 40.0))
        .flat_map(|g| g.plants.iter())
        .filter(|p| p.pos.distance_squared(local) < r2)
        .count()
}

/// Pure: `layer_water_density` (0..1). RETAIL (0xA35100): the layer's
/// registered water emitters / its maximum, from the effects that register
/// into `layer_water`; ADAPTED: the water fraction of a 9 x 9 sample grid
/// within the layer's `Radius` around the camera target.
pub fn water_density(land: &crate::terrain::LandSurface, target: Vec3, radius: f32) -> f32 {
    let mut water = 0;
    let mut n = 0;
    for i in -4..=4 {
        for j in -4..=4 {
            let (dx, dz) = (i as f32 / 4.0 * radius, j as f32 / 4.0 * radius);
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            n += 1;
            if land.is_water(target.x + dx, target.z + dz) {
                water += 1;
            }
        }
    }
    if n == 0 {
        0.0
    } else {
        water as f32 / n as f32
    }
}

/// The ambience patches' terrain inputs (`plantcount`, `layer_water_density`)
/// from the camera, on change (providers).
fn ambience_inputs(
    pd: &mut PdMusic,
    providers: &mut Providers,
    camera: Option<(&GlobalTransform,)>,
    land: Option<&crate::terrain::LandSurface>,
    flora: Option<(&crate::planet_flora::PlanetFlora, &GlobalTransform)>,
) {
    let Some((cam,)) = camera else { return };
    let (target, distance) = camera_target(cam.translation(), cam.forward().as_vec3(), land);
    if let Some((flora, root)) = flora {
        let local = root.affine().inverse().transform_point3(target);
        providers.set(pd, "plantcount", plant_count(&flora.plan, local, distance) as f32);
    }
    if let Some(land) = land {
        let d = water_density(land, target, WATER_LAYER_RADIUS);
        providers.set(pd, "layer_water_density", (d * 100.0).round() / 100.0);
    }
}

#[derive(Default)]
pub struct CreatureFeed {
    on: bool,
    ambience: bool,
    providers: Providers,
    stance: Option<bool>,
    mix: HashMap<String, f32>,
    friendship_full: bool,
    /// The last value sent of each of [`CREATURE_EVENT_INPUTS`].
    events: HashMap<&'static str, f32>,
    /// [`crate::creature_music::MissionPulse`] when last looked at.
    missions: u32,
}

/// The Creature inputs, while the `creature` context runs.
#[allow(clippy::too_many_arguments)]
pub fn feed_creature(
    mut pd: ResMut<PdMusic>,
    stage: Res<State<GameStage>>,
    params: Option<Res<CreatureAudioParams>>,
    hud: Option<Res<crate::creature_hud::CreatureHudState>>,
    run: Res<RunProgress>,
    land: Option<Res<crate::terrain::LandSurface>>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    originals: Option<Res<crate::Originals>>,
    cache: Option<Res<crate::terrain::PlanetCache>>,
    flora: Option<Res<crate::planet_flora::PlanetFlora>>,
    flora_root: Query<&GlobalTransform, With<crate::planet_flora::PlanetFloraRoot>>,
    pulse: Option<Res<crate::creature_music::MissionPulse>>,
    mut feed: Local<CreatureFeed>,
) {
    if *stage.get() != GameStage::Creature || !pd.context_active("creature") {
        if feed.on {
            *feed = CreatureFeed::default();
        }
        return;
    }
    let fresh = !feed.on;
    feed.on = true;
    let pd = &mut *pd;
    start_planet_ambience(pd, "creature", &mut feed.ambience, originals.as_deref(), cache.as_deref());
    let Some(params) = params else { return };
    let p = &params.0;
    for name in CREATURE_PER_FRAME {
        if let Some(v) = p.gfparam(name) {
            pd.set_audio_param(name, v);
        }
    }
    if feed.stance != Some(p.ui_fight) {
        feed.stance = Some(p.ui_fight);
        pd.set_audio_param("ui_social", if p.ui_fight { 0.0 } else { 1.0 });
        pd.set_audio_param("ui_fight", if p.ui_fight { 1.0 } else { 0.0 });
    }
    // The social meter's two bars, every update while it is shown.
    let meter = hud.as_ref().and_then(|h| h.social_meter.clone());
    if let Some(m) = &meter {
        // 0xD9B9C0: `npcsocialbar` = row 0 (the creature's side), `socialbar`
        // = row 1 (the player's), each as a fraction of the bar (0xE319F0).
        pd.set_audio_param("npcsocialbar", m.creature.clamp(0.0, 1.0));
        pd.set_audio_param("socialbar", m.player.clamp(0.0, 1.0));
    }
    let full = meter.as_ref().is_some_and(|m| m.friendship >= 0.999);
    if full && !feed.friendship_full {
        pd.set_audio_param("socialsuccess", 1.0);
    }
    feed.friendship_full = full;
    // The event inputs, on change only.
    for name in CREATURE_EVENT_INPUTS {
        let Some(v) = p.gfparam(name) else { continue };
        if feed.events.get(name).copied().unwrap_or(0.0) != v {
            feed.events.insert(name, v);
            pd.set_audio_param(name, v);
        }
    }
    // A mission completed (0xC2E745): `missioncomplete` 1, every time.
    let missions = pulse.map_or(0, |p| p.0);
    if fresh {
        // Completions of an earlier visit of the stage are old news.
        feed.missions = missions;
    }
    if missions > feed.missions {
        pd.set_audio_param("missioncomplete", 1.0);
    }
    feed.missions = missions;
    // Cinematic mixer events of the stage flow (`mixevent <name> 1/0`).
    for (name, value) in &p.mix {
        if feed.mix.get(name) != Some(value) {
            feed.mix.insert(name.clone(), *value);
            pd.set_audio_param(name, *value);
        }
    }
    let feed = &mut *feed;
    feed.providers.set(pd, "timeofday", timeofday_param(run.species_world.time_of_day));
    if let Ok(cam) = camera.single() {
        let alt = camera_altitude(cam.translation(), land.as_deref());
        feed.providers.set(pd, "planetzoom", (alt * 10.0).round() / 10.0);
    }
    ambience_inputs(pd, &mut feed.providers, camera.single().ok().map(|c| (c,)), land.as_deref(), flora.as_deref().zip(flora_root.iter().next()));
}

#[derive(Default)]
pub struct TribeFeed {
    on: bool,
    ambience: bool,
    providers: Providers,
    camera: Option<Vec3>,
    births: usize,
    gift_state: Option<(f32, Vec<f32>)>,
    social: SocialSync,
    /// [`crate::tribal_audio::GoalPulse`] when last looked at.
    goals: u32,
}

/// The Tribal performance as `music_tribe_social` sees it.
#[derive(Default, Debug, Clone)]
pub struct SocialSync {
    /// Village of the performance announced to the patch.
    pub village: Option<usize>,
    /// The patch answered with its length: its beats drive the notes.
    pub beat_driven: bool,
    pub rounds: u32,
    pub beats: u32,
    /// Seconds since the last beat and the measured beat period.
    pub since_beat: f32,
    pub period: f32,
    /// `performance.note` when the last beat came (or at the start).
    pub note_at_beat: u32,
    pub hits: u32,
    pub prompt: u32,
    /// Orders given when the last frame looked.
    pub orders: u32,
    /// The song is over but its bar is still up (`TribalCampaign::social_end`): the
    /// members play their ending and the stop routine (0xD9FE60) has not run yet.
    pub ending: bool,
}

impl SocialSync {
    /// One frame of a running performance: returns the `time_left` the
    /// current note should have (`Some(0)` = missed at this beat), or
    /// `None` to leave OpenSpore's own timer alone.
    pub fn step(&mut self, dt: f32, beat: bool, note: u32) -> Option<f32> {
        if !self.beat_driven {
            return None;
        }
        self.since_beat += dt;
        if beat {
            self.beats += 1;
            if self.beats > 1 {
                self.period = self.since_beat.max(0.25);
            }
            self.since_beat = 0.0;
            let missed = note == self.note_at_beat;
            self.note_at_beat = note + missed as u32;
            if self.rounds > 0 && self.beats >= self.rounds {
                // The patch plays no more beats: OpenSpore's timer finishes
                // the remaining notes.
                self.beat_driven = false;
            }
            return Some(if missed { 0.0 } else { self.period });
        }
        if self.since_beat > self.period * 3.0 + 4.0 {
            // No beat for too long (patch silent or stopped): fall back.
            self.beat_driven = false;
            return None;
        }
        Some((self.period - self.since_beat).max(0.25))
    }
}

/// The Tribe inputs, while the `tribe` context runs.
#[allow(clippy::too_many_arguments)]
pub fn feed_tribe(
    mut pd: ResMut<PdMusic>,
    stage: Res<State<GameStage>>,
    mut run: ResMut<RunProgress>,
    time: Res<Time>,
    land: Option<Res<crate::terrain::LandSurface>>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    rivals: Query<&crate::tribal_rts::RivalMember, Without<crate::tribal::Raider>>,
    mut sendgame: MessageReader<PdSendGame>,
    originals: Option<Res<crate::Originals>>,
    cache: Option<Res<crate::terrain::PlanetCache>>,
    pause: Option<Res<crate::pause_menu::PauseMenu>>,
    flora: (Option<Res<crate::planet_flora::PlanetFlora>>, Query<&GlobalTransform, With<crate::planet_flora::PlanetFloraRoot>>),
    goals: Option<Res<crate::tribal_audio::GoalPulse>>,
    mut feed: Local<TribeFeed>,
) {
    let sends: Vec<PdSendGame> = sendgame.read().cloned().collect();
    if *stage.get() != GameStage::Tribal || !pd.context_active("tribe") {
        if feed.on {
            *feed = TribeFeed::default();
        }
        return;
    }
    let pd = &mut *pd;
    let feed = &mut *feed;
    let fresh = !feed.on;
    feed.on = true;
    start_planet_ambience(pd, "tribe", &mut feed.ambience, originals.as_deref(), cache.as_deref());
    // --- providers ---------------------------------------------------------
    feed.providers.set(pd, "timeofday", timeofday_param(run.species_world.time_of_day));
    if let Ok(cam) = camera.single() {
        let at = cam.translation();
        let alt = camera_altitude(at, land.as_deref());
        feed.providers.set(pd, "planetzoom", (alt * 10.0).round() / 10.0);
        let moved = feed.camera.map_or(0.0, |c| c.distance(at));
        feed.camera = Some(at);
        feed.providers.set(pd, "cameraspeed", (moved * 100.0).round() / 100.0);
        feed.providers.set(pd, "tribecount", run.tribe.len() as f32 + 1.0);
        let home = crate::tribal::home_position();
        feed.providers.set(pd, "tribedistance", (at.distance(home)).round());
        let mut members: HashMap<usize, u32> = HashMap::new();
        for m in &rivals {
            *members.entry(m.village).or_default() += 1;
        }
        let nearest = members
            .keys()
            .map(|v| (*v, at.distance(crate::tribal::village_position(*v))))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((v, d)) = nearest {
            feed.providers.set(pd, "npctribecount", members[&v] as f32);
            feed.providers.set(pd, "npctribedistance", d.round());
        }
    }
    ambience_inputs(pd, &mut feed.providers, camera.single().ok().map(|c| (c,)), land.as_deref(), flora.0.as_deref().zip(flora.1.iter().next()));
    // --- events -----------------------------------------------------------------
    let campaign = &run.tribal_campaign;
    let births = campaign.births.len();
    if !fresh && births < feed.births {
        // A baby hatched (0xCD7692 / 0xD78890).
        pd.set_audio_param("tribe_babyborn", 1.0);
    }
    feed.births = births;
    // A gift (0xDBC119): the shared cooldown restarts and a village warms
    // up without a performance (the same test as game_audio's observer).
    let relations: Vec<f32> = campaign.villages.iter().map(|v| v.relations).collect();
    if let Some((cooldown, before)) = &feed.gift_state {
        let warmer = relations.iter().zip(before).any(|(a, b)| *a > b + 0.001);
        if campaign.performance.is_none() && campaign.cooldown > cooldown + 0.5 && warmer {
            pd.set_audio_param("giftpresent", 1.0);
        }
    }
    feed.gift_state = Some((campaign.cooldown, relations));
    // A goal card completed (0xC2E745, the mission completion): `missioncomplete`
    // 1, every time (`tribe_drums3` reads it).
    let goals = goals.map_or(0, |g| g.0);
    if fresh {
        // Completions of an earlier visit of the stage are old news.
        feed.goals = goals;
    }
    if goals > feed.goals {
        pd.set_audio_param("missioncomplete", 1.0);
    }
    feed.goals = goals;
    // --- the performance and music_tribe_social -------------------------------
    let mut beat = false;
    for s in &sends {
        match s.name.as_str() {
            "trg_social_length" if s.value > 1.5e-5 => {
                feed.social.rounds = s.value as u32;
                if feed.social.village.is_some() && !feed.social.beat_driven {
                    feed.social.beat_driven = true;
                    feed.social.beats = 0;
                    feed.social.since_beat = 0.0;
                    if feed.social.period <= 0.0 {
                        feed.social.period = crate::tribal::NOTE_WINDOW;
                    }
                }
            }
            "trg_social_beat" => beat = true,
            _ => {}
        }
    }
    let dt = time.delta_secs();
    let performance = run.tribal_campaign.performance;
    let bar_up = run.tribal_campaign.social_end.is_some();
    if performance.is_some() && feed.social.ending {
        // A new song hides the old song's bar (`start_performance_with`): the old
        // song's stop routine runs first.
        for (name, value) in song_end_inputs(false) {
            pd.set_audio_param(name, value);
        }
        feed.social = SocialSync::default();
    }
    match (performance, feed.social.village) {
        (Some(p), None) => {
            let at = crate::tribal::village_position(p.village);
            pd.set_audio_param("tribe_social_x", at.x);
            pd.set_audio_param("tribe_social_y", at.y);
            pd.set_audio_param("tribe_social_z", at.z);
            let (didger, horn, maraca) = instrument_counts(run.tribal_campaign.tools.values());
            pd.set_audio_param(SHAKER_START_RETAIL, maraca as f32);
            pd.set_audio_param("didger_start", didger as f32);
            pd.set_audio_param("flute_start", horn as f32);
            feed.social = SocialSync {
                village: Some(p.village),
                note_at_beat: p.note,
                hits: p.hits,
                prompt: p.prompt,
                period: crate::tribal::NOTE_WINDOW,
                ..default()
            };
            // `trg_social_length` may already have come this frame.
            if feed.social.rounds == 0 {
                feed.social.rounds = sends
                    .iter()
                    .find(|s| s.name == "trg_social_length")
                    .map_or(0, |s| s.value as u32);
                feed.social.beat_driven = feed.social.rounds > 0;
            }
        }
        (Some(p), Some(_)) => {
            if p.orders > feed.social.orders {
                // An instrument was ordered (0xDA0100): the players of that
                // instrument solo.
                let (didger, horn, maraca) = instrument_counts(run.tribal_campaign.tools.values());
                if let Some(name) = solo_param(p.last_order) {
                    let n = match name {
                        "flute_solo" => horn,
                        "shaker_solo" => maraca,
                        _ => didger,
                    };
                    pd.set_audio_param(name, n as f32);
                }
            }
            if p.hits > feed.social.hits {
                // Players of the wanted instrument played: the success gong.
                pd.set_audio_param("trg_social_success", 1.0);
            }
            feed.social.hits = p.hits;
            feed.social.prompt = p.prompt;
            feed.social.orders = p.orders;
            // The game is paused (the pause menu, a held cinematic): the
            // song's notes wait; beats heard meanwhile do not count.
            if dt <= 0.0 || pause.is_some_and(|p| p.open) {
                return;
            }
            if let Some(left) = feed.social.step(dt, beat, p.note) {
                if let Some(perf) = run.tribal_campaign.performance.as_mut() {
                    perf.time_left = left;
                }
            }
        }
        (None, Some(_)) => {
            if bar_up {
                // The song is over and its bar stays up while the members play
                // their ending: the stop routine (0xD9FE60) has not run yet.
                feed.social.ending = true;
            } else {
                // The stop routine ran: the bar went away (or the chieftain walked
                // off, with rounds remaining: 0xDA34BE sets `trg_social_stop_on_beat`).
                let left = !feed.social.ending && feed.social.beats < feed.social.rounds;
                for (name, value) in song_end_inputs(left) {
                    pd.set_audio_param(name, value);
                }
                feed.social = SocialSync::default();
            }
        }
        (None, None) => {}
    }
}

/// Pure: the `terraformscore` (0..1) of a world from its climate axes
/// (temperature, atmosphere in 0..1, optimum 0.5). ADAPTED: OpenSpore keeps
/// the climate point, not the retail score; `1 - 1.41 d` (d = distance to
/// the optimum) puts OpenSpore's T-score bands (d < 0.4 / 0.25 / 0.1) on the
/// thresholds `music_planet` uses for T1 / T2 / T3 (0.434 / 0.648 / 0.858).
pub fn terraformscore(temperature: f32, atmosphere: f32) -> f32 {
    let d = ((temperature - 0.5).powi(2) + (atmosphere - 0.5).powi(2)).sqrt();
    (1.0 - 1.41 * d).clamp(0.0, 1.0)
}

#[derive(Default)]
pub struct SpaceFeed {
    on: bool,
    providers: Providers,
    war_level: Option<i32>,
    comm: Option<bool>,
    /// The black hole loops playing (system, voices).
    black_hole: Option<(u32, Vec<crate::pd::engine::VoiceId>)>,
}

/// The loops of a black hole's effects (`games.effdir` sound blocks, VERIFIED
/// data): `space_blackhole_lp` (3D, 800..1100), `space_blackholeb_lp` (2D),
/// `space_blackhole_eventhorizon_lp` (3D, 250..1300), all in the mixer group
/// the `mix_blackhole` patch would drive (its `spg_wormhole` is never set).
pub const BLACK_HOLE_LOOPS: [&str; 3] = ["space_blackhole_lp", "space_blackholeb_lp", "space_blackhole_eventhorizon_lp"];

/// The Space inputs, while the `space` context runs.
#[allow(clippy::too_many_arguments)]
pub fn feed_space(
    mut pd: ResMut<PdMusic>,
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    view: Option<Res<crate::space_view::SpaceView>>,
    war: Option<Res<crate::space_audio::SpaceWarInputs>>,
    pirates: Query<&crate::space::Pirate>,
    hud: Option<Res<crate::space_hud::SpaceHudState>>,
    galaxy: Option<Res<crate::space_galaxy::Galaxy>>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    mut feed: Local<SpaceFeed>,
) {
    if *stage.get() != GameStage::Space || !pd.context_active("space") {
        if feed.on {
            *feed = SpaceFeed::default();
        }
        return;
    }
    feed.on = true;
    let pd = &mut *pd;
    let feed = &mut *feed;
    let Some(view) = view else { return };
    let zoom = view.zoom();
    // `spacezoom` (0x1003240: the camera radius across the view's range).
    match zoom {
        crate::space_view::SpaceZoom::Galaxy => {
            let z = crate::space_audio::spacezoom(true, view.gal.radius);
            feed.providers.set(pd, "spacezoom", (z * 1000.0).round() / 1000.0);
        }
        crate::space_view::SpaceZoom::System => {
            let z = crate::space_audio::spacezoom(false, view.solar.radius);
            feed.providers.set(pd, "spacezoom", (z * 1000.0).round() / 1000.0);
        }
        crate::space_view::SpaceZoom::Planet => {}
    }
    // `spg_war_music_level`: set when the level changes (0xFFAA70 / 0xFFA170).
    let inputs = war.map(|w| w.clone()).unwrap_or_default();
    let level = crate::space_audio::war_level(&crate::space_audio::WarSituation {
        at_war_planet: inputs.at_war_planet,
        threat_ships: pirates.iter().any(|p| p.hp > 0.0),
        bomber_on_player: inputs.bomber_on_player,
        attackers: inputs.attackers,
        safe: inputs.safe,
    });
    if feed.war_level != Some(level) {
        feed.war_level = Some(level);
        pd.set_audio_param("spg_war_music_level", level as f32);
    }
    // The planet context: altitude and the world's scores (providers).
    if let Some(planet) = view.planet.as_ref().filter(|_| zoom == crate::space_view::SpaceZoom::Planet) {
        feed.providers.set(pd, "planetzoom", planet.altitude.round());
        let eco = run.space_campaign.ecology_for(planet.world);
        let q = |v: f32| (v * 1000.0).round() / 1000.0;
        feed.providers.set(pd, "atmospherescore", q(eco.atmosphere));
        feed.providers.set(pd, "temperaturescore", q(eco.temperature));
        feed.providers.set(pd, "terraformscore", q(terraformscore(eco.temperature, eco.atmosphere)));
        feed.providers.set(pd, "planet_has_player_city", if eco.colony_count > 0 { 1.0 } else { 0.0 });
    }
    // The communication screen (mixmode_commscreen: its mixer patch ducks the
    // city music; the screen's own sounds are played by the comm code).
    let comm = hud.is_some_and(|h| h.comm_open);
    if feed.comm != Some(comm) {
        feed.comm = Some(comm);
        pd.set_audio_param("mixmode_commscreen", if comm { 1.0 } else { 0.0 });
    }
    // A black hole system in the solar view: its effect's loops at the
    // star (the scene's origin), heard from the camera in retail units.
    let system = run.space_campaign.current_system;
    let black_hole = zoom == crate::space_view::SpaceZoom::System
        && galaxy.as_ref().and_then(|g| g.star(system)).is_some_and(|s| s.kind == crate::space_galaxy::StarKind::BlackHole);
    if feed.black_hole.as_ref().is_some_and(|(s, _)| !black_hole || *s != system) {
        for v in feed.black_hole.take().map(|(_, v)| v).unwrap_or_default() {
            pd.stop_voice(v);
        }
        pd.listener = None;
    }
    if black_hole {
        if feed.black_hole.is_none() {
            let voices = BLACK_HOLE_LOOPS.iter().filter_map(|n| pd.start_spatial_plain(crate::pd::fnv(n), "solar", Vec3::ZERO)).collect();
            feed.black_hole = Some((system, voices));
        }
        if let Ok(cam) = camera.single() {
            pd.listener = Some(cam.translation() / crate::space_view::SOLAR_SCALE);
        }
    }
}

/// `ufo_travelling`: the player UFO's engine sound (PlayProceduralAudio
/// 0xE98E59 with the UFO's own track; NPC ships play `ufo_npc`).
pub const UFO_SOUND: &str = "ufo_travelling";

/// Pure: `ufodrivetype` (provider 0xEA63D0, Space game mode only) from the
/// ship's tools. VERIFIED shape: 3 when any of nine tool checks holds, else 2
/// / 1 by two more checks, else 0. INFERRED: the checks are the interstellar
/// drives, best first (upgrades 2-4 -> 3, upgrade 1 -> 2, the first drive
/// -> 1; the tool keys are runtime globals, not read).
pub fn ufodrivetype<S: AsRef<str>>(tools: &[S]) -> f32 {
    let has = |id: &str| tools.iter().any(|t| t.as_ref() == id);
    if ["interstellardrive_upgrade2", "interstellardrive_upgrade3", "interstellardrive_upgrade4"].iter().any(|d| has(d)) {
        3.0
    } else if has("interstellardrive_upgrade1") {
        2.0
    } else if has("interstellardrive") {
        1.0
    } else {
        0.0
    }
}

#[derive(Default)]
pub struct UfoFeed {
    on: bool,
    started: bool,
    providers: Providers,
}

/// The player UFO's engine patch (`ufo_travelling`) and the Space
/// providers it reads. RETAIL (0xE98D90 / 0xE9D2E0): the UFO starts its
/// track when it becomes visible (any context for the player's ship) and
/// stops it when hidden; every update it sets the track position, `bank` =
/// |bank|, `damage` = health / max health, `speed` = |velocity| / the UFO's
/// scale. Providers: `atplanet` 1 while a planet with terrain is loaded
/// (0xEA5D30), `ufodrivetype` ([`ufodrivetype`]). The track's `startstop`
/// 1 / 0 at start / stop: INFERRED from the procedural track object
/// (0xA37760 / 0xA377D0), the patch keeps itself busy 1 s after 0.
#[allow(clippy::too_many_arguments)]
pub fn feed_ufo(
    mut pd: ResMut<PdMusic>,
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    view: Option<Res<crate::space_view::SpaceView>>,
    ship: Query<(&GlobalTransform, &crate::space::ShipMotion, Option<&crate::game::Health>), With<crate::game::Player>>,
    mut feed: Local<UfoFeed>,
) {
    let id = crate::pd::fnv(UFO_SOUND);
    let active = *stage.get() == GameStage::Space && pd.context_active("space");
    let ship = ship.single().ok();
    if !active || ship.is_none() {
        if feed.started {
            pd.set_sound_param_id(id, "startstop", 0.0);
            pd.stop_sound(id);
        }
        if feed.on {
            *feed = UfoFeed::default();
        }
        return;
    }
    feed.on = true;
    let pd = &mut *pd;
    let feed = &mut *feed;
    let Some((at, motion, health)) = ship else { return };
    if !feed.started {
        if pd.start_sound(id, UFO_SOUND, "space").is_none() && pd.instance_of(id).is_none() {
            return;
        }
        feed.started = true;
        pd.set_sound_param_id(id, "startstop", 1.0);
    }
    let planet = view.as_ref().and_then(|v| v.planet.as_ref().filter(|_| v.zoom() == crate::space_view::SpaceZoom::Planet));
    // Retail units: the planet view keeps retail proportion (`unit`), the
    // solar scene is `SOLAR_SCALE` of a retail system.
    let (unit, bank) = match planet {
        Some(p) => (p.unit().max(1e-6), p.bank),
        None => (crate::space_view::SOLAR_SCALE, motion.attitude.bank),
    };
    let pos = at.translation() / unit;
    pd.set_sound_param_id(id, "x", pos.x);
    pd.set_sound_param_id(id, "y", pos.y);
    pd.set_sound_param_id(id, "z", pos.z);
    pd.set_sound_param_id(id, "bank", bank.abs());
    let damage = health.map_or(1.0, |h| if h.max > 0.0 { (h.hp / h.max).clamp(0.0, 1.0) } else { 1.0 });
    pd.set_sound_param_id(id, "damage", damage);
    // ADAPTED: the UFO's scale is 1 (the retail divisor is the model's scale).
    pd.set_sound_param_id(id, "speed", motion.velocity.length() / unit);
    feed.providers.set(pd, "atplanet", if planet.is_some() { 1.0 } else { 0.0 });
    feed.providers.set(pd, "ufodrivetype", ufodrivetype(&run.space_campaign.tools));
}

#[derive(Default)]
pub struct CivFeed {
    on: bool,
    ambience: bool,
    providers: Providers,
    camera: Option<Vec3>,
}

/// The Civilization inputs (opt-in `OPENSPORE_PD_CIV=1`): the providers the
/// mode's patches read (`planetzoom`, `cameraspeed`, `timeofday`) and the
/// planet's ambient sound effect.
#[allow(clippy::too_many_arguments)]
pub fn feed_civ(
    mut pd: ResMut<PdMusic>,
    stage: Res<State<GameStage>>,
    run: Res<RunProgress>,
    land: Option<Res<crate::terrain::LandSurface>>,
    camera: Query<&GlobalTransform, With<crate::GameCamera>>,
    originals: Option<Res<crate::Originals>>,
    cache: Option<Res<crate::terrain::PlanetCache>>,
    flora: Option<Res<crate::planet_flora::PlanetFlora>>,
    flora_root: Query<&GlobalTransform, With<crate::planet_flora::PlanetFloraRoot>>,
    mut feed: Local<CivFeed>,
) {
    if *stage.get() != GameStage::Civilization || !pd.context_active("civ") {
        if feed.on {
            *feed = CivFeed::default();
        }
        return;
    }
    feed.on = true;
    let pd = &mut *pd;
    let feed = &mut *feed;
    start_planet_ambience(pd, "civ", &mut feed.ambience, originals.as_deref(), cache.as_deref());
    feed.providers.set(pd, "timeofday", timeofday_param(run.species_world.time_of_day));
    if let Ok(cam) = camera.single() {
        let at = cam.translation();
        let alt = camera_altitude(at, land.as_deref());
        feed.providers.set(pd, "planetzoom", (alt * 10.0).round() / 10.0);
        let moved = feed.camera.map_or(0.0, |c| c.distance(at));
        feed.camera = Some(at);
        feed.providers.set(pd, "cameraspeed", (moved * 100.0).round() / 100.0);
    }
    ambience_inputs(pd, &mut feed.providers, camera.single().ok().map(|c| (c,)), land.as_deref(), flora.as_deref().zip(flora_root.iter().next()));
}

/// What the open editor shows that its music hears (one frame).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorSnapshot {
    /// `Editors::Mode`: build 0, paint 1, play (Test Drive) 2.
    pub mode: u8,
    /// Parts palette category (tab).
    pub category: usize,
    /// Blocks (parts) of the creation.
    pub parts: usize,
    /// Whole-skin paint style (creature / cell paint theme; 0 = none).
    pub skin_theme: u32,
    /// The skin paint's colours (creature: three layers, cell: one).
    pub skin_colors: Vec<[f32; 3]>,
    /// Rigid-block editors: the style applied to every block, the swatch
    /// (brush) picked, and a signature of the blocks' region paints.
    pub block_theme: Option<u32>,
    pub swatch: Option<u32>,
    pub block_paint: u64,
}

/// The editor events of retail (`notes\audio-sweep-notes.md` E1), as
/// `(instance parameter, value)` sent to the editor's music patch.
#[derive(Clone, Debug, Default)]
pub struct EditorEvents {
    last: Option<EditorSnapshot>,
    /// Global 0x15E4EEC: raised by every part added and every paint style /
    /// swatch event, never reset (VERIFIED 0x577168, 0x591858 ...).
    pub counter: u32,
}

impl EditorEvents {
    /// Forget the previous frame (a new session or another editor): the
    /// next frame is the baseline and only sets the mode.
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// Pure: the events between the previous frame and `now`.
    pub fn step(&mut self, now: EditorSnapshot) -> Vec<(&'static str, f32)> {
        let mut out = vec![];
        let Some(before) = self.last.replace(now.clone()) else {
            // SetActiveMode when the editor opens.
            out.push(("ModeSet", now.mode as f32));
            return out;
        };
        if before.mode != now.mode {
            // SetActiveMode 0x5879F5: the new mode.
            out.push(("ModeSet", now.mode as f32));
        }
        if before.category != now.category {
            // PaletteUI::SetActiveCategory 0x5CB3EE: the category index.
            out.push(("PaletteSet", now.category as f32));
        }
        if now.parts != before.parts {
            // 0x577130 (added: ++counter) / 0x5771F0 (removed: the counter),
            // then SizeChange and BlockCountChange of the model.
            if now.parts > before.parts {
                for _ in before.parts..now.parts {
                    self.counter += 1;
                    out.push(("RigBlockAdded", self.counter as f32));
                }
            } else {
                out.push(("RigBlockRemoved", self.counter as f32));
            }
            // ADAPTED: retail SizeChange is the summed block volume
            // (0x48CF10); only printed by the patches. The block count is
            // OpenSpore's part count (0x48D120 counts the model's blocks).
            out.push(("SizeChange", now.parts as f32));
            out.push(("BlockCountChange", now.parts as f32));
        }
        if now.skin_theme != before.skin_theme && now.skin_theme != 0 {
            // A whole-skin style from the paint palette (message 0xDEE3D8A8).
            self.counter += 1;
            out.push(("SkinPaintTheme", self.counter as f32));
        } else if now.skin_colors != before.skin_colors {
            // The colour picker (cEditor::HandleMessage 0x5932C3): 2g + r + 4b.
            if let Some(c) = now.skin_colors.iter().zip(before.skin_colors.iter().chain(std::iter::repeat(&[-1.0; 3]))).find(|(a, b)| a != b).map(|(a, _)| *a) {
                out.push(("ColorPicked", c[1] * 2.0 + c[0] + c[2] * 4.0));
            }
        }
        let theme = now.block_theme != before.block_theme && now.block_theme.is_some();
        if theme {
            // A style for every block (messages 0x17D37D90 / 0x0674AB27).
            self.counter += 1;
            out.push(("BlockPaintTheme", self.counter as f32));
        }
        if now.swatch != before.swatch && now.swatch.is_some() {
            // A swatch picked (message 0xBD110A25).
            self.counter += 1;
            out.push(("BlockSwatchSelect", self.counter as f32));
        }
        if now.mode == 1 && !theme && now.parts == before.parts && now.block_paint != before.block_paint {
            // A block painted with the brush (cEditor::OnMouseDown 0x58881D).
            // ADAPTED value: the counter (retail sends an editor field the
            // patches only bang on).
            out.push(("BlockPaintApply", self.counter as f32));
        }
        out
    }
}

#[derive(Default)]
pub struct EditorFeed {
    on: bool,
    patch: Option<u32>,
    events: EditorEvents,
}

/// Pure: a signature of the rigid blocks' region paints.
pub fn block_paint_signature(parts: &[crate::game::PartInstance]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut mix = |v: u32| {
        h ^= v as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    };
    for (i, p) in parts.iter().enumerate() {
        for e in p.rigid.iter().flat_map(|b| b.paints.iter()) {
            mix(i as u32);
            mix(e.region);
            mix(e.paint);
            for c in e.color1.iter().chain(e.color2.iter()) {
                mix(c.to_bits());
            }
        }
    }
    h
}

/// The editors' music events: instance parameters of the open editor's
/// `editorBackgroundMusicPatch` (VERIFIED `Audio::SetProperty(0x1D6253C0,
/// id, value)` = 0x436400 on the editor's music track; ids and values in
/// [`EditorEvents::step`]). Every editor: the creature, cell, rigid-block
/// (vehicles, buildings, UFO) editors and the outfitters.
#[allow(clippy::too_many_arguments)]
pub fn feed_editor(
    mut pd: ResMut<PdMusic>,
    run: Res<RunProgress>,
    editor: Option<Res<crate::creature_editor::CreatureEditor>>,
    cell: Option<Res<crate::cell_editor::CellEditor>>,
    rigid: Option<Res<crate::rigid_paint::RigidPaintSession>>,
    mut feed: Local<EditorFeed>,
) {
    if !pd.context_active("editor") {
        if feed.on {
            feed.on = false;
            feed.patch = None;
            feed.events.reset();
        }
        return;
    }
    feed.on = true;
    let Some(patch) = pd.editor_patch_name() else { return };
    if feed.patch != Some(patch) {
        feed.patch = Some(patch);
        feed.events.reset();
    }
    let creature = editor.as_ref().filter(|e| e.active);
    let cell = cell.filter(|c| c.active && creature.is_none());
    let snapshot = if let Some(e) = creature {
        let rigid_session = rigid.as_ref().filter(|r| r.active && e.kind.is_rigid());
        let paint = &run.creature_creation.paint;
        EditorSnapshot {
            mode: match e.mode {
                crate::creature_editor::Mode::Build => 0,
                crate::creature_editor::Mode::Paint => 1,
                crate::creature_editor::Mode::Play => 2,
            },
            category: e.category,
            parts: run.parts.len(),
            skin_theme: if e.kind.is_rigid() { 0 } else { paint.theme },
            skin_colors: if e.kind.is_rigid() { vec![] } else { paint.colors.to_vec() },
            block_theme: rigid_session.and_then(|r| r.theme),
            swatch: rigid_session.and_then(|r| r.brush.map(|(id, _, _)| id)),
            block_paint: if rigid_session.is_some() { block_paint_signature(&run.parts) } else { 0 },
        }
    } else if let Some(c) = cell {
        let paint = &run.cell_creation.paint;
        EditorSnapshot {
            mode: match c.mode {
                crate::cell_editor::Mode::Build => 0,
                crate::cell_editor::Mode::Paint => 1,
            },
            parts: run.parts.len(),
            skin_theme: paint.theme,
            skin_colors: vec![paint.color],
            ..default()
        }
    } else {
        return;
    };
    for (name, value) in feed.events.step(snapshot) {
        pd.set_sound_param_id(patch, name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retail_maracas_id_has_a_trailing_space() {
        assert_eq!(crate::pd::fnv(SHAKER_START_RETAIL), 0x40F3_A3B0);
        assert_eq!(crate::pd::fnv("shaker_start"), 0x86B6_4830);
        assert_eq!(crate::pd::fnv("flute_start"), 0xE491_E7A0);
        assert_eq!(crate::pd::fnv("didger_start"), 0x71D8_757F);
    }

    #[test]
    fn timeofday_puts_the_creature_start_inside_the_dawn_window() {
        // creatureStartTimeOfDay 7 h -> 280: ambience_dawn_dusk's dawn loop
        // runs from 220 to 310, dusk from 760 to 850.
        assert_eq!(timeofday_param(7.0), 280.0);
        assert!((220.0..310.0).contains(&timeofday_param(7.0)));
        assert!((760.0..850.0).contains(&timeofday_param(19.5)));
        assert_eq!(timeofday_param(24.0), 0.0);
    }

    #[test]
    fn instrument_counts_and_solos_follow_the_retail_mapping() {
        let tools = [ToolKind::WoodenHorns, ToolKind::Maracas, ToolKind::Aerophones, ToolKind::Aerophones, ToolKind::StoneAxes];
        assert_eq!(instrument_counts(tools.iter()), (2, 1, 1));
        assert_eq!(solo_param(3), Some("flute_solo"));
        assert_eq!(solo_param(4), Some("shaker_solo"));
        assert_eq!(solo_param(5), Some("didger_solo"));
        assert_eq!(solo_param(1), None);
    }

    #[test]
    fn beats_close_the_answer_windows() {
        let mut s = SocialSync { village: Some(0), beat_driven: true, rounds: 5, period: 4.0, ..default() };
        // Between beats the note waits for the beat.
        assert!(s.step(1.0, false, 0).unwrap() > 0.0);
        // No answer by the beat: missed.
        assert_eq!(s.step(1.0, true, 0), Some(0.0));
        // The miss moved the song on; the next note is answered in time.
        assert!(s.step(1.0, false, 1).unwrap() > 0.0);
        assert!(s.step(1.0, false, 2).unwrap() > 0.0);
        let left = s.step(1.0, true, 2).unwrap();
        assert!(left > 0.0, "answered before the beat");
        assert!((s.period - 3.0).abs() < 1e-4, "period measured between beats: {}", s.period);
        // After the patch's last round OpenSpore's timer takes over.
        s.step(1.0, true, 3);
        s.step(1.0, true, 4);
        s.step(1.0, true, 5);
        assert!(!s.beat_driven);
        assert_eq!(s.step(1.0, false, 6), None);
    }

    #[test]
    fn a_silent_patch_hands_the_notes_back() {
        let mut s = SocialSync { village: Some(0), beat_driven: true, rounds: 5, period: 4.0, ..default() };
        for _ in 0..20 {
            s.step(1.0, false, 0);
        }
        assert!(!s.beat_driven);
    }

    #[test]
    fn terraformscore_bands_meet_the_planet_music_thresholds() {
        // OpenSpore's T-score bands (distance to the optimum 0.4 / 0.25 / 0.1)
        // land on music_planet's T1 / T2 / T3 thresholds.
        for (d, want) in [(0.4f32, 0.434f32), (0.25, 0.648), (0.1, 0.858)] {
            let ts = terraformscore(0.5 + d, 0.5);
            assert!((ts - want).abs() < 0.005, "{d}: {ts}");
        }
        assert_eq!(terraformscore(0.5, 0.5), 1.0);
        assert!(terraformscore(0.0, 0.0) < 0.01);
    }

    #[test]
    fn trixie_reads_track_counts_and_shuffler_attributes() {
        let xml = "<tracks><track name=\"A\"><shuffler soundset=\"S1\" minVolume=\"80\" /><shuffler soundset=\"S2\" minVolume=\"70\" /></track><track name='B'><rhythm name=\"r\" tempo=\"98\"/></track></tracks>";
        let tracks = crate::pd::objects::parse_tracks(xml);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].shufflers.len(), 2);
        assert_eq!(tracks[0].shufflers[1][0], ("soundset".to_string(), crate::pd::Atom::S(crate::pd::sym("S2"))));
        assert_eq!(tracks[0].shufflers[1][1], ("minVolume".to_string(), crate::pd::Atom::F(70.0)));
        assert_eq!(tracks[1].name, "B");
        assert_eq!(tracks[1].rhythms.len(), 1);
    }

    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_space_patches_follow_zoom_war_and_planet() {
        let Some((lib, audio)) = installed() else { return };
        // The stems' gains follow spacezoom.
        let mut h = crate::pd::trace::Headless::new(lib.clone(), audio.clone(), 1_489_853_723);
        h.start("space_music_patch").unwrap();
        h.step(1.0 / 60.0);
        let stems = starts(&h, 0.0, 1.0);
        for s in ["solar_long", "interstellar_long", "galaxy_long"] {
            assert!(stems.iter().any(|x| x == s), "{stems:?}");
        }
        // The war patch: level 4 brings the battle loops, level 1 ends them.
        let mut h = crate::pd::trace::Headless::new(lib.clone(), audio.clone(), 1_489_853_723);
        h.start("music_space_war_patch").unwrap();
        for (t, level) in [(5.0, 2.0), (25.0, 4.0), (55.0, 1.0)] {
            while h.now < t {
                h.step(1.0 / 60.0);
            }
            h.engine.set_global_param("spg_war_music_level", level);
        }
        while h.now < 80.0 {
            h.step(1.0 / 60.0);
        }
        let war = starts(&h, 0.0, 80.0);
        assert!(war.iter().any(|x| x.starts_with("music_space_war_loop")) && war.iter().any(|x| x == "music_space_war_end"), "{war:?}");
        // The planet music: trixie track lists picked by the world's scores.
        let mut h = crate::pd::trace::Headless::new(lib, audio, 1_489_853_723);
        h.start("music_planet").unwrap();
        h.step(1.0 / 60.0);
        h.engine.set_global_param("terraformscore", terraformscore(0.55, 0.5));
        h.engine.set_global_param("atmospherescore", 0.7);
        h.engine.set_global_param("temperaturescore", 0.7);
        while h.now < 60.0 {
            h.step(1.0 / 60.0);
        }
        let planet = starts(&h, 0.0, 60.0);
        eprintln!("music_planet: {planet:?}");
        assert!(planet.len() >= 3, "the shufflers play the chosen track: {planet:?}");
        let real: Vec<_> = h.engine.errors.iter().filter(|e| e.contains("trixie")).collect();
        assert!(real.is_empty(), "{real:?}");
    }

    /// The installed patches with the retail inputs (deterministic seeds).
    fn installed() -> Option<(std::sync::Arc<crate::pd::library::PdLibrary>, std::sync::Arc<crate::game_audio::AudioLibrary>)> {
        let install = crate::spore_install::discover().ok()?;
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        Some((
            std::sync::Arc::new(crate::pd::library::PdLibrary::from_index(&index)),
            std::sync::Arc::new(crate::game_audio::AudioLibrary::from_index(&index)),
        ))
    }

    fn starts(h: &crate::pd::trace::Headless, from: f64, to: f64) -> Vec<String> {
        h.events
            .iter()
            .filter(|e| e.t >= from && e.t < to)
            .filter_map(|e| match &e.ev {
                crate::pd::trace::Ev::Start { sound, .. } => Some(sound.clone()),
                _ => None,
            })
            .collect()
    }

    fn sends(h: &crate::pd::trace::Headless, name: &str) -> Vec<(f64, f32)> {
        h.events
            .iter()
            .filter_map(|e| match &e.ev {
                crate::pd::trace::Ev::SendGame { name: n, value } if n == name => Some((e.t, *value)),
                _ => None,
            })
            .collect()
    }

    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_creature_score_follows_the_retail_feed() {
        let Some((lib, audio)) = installed() else { return };
        let mut h = crate::pd::trace::Headless::new(lib, audio, 1_489_853_723);
        h.start("music_creature_modified").unwrap();
        h.engine.set_global_param("mixmode_creaturegame", 1.0);
        h.engine.set_global_param("ui_social", 1.0);
        h.engine.set_global_param("ui_fight", 0.0);
        h.engine.set_global_param("timeofday", timeofday_param(7.0));
        let mut p = crate::creature_music::Params::default();
        let mut t = 0.0;
        while t < 90.0 {
            // A fight from 20 to 40 s, a befriending from 55 to 75 s.
            p.infight = (20.0..40.0).contains(&t);
            for name in CREATURE_PER_FRAME {
                h.engine.set_global_param(name, p.gfparam(name).unwrap());
            }
            if (55.0..75.0).contains(&t) {
                let bar = ((t - 55.0) / 20.0) as f32;
                h.engine.set_global_param("socialbar", bar);
                h.engine.set_global_param("npcsocialbar", bar);
            }
            h.step(1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        let idle = starts(&h, 0.5, 19.0);
        let fight = starts(&h, 20.0, 41.0);
        let social = starts(&h, 55.0, 76.0);
        eprintln!("idle {idle:?}\nfight {} {:?}\nsocial {} {:?}", fight.len(), &fight[..fight.len().min(8)], social.len(), &social[..social.len().min(8)]);
        assert!(idle.is_empty(), "the exploring score is event driven: {idle:?}");
        assert!(fight.iter().any(|s| s.starts_with("music_fight_")) && fight.iter().any(|s| s == "upright_bass2"), "{fight:?}");
        assert!(social.iter().any(|s| s == "soft") && social.iter().any(|s| s == "piano2"), "{social:?}");
    }

    /// The Creature event inputs reach their patches (`creature-sound2`): rain
    /// density -> `crg_rain`, `crg_ufo` -> `mix_event_crg_ufo`, `crg_meteors` and
    /// `missioncomplete` -> `music_creature_modified`.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_creature_event_inputs_drive_their_patches() {
        let Some((lib, audio)) = installed() else { return };
        let stops = |h: &crate::pd::trace::Headless, from: f64| {
            h.events.iter().filter(|e| e.t >= from && matches!(e.ev, crate::pd::trace::Ev::Stop { .. })).count()
        };
        // Rain: a 1 starts the rain loop after the patch's 3 s delay; a 0 stops it and starts the
        // rain birds.
        let mut h = crate::pd::trace::Headless::new(lib.clone(), audio.clone(), 1_489_853_723);
        h.start("crg_rain").unwrap();
        h.step(1.0 / 60.0);
        assert!(starts(&h, 0.0, 99.0).is_empty(), "no rain before the storm message");
        h.engine.set_global_param("layer_water_density", 0.0);
        h.engine.set_global_param("rain_density", 1.0);
        while h.now < 2.5 {
            h.step(1.0 / 60.0);
        }
        assert!(starts(&h, 0.0, 99.0).is_empty(), "the rain starts 3 s after the message");
        while h.now < 8.0 {
            h.step(1.0 / 60.0);
        }
        assert_eq!(starts(&h, 0.0, 8.0), ["sfx_weather_rain_lp"]);
        assert_eq!(stops(&h, 0.0), 0);
        h.engine.set_global_param("rain_density", 0.0);
        while h.now < 12.0 {
            h.step(1.0 / 60.0);
        }
        assert!(stops(&h, 8.0) >= 1, "the storm leaving stops the rain");
        assert!(starts(&h, 8.0, 12.0).iter().any(|s| s == "sfx_weather_rainbirds_lp"), "{:?}", starts(&h, 8.0, 12.0));
        // A 1 sent every frame keeps restarting the 3 s delay: the rain never starts.
        let mut h = crate::pd::trace::Headless::new(lib.clone(), audio.clone(), 1_489_853_723);
        h.start("crg_rain").unwrap();
        while h.now < 8.0 {
            h.engine.set_global_param("rain_density", 1.0);
            h.step(1.0 / 60.0);
        }
        eprintln!("rain starts with a per-frame 1: {:?}", starts(&h, 0.0, 8.0));
        // The UFO: 1 plays the arrival, 0 the departure.
        let mut h = crate::pd::trace::Headless::new(lib.clone(), audio.clone(), 1_489_853_723);
        h.start("mix_event_crg_ufo").unwrap();
        h.step(1.0 / 60.0);
        h.engine.set_global_param("crg_ufo", 1.0);
        h.step(1.0 / 60.0);
        assert_eq!(starts(&h, 0.0, 99.0), ["sfx_ufo_arrive"]);
        h.engine.set_global_param("crg_ufo", 1.0);
        h.step(1.0 / 60.0);
        assert_eq!(starts(&h, 0.0, 99.0).len(), 1, "a repeated 1 is no change");
        // The event's stop (0xD4FAD0) sets 0.0: the departure.
        h.engine.set_global_param("crg_ufo", 0.0);
        for _ in 0..90 {
            h.step(1.0 / 60.0);
        }
        assert_eq!(starts(&h, 0.0, 99.0), ["sfx_ufo_arrive", "sfx_ufo_depart"]);
        // The score: meteors and mission jingles.
        let mut h = crate::pd::trace::Headless::new(lib, audio, 1_489_853_723);
        h.start("music_creature_modified").unwrap();
        h.engine.set_global_param("mixmode_creaturegame", 1.0);
        h.engine.set_global_param("ui_social", 1.0);
        h.engine.set_global_param("ui_fight", 0.0);
        h.engine.set_global_param("timeofday", timeofday_param(7.0));
        let mut meteors_at = None;
        let mut t = 0.0;
        while t < 60.0 {
            let on = (5.0..40.0).contains(&t);
            if on && meteors_at.is_none() {
                meteors_at = Some(t);
                h.engine.set_global_param("crg_meteors", 1.0);
            }
            if !on && meteors_at.is_some() && t >= 40.0 && t < 40.1 {
                h.engine.set_global_param("crg_meteors", 0.0);
            }
            if (45.0..45.02).contains(&t) || (52.0..52.02).contains(&t) {
                h.engine.set_global_param("missioncomplete", 1.0);
            }
            h.step(1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        let quiet = starts(&h, 0.0, 5.0);
        let shower = starts(&h, 5.0, 40.0);
        let first = starts(&h, 45.0, 51.5);
        let second = starts(&h, 52.0, 58.0);
        eprintln!("meteors {} {:?}
mission 1 {first:?}
mission 2 {second:?}", shower.len(), &shower[..shower.len().min(12)]);
        assert!(quiet.is_empty(), "{quiet:?}");
        assert!(shower.iter().any(|s| s == "sfx_meteorshower_loop"), "the shower loop starts 4 s after the strike: {shower:?}");
        assert!(shower.iter().any(|s| s.starts_with("music_fight_")), "{shower:?}");
        assert!(stops(&h, 40.0) >= 1, "the strike ending stops the shower");
        assert!(!first.is_empty() && !second.is_empty(), "every completed mission plays its jingle: {first:?} {second:?}");
    }

    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_tribe_social_patch_drives_the_rounds() {
        let Some((lib, audio)) = installed() else { return };
        let mut h = crate::pd::trace::Headless::new(lib, audio, 1_489_853_723);
        h.start("music_tribe_social").unwrap();
        h.step(1.0 / 60.0);
        // The troupe stands in place: position, then the counts (the maracas
        // go to the misspelled id, as in retail).
        for (k, v) in [("tribe_social_x", 10.0), ("tribe_social_y", 0.0), ("tribe_social_z", 5.0)] {
            h.engine.set_global_param(k, v);
        }
        h.engine.set_global_param(SHAKER_START_RETAIL, 1.0);
        h.engine.set_global_param("didger_start", 1.0);
        h.engine.set_global_param("flute_start", 2.0);
        let mut t = 0.0;
        let mut sync = SocialSync { village: Some(0), period: crate::tribal::NOTE_WINDOW, ..default() };
        let mut note = 0;
        let mut seen = 0;
        let mut missed = 0;
        while t < 60.0 {
            h.step(1.0 / 60.0);
            t += 1.0 / 60.0;
            let new: Vec<(String, f32)> = h.events[seen..]
                .iter()
                .filter_map(|e| match &e.ev {
                    crate::pd::trace::Ev::SendGame { name, value } => Some((name.clone(), *value)),
                    _ => None,
                })
                .collect();
            seen = h.events.len();
            if let Some((_, v)) = new.iter().find(|(n, _)| n == "trg_social_length") {
                sync.rounds = *v as u32;
                sync.beat_driven = true;
            }
            let beat = new.iter().any(|(n, _)| n == "trg_social_beat");
            // Nobody answers: every beat misses its note.
            if sync.step(1.0 / 60.0, beat, note) == Some(0.0) {
                missed += 1;
                note += 1;
            }
        }
        assert_eq!(missed, 5, "each unanswered round is missed at its beat");
        let length = sends(&h, "trg_social_length");
        let round = sends(&h, "trg_round_length");
        let bubbles = sends(&h, "trg_social_thought_bubble");
        let beats = sends(&h, "trg_social_beat");
        eprintln!("length {length:?} round {round:?}\nbubbles {bubbles:?}\nbeats {beats:?}\nstarts {:?}", starts(&h, 0.0, 60.0).iter().take(12).collect::<Vec<_>>());
        assert_eq!(length.first().map(|s| s.1), Some(5.0));
        assert_eq!(round.first().map(|s| s.1), Some(4.0));
        // Five rounds of 4 s: the thought bubble one second before each beat
        // (the loop keeps pulsing afterwards; the game counts only the
        // `trg_social_length` first beats).
        assert!(beats.len() >= 5 && bubbles.len() >= 5, "{beats:?} {bubbles:?}");
        for w in beats[..5].windows(2) {
            assert!((w[1].0 - w[0].0 - 4.0).abs() < 0.1, "round length: {w:?}");
        }
        for (b, beat) in bubbles.iter().zip(&beats).take(5) {
            assert!((beat.0 - b.0 - 1.0).abs() < 0.1, "the bubble comes a second before its beat: {b:?} {beat:?}");
        }
        let played = starts(&h, 0.0, 60.0);
        assert!(played.iter().any(|s| s == "ssoc_drums_long"), "{played:?}");
        assert!(played.iter().any(|s| s.starts_with("ssoc_flute0") || s.starts_with("ssoc_didger0")), "{played:?}");
        assert!(!played.iter().any(|s| s.starts_with("ssoc_shaker0")), "retail never starts the shaker players: {played:?}");
        assert!(!sync.beat_driven, "the notes go back to OpenSpore after the last round");
    }

    #[test]
    fn the_end_of_a_song_sets_the_stop_inputs_in_the_retail_order() {
        // The stop routine 0xD9FE60: `trg_social_stop` 1, then `trg_social_stop_on_beat` 0.
        assert_eq!(song_end_inputs(false), vec![("trg_social_stop", 1.0), ("trg_social_stop_on_beat", 0.0)]);
        // The chieftain walks off with rounds left (0xDA34BE): `trg_social_stop_on_beat` 1 first.
        assert_eq!(
            song_end_inputs(true),
            vec![("trg_social_stop_on_beat", 1.0), ("trg_social_stop", 1.0), ("trg_social_stop_on_beat", 0.0)]
        );
        // The ids are the immediates of the image (0xD9FF12 / 0xD9FF5B / 0xDA34B9).
        assert_eq!(crate::pd::fnv("trg_social_stop"), 0x1C0E_A007);
        assert_eq!(crate::pd::fnv("trg_social_stop_on_beat"), 0x203E_9874);
        assert_eq!(crate::pd::fnv("missioncomplete"), 0xC1FA_4704);
    }

    /// The Tribal event inputs reach their patches (`tribal-sound2`): `missioncomplete`
    /// -> `tribe_drums3` (the member notes), `giftpresent` / `tribe_babyborn`, and the
    /// two stop inputs of `music_tribe_social`.
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_tribal_event_inputs_drive_their_patches() {
        let Some((lib, audio)) = installed() else { return };
        let drums = |lib: &std::sync::Arc<crate::pd::library::PdLibrary>, audio: &std::sync::Arc<crate::game_audio::AudioLibrary>| {
            let mut h = crate::pd::trace::Headless::new(lib.clone(), audio.clone(), 1_489_853_723);
            h.start("tribe_drums").unwrap();
            h.engine.set_global_param("mixmode_tribegame", 1.0);
            h.engine.set_global_param("planetzoom", 17.0);
            h.engine.set_global_param("tribecount", 4.0);
            h.engine.set_global_param("tribedistance", 20.0);
            h.engine.set_global_param("npctribecount", 6.0);
            h.engine.set_global_param("npctribedistance", 400.0);
            h
        };
        // Control: 30 s of drums, then 12 s more with no event; the same with one goal completed
        // (`missioncomplete` 1) at 30 s.
        let (mut quiet, mut goal) = (drums(&lib, &audio), drums(&lib, &audio));
        while quiet.now < 30.0 {
            quiet.step(1.0 / 60.0);
            goal.step(1.0 / 60.0);
        }
        goal.engine.set_global_param("missioncomplete", 1.0);
        while quiet.now < 42.0 {
            quiet.step(1.0 / 60.0);
            goal.step(1.0 / 60.0);
        }
        let (q, g) = (starts(&quiet, 30.0, 42.0), starts(&goal, 30.0, 42.0));
        eprintln!("goal completed at 30 s: {} starts {:?}\ncontrol: {} starts {:?}", g.len(), &g[..g.len().min(12)], q.len(), &q[..q.len().min(12)]);
        assert!(g.len() > q.len(), "the member notes of `addmember_notes` play on a completed goal: {} vs {}", g.len(), q.len());
        // `distance_to_travel_music` (the camera's distance to the nearest positional raid / gift
        // music: 0xBC2420 returns sqrt of the smallest squared distance, 0xEA7170 passes it on;
        // sqrt(FLT_MAX) with none) scales the drums' patch gain.
        let level = |value: Option<f32>| {
            let mut h = drums(&lib, &audio);
            if let Some(v) = value {
                h.engine.set_global_param("distance_to_travel_music", v);
            }
            while h.now < 30.0 {
                h.step(1.0 / 60.0);
            }
            let gains: Vec<f32> = h
                .events
                .iter()
                .filter_map(|e| match &e.ev {
                    crate::pd::trace::Ev::Param { name, value, .. } if name == "gain" => Some(*value),
                    _ => None,
                })
                .collect();
            let mean = gains.iter().sum::<f32>() / gains.len().max(1) as f32;
            (starts(&h, 0.0, 30.0).len(), gains.len(), mean)
        };
        // The values are camera distances (`tribal_audio::MusicSlots::nearest`): the drums fall
        // silent with a source within 100 units of the camera and are back at full level from 200;
        // `sqrt(FLT_MAX)` (no source) is the same as unset.
        let mut sweep: Vec<(f32, f32)> = vec![];
        for (label, v) in [("unset", None),
            ("sqrt(FLT_MAX) (no source)", Some(f32::MAX.sqrt())),
            ("0", Some(0.0)),
            ("25", Some(25.0)),
            ("50", Some(50.0)),
            ("75", Some(75.0)),
            ("100", Some(100.0)),
            ("125", Some(125.0)),
            ("150", Some(150.0)),
            ("175", Some(175.0)),
            ("200", Some(200.0)),
            ("400", Some(400.0)),
            ("4000", Some(4000.0)),
        ] {
            let (n, g, mean) = level(v);
            eprintln!("distance_to_travel_music {label}: {n} starts, {g} gain params, mean voice gain {mean:.3}");
            if let Some(d) = v.filter(|d| *d < 1.0e6) {
                sweep.push((d, mean));
            }
        }
        for w in sweep.windows(2) {
            assert!(w[1].1 >= w[0].1 - 1e-3, "the drums get louder with the distance to the raid music: {w:?}");
        }
        let at = |d: f32| sweep.iter().find(|(x, _)| *x == d).map(|(_, m)| *m).unwrap();
        assert!(at(0.0) < at(4000.0) && at(100.0) < at(200.0), "{sweep:?}");
        let (unset, far) = (level(None), level(Some(f32::MAX.sqrt())));
        assert_eq!(unset, far, "no source (FLT_MAX) leaves the drums as an unset input does");
        assert!(level(Some(0.0)).2 < unset.2, "a source on top of the camera ducks the drums");
        // A baby hatching and a gift: the sting and the gift sound.
        let mut h = drums(&lib, &audio);
        while h.now < 10.0 {
            h.step(1.0 / 60.0);
        }
        h.engine.set_global_param("tribe_babyborn", 1.0);
        h.engine.set_global_param("giftpresent", 1.0);
        while h.now < 14.0 {
            h.step(1.0 / 60.0);
        }
        let after = starts(&h, 10.0, 14.0);
        assert!(after.iter().any(|s| s == "music_sting_babyborn"), "{after:?}");
        assert!(after.iter().any(|s| s == "sfx_trg_gift_given"), "{after:?}");
        // The social game's end: a normal stop after the ending against a walk-off with rounds left.
        let song = |walk_off: bool, old_way: bool| {
            let mut h = crate::pd::trace::Headless::new(lib.clone(), audio.clone(), 1_489_853_723);
            h.start("music_tribe_social").unwrap();
            h.step(1.0 / 60.0);
            for (k, v) in [("tribe_social_x", 10.0), ("tribe_social_y", 0.0), ("tribe_social_z", 5.0)] {
                h.engine.set_global_param(k, v);
            }
            h.engine.set_global_param(SHAKER_START_RETAIL, 1.0);
            h.engine.set_global_param("didger_start", 1.0);
            h.engine.set_global_param("flute_start", 2.0);
            while h.now < 9.0 {
                h.step(1.0 / 60.0);
            }
            let at = h.now;
            if old_way {
                h.engine.set_global_param("trg_social_stop", 1.0);
            } else {
                for (k, v) in song_end_inputs(walk_off) {
                    h.engine.set_global_param(k, v);
                }
            }
            while h.now < at + 8.0 {
                h.step(1.0 / 60.0);
            }
            let stops = h.events.iter().filter(|e| e.t >= at && matches!(e.ev, crate::pd::trace::Ev::Stop { .. })).count();
            let fades: Vec<(String, f32)> = h
                .events
                .iter()
                .filter(|e| e.t >= at)
                .filter_map(|e| match &e.ev {
                    crate::pd::trace::Ev::Param { name, value, .. } if name == "fadeout" => Some((name.clone(), *value)),
                    _ => None,
                })
                .collect();
            let started = starts(&h, at, at + 8.0);
            let trace: Vec<String> = h.events.iter().filter(|e| e.t >= at).map(|e| format!("{:.3} {:?}", e.t - at, e.ev)).collect();
            (stops, fades, started, trace)
        };
        for (label, walk_off, old) in [("old (stop only)", false, true), ("normal stop", false, false), ("walk-off", true, false)] {
            let (stops, fades, started, trace) = song(walk_off, old);
            eprintln!("song end {label}: {stops} voice stops, fadeouts {fades:?}, new starts {started:?}, {} events", trace.len());
        }
        let (normal, _, normal_starts, normal_trace) = song(false, false);
        let (_, _, old_starts, old_trace) = song(false, true);
        let (_, _, _, walk_trace) = song(true, false);
        assert!(normal >= 1, "the stop ends the backing players");
        assert_eq!(normal_starts, old_starts, "the extra `stop_on_beat` 0 changes nothing for a normal end");
        assert_eq!(normal_trace, old_trace, "`stop_on_beat` 0 before any 1 is not observable in the patch's output");
        let diff = normal_trace.iter().zip(&walk_trace).position(|(a, b)| a != b);
        eprintln!("walk-off against normal stop: first differing event {diff:?} (lengths {} / {})", walk_trace.len(), normal_trace.len());
    }

    /// The effects whose `sound` blocks play the black hole loops and the UFO
    /// engines (`notes\audio-sweep-notes.md` S2).
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_effects_with_space_sounds() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let fnv = crate::pd::fnv;
        let want: Vec<(u32, &str)> = ["space_blackhole_lp", "space_blackholeb_lp", "space_blackhole_eventhorizon_lp", "ufo_travelling", "ufo_npc"]
            .iter()
            .map(|n| (fnv(n), *n))
            .collect();
        let mut found = 0;
        for key in index.records.keys().filter(|k| k.type_id == AMBIENCE_EFFDIR.type_id) {
            let Ok(dir) = index.read(*key).map_err(|e| e.to_string()).and_then(|d| effect_sounds(&d)) else { continue };
            for (effect, sounds) in &dir {
                for (id, name) in &want {
                    if sounds.contains(id) {
                        found += 1;
                        eprintln!("{:08X}!{:08X}: effect {effect:08X} plays {name} (sounds {sounds:08X?})", key.group_id, key.instance_id);
                    }
                }
            }
        }
        assert!(found > 0);
    }

    #[test]
    fn editor_events_follow_the_retail_senders() {
        let mut ev = EditorEvents::default();
        let base = EditorSnapshot { mode: 0, category: 0, parts: 3, ..default() };
        assert_eq!(ev.step(base.clone()), vec![("ModeSet", 0.0)], "the editor opens");
        // Two parts bought: the counter rises per part, then the size / count.
        let more = EditorSnapshot { parts: 5, ..base.clone() };
        let out = ev.step(more.clone());
        assert_eq!(&out[..2], &[("RigBlockAdded", 1.0), ("RigBlockAdded", 2.0)]);
        assert!(out.contains(&("BlockCountChange", 5.0)));
        // One sold: the counter is sent unchanged.
        let fewer = EditorSnapshot { parts: 4, ..more.clone() };
        assert_eq!(ev.step(fewer.clone())[0], ("RigBlockRemoved", 2.0));
        // Palette tab, then paint mode, a skin style and a colour.
        let tab = EditorSnapshot { category: 2, ..fewer.clone() };
        assert_eq!(ev.step(tab.clone()), vec![("PaletteSet", 2.0)]);
        let paint = EditorSnapshot { mode: 1, skin_colors: vec![[0.0; 3]], ..tab.clone() };
        assert_eq!(ev.step(paint.clone()), vec![("ModeSet", 1.0), ("ColorPicked", 0.0)]);
        let styled = EditorSnapshot { skin_theme: 7, ..paint.clone() };
        assert_eq!(ev.step(styled.clone()), vec![("SkinPaintTheme", 3.0)]);
        let colour = EditorSnapshot { skin_colors: vec![[1.0, 0.5, 0.25]], ..styled.clone() };
        assert_eq!(ev.step(colour.clone()), vec![("ColorPicked", 1.0 + 1.0 + 1.0)]);
        // Rigid blocks: a theme, a swatch, a brushed block.
        let theme = EditorSnapshot { block_theme: Some(9), block_paint: 1, ..colour.clone() };
        assert_eq!(ev.step(theme.clone()), vec![("BlockPaintTheme", 4.0)], "the theme repaints without BlockPaintApply");
        let swatch = EditorSnapshot { swatch: Some(4), ..theme.clone() };
        assert_eq!(ev.step(swatch.clone()), vec![("BlockSwatchSelect", 5.0)]);
        let brushed = EditorSnapshot { block_paint: 2, ..swatch.clone() };
        assert_eq!(ev.step(brushed.clone()), vec![("BlockPaintApply", 5.0)]);
        // Test Drive.
        assert_eq!(ev.step(EditorSnapshot { mode: 2, ..brushed }), vec![("ModeSet", 2.0)]);
    }

    #[test]
    fn ufo_drive_type_by_the_best_drive() {
        assert_eq!(ufodrivetype::<&str>(&[]), 0.0);
        assert_eq!(ufodrivetype(&["interstellardrive"]), 1.0);
        assert_eq!(ufodrivetype(&["interstellardrive", "interstellardrive_upgrade1"]), 2.0);
        assert_eq!(ufodrivetype(&["interstellardrive_upgrade4"]), 3.0);
    }

    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_planet_ambience_comes_from_the_theme_effects() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let dir = effect_sounds(&index.read(AMBIENCE_EFFDIR).unwrap()).unwrap();
        let fnv = crate::pd::fnv;
        for (effect, sound) in [
            ("planet_ambience_default", "ambience_no_density"),
            ("planet_ambience_rivers", "ambience_rivers"),
            ("planet_ambience_marsh", "ambience_marsh"),
            ("planet_ambience_crystal", "ambience_crystal"),
            ("planet_ambience_greencrystal", "ambience_greencrystal"),
        ] {
            assert_eq!(dir.get(&fnv(effect)), Some(&vec![fnv(sound)]), "{effect}");
        }
        assert!(dir.get(&fnv("tribe_music")).is_some_and(|s| s.contains(&fnv("planetambiloop"))));
        // Without a stage planet: AllPlanets' default.
        assert_eq!(planet_ambience_sounds(&index, None), vec![fnv("ambience_no_density")]);
        // Every installed terrain script resolves to one of the five.
        let five: Vec<u32> = ["ambience_no_density", "ambience_rivers", "ambience_marsh", "ambience_crystal", "ambience_greencrystal"]
            .iter()
            .map(|n| fnv(n))
            .collect();
        let mut seen = std::collections::BTreeMap::new();
        for script in crate::planet_data::planet_scripts(&index) {
            let s = planet_ambience_sounds(&index, Some(script.terrain_script));
            assert!(s.len() == 1 && five.contains(&s[0]), "{:08X}: {s:08X?}", script.terrain_script);
            *seen.entry(s[0]).or_insert(0) += 1;
        }
        eprintln!("planet ambience per planet script: {seen:08X?}");
    }

    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn installed_tribe_drums_follow_the_tribes() {
        let Some((lib, audio)) = installed() else { return };
        let mut h = crate::pd::trace::Headless::new(lib, audio, 1_489_853_723);
        h.start("tribe_drums").unwrap();
        h.engine.set_global_param("mixmode_tribegame", 1.0);
        h.engine.set_global_param("planetzoom", 17.0);
        h.engine.set_global_param("tribecount", 4.0);
        h.engine.set_global_param("tribedistance", 20.0);
        h.engine.set_global_param("npctribecount", 6.0);
        h.engine.set_global_param("npctribedistance", 400.0);
        for _ in 0..(40 * 60) {
            h.step(1.0 / 60.0);
        }
        h.engine.set_global_param("npctribedistance", 30.0);
        for _ in 0..(40 * 60) {
            h.step(1.0 / 60.0);
        }
        let home = starts(&h, 0.0, 40.0);
        let near = starts(&h, 40.0, 80.0);
        eprintln!("home {} {:?}\nnear npc {} {:?}", home.len(), &home[..home.len().min(10)], near.len(), &near[..near.len().min(10)]);
        assert!(!home.is_empty(), "the tribe's drums play at home");
    }
}
