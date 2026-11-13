//! Cell creature voices (helper `cell-voices`; findings in `cell-voices\CHANGES.md`, the Cell
//! counterpart of `voice_banks.rs`).
//!
//! RETAIL (code image march2017 addresses + installed data):
//! * The Cell clips carry their voice events under base names (`vcl_idle`, `vcl_mov_flg`,
//!   `vcl_npc_mad`, ...). The animation event dispatcher (0x9A6D70, case `sound`) hands every
//!   event to the anim sound player 0x9A4FA0, whose branch for names starting with `v` (or events
//!   with the voice flag 0x400) sends the name and the voice parameters of the animated creature
//!   to the audio system: `0x039FB001` = the creature's voice id (`anim_cid + 0x2D8`, set by the
//!   virtual setter 0xA04C20) and `0x039FB000` = `anim_cid + 0x2DC` (setter 0xA04C40), which the
//!   Cell GFX update (0xE62B80, call at 0xE62C88) refreshes every frame with the cell's
//!   animation state (`+0x18C`, the retail request code) and the creation code (0xE4EF90) starts
//!   at 1.
//! * The voice id of a Cell creature is the instance id of its **structure** (0xE5D018 / 0xE5D20E
//!   push `[structure reference]`); the player's creature is the constant `FNV("cell_hero_L0")`
//!   (0x5837CB). The audio system's voice name modifier (vtable +0xC of the class at 0xA3B350)
//!   looks the id up in a map built at start-up (0xA3B840 / 0xA3B6D0) from the PROP-format
//!   record(s) of group `0x40C0C100` (one installed soundProp-typed record `base`: property id =
//!   FNV of a structure name or of a three-letter mouth code, value = string) and appends
//!   `_` + the string with 0xA3A4D0: `vcl_idle` + `_ch` = `vcl_idle_ch`. The sound is played when
//!   that soundProp exists (0xA393B4 `HasSound`); a name that does not exist stays silent. The
//!   table (84 entries) maps the structures of the Cell species to the two-letter codes of the
//!   255 installed `vcl_*` soundProps (`al av bz ch dk ed ek em go jt jw mm no pf pk po ps sg sh
//!   sq st`); several structures share a code (`struct_goldy*` are `ek`, `struct_squiggly` and
//!   `struct_chomperSmall` are `sh`, `struct_puffish` is `ps`). Structures without an entry
//!   (`player_cell`, `cell_mate`, hummingbird, octamouth, creepers) have no voice.
//! * The soundProps' `primitives` bind `mindistance` / `maxdistance` to input `0x039FB000`
//!   through the named curves of the base (`smallcurve`, `largecurve`), and the curve slots
//!   `0xBC2614D9` / `0xF70E571C` drive the track's `pitch` (0x71BC3009) / `gain` (0x25DF0108)
//!   from the same input (0xA41D44, only while the input exists, i.e. while the cell's state
//!   is not 0). Every Cell state but idle is above the last curve point (5), so an NPC that
//!   idles uses the soundProp's static distances / pitch / gain, and one that acts the last
//!   point of every curve.
//!
//! VERIFIED (helper `cell-audio3`): the curve evaluator 0xA1A090 is piecewise linear with the end
//! values held (a table of `[x, y, x_prev, y_prev, slope]` per point built at 0xA1A572; binary search
//! for the first point with `x_i > x`; see `cell_acoustics`), which is what [`eval_curve`] does. The
//! voices are positional Cell voices (`cell_spatial`, helper `cell-audio4`): the track's roll-off,
//! `atmospheric` low-pass, distance high-pass, Pan2D1 gains and the `wetlevelcurve` reverb send (the
//! voices' `vo_base` carries it) follow the creature every frame; the `Play` this module builds carries
//! no `anchor` itself, `cell_audio` sets it.
//!
//! VERIFIED (helper `cell-audio5`): the state input is **per voice event, not per frame**. The only
//! sender of `0x039FB000` is the voice branch of the anim sound player (0x9A5520, in the message with
//! the sound's name, handle and position; the Cell GFX refreshes `+0x2DC` every frame, but only the
//! next event reads it); the only reader is the track update 0xA41D00, which every update evaluates
//! the `pitch` / `gain` curves (slots +0x24D8 / +0x283C, flags +0xCE4 bits 0x100000 / 0x200000) at
//! that stored value and sets `pitch` (0x71BC3009) / `gain` (0x25DF0108); the `primitives` bind
//! `mindistance` / `maxdistance` to the same input. The curve values are therefore constant for the
//! life of a voice (OpenSpore evaluates them once, from the animation state the event fired in), while
//! the roll-off (0xA41C10) is evaluated on every update against the changing distance with those
//! distances: [`Play::range`] carries them to the positional voice (`cell_spatial`), which
//! re-evaluates the roll-off every frame (before this round the start distance was baked into the
//! gain and the voice then rolled off a second time with the soundProp's own distances).
//!
//! ADAPTED: the curve gain / pitch replace the soundProp's own values (as the retail parameter set
//! does) by scaling the played sound's own gain / pitch; a voice event of a creature without a voice
//! id is silent (the retail branch returns before it sends anything); a creature whose id is not in
//! the table (`player_cell`, creepers, hummingbird ...) plays the plain event name when that is
//! installed.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use bevy::prelude::*;

use crate::{
    asset_catalog::ResourceIndex,
    cell_audio::Play,
    cell_data::{AttachmentType, CellDatabase},
    dbpf::ResourceKey,
    game_audio::{AudioLibrary, Bus, SoundDef, AUDIO_GROUP, TYPE_SNR, TYPE_SOUND_PROP},
    prop::{Property, PropertyList},
    world_objects::name_id,
};

/// Group of the voice id table (`base`, a PROP-format record of soundProp type).
pub const VOICE_TABLE_GROUP: u32 = 0x40C0_C100;
/// The voice id of the player's creature (0x5837CB pushes the hash of this name).
pub const AVATAR_VOICE: &str = "cell_hero_L0";
/// Audio parameter `0x039FB000`: what the voice curves are keyed by.
const INPUT_STATE: u32 = 0x039F_B000;
const P_PARENT: u32 = 0x5F63_17D5;
const P_PRIMITIVES: u32 = 0x3DA0_A727;
const P_MIN_DISTANCE: u32 = 0x0F61_6B72;
const P_MAX_DISTANCE: u32 = 0x0359_3710;
/// Curve slots the sound update evaluates at the input (0xA41D44).
const P_PITCH_CURVE: u32 = 0xBC26_14D9;
const P_GAIN_CURVE: u32 = 0xF70E_571C;

/// The voice id -> code table: property id (FNV of a structure name or a mouth code) -> string.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoiceTable {
    codes: BTreeMap<u32, String>,
}

impl VoiceTable {
    /// Merge the string properties of PROP-format records (retail 0xA3B6D0: every property of
    /// every record of the group; a later record replaces an earlier id).
    pub fn from_lists<'a>(lists: impl IntoIterator<Item = &'a PropertyList>) -> Self {
        let mut codes = BTreeMap::new();
        for list in lists {
            for (id, property) in &list.0 {
                // Kind 0x12 is an ASCII string.
                if property.kind != 0x12 {
                    continue;
                }
                let Some(value) = property.values.first() else { continue };
                let text = String::from_utf8_lossy(value);
                let text = text.trim_end_matches('\0').to_string();
                if !text.is_empty() {
                    codes.insert(*id, text);
                }
            }
        }
        Self { codes }
    }

    /// Read the table from the installed records of [`VOICE_TABLE_GROUP`].
    pub fn load(index: &ResourceIndex) -> Self {
        let mut keys: Vec<ResourceKey> = index.records.keys().filter(|k| k.group_id == VOICE_TABLE_GROUP).copied().collect();
        keys.sort_by_key(|k| (k.type_id, k.instance_id));
        let lists: Vec<PropertyList> = keys
            .into_iter()
            .filter_map(|k| index.read(k).ok().and_then(|b| crate::prop::parse(&b).ok()))
            .collect();
        Self::from_lists(&lists)
    }

    /// The suffix code of a voice id (`None`: the creature has no voice).
    pub fn code(&self, voice_id: u32) -> Option<&str> {
        self.codes.get(&voice_id).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.codes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    /// Every distinct code of the table.
    pub fn distinct_codes(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.codes.values().map(String::as_str).collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// The sound a voice event resolves to (0xA3A4D0 with the underscore flag): `<event>_<code>`.
pub fn voice_name(event: &str, code: &str) -> String {
    format!("{event}_{code}")
}

/// The voice id of a Cell creature: the avatar's constant, else the instance id of its structure.
pub fn voice_id(is_player: bool, structure: Option<u32>) -> Option<u32> {
    if is_player {
        Some(name_id(AVATAR_VOICE))
    } else {
        structure.filter(|s| *s != 0)
    }
}

/// The voice id a cell record gets from its structure: creatures (structures with a creature
/// attachment, the cells whose GFX is an animated creature, 0xE5D1EE) carry the structure's id as
/// their voice id, rocks, food and bubbles carry none (their `v...` events return early).
pub fn structure_voice(db: &CellDatabase, cell: u32) -> Option<u32> {
    let record = db.cell(cell)?;
    let mut stack = vec![record.structure];
    let mut seen = 0;
    while let Some(id) = stack.pop() {
        seen += 1;
        if seen > 8 {
            break;
        }
        let Some(structure) = db.structures.get(&id) else { continue };
        for a in &structure.attachments {
            match a.kind {
                AttachmentType::Structure => stack.push(a.structure),
                AttachmentType::Creature | AttachmentType::RandomCreature | AttachmentType::PlayerCreature => {
                    return Some(record.structure);
                }
                _ => {}
            }
        }
    }
    None
}

/// Piecewise linear curve through `(x, y)` points sorted by x, the end values held (VERIFIED: the
/// evaluator 0xA1A090).
pub fn eval_curve(points: &[(f32, f32)], x: f32) -> f32 {
    let Some(&(x0, y0)) = points.first() else { return 0.0 };
    if x <= x0 {
        return y0;
    }
    for pair in points.windows(2) {
        let ((xa, ya), (xb, yb)) = (pair[0], pair[1]);
        if x <= xb {
            return if xb > xa { ya + (yb - ya) * (x - xa) / (xb - xa) } else { yb };
        }
    }
    points.last().map_or(0.0, |p| p.1)
}

/// The values a voice plays with at one state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceParams {
    pub min_distance: f32,
    pub max_distance: f32,
    pub pitch: f32,
    pub gain: f32,
}

/// The state-keyed curves of a voice soundProp (nearest definer in the parent chain wins).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoiceCurves {
    pub min_distance: Vec<(f32, f32)>,
    pub max_distance: Vec<(f32, f32)>,
    pub pitch: Vec<(f32, f32)>,
    pub gain: Vec<(f32, f32)>,
}

fn floats(property: &Property) -> Vec<(f32, f32)> {
    let v: Vec<f32> = property
        .values
        .iter()
        .filter_map(|b| Some(f32::from_be_bytes(b.get(..4)?.try_into().ok()?)))
        .collect();
    v.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

impl VoiceCurves {
    /// From the property lists of a soundProp and its ancestors, child first.
    pub fn from_chain(chain: &[PropertyList]) -> Self {
        let find = |id: u32| chain.iter().find_map(|l| l.0.get(&id));
        // `primitives`: (target property, input, curve property) triples.
        let mut min_curve = None;
        let mut max_curve = None;
        if let Some(primitives) = find(P_PRIMITIVES) {
            let words: Vec<u32> = primitives
                .values
                .iter()
                .filter_map(|b| Some(u32::from_be_bytes(b.get(..4)?.try_into().ok()?)))
                .collect();
            for triple in words.chunks_exact(3) {
                if triple[1] != INPUT_STATE {
                    continue;
                }
                match triple[0] {
                    P_MIN_DISTANCE => min_curve = Some(triple[2]),
                    P_MAX_DISTANCE => max_curve = Some(triple[2]),
                    _ => {}
                }
            }
        }
        let curve = |id: Option<u32>| id.and_then(find).map(floats).unwrap_or_default();
        Self {
            min_distance: curve(min_curve),
            max_distance: curve(max_curve),
            pitch: find(P_PITCH_CURVE).map(floats).unwrap_or_default(),
            gain: find(P_GAIN_CURVE).map(floats).unwrap_or_default(),
        }
    }

    /// Read the chain of a soundProp from the resource index.
    pub fn read(index: &ResourceIndex, name: &str) -> Self {
        let mut chain: Vec<PropertyList> = vec![];
        let mut current = name_id(name);
        while chain.len() < 16 {
            let key = ResourceKey { type_id: TYPE_SOUND_PROP, group_id: AUDIO_GROUP, instance_id: current };
            let Some(list) = index.read(key).ok().and_then(|b| crate::prop::parse(&b).ok()) else { break };
            let parent = list.0.get(&P_PARENT).and_then(Property::key).map(|k| k.instance_id).filter(|p| *p != 0);
            chain.push(list);
            match parent {
                Some(p) if p != current => current = p,
                _ => break,
            }
        }
        Self::from_chain(&chain)
    }

    /// The parameters at animation state `state`. The input only exists while the state is not
    /// 0 (0x9A54EC: `+0x2DC` is sent when non-zero); without it the soundProp's own values play.
    pub fn at(&self, state: i32, def: &SoundDef) -> VoiceParams {
        let mut p = VoiceParams { min_distance: def.min_distance, max_distance: def.max_distance, pitch: def.pitch, gain: def.gain };
        if state == 0 {
            return p;
        }
        let x = state as f32;
        if !self.min_distance.is_empty() {
            p.min_distance = eval_curve(&self.min_distance, x).max(0.0);
        }
        if !self.max_distance.is_empty() {
            p.max_distance = eval_curve(&self.max_distance, x).max(0.0);
        }
        if !self.pitch.is_empty() {
            p.pitch = eval_curve(&self.pitch, x).clamp(0.1, 4.0);
        }
        if !self.gain.is_empty() {
            p.gain = eval_curve(&self.gain, x).clamp(0.0, 4.0);
        }
        p
    }
}

/// Whether the library has a playable record named `name`.
fn installed(library: &AudioLibrary, name: &str) -> bool {
    let id = name_id(name);
    library.contains(TYPE_SOUND_PROP, id) || library.contains(TYPE_SNR, id)
}

/// Whether a clip event is a voice event (0x9A4FA0: the name starts with `v`).
pub fn is_voice_event(name: &str) -> bool {
    name.starts_with('v')
}

struct VoiceDef {
    def: SoundDef,
    curves: VoiceCurves,
}

/// The Cell voices: the id table, the resolved names and the curves of the voice soundProps.
#[derive(Resource, Default)]
pub struct CellVoices {
    table: Option<Arc<VoiceTable>>,
    names: HashMap<(u32, String), Option<(&'static str, bool)>>,
    defs: HashMap<&'static str, Option<Arc<VoiceDef>>>,
    /// Voice events started (tests, debug).
    pub started: u64,
}

impl CellVoices {
    pub fn with_table(table: VoiceTable) -> Self {
        Self { table: Some(Arc::new(table)), ..default() }
    }

    fn table(&mut self, index: &ResourceIndex) -> Arc<VoiceTable> {
        self.table.get_or_insert_with(|| Arc::new(VoiceTable::load(index))).clone()
    }

    /// The sound of a voice event of voice id `voice_id`: `<event>_<code>` when the id has an entry
    /// in the table (silent when that sound is not installed: the modified name stays, 0xA393C9),
    /// else the plain event name when it is installed (nothing modifies it). The flag says
    /// whether the species suffix was applied.
    pub fn resolve(&mut self, index: &ResourceIndex, library: &AudioLibrary, voice_id: u32, event: &str) -> Option<(&'static str, bool)> {
        if let Some(found) = self.names.get(&(voice_id, event.to_string())) {
            return *found;
        }
        let table = self.table(index);
        let found = match table.code(voice_id) {
            Some(code) => Some(voice_name(event, code)).filter(|n| installed(library, n)).map(|n| (crate::anim_audio::intern(n), true)),
            None => installed(library, event).then(|| (crate::anim_audio::intern(event.to_string()), false)),
        };
        self.names.insert((voice_id, event.to_string()), found);
        found
    }

    fn def(&mut self, index: &ResourceIndex, library: &AudioLibrary, name: &'static str) -> Option<Arc<VoiceDef>> {
        self.defs
            .entry(name)
            .or_insert_with(|| {
                let def = library.resolve(name).ok()?;
                Some(Arc::new(VoiceDef { curves: VoiceCurves::read(index, name), def }))
            })
            .clone()
    }

    /// The one-shot a voice event of a creature plays, positioned like the other Cell clip
    /// sounds (`distance` in retail units from the listener). `None`: silent (no voice id, or
    /// the sound of its species is not installed).
    #[allow(clippy::too_many_arguments)]
    pub fn play(
        &mut self,
        index: &ResourceIndex,
        library: &AudioLibrary,
        voice_id: Option<u32>,
        event: &str,
        state: i32,
        distance: Option<f32>,
    ) -> Option<Play> {
        let (name, suffixed) = self.resolve(index, library, voice_id?, event)?;
        if !suffixed {
            // No table entry: the event's own sound plays as any clip sound does.
            let mut play = Play::named(name, "cell voice");
            play.distance = distance;
            self.started += 1;
            return Some(play);
        }
        let voice = self.def(index, library, name)?;
        let p = voice.curves.at(state, &voice.def);
        let mut play = Play::named(name, "cell voice");
        // `vo_base` puts the voices in `vo_group`: the Settings > Sound "Voices" slider.
        play.bus = Bus::Voice;
        // `play_sounds` scales by the sound's own gain / pitch: these ratios make the curve
        // values replace them.
        play.pitch = p.pitch / voice.def.pitch.max(1e-3);
        play.gain = p.gain / voice.def.gain.max(1e-3);
        // The roll-off against the distance (re-evaluated on every frame by the positional
        // voice) uses the distances the state curves gave.
        play.distance = distance;
        play.range = Some((p.min_distance, p.max_distance));
        self.started += 1;
        Some(play)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_property(text: &str) -> Property {
        Property { kind: 0x12, array: false, values: vec![text.as_bytes().to_vec()] }
    }

    fn float_array(values: &[f32]) -> Property {
        Property { kind: 0x0D, array: true, values: values.iter().map(|v| v.to_be_bytes().to_vec()).collect() }
    }

    fn word_array(values: &[u32]) -> Property {
        Property { kind: 0x0A, array: true, values: values.iter().map(|v| v.to_be_bytes().to_vec()).collect() }
    }

    fn list(items: Vec<(u32, Property)>) -> PropertyList {
        PropertyList(items.into_iter().collect())
    }

    #[test]
    fn the_table_maps_voice_ids_to_codes() {
        let table = VoiceTable::from_lists(&[list(vec![
            (name_id("struct_chomper"), string_property("ch")),
            (name_id("cell_hero_L0"), string_property("av")),
            (name_id("junk"), Property { kind: 0x0D, array: false, values: vec![vec![0; 4]] }),
        ])]);
        assert_eq!(table.len(), 2);
        assert_eq!(table.code(name_id("struct_chomper")), Some("ch"));
        assert_eq!(table.code(name_id("cell_hero_L0")), Some("av"));
        assert_eq!(table.code(name_id("struct_creeper")), None);
        assert_eq!(table.distinct_codes(), ["av", "ch"]);
    }

    #[test]
    fn a_voice_event_gets_its_species_suffix() {
        assert_eq!(voice_name("vcl_idle", "ch"), "vcl_idle_ch");
        assert_eq!(voice_name("vcl_idle_blinka", "pk"), "vcl_idle_blinka_pk");
        assert!(is_voice_event("vcl_mov_flg") && is_voice_event("vfx_cell_move_jet"));
        assert!(!is_voice_event("sfx_cell_eat"));
    }

    #[test]
    fn the_player_has_the_avatar_voice_and_others_their_structure() {
        assert_eq!(voice_id(true, Some(5)), Some(0xCBCD_1287));
        assert_eq!(voice_id(false, Some(5)), Some(5));
        assert_eq!(voice_id(false, Some(0)), None);
        assert_eq!(voice_id(false, None), None);
    }

    #[test]
    fn curves_interpolate_and_hold_their_ends() {
        let c = [(1.0, 2.0), (2.0, 4.0), (5.0, 10.0)];
        assert_eq!(eval_curve(&c, 0.0), 2.0);
        assert_eq!(eval_curve(&c, 1.5), 3.0);
        assert_eq!(eval_curve(&c, 3.5), 7.0);
        assert_eq!(eval_curve(&c, 18.0), 10.0);
        assert_eq!(eval_curve(&[], 3.0), 0.0);
    }

    #[test]
    fn the_curves_come_from_the_nearest_definer() {
        let base = list(vec![
            (P_PRIMITIVES, word_array(&[P_MIN_DISTANCE, INPUT_STATE, 0xAAAA, P_MAX_DISTANCE, INPUT_STATE, 0xBBBB])),
            (0xAAAA, float_array(&[1.0, 2.0, 5.0, 15.0])),
            (0xBBBB, float_array(&[1.0, 7.0, 5.0, 20.0])),
            (P_PITCH_CURVE, float_array(&[1.0, 1.6, 5.0, 0.6])),
            (P_GAIN_CURVE, float_array(&[1.0, 0.1, 5.0, 1.0])),
        ]);
        let child = list(vec![(P_PITCH_CURVE, float_array(&[1.0, 3.0, 5.0, 0.9]))]);
        let curves = VoiceCurves::from_chain(&[child, base]);
        assert_eq!(curves.pitch, [(1.0, 3.0), (5.0, 0.9)], "the child's curve wins");
        assert_eq!(curves.gain, [(1.0, 0.1), (5.0, 1.0)]);
        assert_eq!(curves.min_distance, [(1.0, 2.0), (5.0, 15.0)]);
        assert_eq!(curves.max_distance, [(1.0, 7.0), (5.0, 20.0)]);
        // A primitive on another input is not a state curve.
        let other = list(vec![
            (P_PRIMITIVES, word_array(&[P_MIN_DISTANCE, 0x1234, 0xAAAA])),
            (0xAAAA, float_array(&[1.0, 2.0, 5.0, 15.0])),
        ]);
        assert!(VoiceCurves::from_chain(&[other]).min_distance.is_empty());
    }

    fn def(min: f32, max: f32) -> SoundDef {
        SoundDef {
            name: "vcl_idle_pk".into(),
            samples: vec![1],
            gain: 0.85,
            looped: false,
            pitch: 1.0,
            fade_in: 0.0,
            fade_out: 0.0,
            inheritance: 3,
            is_3d: true,
            min_distance: min,
            max_distance: max,
            polyphony: 0,
            start_delay: 0.0,
            curve: vec![],
            silent: false,
            groups: vec![],
        }
    }

    #[test]
    fn idle_plays_with_the_soundprop_values_and_every_other_state_with_the_last_curve_point() {
        let curves = VoiceCurves {
            min_distance: vec![(1.0, 2.0), (5.0, 15.0)],
            max_distance: vec![(1.0, 7.0), (5.0, 20.0)],
            pitch: vec![(1.0, 1.6), (5.0, 0.6)],
            gain: vec![(1.0, 0.1), (5.0, 1.0)],
        };
        let d = def(25.0, 35.0);
        let idle = curves.at(0, &d);
        assert_eq!(idle, VoiceParams { min_distance: 25.0, max_distance: 35.0, pitch: 1.0, gain: 0.85 });
        // Request codes 18 (moving), 22 (mad), 10 (dying) are all past the 5th point.
        for state in [10, 18, 22, 67] {
            let p = curves.at(state, &d);
            assert_eq!(p, VoiceParams { min_distance: 15.0, max_distance: 20.0, pitch: 0.6, gain: 1.0 }, "state {state}");
        }
        // A voice without curves keeps its own values in every state.
        assert_eq!(VoiceCurves::default().at(18, &d), idle);
    }

    #[test]
    fn the_roll_off_uses_the_distances_of_the_state_curves() {
        let d = def(25.0, 35.0);
        // Idle: the soundProp's 25 .. 35; acting: the last curve points 15 .. 20.
        let own = Some((15.0, 20.0));
        let (static_level, state_level) = (crate::cell_acoustics::roll_off(&d, None, 18.0), crate::cell_acoustics::roll_off(&d, own, 18.0));
        assert_eq!(static_level, 1.0, "inside the soundProp's own min distance");
        assert!((state_level - 0.4).abs() < 1e-6, "{state_level}: 3 of 5 units into 15 .. 20");
        assert_eq!(crate::cell_acoustics::roll_off(&d, own, 21.0), 0.0);
        assert_eq!(crate::cell_acoustics::roll_off(&d, own, 10.0), 1.0);
        // A flat sound or an empty range is never attenuated.
        assert_eq!(crate::cell_acoustics::roll_off(&d, Some((20.0, 20.0)), 99.0), 1.0);
        let flat = SoundDef { is_3d: false, ..d };
        assert_eq!(crate::cell_acoustics::roll_off(&flat, own, 99.0), 1.0);
    }

    #[test]
    fn a_voice_is_silent_without_an_id_or_without_an_installed_sound() {
        let mut voices = CellVoices::with_table(VoiceTable::from_lists(&[list(vec![(7, string_property("ch"))])]));
        let index = ResourceIndex { records: Default::default(), errors: vec![] };
        let library = AudioLibrary::default();
        assert!(voices.play(&index, &library, None, "vcl_idle", 0, Some(1.0)).is_none());
        assert!(voices.play(&index, &library, Some(8), "vcl_idle", 0, Some(1.0)).is_none(), "no table entry");
        assert!(voices.play(&index, &library, Some(7), "vcl_idle", 0, Some(1.0)).is_none(), "nothing installed");
        assert_eq!(voices.started, 0);
    }

    // -----------------------------------------------------------------------
    // Installed data
    // -----------------------------------------------------------------------

    /// Every `vcl_*` event the installed clips carry, with which of the table's codes make it a
    /// real sound; before = the plain event name (what OpenSpore played until now).
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_cell_voice_census() {
        use crate::anim_audio::{clip_cues, Cue};
        let install = crate::spore_install::discover().expect("Spore install");
        let index = ResourceIndex::load(&install.packages);
        let library = AudioLibrary::from_index(&index);
        let table = VoiceTable::load(&index);
        let db = crate::cell_data::CellDatabase::load(&index);
        println!("TABLE|{} entries|{} codes", table.len(), table.distinct_codes().len());
        // The codes the Cell structures (and the avatar) use.
        let mut cell_codes: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (id, structure) in &db.structures {
            let _ = structure;
            if let Some(code) = table.code(*id) {
                cell_codes.entry(code.to_string()).or_default().push(format!("{id:08X}"));
            }
        }
        if let Some(code) = table.code(name_id(AVATAR_VOICE)) {
            cell_codes.entry(code.to_string()).or_default().push("avatar".into());
        }
        println!("CELLCODES|{:?}", cell_codes.keys().collect::<Vec<_>>());
        // The v-events of the Cell clips (their names start with vcl_ / vfx_ / vgen_ / vEpic).
        let keys: Vec<_> = index.records.keys().filter(|k| k.type_id == crate::decode::TYPE_ANIM).copied().collect();
        let mut events: BTreeMap<String, usize> = BTreeMap::new();
        for key in keys {
            let Ok(anim) = index.read(key).map_err(|e| e.to_string()).and_then(|b| crate::animation::parse(&b)) else { continue };
            for c in clip_cues(&anim) {
                if let Cue::Sound { name, .. } = c.cue {
                    if name.starts_with("vcl_") || name.starts_with("vfx_") || name.starts_with("vgen_") || name.starts_with("vEpic") {
                        *events.entry(name).or_default() += 1;
                    }
                }
            }
        }
        let (mut plain_before, mut pairs_after, mut events_resolving) = (0, 0, 0);
        for (name, clips) in &events {
            let plain = installed(&library, name);
            let codes: Vec<&String> = cell_codes.keys().filter(|c| installed(&library, &voice_name(name, c))).collect();
            if plain {
                plain_before += 1;
            }
            if !codes.is_empty() {
                events_resolving += 1;
            }
            pairs_after += codes.len();
            println!("EVENT|{name}|clips={clips}|plain={plain}|codes={}|{codes:?}", codes.len());
        }
        println!("SUMMARY|events={}|resolving_before={plain_before}|resolving_after={events_resolving}|species_pairs_after={pairs_after}", events.len());
    }

    /// Every installed `vcl_*` code is reachable: each code of the table that belongs to a Cell
    /// structure (or the avatar) has installed voice sounds, and the codes of the installed
    /// sounds are all in the table.
    #[test]
    #[ignore = "needs the installed Spore packages"]
    fn installed_cell_voice_codes_all_resolve() {
        let install = crate::spore_install::discover().expect("Spore install");
        let index = ResourceIndex::load(&install.packages);
        let library = AudioLibrary::from_index(&index);
        let table = VoiceTable::load(&index);
        assert!(table.len() > 60, "{} entries", table.len());
        assert_eq!(table.code(name_id("cell_hero_L0")), Some("av"));
        assert_eq!(table.code(name_id("struct_chomper")), Some("ch"));
        assert_eq!(table.code(name_id("struct_pinky")), Some("pk"));
        assert_eq!(table.code(name_id("struct_squiggly")), Some("sh"));
        // The events every species code is checked against.
        let events = [
            "vcl_idle", "vcl_idle_blinka", "vcl_idle_blink1", "vcl_mad", "vcl_npc_mad", "vcl_npc_scared", "vcl_move_npc_scared",
            "vcl_eat_mand_npc", "vcl_eat_mand_chew", "vcl_eat_filter_npc", "vcl_eat_prob_stun", "vcl_takedmg_stab_npc",
            "vcl_takedmg_poison_npc", "vcl_takedmg_elec_npc", "vcl_hatch", "vcl_mov_flg_big_mad", "vcl_npc_mad_big",
            "vcl_eat_filter_chew", "vcl_death_avatar", "vcl_eat_prob_retractTrgt",
        ];
        // The two-letter codes of the installed soundProps.
        let all_codes = [
            "al", "av", "bz", "ch", "dk", "ed", "ek", "em", "go", "jt", "jw", "mm", "no", "pf", "pk", "po", "ps", "sg", "sh", "sq", "st",
        ];
        let used: Vec<&str> = table.distinct_codes();
        for code in all_codes {
            let sounds = events.iter().filter(|e| installed(&library, &voice_name(e, code))).count();
            assert!(sounds > 0, "code {code} has no installed voice sound");
            // Codes the retail table never points a Cell structure at stay unused (`sg`, `pf`).
            println!("CODE|{code}|used_by_table={}|sounds_of_the_sample_events={sounds}", used.contains(&code));
        }
        // Every structure of a Cell species with an entry resolves at least one idle-class sound.
        let db = crate::cell_data::CellDatabase::load(&index);
        let mut with_voice = 0;
        for (id, _) in &db.structures {
            let Some(code) = table.code(*id) else { continue };
            with_voice += 1;
            let any = events.iter().any(|e| installed(&library, &voice_name(e, code)));
            assert!(any, "structure {id:08X} code {code}");
        }
        println!("STRUCTURES|{with_voice} of {} have a voice", db.structures.len());
        assert!(with_voice >= 30, "{with_voice}");
        // Cells to voice ids: creatures carry their structure's id, food / bubbles none, the
        // own-species copy has an id without a table entry.
        let of = |cell: &str| structure_voice(&db, name_id(cell));
        assert_eq!(of("cell_chomper").and_then(|v| table.code(v)), Some("ch"));
        assert_eq!(of("Cell_ChomperSmall").and_then(|v| table.code(v)), Some("sh"));
        assert_eq!(of("cell_pinky").and_then(|v| table.code(v)), Some("pk"));
        assert_eq!(of("cell_hero_L0").and_then(|v| table.code(v)), Some("av"));
        assert_eq!(of("cell_food_L1"), None);
        assert_eq!(of("cell_bubble_L0"), None);
        assert!(of("player_cell").is_some_and(|v| table.code(v).is_none()));
        // A chomper's idle: the soundProp's own values at state 0, the last curve point once the
        // cell acts (the chomper's pitch curve ends at 0.9).
        let mut voices = CellVoices::with_table(table.clone());
        let chomper = of("cell_chomper");
        let idle = voices.play(&index, &library, chomper, "vcl_idle", 0, Some(10.0)).expect("vcl_idle_ch");
        assert_eq!(idle.name, Some("vcl_idle_ch"));
        assert_eq!(idle.bus, Bus::Voice);
        assert!(idle.distance == Some(10.0) && idle.gain > 0.0 && (idle.pitch - 1.0).abs() < 1e-3, "{idle:?}");
        let mad = voices.play(&index, &library, chomper, "vcl_idle", 22, Some(10.0)).unwrap();
        assert!((mad.pitch - 0.9).abs() < 1e-3, "{mad:?}");
        // The roll-off uses the state's distances (large base: 25..30 idle, 15..25 acting) and is
        // evaluated by the voice against the distance: beyond max distance it fades out.
        let far = voices.play(&index, &library, chomper, "vcl_idle", 22, Some(40.0)).unwrap();
        assert_eq!(far.range, mad.range);
        assert_eq!(crate::cell_acoustics::roll_off(&library.resolve("vcl_idle_ch").unwrap(), far.range, 40.0), 0.0);
        let acting = far.range.unwrap();
        let def = library.resolve("vcl_idle_ch").unwrap();
        assert_eq!(acting, (15.0, 25.0), "the last curve point of the large base");
        assert!(crate::cell_acoustics::roll_off(&def, Some(acting), 20.0) < crate::cell_acoustics::roll_off(&def, None, 20.0));
        // The avatar keeps the static roll-off of `_cell_vox_avatar` (15..30).
        let avatar = voices.play(&index, &library, voice_id(true, None), "vcl_death_avatar", 10, Some(10.0)).unwrap();
        assert_eq!(avatar.name, Some("vcl_death_avatar_av"));
        assert!(avatar.gain > 0.9 && (avatar.pitch - 1.0).abs() < 1e-3, "{avatar:?}");
        assert_eq!(avatar.range, Some((15.0, 30.0)));
        let avatar_def = library.resolve("vcl_death_avatar_av").unwrap();
        assert_eq!(crate::cell_acoustics::roll_off(&avatar_def, avatar.range, 10.0), 1.0);
        assert_eq!(crate::cell_acoustics::roll_off(&avatar_def, avatar.range, 31.0), 0.0);
        // A creature without a table entry plays the plain name only when that is installed.
        assert!(voices.play(&index, &library, of("player_cell"), "vcl_idle", 0, Some(1.0)).is_none());
        // A species without the variant stays silent.
        assert!(voices.play(&index, &library, chomper, "vcl_hatch", 0, Some(1.0)).is_none(), "no vcl_hatch_ch");
        // The curves of a small and a large voice read from the soundProp chains.
        let pinky = VoiceCurves::read(&index, "vcl_idle_pk");
        assert_eq!(pinky.min_distance.first(), Some(&(1.0, 2.0)), "{pinky:?}");
        assert_eq!(pinky.gain.last(), Some(&(5.0, 1.0)));
        let chomper = VoiceCurves::read(&index, "vcl_idle_ch");
        assert_eq!(chomper.pitch.first(), Some(&(1.0, 3.0)), "the chomper overrides the pitch curve: {chomper:?}");
        assert_eq!(chomper.max_distance.last(), Some(&(5.0, 25.0)), "{chomper:?}");
        // The avatar's voice has no state curves, only static distances.
        assert_eq!(VoiceCurves::read(&index, "vcl_death_avatar_av"), VoiceCurves::default());
    }
}
