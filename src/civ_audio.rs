//! Civilization stage audio (notes\civ-re-notes.md V7), added to the stage's
//! observed cues in `game_audio::observe_civ`.
//!
//! VERIFIED (call sites in the Civ code range and installed data):
//! * `ui_civ_error` on invalid commands (0xCE8CDC, 0xCF2A12, 0xCF3AC1,
//!   0xCF3FB9), `ui_civ_target_military` when a military target is chosen
//!   (0xCF35A4), `ui_civ_alarm_lp` (loop) and `sfx_civ_underattack_alarm_close`
//!   while a city is attacked (0xC00E0B / 0xD5B5FF), `ui_mission_complete`.
//! * The conversion music gates `music_civ_recruita/b/c` (positive) and
//!   `_recruit_bada/b/c` (negative, NOT installed: silent in retail too),
//!   `music_civ_relig_concert`, `music_civ_lose` (+ `Sfx_civ_lose_game`).
//!
//! VERIFIED (civ-audit-b): the target sounds are one routine, the Civ UI's order
//! message handler: message 0x37E95B9 plays `ui_civ_target_military` (0xCF35A4), 0x37E95BA
//! `ui_civ_target_cult` (0xCF3793) and 0x37E95BC `ui_civ_target_diplo_trade` (0xCF38DE) with
//! `PlayAudio(id, 0x37FD5B2)` once the order list holds at least one vehicle of that purpose
//! (0 military, 1 religious, 2 economic) that accepted it; the OpenSpore mapping by vehicle
//! class is the same sound at the same moment.
//!
//! ADAPTED (not traced): the conversion thresholds of the three gates (0, 0.35, 0.7 of the progress),
//! the hold times, the message patterns that stand for "an invalid command"
//! (the rules code reports refusals as text), `ui_hut_rotate` on the assembly
//! editor's rotate key (the retail call is in the city planner, 0xD0C542).
//!
//! Loops (alarm, gates) run their own voices through `GameAudio::clip`; the
//! one-shots are `PlayCue` messages.

use std::collections::HashMap;

use bevy::{
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume},
    prelude::*,
};

use crate::{
    civ::{self, UnitOrder, HOME_INDEX},
    civ_controls::AggressiveChase,
    civ_rts::{CivUnit, RtsState},
    game::RunProgress,
    game_audio::{Bus, Cue, GameAudio, PlayCue},
    stages::{GameStage, StageOwned},
};

/// The city alarm keeps sounding this long after the last damage.
pub const ALARM_HOLD_SECS: f64 = 5.0;
/// Conversion progress at which the second and third music gate open.
pub const GATE_THRESHOLDS: [f32; 3] = [0.0, 0.35, 0.7];
/// The gates fade out this long after the last preaching (`TimeInConverted`).
pub const GATE_HOLD_SECS: f64 = 4.0;
const FADE_SECS: f32 = 1.5;
const GATE_TRIM: f32 = 0.8;

/// Pure: does a rules message report a refused command? The Civ rules write
/// every refusal as a line of text, so the observers recognise them by their
/// wording (ADAPTED: the retail code calls `ui_civ_error` at each refusal).
pub fn is_error_message(message: &str) -> bool {
    const MARKS: [&str; 17] = [
        "Select vehicles first",
        "Select a vehicle",
        "No free spice geyser",
        "No rival city",
        "No rival vehicles",
        "You have no",
        "support no more vehicles",
        "is not yours",
        "Aircraft need",
        "Sea vehicles need",
        "every plot is built",
        "Move the camera over one of your cities",
        "fully upgraded",
        "No Creature consequence",
        "No Tribal consequence",
        "No Cell consequence",
        "cannot ",
    ];
    if MARKS.iter().any(|m| message.contains(m)) {
        return true;
    }
    // "A land military vehicle costs 60 spice", "Engine 2 needs 90 spice".
    (message.contains(" costs ") || message.contains(" needs ")) && message.contains("spice")
}

/// Pure: which of the three gates (`music_civ_recruita/b/c`) are open for a
/// conversion progress (None = nobody is preaching).
pub fn gates_open(progress: Option<f32>) -> [bool; 3] {
    match progress {
        None => [false; 3],
        Some(p) => GATE_THRESHOLDS.map(|t| p >= t),
    }
}

/// Pure: the alarm is on for `ALARM_HOLD_SECS` after the last attack.
pub fn alarm_active(now: f64, last_attack: Option<f64>) -> bool {
    last_attack.is_some_and(|at| now - at < ALARM_HOLD_SECS)
}

/// Pure: the target sound of a vehicle order by vehicle class (VERIFIED for all three, see
/// the module notes).
pub fn target_cue(kind: civ::Approach) -> Cue {
    match kind {
        civ::Approach::Military => Cue::CivTargetMilitary,
        civ::Approach::Religious => Cue::CivTargetCult,
        civ::Approach::Economic => Cue::CivTargetTrade,
    }
}

/// What the observers remember between frames.
#[derive(Default)]
pub struct Memory {
    message: String,
    orders: HashMap<u32, UnitOrder>,
    city_hp: Option<f32>,
    owned: Option<usize>,
    conversions: Vec<f32>,
    yaws: Vec<f32>,
    last_attack: Option<f64>,
    last_preach: Option<f64>,
}

/// Commands whose refusal should be heard even when the text repeats.
fn command_pressed(keys: &ButtonInput<KeyCode>, mouse: Option<&ButtonInput<MouseButton>>) -> bool {
    [
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyJ,
        KeyCode::KeyX,
        KeyCode::KeyF,
        KeyCode::KeyQ,
        KeyCode::KeyB,
        KeyCode::KeyT,
        KeyCode::KeyE,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
        KeyCode::KeyU,
        KeyCode::KeyI,
        KeyCode::KeyO,
        KeyCode::KeyP,
    ]
    .iter()
    .any(|k| keys.just_pressed(*k))
        || mouse.is_some_and(|m| m.just_released(MouseButton::Right))
}

/// One-shot observers: refused commands, order sounds, the alarm start,
/// captures and losses, the editor's rotate.
#[allow(clippy::too_many_arguments)]
fn observe(
    run: Res<RunProgress>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    time: Res<Time<Real>>,
    units: Query<(&CivUnit, Option<&AggressiveChase>)>,
    mut memory: Local<Memory>,
    mut out: MessageWriter<PlayCue>,
) {
    let now = time.elapsed_secs_f64();
    let campaign = &run.civ_campaign;
    let memory = &mut *memory;

    // Refused commands.
    let changed = memory.message != campaign.message;
    if (changed || command_pressed(&keys, mouse.as_deref()))
        && is_error_message(&campaign.message)
    {
        out.write(PlayCue(Cue::CivError));
    }
    if changed {
        memory.message = campaign.message.clone();
    }

    // Orders: the target sound of the vehicle class on a new attack, preach or
    // trade order (a stance chase is not an order of the player).
    let mut heard = false;
    let mut seen: HashMap<u32, UnitOrder> = HashMap::with_capacity(memory.orders.len());
    for (unit, chase) in &units {
        if unit.nation != HOME_INDEX {
            continue;
        }
        seen.insert(unit.id, unit.order);
        if heard || chase.is_some() {
            continue;
        }
        let before = memory.orders.get(&unit.id).copied();
        if before.is_some_and(|b| b != unit.order) {
            let hostile = match unit.order {
                UnitOrder::Unit(_) | UnitOrder::Epic(_) => true,
                UnitOrder::City(c) => campaign.cities.get(c).is_some_and(|city| !city.owned()),
                _ => false,
            };
            if hostile {
                out.write(PlayCue(target_cue(unit.kind)));
                heard = true;
            }
        }
    }
    memory.orders = seen;

    // A city of yours is attacked: the sting, and the alarm loop starts.
    let city_hp: f32 = campaign
        .cities
        .iter()
        .filter(|c| c.owned())
        .map(|c| c.hp)
        .sum();
    if memory.city_hp.is_some_and(|before| city_hp < before - 0.01) {
        if !alarm_active(now, memory.last_attack) {
            out.write(PlayCue(Cue::CivUnderAttack));
        }
        memory.last_attack = Some(now);
    }
    memory.city_hp = Some(city_hp);

    // Captures and losses.
    let owned = campaign.owned_count();
    if let Some(before) = memory.owned {
        if owned > before {
            out.write(PlayCue(Cue::CivMissionComplete));
            // A city taken by conversion (full progress) gets the concert.
            let converted = campaign.cities.iter().enumerate().any(|(i, c)| {
                c.owned()
                    && memory
                        .conversions
                        .get(i)
                        .is_some_and(|conversion| *conversion >= 0.95)
            });
            if converted {
                out.write(PlayCue(Cue::CivReligConcert));
            }
        } else if owned < before {
            out.write(PlayCue(Cue::CivLose));
            if owned == 0 {
                out.write(PlayCue(Cue::CivLoseSting));
            }
        }
    }
    memory.owned = Some(owned);
    memory.conversions = campaign.cities.iter().map(|c| c.conversion).collect();

    // The assembly editor's rotate key.
    let yaws: Vec<f32> = run
        .vehicle
        .parts
        .iter()
        .chain(run.building.parts.iter())
        .map(|p| p.yaw)
        .collect();
    if yaws.len() == memory.yaws.len() && yaws != memory.yaws && !memory.yaws.is_empty() {
        out.write(PlayCue(Cue::CivHutRotate));
    }
    memory.yaws = yaws;
}

/// A looping voice of this module (alarm or conversion gate).
#[derive(Component)]
pub struct CivLoopVoice {
    pub cue: Cue,
    level: f32,
    target: f32,
}

/// The looped voices: the alarm while a city is attacked, the three
/// conversion music gates while vehicles preach.
#[allow(clippy::too_many_arguments)]
fn loops(
    mut commands: Commands,
    mut audio: ResMut<GameAudio>,
    run: Res<RunProgress>,
    rts: Option<Res<RtsState>>,
    time: Res<Time<Real>>,
    mut memory: Local<Memory>,
    mut voices: Query<(Entity, &mut CivLoopVoice, Option<&mut AudioSink>)>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();

    // Alarm: any owned city losing armour keeps it on.
    let city_hp: f32 = run
        .civ_campaign
        .cities
        .iter()
        .filter(|c| c.owned())
        .map(|c| c.hp)
        .sum();
    if memory.city_hp.is_some_and(|before| city_hp < before - 0.01) {
        memory.last_attack = Some(now);
    }
    memory.city_hp = Some(city_hp);
    let alarm = alarm_active(now, memory.last_attack);

    // Gates: the most advanced conversion among the cities being preached to.
    let progress = rts.as_deref().and_then(|rts| {
        rts.preaching
            .iter()
            .filter_map(|c| run.civ_campaign.cities.get(*c))
            .map(|c| c.conversion)
            .reduce(f32::max)
    });
    if progress.is_some() {
        memory.last_preach = Some(now);
    }
    let holding = memory
        .last_preach
        .is_some_and(|at| now - at < GATE_HOLD_SECS);
    // With the Pd engine the retail patch `music_civ_recruit` plays instead
    // (`civ_music::feed_recruit_music`, 0xE89F60).
    let open = if holding && !crate::pd_audio::civ_enabled() {
        gates_open(Some(progress.unwrap_or(1.0).max(0.0)))
    } else {
        [false; 3]
    };
    let wanted: [(Cue, bool); 4] = [
        (Cue::CivAlarmLoop, alarm),
        (Cue::CivRecruitA, open[0]),
        (Cue::CivRecruitB, open[1]),
        (Cue::CivRecruitC, open[2]),
    ];

    let mute = if audio.muted { 0.0 } else { 1.0 };
    for (cue, on) in wanted {
        let existing = voices.iter_mut().find(|(_, v, _)| v.cue == cue);
        match existing {
            Some((_, mut voice, _)) => voice.target = if on { 1.0 } else { 0.0 },
            None if on => {
                audio.request(cue);
                if let Some((handle, _)) = audio.clip(cue) {
                    commands.spawn((
                        Name::new(format!("civ loop {}", cue.spec().key)),
                        AudioPlayer::new(handle),
                        PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
                        CivLoopVoice {
                            cue,
                            level: 0.0,
                            target: 1.0,
                        },
                        StageOwned,
                    ));
                }
            }
            None => {}
        }
    }
    for (entity, mut voice, sink) in &mut voices {
        let step = dt / FADE_SECS;
        voice.level = if voice.level < voice.target {
            (voice.level + step).min(voice.target)
        } else {
            (voice.level - step).max(voice.target)
        };
        let gain = match audio.clip(voice.cue) {
            Some((_, gain)) => {
                let spec = voice.cue.spec();
                let trim = if matches!(spec.bus, Bus::Music) {
                    GATE_TRIM
                } else {
                    1.0
                };
                spec.bus.volume() * gain * spec.trim * trim
            }
            None => 0.0,
        };
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(gain * voice.level * mute));
        }
        if voice.target == 0.0 && voice.level <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

pub struct CivAudioPlugin;

impl Plugin for CivAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (observe, loops)
                .run_if(in_state(GameStage::Civilization))
                .after(crate::GameSystems::Simulation),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_commands_are_recognised_by_their_text() {
        for refused in [
            "Select vehicles first (click, drag a box, or Q)",
            "No free spice geyser is left to claim",
            "No rival city is left",
            "You have no vehicles: build some with G",
            "Your cities support no more vehicles (4/4): build houses or take cities",
            "C2 is not yours",
            "Aircraft need 3 cities (you hold 1)",
            "A military land vehicle costs 60 spice",
            "Engine 2 needs 90 spice (have 10)",
            "C1: every plot is built",
            "Move the camera over one of your cities (C1 is home) to build there",
        ] {
            assert!(is_error_message(refused), "{refused}");
        }
        for fine in [
            "12 vehicles selected",
            "Fleet sent against C2: 3 military, 0 religious, 0 economic",
            "Vehicle 2 of 5",
            "Group 1 defined: 3 vehicles",
            "Aggressive stance",
            "",
        ] {
            assert!(!is_error_message(fine), "{fine}");
        }
    }

    #[test]
    fn conversion_gates_open_with_progress() {
        assert_eq!(gates_open(None), [false; 3]);
        assert_eq!(gates_open(Some(0.0)), [true, false, false]);
        assert_eq!(gates_open(Some(0.4)), [true, true, false]);
        assert_eq!(gates_open(Some(0.9)), [true, true, true]);
    }

    #[test]
    fn alarm_lasts_five_seconds_after_the_last_hit() {
        assert!(!alarm_active(10.0, None));
        assert!(alarm_active(10.0, Some(8.0)));
        assert!(!alarm_active(14.0, Some(8.0)));
    }

    #[test]
    fn each_vehicle_class_has_its_target_sound() {
        assert_eq!(target_cue(civ::Approach::Military), Cue::CivTargetMilitary);
        assert_eq!(target_cue(civ::Approach::Religious), Cue::CivTargetCult);
        assert_eq!(target_cue(civ::Approach::Economic), Cue::CivTargetTrade);
        assert_eq!(Cue::CivTargetMilitary.spec().names, &["ui_civ_target_military"]);
        assert_eq!(Cue::CivAlarmLoop.spec().names, &["ui_civ_alarm_lp"]);
        assert_eq!(Cue::CivRecruitA.spec().names, &["music_civ_recruita"]);
        assert_eq!(Cue::CivError.spec().names, &["ui_civ_error"]);
    }

    /// The observers in a small app: a refused command, a city taking damage,
    /// a capture and a loss.
    #[test]
    fn observers_play_the_cues() {
        let mut app = App::new();
        app.add_message::<PlayCue>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time<Real>>()
            .insert_resource(RunProgress::default())
            .add_systems(Update, observe);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .civ_campaign
            .ensure_cities();
        let drain = |app: &mut App| -> Vec<Cue> {
            app.world_mut()
                .resource_mut::<Messages<PlayCue>>()
                .drain()
                .map(|PlayCue(c)| c)
                .collect()
        };
        app.update();
        assert!(drain(&mut app).is_empty());

        // A refused command.
        app.world_mut()
            .resource_mut::<RunProgress>()
            .civ_campaign
            .message = "No free spice geyser is left to claim".into();
        app.update();
        assert_eq!(drain(&mut app), vec![Cue::CivError]);

        // Damage to a city of yours: the alarm sting.
        app.world_mut()
            .resource_mut::<RunProgress>()
            .civ_campaign
            .message = "fine".into();
        app.update();
        drain(&mut app);
        app.world_mut()
            .resource_mut::<RunProgress>()
            .civ_campaign
            .cities[HOME_INDEX]
            .damage_building(0, 10.0, 1);
        app.update();
        assert!(drain(&mut app).contains(&Cue::CivUnderAttack));
    }
}
