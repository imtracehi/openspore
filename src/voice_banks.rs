//! Creature / citizen voice banks (helper `voices-wire`; `notes\audio-codec.md` section 1, the
//! codec 0 voice banks `vmot_` / `vidle_` / `vsp_` / `vcode_`).
//!
//! RETAIL (installed data): a clip event whose name starts with `v` (`vmot_panic`, `vidle_stamp_foot`,
//! `vcode_crowd_awe_lp`, `vlib_idle_shrt` ...) is *not* the name of a sound. The audio system
//! (`anim sound player 0x9A4FA0`, which hands the event name and the creature's voice parameters to the
//! `AudioSystem` as a track; the parameters come from the creature's anim block) builds the sound
//! name from the event name and two suffix families of soundProps:
//! * `<event><Mouth>XCr`: creatures built with a creature-editor mouth, one bank per mouth family
//!   (`Ao Bc Bh Fc Jc Jh Mc Po Ro Sh`, the first two letters of the part's `mouthtype`;
//!   `vlib_idle_shrtRoXCr`, `vsoc_angry_to_curiousBcXCr` ...);
//! * `<event><Class>AIn`: the tribal / civ / space citizens' banks, one per voice class `Mam`
//!   (mammal), `Bir` (bird) and `Ins` / `In` (insect): `vmot_panicMamAIn`, `vidle_stamp_footBirAIn`,
//!   `vcode_crowd_awe_lpInsAIn`, `vhunt_ax_hit_GInsAIn`, ... Most citizen events (`vmot_*`, `vidle_*`,
//!   `vsp_*`, `vcode_*`, `vhunt_*`, `vcom_*`, `vchief_*`) exist ONLY in this family: the plain event
//!   name has neither a soundProp nor an SNR, so a lookup of the clip's own name found nothing and
//!   those voices were silent even after `audio-codec` made the codec 0 samples decode.
//!
//! ADAPTED (what the retail resolver reads to pick the class is not decoded; the voice parameters
//! `[anim block + 0x2d8..0x2dc]` of 0x9A4FA0 are filled by creature code that was not traced):
//! the class follows the creature's mouth family, the part that also picks the `XCr` bank: beak
//! mouths (`Bc`, `Bh`) are the bird voices, mandible and radial mouths (`Mc`, `Ro`) the insect voices,
//! every other mouth the mammal voices. The `XCr` bank is tried first (as before), the class bank
//! next, the other classes last (a few events have only some classes), then the plain name.

use std::sync::Arc;

use bevy::prelude::*;

use crate::{
    cell_audio::{NamedSounds, Play},
    game::RunProgress,
    game_audio::{AudioLibrary, Bus, GameAudio, TYPE_SNR, TYPE_SOUND_PROP},
    stages::GameStage,
    tribal_controls::{CitizenAction, TribalControls},
    tribal_rts::Selected,
};

/// The voice class of a citizen species (the `AIn` banks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceClass {
    Mammal,
    Bird,
    Insect,
}

impl VoiceClass {
    pub const ALL: [VoiceClass; 3] = [VoiceClass::Mammal, VoiceClass::Bird, VoiceClass::Insect];

    /// The soundProp suffixes of the class (the insect bank is spelled `InsAIn` and `InAIn`).
    pub fn suffixes(self) -> &'static [&'static str] {
        match self {
            VoiceClass::Mammal => &["MamAIn"],
            VoiceClass::Bird => &["BirAIn"],
            VoiceClass::Insect => &["InsAIn", "InAIn"],
        }
    }

    /// ADAPTED: the class of a creature from the two-letter family of its mouth.
    pub fn of_mouth(mouth: &str) -> VoiceClass {
        match mouth {
            "Bc" | "Bh" => VoiceClass::Bird,
            "Mc" | "Ro" => VoiceClass::Insect,
            _ => VoiceClass::Mammal,
        }
    }
}

/// The sound names a voice event can resolve to, best first: the creature's mouth bank, the
/// bank of its class, the banks of the other classes, the plain name.
pub fn candidates(event: &str, mouth: &str) -> Vec<String> {
    let class = VoiceClass::of_mouth(mouth);
    let mut out = vec![format!("{event}{mouth}XCr")];
    let mut order = vec![class];
    order.extend(VoiceClass::ALL.into_iter().filter(|c| *c != class));
    for c in order {
        for suffix in c.suffixes() {
            out.push(format!("{event}{suffix}"));
        }
    }
    out.push(event.to_string());
    out
}

/// Whether the library has a playable record named `name` (a soundProp or an SNR).
fn installed(library: &AudioLibrary, name: &str) -> bool {
    let id = crate::world_objects::name_id(name);
    library.contains(TYPE_SOUND_PROP, id) || library.contains(TYPE_SNR, id)
}

/// The first installed candidate for a voice event. `intern` makes the name live for the program
/// (`Play::name` is `&'static`).
pub fn pick(
    library: &Option<Arc<AudioLibrary>>,
    event: &str,
    mouth: &str,
    intern: impl Fn(String) -> &'static str,
) -> Option<&'static str> {
    let lib = library.as_ref()?;
    candidates(event, mouth).into_iter().find(|n| installed(lib, n)).map(intern)
}

// ---------------------------------------------------------------------------
// Tribe command voices
// ---------------------------------------------------------------------------

/// The code-played voice events of the tribe members (`vcode_*`: not clip events; the plain
/// names have only the `AIn` banks). The retail routines that play them were not traced; the
/// names say what they are, the triggers are ADAPTED: a member answers a selection and a command.
pub const SELECT: &str = "vcode_select";
pub const GO_MOVE: &str = "vcode_go_mot";
pub const GO_EAT: &str = "vcode_go_eat";
pub const GO_BATTLE: &str = "vcode_go_battle";

/// ADAPTED: which answer a command gets (by what the order is).
pub fn command_event(action: CitizenAction) -> Option<&'static str> {
    match action {
        CitizenAction::WalkTo | CitizenAction::ReturnToTribe => Some(GO_MOVE),
        CitizenAction::Eat
        | CitizenAction::Fish
        | CitizenAction::Gather
        | CitizenAction::GatherMeat
        | CitizenAction::FeedWild
        | CitizenAction::Bundle => Some(GO_EAT),
        CitizenAction::Attack | CitizenAction::RaidFood | CitizenAction::RaidHuts | CitizenAction::Hunt => {
            Some(GO_BATTLE)
        }
        _ => None,
    }
}

/// Seconds between two answers of the tribe (ADAPTED).
pub const ANSWER_EVERY: f64 = 1.2;

/// The tribe answers a selection and the commands the player gives (`TribalControls::issued`) in
/// the voice class of the player's species, on the Voices bus.
#[allow(clippy::too_many_arguments)]
fn tribe_voices(
    time: Res<Time<Real>>,
    originals: Option<Res<crate::Originals>>,
    run: Res<RunProgress>,
    controls: Res<TribalControls>,
    audio: Res<GameAudio>,
    mut sounds: ResMut<NamedSounds>,
    selected: Query<(), Added<Selected>>,
    mut last: Local<f64>,
    mut mouth: Local<Option<(usize, &'static str)>>,
) {
    let mut events: Vec<&'static str> = Vec::new();
    if !selected.is_empty() {
        events.push(SELECT);
    }
    events.extend(controls.issued.iter().filter_map(|i| command_event(i.action)));
    let Some(event) = events.last().copied() else { return };
    let now = time.elapsed_secs_f64();
    if now - *last < ANSWER_EVERY || !sounds.is_running() {
        return;
    }
    let Some(originals) = originals else { return };
    if mouth.is_none_or(|(n, _)| n != run.parts.len()) {
        *mouth = Some((run.parts.len(), crate::anim_audio::player_mouth(&originals, &run)));
    }
    let mouth = mouth.map_or(crate::anim_audio::DEFAULT_MOUTH, |(_, m)| m);
    let library = audio.library();
    let Some(name) = pick(&library, event, mouth, crate::anim_audio::intern) else { return };
    *last = now;
    let mut play = Play::named(name, "tribe voice");
    play.bus = Bus::Voice;
    sounds.play(play, now, 0.0);
}

pub struct VoiceBanksPlugin;

impl Plugin for VoiceBanksPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            tribe_voices
                .run_if(in_state(GameStage::Tribal))
                .run_if(resource_exists::<RunProgress>)
                .run_if(resource_exists::<TribalControls>)
                .run_if(resource_exists::<NamedSounds>)
                .run_if(resource_exists::<GameAudio>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tribe_commands_get_their_answers() {
        assert_eq!(command_event(CitizenAction::WalkTo), Some("vcode_go_mot"));
        assert_eq!(command_event(CitizenAction::Gather), Some("vcode_go_eat"));
        assert_eq!(command_event(CitizenAction::Attack), Some("vcode_go_battle"));
        assert_eq!(command_event(CitizenAction::Sleep), None);
        assert_eq!(command_event(CitizenAction::Party), None);
    }

    #[test]
    fn classes_follow_the_mouth_family() {
        assert_eq!(VoiceClass::of_mouth("Bc"), VoiceClass::Bird);
        assert_eq!(VoiceClass::of_mouth("Bh"), VoiceClass::Bird);
        assert_eq!(VoiceClass::of_mouth("Mc"), VoiceClass::Insect);
        assert_eq!(VoiceClass::of_mouth("Ro"), VoiceClass::Insect);
        for m in ["Ao", "Fc", "Jc", "Jh", "Po", "Sh", "??"] {
            assert_eq!(VoiceClass::of_mouth(m), VoiceClass::Mammal, "{m}");
        }
    }

    #[test]
    fn candidates_try_the_mouth_bank_then_the_class_then_the_rest() {
        let c = candidates("vmot_panic", "Bc");
        assert_eq!(
            c,
            [
                "vmot_panicBcXCr",
                "vmot_panicBirAIn",
                "vmot_panicMamAIn",
                "vmot_panicInsAIn",
                "vmot_panicInAIn",
                "vmot_panic"
            ]
        );
        let c = candidates("vidle_stamp_foot", "Jh");
        assert_eq!(c[0], "vidle_stamp_footJhXCr");
        assert_eq!(c[1], "vidle_stamp_footMamAIn");
        assert_eq!(c.last().map(String::as_str), Some("vidle_stamp_foot"));
        // The insect bank has both spellings, right after the class's own.
        let c = candidates("vsp_drop", "Mc");
        assert_eq!(&c[1..3], ["vsp_dropInsAIn", "vsp_dropInAIn"]);
    }

    /// Dump the soundProps of some voice names (`VW_NAMES=a,b,c`).
    #[test]
    #[ignore = "needs the installed Spore audio packages"]
    fn dump_voice_sound_props() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let names = std::env::var("VW_NAMES").unwrap_or_default();
        for name in names.split(',').filter(|n| !n.is_empty()) {
            let id = crate::world_objects::name_id(name);
            let has_prop = library.contains(TYPE_SOUND_PROP, id);
            let has_snr = library.contains(TYPE_SNR, id);
            println!("NAME|{name}|{id:08X}|prop={has_prop}|snr={has_snr}");
            match library.resolve(name) {
                Ok(d) => println!(
                    "  DEF|samples={}|looped={}|3d={}|{}..{}|gain={}|poly={}|curve={}|silent={}",
                    d.samples.len(),
                    d.looped,
                    d.is_3d,
                    d.min_distance,
                    d.max_distance,
                    d.gain,
                    d.polyphony,
                    d.curve.len(),
                    d.silent
                ),
                Err(e) => println!("  DEF|{e}"),
            }
        }
    }

    /// Which voice events installed clips carry, and which variant families resolve.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_voice_event_census() {
        use crate::anim_audio::{clip_cues, Cue};
        let install = crate::spore_install::discover().expect("Spore install");
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let library = crate::game_audio::AudioLibrary::from_index(&index);
        let keys: Vec<_> = index.records.keys().filter(|k| k.type_id == crate::decode::TYPE_ANIM).copied().collect();
        let mut tally: std::collections::BTreeMap<String, usize> = Default::default();
        for key in keys {
            let Ok(anim) = index.read(key).map_err(|e| e.to_string()).and_then(|b| crate::animation::parse(&b)) else { continue };
            for c in clip_cues(&anim) {
                if let Cue::Sound { name, .. } = c.cue {
                    if name.starts_with('v') || name.starts_with("bc") || name.starts_with("bs") {
                        *tally.entry(name).or_default() += 1;
                    }
                }
            }
        }
        let ok = |n: &str| library.resolve(n).is_ok();
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        let mut none = 0;
        for (name, clips) in &tally {
            let plain = ok(name);
            let xcr = crate::anim_audio::MOUTH_CODES.iter().filter(|m| ok(&format!("{name}{m}XCr"))).count();
            let ain: Vec<bool> = ["Mam", "Bir", "Ins"].iter().map(|c| ok(&format!("{name}{c}AIn"))).collect();
            let in_ain = ok(&format!("{name}InAIn"));
            let family = name.split('_').next().unwrap_or("").to_string();
            let class = match (plain, xcr > 0, ain.iter().any(|a| *a) || in_ain) {
                (true, false, false) => "plain",
                (_, true, false) => "xcr",
                (_, false, true) => "ain",
                (_, true, true) => "xcr+ain",
                (false, false, false) => "NONE",
            };
            if class == "NONE" {
                none += 1;
            }
            *counts.entry(format!("{family}|{class}")).or_default() += clips;
            println!("VOICE|{name}|{clips}|plain={plain}|xcr={xcr}|ain={ain:?}|InAIn={in_ain}|{class}");
        }
        for (k, v) in counts {
            println!("FAMILY|{k}|{v}");
        }
        println!("VOICE events {} without any bank {none}", tally.len());
    }
}
