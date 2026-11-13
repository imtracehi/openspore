//! The anthem editor (`CityMusicUI`, layouts_atlas~ 0x6C4E59FB), opened from
//! the City Planner (`civ_planner`): the player's city anthem
//! (`CivCampaign::anthem`, played by `music_city_planner`, `civ_music`).
//!
//! RETAIL (VERIFIED, `notes\civ-planner-re.md` A1-A3; the editor's message
//! handler 0xEA08F0 and the `cPlayerCityMusic` setters):
//! * Beat: ten radio buttons (command 0x04191515) -> 0xEACD80 (the drums; the
//!   same set again = none); Anthem: ten instrument buttons (0x0419151C) ->
//!   0xEACE00 (the same; a new instrument plays the anthem from its first
//!   note); Ambience: twenty toggles (0x0419150A) -> 0xEAE0C0 add (the first
//!   empty of the 4 layers, else the oldest added layer is replaced and its
//!   button unchecked) / 0xEADD30 remove.
//! * Play anthem (0x04191520) -> 0xEACE80(0); Generate new anthem
//!   (0x04ED09B8) -> 0xEB00F0 a new melody, then play; Add note (0x04191529)
//!   -> 0xEAF4E0 then play that note (0xEACEE0); Remove note (0x0419154D) ->
//!   0xEAE5E0 the last; Randomize your music (0x0456FF90) -> 0xEB0430, play;
//!   Undo / Redo (0x04F8C2C3 / 0x04F8C2C7, `MaxUndos` 20) then play;
//!   Auto-play (0x04EE92A8) -> 0xEA3020 the loop flag (0xEACF40); the volume
//!   sliders (0xEF00A884) -> 0xEAD060 gain = fraction^2 (0xE9FF00): beat ->
//!   rhythm gain, anthem -> melody gain, ambience -> ambience gain.
//! * Button icons: `audio~!playercitymusic` `BeatIcons` / `InstrumentIcons`
//!   / `AmbienceIcons` (PNGs in group 0x05C01E47).
//!
//! * Load / Save (0x05023C10 / 0x05023C00) -> 0xEA2080 / 0xEA2A40: retail
//!   saves the anthem as a `.cmp` (type 0x04F684A4) with a 128 x 128
//!   `CityMusicThumbnail` into `%APPDATA%\Spore\CityMusic`; an anthem record
//!   is read back by 0xEB0B40: rhythm (+0x14), instrument (+0x18), the note
//!   pitches (+0x1C) and durations (+0x30), the ambience layers (+0x44) and
//!   the three gains (+0x58, taken only when the counts match).
//!
//! ADAPTED: the panel opens beside the planner's palette (Anthem button);
//! the `.cmp` files live in `<save stem>-CityMusic\` beside the player's save
//! (never the user's Spore folder) as the retail ClassSerializer stream of
//! `cPlayerCityMusic` (`encode_cmp`, VERIFIED attribute table 0x015A9198; the
//! retail creation container / thumbnail around it is not reproduced); Save writes
//! the next `anthem-NNN.cmp` and Load steps through the saved files (retail
//! opens a naming dialog / browser); a slider takes the pointer's place along
//! its width while the left button is held (0xE9FF00's fraction of the
//! range, squared); the melody area draws one bar per note (pitch up, onset
//! across) instead of the retail `CityMusicNoteOnOff` note windows.

use bevy::prelude::*;

use crate::civ_music::{self, AnthemSetup, CityAnthem};
use crate::game::RunProgress;
use crate::spui::{
    model::ImageRef,
    solve::RootPolicy,
    view::{self, SpuiAssets, SpuiState},
};
use crate::stages::{GameStage, StageOwned};

pub const LAYOUT: &str = "CityMusicUI";

/// Control ids and commands of `CityMusicUI`.
#[allow(dead_code)]
pub mod id {
    pub const BEAT_GROUP: u32 = 0x0419_14F0;
    pub const INSTRUMENT_GROUP: u32 = 0x0419_14F3;
    pub const AMBIENCE_GROUP: u32 = 0x0419_14E9;
    /// The icon button inside each beat / instrument / ambience button.
    pub const ICON: u32 = 0x05BF_E9D0;
    pub const MELODY: u32 = 0x0419_18A2;
    pub const AUTO_PLAY: u32 = 0x04EF_C540;
    pub const ADD_NOTE: u32 = 0x04EC_FE40;
    pub const REMOVE_NOTE: u32 = 0x04EC_FE08;
    pub const UNDO: u32 = 0x04F8_C2B2;
    pub const REDO: u32 = 0x04F8_C2BD;
    pub const LOAD: u32 = 0x0502_3C10;
    pub const SAVE: u32 = 0x0502_3C00;
    pub const BEAT_VOLUME: u32 = 0x04D8_2FB8;
    pub const ANTHEM_VOLUME: u32 = 0x04D8_2FC0;
    pub const AMBIENCE_VOLUME: u32 = 0x04D8_2FC8;
    // Commands (0xEA08F0).
    pub const CMD_BEAT: u32 = 0x0419_1515;
    pub const CMD_INSTRUMENT: u32 = 0x0419_151C;
    pub const CMD_AMBIENCE: u32 = 0x0419_150A;
    pub const CMD_PLAY: u32 = 0x0419_1520;
    pub const CMD_NEW_ANTHEM: u32 = 0x04ED_09B8;
    pub const CMD_ADD_NOTE: u32 = 0x0419_1529;
    pub const CMD_REMOVE_NOTE: u32 = 0x0419_154D;
    pub const CMD_RANDOMIZE: u32 = 0x0456_FF90;
    pub const CMD_UNDO: u32 = 0x04F8_C2C3;
    pub const CMD_REDO: u32 = 0x04F8_C2C7;
    pub const CMD_AUTO_PLAY: u32 = 0x04EE_92A8;
    pub const CMD_LOAD: u32 = 0x0502_3C10;
    pub const CMD_SAVE: u32 = 0x0502_3C00;
}

/// The setup prop's icon lists (`AmbienceIcons` 0xAFFACD19, `InstrumentIcons`
/// 0xE0ABFD7C, `BeatIcons` 0xEEA41E91) and their image group.
const ICON_GROUP: u32 = 0x05C0_1E47;
const PNG: u32 = 0x2F7D_0004;
const AMBIENCE_ICONS: u32 = 0xAFFA_CD19;
const INSTRUMENT_ICONS: u32 = 0xE0AB_FD7C;
const BEAT_ICONS: u32 = 0xEEA4_1E91;
/// `MaxUndos` 20.
pub const MAX_UNDOS: usize = 20;

/// What a click asks of the anthem.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnthemEdit {
    Beat(u32),
    Instrument(u32),
    /// Ambience `id` turned on (true) or off.
    Ambience(u32, bool),
    Play,
    NewAnthem,
    AddNote,
    RemoveNote,
    Randomize,
    Undo,
    Redo,
    AutoPlay,
}

/// What the city music must play after an edit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayRequest {
    /// `city_music_melody_play 0` (0xEACE80).
    FromStart,
    /// `city_music_melody_play_note n` (0xEACEE0).
    Note(u32),
}

/// The editor's state: open, the undo / redo stacks, the order the ambience
/// layers were added (0xEAE0C0 replaces the oldest), the play request for
/// `civ_music`.
#[derive(Resource, Default, Debug)]
pub struct AnthemEditor {
    pub open: bool,
    pub undo: Vec<CityAnthem>,
    pub redo: Vec<CityAnthem>,
    pub order: Vec<u32>,
    pub play: Option<PlayRequest>,
    /// The volume slider being dragged (its control id).
    pub dragging: Option<u32>,
    /// The next saved anthem Load reads.
    pub load_cursor: usize,
    seed: u64,
    setup: Option<AnthemSetup>,
    icons: Option<[Vec<u32>; 3]>,
}

/// The gain index (0 ambience, 1 rhythm, 2 melody) a volume slider sets
/// (0xEAD060: beat -> gain[1], anthem -> gain[2], ambience -> gain[0]).
pub fn slider_gain(control: u32) -> Option<usize> {
    match control {
        id::BEAT_VOLUME => Some(1),
        id::ANTHEM_VOLUME => Some(2),
        id::AMBIENCE_VOLUME => Some(0),
        _ => None,
    }
}

/// Pure: 0xE9FF00: the gain of a slider at `x` across `[x1, x2]`.
pub fn slider_value(x: f32, x1: f32, x2: f32) -> f32 {
    let f = if x2 > x1 { ((x - x1) / (x2 - x1)).clamp(0.0, 1.0) } else { 0.0 };
    f * f
}

/// The anthem stream (VERIFIED: `cPlayerCityMusic`'s ClassSerializer 0xEB0A30
/// with the attribute table built by the static initializer 0x1387660 into
/// 0x015A9198, and `ClassSerializer::Write` 0x6928C0 / `Read` 0x693DE0):
/// big-endian `u32` class id 0x01A80D26, `u32` size of the rest, `u32`
/// attribute count, (`u32` id, `u32` size) per attribute, then each
/// attribute's data. Attributes (id, object offset, encoding):
/// `mName` 0x0511DFB9 +0x178 and `mDescription` 0x0511DFC3 +0x188 (string16:
/// `u32` count + UTF-16LE, 0x692E10), `mVolumes` 0x04D81CFE +0x198 (the three
/// gains), `mAmbienceHistory` 0x04226180 +0x1B8 (the ambience ids in the
/// order added), `mRhythm` 0x04226186 +0x1C4, `mInstrument` 0x04226189
/// +0x1C8, `mNotePitches` 0x04E551EE +0x1CC, `mNoteDurations` 0x04F7C02D
/// +0x1E0 (vectors and lists: `u32` count + 4-byte values; `Read` skips
/// unknown ids by their size). The loop flag and the beat pattern are not
/// in the stream. INFERRED: a saved `.cmp` is this stream (the save names the
/// anthem in `mName` / `mDescription`); ADAPTED: written as a bare file (the
/// retail creation container and its thumbnail are not reproduced), the
/// ambience history is the layers in layer order.
pub const CMP_CLASS_ID: u32 = 0x01A8_0D26;
pub mod cmp_attr {
    pub const NAME: u32 = 0x0511_DFB9;
    pub const DESCRIPTION: u32 = 0x0511_DFC3;
    pub const VOLUMES: u32 = 0x04D8_1CFE;
    pub const AMBIENCE_HISTORY: u32 = 0x0422_6180;
    pub const RHYTHM: u32 = 0x0422_6186;
    pub const INSTRUMENT: u32 = 0x0422_6189;
    pub const NOTE_PITCHES: u32 = 0x04E5_51EE;
    pub const NOTE_DURATIONS: u32 = 0x04F7_C02D;
}

/// Earlier OpenSpore anthem files (kept readable).
const OLD_CMP_MAGIC: &[u8; 4] = b"OSCM";
const OLD_CMP_VERSION: u32 = 1;

fn words(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    let values: Vec<u32> = values.into_iter().collect();
    let mut out = (values.len() as u32).to_be_bytes().to_vec();
    for v in values {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out
}

fn string16(s: &str) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut out = (units.len() as u32).to_be_bytes().to_vec();
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

/// Pure: an anthem as a `.cmp` stream named `name` (see `CMP_CLASS_ID`).
pub fn encode_cmp_named(a: &CityAnthem, name: &str) -> Vec<u8> {
    use cmp_attr::*;
    let attributes: [(u32, Vec<u8>); 8] = [
        (NAME, string16(name)),
        (DESCRIPTION, string16("")),
        (VOLUMES, words(a.gains.iter().map(|g| g.to_bits()))),
        (AMBIENCE_HISTORY, words(a.ambience.iter().copied().filter(|x| *x != 0))),
        (RHYTHM, a.rhythm.to_be_bytes().to_vec()),
        (INSTRUMENT, a.instrument.to_be_bytes().to_vec()),
        (NOTE_PITCHES, words(a.pitches.iter().map(|p| *p as u32))),
        (NOTE_DURATIONS, words(a.durations.iter().map(|d| d.to_bits()))),
    ];
    let mut rest = (attributes.len() as u32).to_be_bytes().to_vec();
    for (id, data) in &attributes {
        rest.extend_from_slice(&id.to_be_bytes());
        rest.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    for (_, data) in &attributes {
        rest.extend_from_slice(data);
    }
    let mut out = CMP_CLASS_ID.to_be_bytes().to_vec();
    out.extend_from_slice(&(rest.len() as u32).to_be_bytes());
    out.extend_from_slice(&rest);
    out
}

/// Pure: an anthem as a `.cmp` stream (unnamed).
pub fn encode_cmp(a: &CityAnthem) -> Vec<u8> {
    encode_cmp_named(a, "")
}

/// Pure: read a `.cmp` stream (`None` when it is not one; earlier OpenSpore
/// files are read too). As 0xEB0B40, the gains are taken only when there are
/// three; layers beyond `MaxAmbienceLayers` 4 are dropped.
pub fn decode_cmp(bytes: &[u8], current_gains: [f32; 3]) -> Option<CityAnthem> {
    if bytes.starts_with(OLD_CMP_MAGIC) {
        return decode_old_cmp(bytes, current_gains);
    }
    let be = |at: usize| bytes.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
    if be(0)? != CMP_CLASS_ID {
        return None;
    }
    let end = 8usize.checked_add(be(4)? as usize)?.min(bytes.len());
    let count = be(8)? as usize;
    if count > 128 {
        return None;
    }
    let mut table = Vec::with_capacity(count);
    for i in 0..count {
        table.push((be(12 + i * 8)?, be(16 + i * 8)? as usize));
    }
    let mut at = 12 + count * 8;
    let mut a = CityAnthem {
        ambience: vec![0; 4],
        rhythm: 0,
        instrument: 0,
        pitches: vec![],
        durations: vec![],
        gains: current_gains,
        looped: false,
        seed: 0,
        pattern: 0,
    };
    let list = |at: usize, size: usize| -> Option<Vec<u32>> {
        let n = be(at)? as usize;
        if 4 + n * 4 > size {
            return None;
        }
        (0..n).map(|k| be(at + 4 + k * 4)).collect()
    };
    for (id, size) in table {
        if at + size > end {
            return None;
        }
        match id {
            cmp_attr::RHYTHM => a.rhythm = be(at)?,
            cmp_attr::INSTRUMENT => a.instrument = be(at)?,
            cmp_attr::NOTE_PITCHES => a.pitches = list(at, size)?.into_iter().map(|p| p as i32).collect(),
            cmp_attr::NOTE_DURATIONS => a.durations = list(at, size)?.into_iter().map(f32::from_bits).collect(),
            cmp_attr::AMBIENCE_HISTORY => {
                let mut layers = list(at, size)?;
                layers.truncate(4);
                layers.resize(4, 0);
                a.ambience = layers;
            }
            cmp_attr::VOLUMES => {
                let g = list(at, size)?;
                if g.len() == 3 {
                    a.gains = [f32::from_bits(g[0]), f32::from_bits(g[1]), f32::from_bits(g[2])];
                }
            }
            _ => {}
        }
        at += size;
    }
    Some(a)
}

/// The name stored in a `.cmp` stream (`mName`), if any.
pub fn cmp_name(bytes: &[u8]) -> Option<String> {
    let be = |at: usize| bytes.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
    if be(0)? != CMP_CLASS_ID {
        return None;
    }
    let count = be(8)? as usize;
    let mut at = 12 + count.min(128) * 8;
    for i in 0..count.min(128) {
        let (id, size) = (be(12 + i * 8)?, be(16 + i * 8)? as usize);
        if id == cmp_attr::NAME {
            let n = be(at)? as usize;
            let units: Vec<u16> = (0..n)
                .map(|k| bytes.get(at + 4 + k * 2..at + 6 + k * 2).map(|s| u16::from_le_bytes([s[0], s[1]])))
                .collect::<Option<_>>()?;
            return String::from_utf16(&units).ok();
        }
        at += size;
    }
    None
}

/// Earlier OpenSpore files (`OSCM` 1: the record's fields little-endian, the
/// loop flag and the beat pattern).
fn decode_old_cmp(bytes: &[u8], current_gains: [f32; 3]) -> Option<CityAnthem> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = bytes.get(at..at + n)?;
        at += n;
        Some(s)
    };
    if take(4)? != OLD_CMP_MAGIC {
        return None;
    }
    let u = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    if u(take(4)?) != OLD_CMP_VERSION {
        return None;
    }
    let rhythm = u(take(4)?);
    let instrument = u(take(4)?);
    let n = u(take(4)?) as usize;
    if n > 64 {
        return None;
    }
    let mut pitches = Vec::with_capacity(n);
    for _ in 0..n {
        pitches.push(u(take(4)?) as i32);
    }
    let n = u(take(4)?) as usize;
    if n > 64 {
        return None;
    }
    let mut durations = Vec::with_capacity(n);
    for _ in 0..n {
        durations.push(f32::from_bits(u(take(4)?)));
    }
    let n = u(take(4)?) as usize;
    if n > 16 {
        return None;
    }
    let mut ambience = Vec::with_capacity(n);
    for _ in 0..n {
        ambience.push(u(take(4)?));
    }
    let n = u(take(4)?) as usize;
    let mut gains = Vec::new();
    for _ in 0..n.min(8) {
        gains.push(f32::from_bits(u(take(4)?)));
    }
    let looped = take(1)?[0] != 0;
    let pattern = u(take(4)?);
    let gains = if gains.len() == 3 { [gains[0], gains[1], gains[2]] } else { current_gains };
    Some(CityAnthem { ambience, rhythm, instrument, pitches, durations, gains, looped, seed: 0, pattern })
}

/// The folder of the player's saved anthems: `<save stem>-CityMusic` beside
/// the save file.
pub fn cmp_dir(save: &std::path::Path) -> std::path::PathBuf {
    let stem = save.file_stem().and_then(|s| s.to_str()).unwrap_or("openspore_save");
    save.with_file_name(format!("{stem}-CityMusic"))
}

/// The saved anthems in name order.
pub fn saved_anthems(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmp")))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Save: the next free `anthem-NNN.cmp` in the folder.
pub fn save_anthem(dir: &std::path::Path, a: &CityAnthem) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = (1..10_000)
        .map(|n| dir.join(format!("anthem-{n:03}.cmp")))
        .find(|p| !p.exists())
        .ok_or("no free anthem name")?;
    std::fs::write(&path, encode_cmp(a)).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Pure: 0xEACD80 / 0xEACE00: set `id`; the same id again clears it.
/// Returns whether it is set now.
pub fn toggle(current: &mut u32, id: u32) -> bool {
    *current = if *current == id { 0 } else { id };
    *current != 0
}

/// Pure: 0xEAE0C0: add ambience `id` in the first empty layer, else replace
/// the oldest added one (`order`, oldest first). Returns the replaced id.
pub fn add_ambience(a: &mut CityAnthem, order: &mut Vec<u32>, id: u32) -> Option<u32> {
    if a.ambience.contains(&id) {
        return None;
    }
    // Layers set before the editor opened count as added in layer order.
    for x in &a.ambience {
        if *x != 0 && !order.contains(x) {
            order.push(*x);
        }
    }
    order.retain(|x| a.ambience.contains(x));
    if let Some(slot) = a.ambience.iter().position(|x| *x == 0) {
        a.ambience[slot] = id;
        order.push(id);
        return None;
    }
    let oldest = *order.first()?;
    order.remove(0);
    if let Some(slot) = a.ambience.iter().position(|x| *x == oldest) {
        a.ambience[slot] = id;
    }
    order.push(id);
    Some(oldest)
}

/// Pure: 0xEADD30: remove ambience `id` (its layer becomes empty).
pub fn remove_ambience(a: &mut CityAnthem, order: &mut Vec<u32>, id: u32) {
    for x in a.ambience.iter_mut().filter(|x| **x == id) {
        *x = 0;
    }
    order.retain(|x| *x != id);
}

/// Pure: apply one edit (with the undo rule: every change can be undone,
/// at most `MAX_UNDOS` deep); returns what to play.
pub fn apply(
    editor: &mut AnthemEditor,
    setup: &AnthemSetup,
    anthem: &mut CityAnthem,
    edit: AnthemEdit,
    seed: u64,
) -> Option<PlayRequest> {
    let before = anthem.clone();
    let play = match edit {
        AnthemEdit::Beat(i) => {
            toggle(&mut anthem.rhythm, i);
            None
        }
        AnthemEdit::Instrument(i) => toggle(&mut anthem.instrument, i).then_some(PlayRequest::FromStart),
        AnthemEdit::Ambience(i, true) => {
            add_ambience(anthem, &mut editor.order, i);
            None
        }
        AnthemEdit::Ambience(i, false) => {
            remove_ambience(anthem, &mut editor.order, i);
            None
        }
        AnthemEdit::Play => return Some(PlayRequest::FromStart),
        AnthemEdit::NewAnthem => {
            civ_music::new_melody(setup, anthem, seed);
            Some(PlayRequest::FromStart)
        }
        AnthemEdit::AddNote => {
            if anthem.pitches.len() >= setup.max_notes as usize {
                return None;
            }
            let mut rng = civ_music::AnthemRng::new(seed);
            civ_music::push_note(setup, anthem, &mut rng);
            Some(PlayRequest::Note(anthem.pitches.len() as u32 - 1))
        }
        AnthemEdit::RemoveNote => {
            anthem.pitches.pop();
            anthem.durations.pop();
            None
        }
        AnthemEdit::Randomize => {
            let gains = anthem.gains;
            let looped = anthem.looped;
            *anthem = civ_music::random_anthem(setup, seed);
            anthem.gains = gains;
            anthem.looped = looped;
            editor.order.clear();
            Some(PlayRequest::FromStart)
        }
        AnthemEdit::Undo => {
            let Some(back) = editor.undo.pop() else { return None };
            editor.redo.push(std::mem::replace(anthem, back));
            return Some(PlayRequest::FromStart);
        }
        AnthemEdit::Redo => {
            let Some(next) = editor.redo.pop() else { return None };
            editor.undo.push(std::mem::replace(anthem, next));
            return Some(PlayRequest::FromStart);
        }
        AnthemEdit::AutoPlay => {
            anthem.looped = !anthem.looped;
            None
        }
    };
    if *anthem != before {
        editor.undo.push(before);
        if editor.undo.len() > MAX_UNDOS {
            editor.undo.remove(0);
        }
        editor.redo.clear();
    }
    play
}

/// Which layout root is the editor.
#[derive(Component)]
pub struct AnthemRoot;

/// One bar of the melody area.
#[derive(Component)]
pub struct MelodyBar;

pub struct CivAnthemPlugin;

impl Plugin for CivAnthemPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnthemEditor>()
            .add_systems(Update, (manage, clicks, bind).chain().after(view::input).before(view::sync))
            .add_systems(Update, melody_bars.after(view::sync));
    }
}

/// The editor is open while the planner is (and the Anthem button is on).
fn manage(
    mut commands: Commands,
    stage: Res<State<GameStage>>,
    planner: Res<crate::civ_planner::CivPlanner>,
    mut editor: ResMut<AnthemEditor>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    camera: Query<Entity, With<crate::GameCamera>>,
    roots: Query<Entity, With<AnthemRoot>>,
    bars: Query<Entity, With<MelodyBar>>,
) {
    if *stage.get() != GameStage::Civilization || (!planner.is_open() && planner.request.is_none()) {
        editor.open = false;
    }
    if !editor.open {
        for e in roots.iter().chain(bars.iter()) {
            commands.entity(e).despawn();
        }
        return;
    }
    if editor.setup.is_none() && !originals.resource_index.records.is_empty() {
        let index = &originals.resource_index;
        editor.setup = Some(AnthemSetup::from_index(index));
        editor.icons = Some(icon_lists(index));
    }
    if !roots.is_empty() || originals.resource_index.records.is_empty() {
        return;
    }
    let Ok(camera) = camera.single() else {
        return;
    };
    if let Some(layout) = assets.layout(&originals.resource_index, LAYOUT) {
        let root = view::spawn(&mut commands, layout, RootPolicy::Authored, Some(camera), 112);
        commands.entity(root).insert(AnthemRoot);
    }
}

/// The installed icon lists (beat, instrument, ambience instance ids).
fn icon_lists(index: &crate::asset_catalog::ResourceIndex) -> [Vec<u32>; 3] {
    let key = crate::dbpf::ResourceKey {
        type_id: 0x00B1_B104,
        group_id: 0x0214_07EE,
        instance_id: civ_music::SETUP_PROP,
    };
    let Ok(p) = index.properties(key) else {
        return [vec![], vec![], vec![]];
    };
    let list = |id: u32| -> Vec<u32> {
        p.0.get(&id)
            .map(|v| {
                v.values
                    .iter()
                    .filter_map(|b| b.get(..4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]])))
                    .collect()
            })
            .unwrap_or_default()
    };
    [list(BEAT_ICONS), list(INSTRUMENT_ICONS), list(AMBIENCE_ICONS)]
}

/// The group (beat / instrument / ambience) a window is in, and its button.
fn button_of(ui: &SpuiState, mut w: usize) -> (usize, u32) {
    // A click on the icon inside a button is the button's.
    if ui.layout.window(w).control_id == id::ICON {
        if let Some(p) = ui.layout.window(w).parent {
            w = p;
        }
    }
    (w, ui.layout.window(w).command_id)
}

fn clicks(
    mut editor: ResMut<AnthemEditor>,
    mut run: ResMut<RunProgress>,
    mut roots: Query<&mut SpuiState, With<AnthemRoot>>,
    time: Res<Time<Real>>,
    pointer: Option<Res<crate::spui::view::SpuiPointer>>,
) {
    if !editor.open {
        editor.dragging = None;
        return;
    }
    let Some(setup) = editor.setup.clone() else {
        return;
    };
    // The volume sliders follow the pointer while the button is held.
    let (at, pressed) = pointer.as_ref().map_or((None, false), |p| (p.position, p.pressed));
    if !pressed && editor.dragging.take().is_some() {
        run.request_save();
    }
    if let (true, Some(at)) = (pressed, at) {
        for ui in &roots {
            for control in [id::BEAT_VOLUME, id::ANTHEM_VOLUME, id::AMBIENCE_VOLUME] {
                let Some(w) = ui.find(control) else { continue };
                let r = ui.rect(w);
                let inside = r.contains(at.x, at.y);
                if editor.dragging.is_none() && inside {
                    editor.dragging = Some(control);
                }
                if editor.dragging == Some(control) {
                    if let (Some(k), Some(anthem)) = (slider_gain(control), run.civ_campaign.anthem.as_mut()) {
                        let gain = slider_value(at.x, r.x1, r.x2);
                        if (anthem.gains[k] - gain).abs() > 1e-4 {
                            anthem.gains[k] = gain;
                        }
                    }
                }
            }
        }
    }
    for mut ui in &mut roots {
        for click in ui.take_clicks() {
            let (w, command) = button_of(&ui, click);
            let index = ui.layout.window(w).control_id;
            let selected = ui.states[w] & crate::spui::ids::state::SELECTED != 0;
            // Load / Save (0xEA2080 / 0xEA2A40).
            if command == id::CMD_SAVE || index == id::SAVE {
                let dir = cmp_dir(&RunProgress::save_path());
                if let Some(anthem) = run.civ_campaign.anthem.clone() {
                    run.civ_campaign.message = match save_anthem(&dir, &anthem) {
                        Ok(path) => format!("Anthem saved: {}", path.file_name().and_then(|n| n.to_str()).unwrap_or("")),
                        Err(e) => format!("The anthem was not saved: {e}"),
                    };
                }
                continue;
            }
            if command == id::CMD_LOAD || index == id::LOAD {
                let files = saved_anthems(&cmp_dir(&RunProgress::save_path()));
                if files.is_empty() {
                    run.civ_campaign.message = "No saved anthems yet".into();
                    continue;
                }
                let path = files[editor.load_cursor % files.len()].clone();
                editor.load_cursor = editor.load_cursor.wrapping_add(1);
                let gains = run.civ_campaign.anthem.as_ref().map_or([1.0; 3], |a| a.gains);
                let bytes = std::fs::read(&path).unwrap_or_default();
                let name = cmp_name(&bytes).filter(|n| !n.is_empty());
                match decode_cmp(&bytes, gains) {
                    Some(mut loaded) => {
                        if let Some(anthem) = run.civ_campaign.anthem.as_mut() {
                            // The loop flag, beat pattern and seed are the
                            // object's, not the stream's (0xEB0A30).
                            loaded.looped = anthem.looped;
                            loaded.pattern = anthem.pattern;
                            loaded.seed = anthem.seed;
                            let before = std::mem::replace(anthem, loaded);
                            editor.undo.push(before);
                            if editor.undo.len() > MAX_UNDOS {
                                editor.undo.remove(0);
                            }
                            editor.redo.clear();
                            editor.order = anthem.ambience.iter().copied().filter(|a| *a != 0).collect();
                            editor.play = Some(PlayRequest::FromStart);
                        }
                        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                        run.civ_campaign.message = format!("Anthem loaded: {}", name.unwrap_or(file));
                        run.request_save();
                    }
                    None => run.civ_campaign.message = "That anthem file could not be read".into(),
                }
                continue;
            }
            let edit = match command {
                id::CMD_BEAT => AnthemEdit::Beat(index),
                id::CMD_INSTRUMENT => AnthemEdit::Instrument(index),
                id::CMD_AMBIENCE => AnthemEdit::Ambience(index, !selected),
                id::CMD_PLAY => AnthemEdit::Play,
                id::CMD_NEW_ANTHEM => AnthemEdit::NewAnthem,
                id::CMD_ADD_NOTE => AnthemEdit::AddNote,
                id::CMD_REMOVE_NOTE => AnthemEdit::RemoveNote,
                id::CMD_RANDOMIZE => AnthemEdit::Randomize,
                id::CMD_UNDO => AnthemEdit::Undo,
                id::CMD_REDO => AnthemEdit::Redo,
                id::CMD_AUTO_PLAY => AnthemEdit::AutoPlay,
                _ => continue,
            };
            let Some(anthem) = run.civ_campaign.anthem.as_mut() else {
                continue;
            };
            editor.seed = editor.seed.wrapping_add(1) ^ time.elapsed().as_nanos() as u64;
            let seed = editor.seed;
            if let Some(play) = apply(&mut editor, &setup, anthem, edit, seed) {
                editor.play = Some(play);
            }
            run.request_save();
        }
    }
}

/// Selection states, icons, slider positions.
#[allow(clippy::too_many_arguments)]
fn bind(
    editor: Res<AnthemEditor>,
    run: Res<RunProgress>,
    originals: Res<crate::Originals>,
    mut assets: ResMut<SpuiAssets>,
    mut images: ResMut<Assets<Image>>,
    mut roots: Query<&mut SpuiState, With<AnthemRoot>>,
) {
    if !editor.open {
        return;
    }
    let Some(anthem) = run.civ_campaign.anthem.as_ref() else {
        return;
    };
    let index = &originals.resource_index;
    for mut ui in &mut roots {
        // The root is authored hidden (the retail code fades it in).
        for r in ui.layout.roots.clone() {
            ui.set_visible(r, true);
        }
        let windows: Vec<usize> = (0..ui.layout.windows.len()).collect();
        for w in windows {
            let win = ui.layout.window(w);
            let command = win.command_id;
            let control = win.control_id;
            let group = match command {
                id::CMD_BEAT => 0,
                id::CMD_INSTRUMENT => 1,
                id::CMD_AMBIENCE => 2,
                _ => continue,
            };
            let on = match group {
                0 => anthem.rhythm == control,
                1 => anthem.instrument == control,
                _ => anthem.ambience.contains(&control),
            };
            ui.set_selected(w, on);
            // The icon of the button (`BeatIcons` / `InstrumentIcons` /
            // `AmbienceIcons`, 1-based ids).
            let icon = editor.icons.as_ref().and_then(|l| l[group].get(control.wrapping_sub(1) as usize).copied());
            let child = ui.layout.window(w).children.iter().copied().find(|c| ui.layout.window(*c).control_id == id::ICON);
            if let (Some(icon), Some(child)) = (icon, child) {
                let key = crate::dbpf::ResourceKey { type_id: PNG, group_id: ICON_GROUP, instance_id: icon };
                if assets.texture(index, &mut images, key).is_some() {
                    ui.set_icon(child, Some(ImageRef { file: key, uv: [0.0, 0.0, 1.0, 1.0], dims: None }));
                }
            }
        }
        if let Some(w) = ui.find(id::AUTO_PLAY) {
            ui.set_selected(w, anthem.looped);
        }
        for (control, gain) in [(id::BEAT_VOLUME, anthem.gains[1]), (id::ANTHEM_VOLUME, anthem.gains[2]), (id::AMBIENCE_VOLUME, anthem.gains[0])] {
            if let Some(w) = ui.find(control) {
                // 0xE9FF00: gain = fraction^2.
                ui.set_range(w, Some(gain.max(0.0).sqrt().min(1.0)));
            }
        }
        for (control, on) in [(id::UNDO, !editor.undo.is_empty()), (id::REDO, !editor.redo.is_empty())] {
            if let Some(w) = ui.find(control) {
                ui.set_enabled(w, on);
            }
        }
    }
}

/// The melody area: one bar per note over the Play anthem button (ADAPTED).
fn melody_bars(
    mut commands: Commands,
    editor: Res<AnthemEditor>,
    run: Res<RunProgress>,
    roots: Query<&SpuiState, With<AnthemRoot>>,
    bars: Query<Entity, With<MelodyBar>>,
    mut last: Local<Option<(CityAnthem, [f32; 4])>>,
) {
    if !editor.open {
        *last = None;
        return;
    }
    let (Some(anthem), Some(ui)) = (run.civ_campaign.anthem.as_ref(), roots.iter().next()) else {
        return;
    };
    let Some(w) = ui.find(id::MELODY) else {
        return;
    };
    let s = ui.rect(w);
    let r = Rect::new(s.x1, s.y1, s.x2, s.y2);
    let rect = [s.x1, s.y1, s.x2, s.y2];
    if last.as_ref().is_some_and(|(a, b)| a == anthem && *b == rect) {
        return;
    }
    *last = Some((anthem.clone(), rect));
    for e in &bars {
        commands.entity(e).despawn();
    }
    let total: f32 = anthem.durations.iter().sum::<f32>().max(1.0);
    let mut onset = 0.0f32;
    for (pitch, d) in anthem.pitches.iter().zip(&anthem.durations) {
        let x = r.min.x + 6.0 + (r.width() - 12.0) * onset / total;
        let width = ((r.width() - 12.0) * d / total - 2.0).max(3.0);
        let level = ((*pitch - 60) as f32 / 24.0).clamp(0.0, 1.0);
        let y = r.min.y + 6.0 + (r.height() - 18.0) * (1.0 - level);
        commands.spawn((
            StageOwned,
            MelodyBar,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(x),
                top: Val::Px(y),
                width: Val::Px(width),
                height: Val::Px(6.0),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.95, 0.8, 0.25)),
            GlobalZIndex(200),
        ));
        onset += d;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anthem() -> CityAnthem {
        civ_music::random_anthem(&AnthemSetup::retail(), 7)
    }

    #[test]
    fn anthems_round_trip_through_cmp_files_and_sliders_square_their_place() {
        let mut a = anthem();
        a.gains = [0.25, 0.5, 1.0];
        a.looped = true;
        let bytes = encode_cmp_named(&a, "Ode");
        // The retail stream: class id, size of the rest, 8 attributes.
        assert_eq!(&bytes[..4], &CMP_CLASS_ID.to_be_bytes());
        assert_eq!(u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8);
        assert_eq!(u32::from_be_bytes(bytes[8..12].try_into().unwrap()), 8);
        assert_eq!(u32::from_be_bytes(bytes[12..16].try_into().unwrap()), cmp_attr::NAME);
        assert_eq!(cmp_name(&bytes).as_deref(), Some("Ode"));
        let back = decode_cmp(&bytes, [1.0; 3]).unwrap();
        assert_eq!(back.pitches, a.pitches);
        assert_eq!(back.durations, a.durations);
        let layers = |v: &[u32]| v.iter().copied().filter(|x| *x != 0).collect::<Vec<_>>();
        assert_eq!(layers(&back.ambience), layers(&a.ambience), "the ambience history");
        assert_eq!(back.ambience.len(), 4);
        assert_eq!((back.rhythm, back.instrument, back.gains), (a.rhythm, a.instrument, a.gains));
        assert!(!back.looped, "the loop flag is not in the stream");
        assert!(decode_cmp(b"junk", [1.0; 3]).is_none());
        assert!(decode_cmp(&bytes[..bytes.len() - 2], [1.0; 3]).is_none());
        // Unknown attributes are skipped by their size; earlier OpenSpore
        // files still load.
        let mut extra = bytes.clone();
        extra[12..16].copy_from_slice(&0x1234_5678u32.to_be_bytes());
        assert_eq!(decode_cmp(&extra, [1.0; 3]).unwrap().pitches, a.pitches);
        let mut old = b"OSCM".to_vec();
        for w in [1u32, 3, 2, 0, 0, 0, 0] {
            old.extend_from_slice(&w.to_le_bytes());
        }
        old.push(1);
        old.extend_from_slice(&0u32.to_le_bytes());
        let o = decode_cmp(&old, [1.0; 3]).unwrap();
        assert_eq!((o.rhythm, o.instrument, o.looped), (3, 2, true));
        let dir = cmp_dir(std::path::Path::new(r"C:\x\qc_save.json"));
        assert!(dir.ends_with("qc_save-CityMusic"));
        assert_eq!(slider_value(50.0, 0.0, 100.0), 0.25);
        assert_eq!(slider_value(-5.0, 0.0, 100.0), 0.0);
        assert_eq!(slider_gain(id::ANTHEM_VOLUME), Some(2));
    }

    #[test]
    fn drums_and_instruments_toggle_like_the_setters() {
        let mut x = 3;
        assert!(toggle(&mut x, 5));
        assert_eq!(x, 5);
        assert!(!toggle(&mut x, 5), "the same id again clears it");
        assert_eq!(x, 0);
    }

    #[test]
    fn a_fifth_ambience_replaces_the_oldest_layer() {
        let mut a = anthem();
        a.ambience = vec![0; 4];
        let mut order = vec![];
        for id in [3, 9, 12, 1] {
            assert_eq!(add_ambience(&mut a, &mut order, id), None);
        }
        assert_eq!(a.ambience, vec![3, 9, 12, 1]);
        assert_eq!(add_ambience(&mut a, &mut order, 20), Some(3));
        assert_eq!(a.ambience, vec![20, 9, 12, 1]);
        remove_ambience(&mut a, &mut order, 12);
        assert_eq!(a.ambience, vec![20, 9, 0, 1]);
        assert_eq!(add_ambience(&mut a, &mut order, 5), None, "an empty layer is used first");
        assert_eq!(a.ambience, vec![20, 9, 5, 1]);
        assert_eq!(add_ambience(&mut a, &mut order, 7), Some(9));
    }

    #[test]
    fn edits_can_be_undone_and_play_as_the_editor_does() {
        let setup = AnthemSetup::retail();
        let mut editor = AnthemEditor::default();
        let mut a = anthem();
        let first = a.clone();
        let instrument = if a.instrument == 4 { 5 } else { 4 };
        assert_eq!(apply(&mut editor, &setup, &mut a, AnthemEdit::Instrument(instrument), 1), Some(PlayRequest::FromStart));
        let n = a.pitches.len();
        let play = apply(&mut editor, &setup, &mut a, AnthemEdit::AddNote, 2);
        if n < setup.max_notes as usize {
            assert_eq!(play, Some(PlayRequest::Note(n as u32)));
            assert_eq!(a.pitches.len(), n + 1);
            assert!(a.pitches.iter().all(|p| (60..=84).contains(p)));
        }
        apply(&mut editor, &setup, &mut a, AnthemEdit::RemoveNote, 3);
        assert_eq!(a.pitches.len(), n);
        apply(&mut editor, &setup, &mut a, AnthemEdit::NewAnthem, 4);
        assert_eq!(a.instrument, instrument, "a new melody keeps the instrument");
        // Undo walks back to the first anthem, redo forward again.
        while apply(&mut editor, &setup, &mut a, AnthemEdit::Undo, 0).is_some() {}
        assert_eq!(a, first);
        apply(&mut editor, &setup, &mut a, AnthemEdit::Redo, 0);
        assert_eq!(a.instrument, instrument);
        // Play changes nothing (nothing to undo).
        let undo = editor.undo.len();
        assert_eq!(apply(&mut editor, &setup, &mut a, AnthemEdit::Play, 0), Some(PlayRequest::FromStart));
        assert_eq!(editor.undo.len(), undo);
        for k in 0..30 {
            apply(&mut editor, &setup, &mut a, AnthemEdit::Beat(1 + k % 10), 0);
        }
        assert!(editor.undo.len() <= MAX_UNDOS);
        // The diff sends only what changed.
        let mut b = a.clone();
        b.rhythm = if a.rhythm == 2 { 3 } else { 2 };
        let diff = civ_music::anthem_diff(&a, &b);
        assert!(diff.iter().all(|(n, _)| n.starts_with("city_music_rhythm")), "{diff:?}");
    }
}
