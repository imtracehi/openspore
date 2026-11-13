//! Typed cinematic commands: the 41 keywords the cinematic manager registers
//! (Initialize 0xADE830; parse functions and defaults in notes section 3).
//! Each script line becomes one [`Command`]; unknown keywords are kept as
//! [`Command::Unknown`] and reported by the load diagnostics (never fatal).

use super::camera::{Aim, CameraMode, CameraSpec};
use super::script::{parse_hash, parse_resource_ref, Line, Token};
use crate::world_objects::name_id;

/// Letterbox style (`-speech` 1, `-thought` 2, `-caption` 3, `-radio` 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LetterboxStyle {
    #[default]
    Plain,
    Speech,
    Thought,
    Caption,
    Radio,
}

/// `wait` kinds (0xADA1D0): seconds, or a UI wait ended by message 0x445F729.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WaitKind {
    Secs(f32),
    /// Continue button shown once every blocker is done.
    ForButton,
    /// A game event continues (no UI).
    ForContinue,
    /// "Next" arrow shown once every blocker is done.
    ForNext,
}

/// Position types (table 0x15662E0, ModAPI
/// `CinematicVignetteActionData::PositionType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PositionType {
    #[default]
    Default,
    Effect,
    ActorNest,
    ActorHut,
    ActorTribe,
    ActorCity,
    Object,
    Actor,
    NearestWater,
    NearestLand,
}

impl PositionType {
    pub fn parse(text: &str) -> PositionType {
        match text.to_ascii_lowercase().as_str() {
            "effect" => PositionType::Effect,
            "actornest" => PositionType::ActorNest,
            "actorhut" => PositionType::ActorHut,
            "actortribe" => PositionType::ActorTribe,
            "actorcity" => PositionType::ActorCity,
            "object" => PositionType::Object,
            "actor" => PositionType::Actor,
            "nearestwater" => PositionType::NearestWater,
            "nearestland" => PositionType::NearestLand,
            _ => PositionType::Default,
        }
    }
}

/// A position relative to a named object: `<posType> "<name>" [(x, y, z)]`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RelPos {
    pub kind: PositionType,
    /// Name hash of the object / actor.
    pub name: u32,
    /// Offset in the reference's frame (Spore axes).
    pub offset: [f32; 3],
}

impl RelPos {
    fn from_values(values: &[Token]) -> Option<RelPos> {
        let kind = PositionType::parse(values.first()?.text());
        let name = values.get(1).map_or(0, Token::hash);
        let offset = values.get(2).and_then(Token::vector).unwrap_or([0.0; 3]);
        Some(RelPos { kind, name, offset })
    }
}

/// Actor types (table 0x1566338).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorType {
    Avatar,
    PlayerSpecies,
    Npc,
    Rare,
    Ufo,
    NearestNpcUfo,
    Homestar,
    TargetNpc,
    Posse,
    Other,
}

impl ActorType {
    pub fn parse(text: &str) -> ActorType {
        match text.to_ascii_lowercase().as_str() {
            "avatar" => ActorType::Avatar,
            "playerspecies" => ActorType::PlayerSpecies,
            "npc" => ActorType::Npc,
            "rare" => ActorType::Rare,
            "ufo" => ActorType::Ufo,
            "nearestnpcufo" => ActorType::NearestNpcUfo,
            "homestar" => ActorType::Homestar,
            "targetnpc" => ActorType::TargetNpc,
            "posse" => ActorType::Posse,
            _ => ActorType::Other,
        }
    }
}

/// `startSound` tracking mode: stopped at the end (0), also on Escape
/// (`-escStop`, 1) or never (`-noStop`, 2). INFERRED from the tracked-sound
/// list; see [`super::runtime`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SoundMode {
    #[default]
    Default,
    EscStop,
    NoStop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectSpec {
    /// Name hash the script uses for `stopEffect`.
    pub name: u32,
    pub label: String,
    /// Installed effect id (FNV-1 of the effect name).
    pub effect: u32,
    pub effect_name: String,
    pub target: Option<u32>,
    pub offset: [f32; 3],
    pub extra_offset: [f32; 3],
    pub attached: bool,
    pub use_radius: bool,
    pub use_height: bool,
    pub pin_to_surface: bool,
    pub persist: bool,
    pub soft_stop: bool,
    pub blocking: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddActor {
    pub name: u32,
    pub label: String,
    pub kind: ActorType,
    pub initial: Option<RelPos>,
    pub high_lod: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddObject {
    pub name: u32,
    pub label: String,
    pub initial: Option<RelPos>,
    pub face: Option<RelPos>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveActor {
    pub actor: u32,
    pub position: [f32; 3],
    pub stop_distance: f32,
    /// `None`: relative to the actor itself.
    pub relative: Option<RelPos>,
    pub face: Option<RelPos>,
    pub blocking: bool,
    pub snap: bool,
    pub use_footprint_radius: bool,
    pub use_target_width: bool,
    pub use_target_height: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayAnimation {
    pub actor: u32,
    /// Animation ids, played in sequence.
    pub ids: Vec<u32>,
    pub blocking: bool,
    /// `-target <posType> "<name>"`: what the clip is played at (the actor
    /// turns to face it).
    pub target: Option<RelPos>,
    /// `-processAnimEventsUsingTarget`: the clip's animation events (effects,
    /// sounds) play at the target instead of on the actor.
    pub events_on_target: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StartVignette {
    pub actor: u32,
    /// `group!prop` of the vignette definition.
    pub prop: String,
    pub position: [f32; 3],
    pub relative: Option<RelPos>,
    pub face: Option<RelPos>,
    pub blocking: bool,
}

/// One script command.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Letterbox { on: bool, style: LetterboxStyle },
    StartCamera(Box<CameraSpec>),
    StopCamera,
    /// `text` / `textUpper`: locale `table!id`; the quoted first argument is
    /// a developer comment (kept as a fallback when the table lacks the id).
    Text { upper: bool, table: u32, id: u32, dev: String },
    TextHide { upper: bool },
    TextFromCode { key: String },
    ScrollingText { table: u32, id: u32, duration: f32 },
    StartEffect(Box<EffectSpec>),
    StopEffect { name: u32, soft: bool },
    SendMessage { id: u32, data: [u32; 4] },
    Wait(WaitKind),
    StateDuration(f32),
    NextState(u32),
    NextStateIf(Vec<u32>),
    PauseGame { graphics: bool, screen_effect: Option<u32> },
    UnpauseGame,
    LetAiRun { avatar_only: bool, stop: bool },
    StartSound { key: u32, sound: u32, name: String, mode: SoundMode },
    StopSound { key: u32 },
    MixEvent { name: String, hash: u32, value: f32 },
    ShowTimeline { zoom_to_end: bool },
    DisableEscExit,
    EnableEscExit,
    /// `None` = `null` (Escape stops the cinematic).
    SetExitState(Option<u32>),
    DisableDuringDemo,
    PreserveCinematicCamView,
    ShowUi(bool),
    ClearAreaAroundObject { object: u32, radius: f32, flags: u8 },
    BlackScreen(bool),
    AlphaObstacles { on: bool, camera: bool, actors: Vec<u32>, radius: f32 },
    StopCinematic,
    SwitchToExitState,
    AddActor(Box<AddActor>),
    AddObject(Box<AddObject>),
    MoveActor(Box<MoveActor>),
    PlayAnimation(Box<PlayAnimation>),
    StartVignette(Box<StartVignette>),
    PhysicsCollisions,
    TogglePlumping,
    TakeSnapshot,
    Description(String),
    Unknown(String),
}

/// clearAreaAroundObject flags (VERIFIED parse 0xADABF0).
pub mod clear_flags {
    pub const OBJECTS: u8 = 1;
    pub const ANIMALS: u8 = 2;
    pub const SNAP: u8 = 4;
    pub const CITIZENS: u8 = 8;
}

/// `null` (DAT_015663EC).
pub const NULL_STATE: u32 = 0x2EBC_D6A6;

fn data_value(token: &Token) -> u32 {
    let text = token.text();
    if text.starts_with("0x") || text.starts_with("0X") {
        return parse_hash(text);
    }
    match text.parse::<i64>() {
        Ok(v) => v as u32,
        Err(_) => text.parse::<f32>().map(f32::to_bits).unwrap_or_else(|_| name_id(text)),
    }
}

fn camera_spec(line: &Line, path: bool) -> CameraSpec {
    let mut spec = CameraSpec {
        target: line.option_hash("target"),
        offset: line.option_float("offset").unwrap_or(0.0),
        target_offset: line.option_vector("targetOffset").unwrap_or([0.0; 3]),
        extra_target_offset: line.option_vector("extraTargetOffset").unwrap_or([0.0; 3]),
        follow: line.flag("follow"),
        orient: line.flag("orient"),
        orient_flat: line.flag("orientFlat"),
        use_footprint: line.flag("useFootprint"),
        use_radius: line.flag("useRadius"),
        use_height: line.flag("useHeight"),
        radius_target: line.option_hash("radiusTarget"),
        aim: if line.flag("targetTerrain") {
            Aim::Terrain
        } else if line.flag("targetHead") {
            Aim::Head
        } else {
            Aim::Origin
        },
        blocking: !line.flag("noWait"),
        snap: line.flag("snap"),
        ease: line.flag("ease"),
        mode: if line.flag("anchored") {
            CameraMode::Anchored
        } else if line.flag("ballistic") {
            CameraMode::Ballistic
        } else if line.flag("satellite") {
            CameraMode::Satellite
        } else if path {
            CameraMode::Path
        } else {
            CameraMode::Orbit
        },
        back_to_start: line.flag("backToStart"),
        heading: line.option_float("heading").unwrap_or(0.0),
        pitch: line.option_float("pitch").unwrap_or(0.0),
        roll: line.option_float("roll").unwrap_or(0.0),
        fov: line.option_float("fov").unwrap_or(0.0),
        duration: line.option_float("duration").unwrap_or(1.0),
        sphere: line.flag("sphere"),
        near_clip: line.option_float("nearClip"),
        far_clip: line.option_float("farClip"),
        target_pos: line.option_vector("targetpos"),
        path: Vec::new(),
        path_end_start: false,
    };
    if path {
        // 0xADE150: `-path` points, else `-pathEndStart` points (which also
        // set the job flag +0x44).
        if let Some(values) = line.option("path") {
            spec.path = values.iter().filter_map(Token::vector).collect();
        } else if let Some(values) = line.option("pathEndStart") {
            spec.path = values.iter().filter_map(Token::vector).collect();
            spec.path_end_start = true;
        }
        if spec.target_pos.is_none() {
            spec.target_pos = line.option_vector("pos");
        }
    }
    spec
}

fn named(token: Option<&Token>) -> (u32, String) {
    match token {
        Some(t) => (t.hash(), t.text().to_string()),
        None => (0, String::new()),
    }
}

impl Command {
    pub fn parse(line: &Line) -> Command {
        match line.keyword.as_str() {
            "letterbox" => Command::Letterbox {
                on: !line.flag("off"),
                style: if line.flag("speech") {
                    LetterboxStyle::Speech
                } else if line.flag("thought") {
                    LetterboxStyle::Thought
                } else if line.flag("caption") {
                    LetterboxStyle::Caption
                } else if line.flag("radio") {
                    LetterboxStyle::Radio
                } else {
                    LetterboxStyle::Plain
                },
            },
            "startcamera" => Command::StartCamera(Box::new(camera_spec(line, false))),
            "pathcamera" => Command::StartCamera(Box::new(camera_spec(line, true))),
            "stopcamera" => Command::StopCamera,
            "text" | "textupper" => {
                let upper = line.keyword == "textupper";
                if line.flag("hide") {
                    return Command::TextHide { upper };
                }
                // The quoted developer text may contain '!' itself.
                let reference = line
                    .args
                    .iter()
                    .filter(|t| matches!(t, Token::Word(_)))
                    .find_map(|t| parse_resource_ref(t.text()));
                let dev = line
                    .args
                    .iter()
                    .find(|t| matches!(t, Token::Quoted(_)))
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                // A line with only the developer text (test.trigger) shows it.
                let (table, id) = reference.unwrap_or((0, 0));
                Command::Text { upper, table, id, dev }
            }
            "textfromcode" => Command::TextFromCode {
                key: line.arg_text(0).unwrap_or("").to_string(),
            },
            "scrollingtext" => {
                let (table, id) = line
                    .args
                    .iter()
                    .filter(|t| matches!(t, Token::Word(_)))
                    .find_map(|t| parse_resource_ref(t.text()))
                    .unwrap_or((0, 0));
                // 0xADBA20: `-duration` defaults to 2 s (0x40000000).
                Command::ScrollingText { table, id, duration: line.option_float("duration").unwrap_or(2.0) }
            }
            "starteffect" => {
                let (name, label) = named(line.arg(0));
                let (effect, effect_name) = named(line.arg(1));
                Command::StartEffect(Box::new(EffectSpec {
                    name,
                    label,
                    effect,
                    effect_name,
                    target: line.option_hash("target"),
                    offset: line.option_vector("offset").unwrap_or([0.0; 3]),
                    extra_offset: line.option_vector("extraOffset").unwrap_or([0.0; 3]),
                    attached: line.flag("attached"),
                    use_radius: line.flag("useRadius"),
                    use_height: line.flag("useHeight"),
                    pin_to_surface: line.flag("pinToSurface"),
                    persist: line.flag("persist"),
                    soft_stop: line.flag("softStop"),
                    blocking: !line.flag("noWait"),
                }))
            }
            "stopeffect" => Command::StopEffect { name: named(line.arg(0)).0, soft: line.flag("soft") },
            "sendmessage" => {
                let mut data = [0u32; 4];
                for (i, slot) in data.iter_mut().enumerate() {
                    if let Some(v) = line.option(&format!("data{}", i + 1)).and_then(|v| v.first()) {
                        *slot = data_value(v);
                    }
                }
                Command::SendMessage { id: line.option_hash("id").unwrap_or(0), data }
            }
            "wait" => Command::Wait(if line.flag("forButton") {
                WaitKind::ForButton
            } else if line.flag("forContinue") {
                WaitKind::ForContinue
            } else if line.flag("forNext") {
                WaitKind::ForNext
            } else {
                WaitKind::Secs(line.option_float("secs").unwrap_or(0.0))
            }),
            "stateduration" => Command::StateDuration(line.arg(0).and_then(Token::float).unwrap_or(0.0)),
            "nextstate" => Command::NextState(line.arg(0).map_or(0, Token::hash)),
            "nextstateif" => Command::NextStateIf(line.args.iter().map(Token::hash).collect()),
            "pausegame" => {
                let graphics = line.flag("pauseGraphics");
                let mut effect = line.flag("screenEffect").then_some(name_id("TutorialFreezeFrame"));
                if let Some(name) = line.option_text("effect") {
                    effect = Some(name_id(name));
                }
                Command::PauseGame { graphics, screen_effect: effect }
            }
            "unpausegame" => Command::UnpauseGame,
            "letairun" => Command::LetAiRun { avatar_only: line.flag("avatarOnly"), stop: line.flag("stop") },
            "startsound" => {
                let key = line.arg(0).map_or(0, Token::hash);
                let sound_token = line.arg(1).or(line.arg(0));
                let mode = if line.flag("noStop") {
                    SoundMode::NoStop
                } else if line.flag("escStop") {
                    SoundMode::EscStop
                } else {
                    SoundMode::Default
                };
                Command::StartSound {
                    key,
                    sound: sound_token.map_or(0, Token::hash),
                    name: sound_token.map(|t| t.text().to_string()).unwrap_or_default(),
                    mode,
                }
            }
            "stopsound" => Command::StopSound { key: line.arg(0).map_or(0, Token::hash) },
            "mixevent" => {
                let name = line.arg_text(0).unwrap_or("").to_string();
                Command::MixEvent {
                    hash: name_id(&name),
                    value: line.arg(1).and_then(Token::float).unwrap_or(0.0),
                    name,
                }
            }
            "showtimeline" => Command::ShowTimeline { zoom_to_end: line.flag("zoomToEnd") },
            "disableescexit" => Command::DisableEscExit,
            "enableescexit" => Command::EnableEscExit,
            "setexitstate" => {
                let id = line.arg(0).map_or(NULL_STATE, Token::hash);
                Command::SetExitState((id != NULL_STATE).then_some(id))
            }
            "disableduringdemo" => Command::DisableDuringDemo,
            "preservecinematiccamview" => Command::PreserveCinematicCamView,
            "showui" => Command::ShowUi(!line.flag("off")),
            "clearareaaroundobject" => {
                let mut flags = 0;
                for (name, bit) in [
                    ("objects", clear_flags::OBJECTS),
                    ("animals", clear_flags::ANIMALS),
                    ("snap", clear_flags::SNAP),
                    ("citizens", clear_flags::CITIZENS),
                ] {
                    if line.flag(name) {
                        flags |= bit;
                    }
                }
                Command::ClearAreaAroundObject {
                    object: line.arg(0).map_or(0, Token::hash),
                    radius: line.arg(1).and_then(Token::float).unwrap_or(0.0),
                    flags,
                }
            }
            "blackscreen" => Command::BlackScreen(!line.flag("off")),
            "alphaobstacles" => Command::AlphaObstacles {
                on: !line.arg_text(0).is_some_and(|a| a.eq_ignore_ascii_case("off")),
                camera: line.flag("camera"),
                actors: line.option("actors").map(|v| v.iter().map(Token::hash).collect()).unwrap_or_default(),
                radius: line.option_float("radius").unwrap_or(0.0),
            },
            "stopcinematic" => Command::StopCinematic,
            "switchtoexitstate" => Command::SwitchToExitState,
            "addactor" => {
                let (name, label) = named(line.arg(0));
                Command::AddActor(Box::new(AddActor {
                    name,
                    label,
                    kind: ActorType::parse(line.arg_text(1).unwrap_or("")),
                    initial: line.option("initialRelativePosition").and_then(RelPos::from_values),
                    high_lod: line.flag("highLOD"),
                }))
            }
            "addobject" => {
                let (name, label) = named(line.arg(0));
                Command::AddObject(Box::new(AddObject {
                    name,
                    label,
                    initial: line.option("initialRelativePosition").and_then(RelPos::from_values),
                    face: line.option("face").and_then(RelPos::from_values),
                }))
            }
            "moveactor" => Command::MoveActor(Box::new(MoveActor {
                actor: line.arg(0).map_or(0, Token::hash),
                position: line.arg(1).and_then(Token::vector).unwrap_or([0.0; 3]),
                stop_distance: line.arg(2).and_then(Token::float).unwrap_or(0.0),
                relative: line.option("relative").and_then(RelPos::from_values),
                face: line.option("face").and_then(RelPos::from_values),
                blocking: !line.flag("noWait"),
                snap: line.flag("snap"),
                use_footprint_radius: line.flag("useFootprintRadius"),
                use_target_width: line.flag("useTargetWidth"),
                use_target_height: line.flag("useTargetHeight"),
            })),
            "playanimation" => Command::PlayAnimation(Box::new(PlayAnimation {
                actor: line.arg(0).map_or(0, Token::hash),
                ids: line.args.iter().skip(1).flat_map(Token::id_list).collect(),
                blocking: !line.flag("noWait"),
                target: line.option("target").and_then(RelPos::from_values),
                events_on_target: line.flag("processAnimEventsUsingTarget"),
            })),
            "startvignette" => Command::StartVignette(Box::new(StartVignette {
                actor: line.arg(0).map_or(0, Token::hash),
                prop: line.arg_text(1).unwrap_or("").to_string(),
                position: line.arg(2).and_then(Token::vector).unwrap_or([0.0; 3]),
                relative: line.option("relative").and_then(RelPos::from_values),
                face: line.option("face").and_then(RelPos::from_values),
                blocking: !line.flag("noWait"),
            })),
            "physicscollisions" => Command::PhysicsCollisions,
            "toggleplumping" => Command::TogglePlumping,
            "takesnapshot" => Command::TakeSnapshot,
            "description" => Command::Description(line.arg_text(0).unwrap_or("").to_string()),
            other => Command::Unknown(other.to_string()),
        }
    }

    /// Commands that do something in this build (the rest are parsed and
    /// skipped: Space-only or unused by the installed scripts).
    pub fn is_unknown(&self) -> bool {
        matches!(self, Command::Unknown(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(text: &str) -> Command {
        Command::parse(&Line::parse(text, 1).unwrap())
    }

    #[test]
    fn camera_options_and_defaults() {
        let Command::StartCamera(spec) = cmd(
            "startcamera -target \"TheAvatar\" -targetOffset (0, 0, 0) -extraTargetOffset (0, 0, -.5) -targetTerrain -useRadius -useFootprint -pitch -.05 -heading .5 -ease -offset 6 -orient -follow -fov .6 -snap -sphere",
        ) else {
            panic!()
        };
        assert_eq!(spec.target, Some(name_id("TheAvatar")));
        assert_eq!(spec.extra_target_offset, [0.0, 0.0, -0.5]);
        assert_eq!(spec.aim, Aim::Terrain);
        assert!(spec.use_radius && spec.use_footprint && spec.ease && spec.orient && spec.follow && spec.snap && spec.sphere);
        assert_eq!((spec.heading, spec.pitch, spec.offset, spec.fov), (0.5, -0.05, 6.0, 0.6));
        assert_eq!(spec.duration, 1.0);
        assert_eq!(spec.mode, CameraMode::Orbit);
        assert!(spec.blocking);
        let Command::StartCamera(anchored) = cmd("startCamera -target \"TribeHut\" -targetOffset (0, 3, 1.5) -anchored -offset -15 -ease -fov .75 -duration 7 -noWait -sphere") else {
            panic!()
        };
        assert_eq!(anchored.mode, CameraMode::Anchored);
        assert!(!anchored.blocking);
        assert_eq!(anchored.duration, 7.0);
    }

    #[test]
    fn text_wait_sound_and_flow_commands() {
        assert_eq!(
            cmd("text \"crggoals3\" cinematics_crg!0x04864c3c"),
            Command::Text { upper: false, table: name_id("cinematics_crg"), id: 0x0486_4C3C, dev: "crggoals3".into() }
        );
        assert_eq!(cmd("text -hide"), Command::TextHide { upper: false });
        assert_eq!(
            cmd("text \"Congratulations! You have gained a posse slot.\" cinematics_crg!0x054076aa"),
            Command::Text {
                upper: false,
                table: name_id("cinematics_crg"),
                id: 0x0540_76AA,
                dev: "Congratulations! You have gained a posse slot.".into()
            }
        );
        assert_eq!(cmd("wait -secs 8.8"), Command::Wait(WaitKind::Secs(8.8)));
        assert_eq!(cmd("wait -forButton"), Command::Wait(WaitKind::ForButton));
        assert_eq!(cmd("wait -forContinue"), Command::Wait(WaitKind::ForContinue));
        assert_eq!(cmd("nextstate creaturegoals1"), Command::NextState(name_id("creaturegoals1")));
        assert_eq!(cmd("setExitState null"), Command::SetExitState(None));
        assert_eq!(cmd("setExitState setupCreature"), Command::SetExitState(Some(name_id("setupCreature"))));
        assert_eq!(
            cmd("startsound music_brain_upgrade -escStop"),
            Command::StartSound {
                key: name_id("music_brain_upgrade"),
                sound: name_id("music_brain_upgrade"),
                name: "music_brain_upgrade".into(),
                mode: SoundMode::EscStop
            }
        );
        assert!(matches!(cmd("startsound x -escstop"), Command::StartSound { mode: SoundMode::EscStop, .. }));
        assert_eq!(
            cmd("mixevent cell_trans_end 1"),
            Command::MixEvent { name: "cell_trans_end".into(), hash: name_id("cell_trans_end"), value: 1.0 }
        );
        assert_eq!(
            cmd("sendMessage -id 0x04b719d4 -data1 0x01654C02"),
            Command::SendMessage { id: 0x04B7_19D4, data: [0x0165_4C02, 0, 0, 0] }
        );
        assert_eq!(
            cmd("pauseGame -screenEffect -pauseGraphics"),
            Command::PauseGame { graphics: true, screen_effect: Some(0xDFE9_0CAD) }
        );
        assert_eq!(
            cmd("pauseGame -effect DeathFreezeFrame"),
            Command::PauseGame { graphics: false, screen_effect: Some(name_id("DeathFreezeFrame")) }
        );
        assert_eq!(
            cmd("clearAreaAroundObject \"TheAvatar\" 10.0 -animals -snap"),
            Command::ClearAreaAroundObject { object: name_id("TheAvatar"), radius: 10.0, flags: clear_flags::ANIMALS | clear_flags::SNAP }
        );
        assert!(cmd("frobnicate 1 2").is_unknown());
    }

    #[test]
    fn actor_commands() {
        let Command::AddActor(a) = cmd("addActor \"Buddy1\" playerSpecies -highLOD -initialRelativePosition nearestWater \"AvatarNest\" (50, -20, 0)") else {
            panic!()
        };
        assert_eq!(a.kind, ActorType::PlayerSpecies);
        assert!(a.high_lod);
        assert_eq!(
            a.initial,
            Some(RelPos { kind: PositionType::NearestWater, name: name_id("AvatarNest"), offset: [50.0, -20.0, 0.0] })
        );
        let Command::MoveActor(m) = cmd("moveActor \"TheAvatar\" (0, 0, 0) 0 -relative object \"AvatarNest\" -face actor \"AvatarNest\" -noWait -snap") else {
            panic!()
        };
        assert_eq!(m.relative.unwrap().kind, PositionType::Object);
        assert_eq!(m.face.unwrap().kind, PositionType::Actor);
        assert!(!m.blocking && m.snap);
        let Command::MoveActor(face) = cmd("moveActor \"Chieftain\" (-3.5, 3, 0) -relative object \"TribeFire\" -face object  \"TribeHut\" (0, 0, 0) -snap") else {
            panic!()
        };
        assert_eq!(face.position, [-3.5, 3.0, 0.0]);
        assert!(face.blocking);
        let Command::PlayAnimation(p) = cmd("playAnimation \"TheAvatar\" (0x053d9bc9) -noWait") else { panic!() };
        assert_eq!(p.ids, vec![0x053D_9BC9]);
        assert!(!p.blocking);
        let Command::StartEffect(e) = cmd("startEffect \"fadeFromBlack\" fade_from_black_2 -noWait") else { panic!() };
        assert_eq!(e.effect, name_id("fade_from_black_2"));
        assert_eq!(e.name, name_id("fadeFromBlack"));
        assert!(!e.blocking);
    }
}
