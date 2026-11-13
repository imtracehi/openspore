//! Installed data the Pd engine needs: patches (`pd`), bundles (`pdr`),
//! mix modes (`mode`), soundProps (patch soundProps, mixer groups) and note
//! instruments (`instrument`).
//!
//! RETAIL (`notes\pd-music-re-notes.md` 1.1-1.3, VERIFIED resource keys):
//! * a sound whose soundProp `samples` (or `patch` / `sample`) key names a
//!   `pdr` bundle runs that bundle as a Pd instance (`cellgame_seed_music` ->
//!   `cellgame_seed_music6`); a soundProp without one but with a same-named
//!   bundle also does (INFERRED name fallback);
//! * `mode` records (PROP): `startsounds` (keys of the sounds a mix mode
//!   starts), `pausegroups` (mixer groups paused while the mode is active);
//! * soundProps put sounds in mixer groups through `groups` / `addgroups` /
//!   `removegroups` along the `musicTemplate` (parent) chain;
//! * `instrument` records are text: `atk hold dec sus rel pitch krng vrng
//!   sampleid length loopstrt loopend` per zone (krng/vrng = hi<<8 | lo).

use super::parse::{self, Canvas};
use super::fnv;
use crate::asset_catalog::ResourceIndex;
use crate::dbpf::{self, DbpfEntry};
use crate::game_audio::{AUDIO_GROUP, TYPE_SNR, TYPE_SOUND_PROP};
use crate::prop;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const TYPE_PD: u32 = 0x6177_15D9;
pub const TYPE_PDR: u32 = 0x022D_2C83;
pub const TYPE_MODE: u32 = 0x0497_925E;
pub const TYPE_INSTRUMENT: u32 = 0x0305_5F61;
/// `eapdPixie`: XML data files of `pixie` (pattrstorage) and `trixie` (track lists).
pub const TYPE_PIXIE: u32 = 0x0330_A65E;
/// The positional companion record of a patch sound (VERIFIED data:
/// `music_city_planner` 0x03F51892 holds `is3d`, `mindistance` 100,
/// `maxdistance` 200 and the `attenuation` curve; `notes\civ-polish-re.md` M5).
pub const TYPE_SOUND_3D: u32 = 0x03F5_1892;
const PROP_IS_3D: u32 = 0x12D2_A4D4;
const PROP_MIN_DISTANCE: u32 = 0x0F61_6B72;
const PROP_MAX_DISTANCE: u32 = 0x0359_3710;
const PROP_ATTENUATION: u32 = 0x1E4D_B1EB;

/// Distance roll-off of a positional sound.
#[derive(Clone, Debug, PartialEq)]
pub struct Spatial {
    pub min: f32,
    pub max: f32,
    /// `attenuation` (x, y) points; empty = linear 1 -> 0.
    pub curve: Vec<(f32, f32)>,
}

impl Spatial {
    /// Gain at `distance` from the listener: full inside `min`, silent beyond
    /// `max`, the `attenuation` curve in between. INFERRED: the curve's x is
    /// `(distance - min) / (max - min)` and y the gain.
    pub fn gain(&self, distance: f32) -> f32 {
        if self.max <= self.min {
            return if distance <= self.min { 1.0 } else { 0.0 };
        }
        let x = ((distance - self.min) / (self.max - self.min)).clamp(0.0, 1.0);
        if self.curve.len() < 2 {
            return 1.0 - x;
        }
        let first = self.curve[0];
        if x <= first.0 {
            return first.1.clamp(0.0, 1.0);
        }
        for w in self.curve.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if x <= x1 {
                let f = if x1 > x0 { (x - x0) / (x1 - x0) } else { 1.0 };
                return (y0 + (y1 - y0) * f).clamp(0.0, 1.0);
            }
        }
        self.curve.last().map_or(0.0, |p| p.1.clamp(0.0, 1.0))
    }
}

const PROP_PARENT: u32 = 0x5F63_17D5;
const PROP_SAMPLES: u32 = 0x701E_D91E;
const PROP_GAIN: u32 = 0x25DF_0108;
const PROP_PITCH: u32 = 0x71BC_3009;
const PROP_FADE_OUT: u32 = 0xE8AB_99B9;
const PROP_GROUPS: u32 = 0x7CB8_1A35;
const PROP_ADD_GROUPS: u32 = 0xF58C_A0BE;
const PROP_REMOVE_GROUPS: u32 = 0x1404_3549;

/// A loaded bundle: its top patch and abstractions.
#[derive(Clone, Debug)]
pub struct Bundle {
    pub name: String,
    pub top: Arc<Canvas>,
    pub entries: HashMap<String, Arc<Canvas>>,
}

/// What a started sound turns into when it is a Pd patch.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchSound {
    /// The soundProp name hash (or the bundle's own id).
    pub sound: u32,
    pub bundle: u32,
    pub gain: f32,
    pub pitch: f32,
    pub fade_out: f32,
    /// Mixer groups of the soundProp (name hashes).
    pub groups: Vec<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModeDef {
    pub startsounds: Vec<u32>,
    pub pausegroups: Vec<u32>,
}

/// One zone of a note instrument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub root: f32,
    pub key_lo: u8,
    pub key_hi: u8,
    pub vel_lo: u8,
    pub vel_hi: u8,
    pub sample: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Instrument {
    pub zones: Vec<Zone>,
}

impl Instrument {
    pub fn parse(text: &str) -> Option<Instrument> {
        let mut zones = vec![];
        for line in text.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 9 || cols[0].starts_with('#') {
                continue;
            }
            let f = |i: usize| cols[i].parse::<f32>().ok();
            let h = |i: usize| u32::from_str_radix(cols[i].trim_start_matches("0x"), 16).ok();
            let (Some(attack), Some(hold), Some(decay), Some(sustain), Some(release), Some(root)) = (f(0), f(1), f(2), f(3), f(4), f(5))
            else {
                continue;
            };
            let (Some(k), Some(v), Some(sample)) = (h(6), h(7), h(8)) else { continue };
            zones.push(Zone {
                attack,
                hold,
                decay,
                sustain,
                release,
                root,
                key_lo: (k & 0xFF) as u8,
                key_hi: ((k >> 8) & 0xFF) as u8,
                vel_lo: (v & 0xFF) as u8,
                vel_hi: ((v >> 8) & 0xFF) as u8,
                sample,
            });
        }
        (!zones.is_empty()).then_some(Instrument { zones })
    }

    /// The zone for a note: key and velocity inside its ranges, else the
    /// zone whose root is nearest.
    pub fn zone(&self, note: f32, velocity: f32) -> Option<&Zone> {
        let n = note.round().clamp(0.0, 127.0) as u8;
        let v = velocity.round().clamp(0.0, 127.0) as u8;
        self.zones
            .iter()
            .find(|z| (z.key_lo..=z.key_hi).contains(&n) && (z.vel_lo..=z.vel_hi).contains(&v))
            .or_else(|| self.zones.iter().find(|z| (z.key_lo..=z.key_hi).contains(&n)))
            .or_else(|| self.zones.iter().min_by(|a, b| (a.root - note).abs().total_cmp(&(b.root - note).abs())))
    }
}

/// Index of the installed Pd data (cut from the game's resource index).
#[derive(Default)]
pub struct PdLibrary {
    records: HashMap<(u32, u32), (PathBuf, DbpfEntry)>,
    canvases: Mutex<HashMap<String, Option<Arc<Canvas>>>>,
    bundles: Mutex<HashMap<u32, Option<Arc<Bundle>>>>,
}

impl PdLibrary {
    pub fn from_index(index: &ResourceIndex) -> Self {
        let mut records: HashMap<(u32, u32), (PathBuf, DbpfEntry)> = HashMap::new();
        for (k, v) in &index.records {
            if matches!(
                k.type_id,
                TYPE_PD | TYPE_PDR | TYPE_MODE | TYPE_INSTRUMENT | TYPE_SOUND_PROP | TYPE_SNR | TYPE_PIXIE | TYPE_SOUND_3D | crate::game_audio::TYPE_SNS
            ) {
                // The audio group wins when a key exists in several groups.
                let e = records.entry((k.type_id, k.instance_id));
                match e {
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        slot.insert(v.clone());
                    }
                    std::collections::hash_map::Entry::Occupied(mut slot) => {
                        if k.group_id == AUDIO_GROUP {
                            slot.insert(v.clone());
                        }
                    }
                }
            }
        }
        PdLibrary { records, ..Default::default() }
    }

    /// Every installed `eapdPixie` file (instance = FNV of its name).
    pub fn data_files(&self) -> HashMap<u32, Arc<str>> {
        let ids: Vec<u32> = self.records.keys().filter(|(t, _)| *t == TYPE_PIXIE).map(|(_, i)| *i).collect();
        ids.into_iter()
            .filter_map(|i| Some((i, Arc::<str>::from(String::from_utf8_lossy(&self.read(TYPE_PIXIE, i).ok()?).as_ref()))))
            .collect()
    }

    pub fn count(&self, type_id: u32) -> usize {
        self.records.keys().filter(|(t, _)| *t == type_id).count()
    }

    pub fn contains(&self, type_id: u32, instance: u32) -> bool {
        self.records.contains_key(&(type_id, instance))
    }

    pub fn read(&self, type_id: u32, instance: u32) -> Result<Vec<u8>, String> {
        let (path, entry) = self
            .records
            .get(&(type_id, instance))
            .ok_or_else(|| format!("missing {instance:08x}.{type_id:08x}"))?;
        if entry.mem_size > 64 * 1024 * 1024 {
            return Err("record exceeds decode limit".into());
        }
        dbpf::read_entry_bytes(path, entry)
    }

    /// A `pd` record parsed (abstractions not shipped inside the bundle).
    pub fn abstraction(&self, name: &str) -> Option<Arc<Canvas>> {
        let key = name.to_lowercase();
        if let Some(c) = self.canvases.lock().ok()?.get(&key) {
            return c.clone();
        }
        let parsed = self
            .read(TYPE_PD, fnv(name))
            .ok()
            .and_then(|b| parse::parse_patch(name, &String::from_utf8_lossy(&b)).ok())
            .map(Arc::new);
        self.canvases.lock().ok()?.insert(key, parsed.clone());
        parsed
    }

    /// A `pdr` bundle by id.
    pub fn bundle(&self, id: u32) -> Option<Arc<Bundle>> {
        if let Some(b) = self.bundles.lock().ok()?.get(&id) {
            return b.clone();
        }
        let built = self.read(TYPE_PDR, id).ok().and_then(|bytes| {
            let entries = parse::parse_bundle(&bytes).ok()?;
            let mut map = HashMap::new();
            let mut top: Option<(String, Arc<Canvas>)> = None;
            for (i, e) in entries.iter().enumerate() {
                if e.tag != "pd" && !e.tag.is_empty() {
                    continue;
                }
                let Ok(c) = parse::parse_patch(&e.name, &String::from_utf8_lossy(&e.data)) else { continue };
                let c = Arc::new(c);
                // The bundle's own name is the top patch (else the first entry).
                if fnv(&e.name) == id || (i == 0 && top.is_none()) {
                    top = Some((e.name.clone(), c.clone()));
                }
                map.insert(e.name.to_lowercase(), c);
            }
            let (name, top) = top?;
            Some(Arc::new(Bundle { name, top, entries: map }))
        });
        self.bundles.lock().ok()?.insert(id, built.clone());
        built
    }

    /// Abstraction lookup for one bundle: its own entries first, then the
    /// installed `pd` records.
    pub fn resolver<'a>(&'a self, bundle: &'a Bundle) -> impl Fn(&str) -> Option<Arc<Canvas>> + 'a {
        move |name: &str| bundle.entries.get(&name.to_lowercase()).cloned().or_else(|| self.abstraction(name))
    }

    fn sound_prop(&self, id: u32) -> Option<prop::PropertyList> {
        prop::parse(&self.read(TYPE_SOUND_PROP, id).ok()?).ok()
    }

    /// soundProp property lists along the `musicTemplate` chain (child first).
    fn chain(&self, id: u32) -> Vec<prop::PropertyList> {
        let mut out = vec![];
        let mut seen = HashSet::new();
        let mut cur = id;
        while seen.insert(cur) && out.len() < 16 {
            let Some(p) = self.sound_prop(cur) else { break };
            let parent = p.0.get(&PROP_PARENT).and_then(key_instance);
            out.push(p);
            match parent {
                Some(next) if next != 0 => cur = next,
                _ => break,
            }
        }
        out
    }

    fn number(chain: &[prop::PropertyList], id: u32) -> Option<f32> {
        chain.iter().find_map(|p| p.0.get(&id).and_then(|v| v.number())).filter(|n| n.is_finite())
    }

    /// The mixer groups a sound belongs to (`groups` of the nearest
    /// definer, plus every `addgroups`, minus `removegroups`).
    pub fn sound_groups(&self, id: u32) -> Vec<u32> {
        let chain = self.chain(id);
        let mut groups: Vec<u32> = vec![];
        // Walk from the root template down so children override.
        for p in chain.iter().rev() {
            if let Some(g) = p.0.get(&PROP_GROUPS) {
                groups = key_instances(g);
            }
            if let Some(g) = p.0.get(&PROP_ADD_GROUPS) {
                for k in key_instances(g) {
                    if !groups.contains(&k) {
                        groups.push(k);
                    }
                }
            }
            if let Some(g) = p.0.get(&PROP_REMOVE_GROUPS) {
                let rm = key_instances(g);
                groups.retain(|k| !rm.contains(k));
            }
        }
        groups
    }

    /// Whether a started sound is a Pd patch, and how it is set up.
    pub fn patch_sound(&self, id: u32) -> Option<PatchSound> {
        let chain = self.chain(id);
        let mut bundle = None;
        if let Some(p) = chain.first() {
            for key in [PROP_SAMPLES, fnv("patch"), fnv("sample")] {
                if let Some(v) = p.0.get(&key) {
                    for inst in key_instances(v) {
                        if self.contains(TYPE_PDR, inst) {
                            bundle = Some(inst);
                            break;
                        }
                    }
                }
                if bundle.is_some() {
                    break;
                }
            }
        }
        if bundle.is_none() && self.contains(TYPE_PDR, id) {
            bundle = Some(id);
        }
        let bundle = bundle?;
        Some(PatchSound {
            sound: id,
            bundle,
            gain: Self::number(&chain, PROP_GAIN).unwrap_or(1.0),
            pitch: Self::number(&chain, PROP_PITCH).unwrap_or(1.0),
            fade_out: Self::number(&chain, PROP_FADE_OUT).unwrap_or(0.0).max(0.0),
            groups: self.sound_groups(id),
        })
    }

    /// The roll-off of a positional sound: its 0x03F51892 record when it has
    /// one, else the soundProp chain; `None` unless `is3d` and a max distance.
    pub fn spatial(&self, id: u32) -> Option<Spatial> {
        let own = self.read(TYPE_SOUND_3D, id).ok().and_then(|b| prop::parse(&b).ok());
        let mut chain: Vec<prop::PropertyList> = own.into_iter().collect();
        chain.extend(self.chain(id));
        let is3d = Self::number(&chain, PROP_IS_3D).unwrap_or(0.0) != 0.0;
        let min = Self::number(&chain, PROP_MIN_DISTANCE).unwrap_or(0.0).max(0.0);
        let max = Self::number(&chain, PROP_MAX_DISTANCE).unwrap_or(0.0).max(0.0);
        if !is3d || max <= 0.0 {
            return None;
        }
        let curve = chain
            .iter()
            .find_map(|p| p.0.get(&PROP_ATTENUATION))
            .map(|p| {
                let v: Vec<f32> = p
                    .values
                    .iter()
                    .filter_map(|b| Some(f32::from_be_bytes(b.get(..4)?.try_into().ok()?)))
                    .collect();
                v.chunks_exact(2).map(|c| (c[0], c[1])).collect()
            })
            .unwrap_or_default();
        Some(Spatial { min, max, curve })
    }

    pub fn mode(&self, name: &str) -> Option<ModeDef> {
        let p = prop::parse(&self.read(TYPE_MODE, fnv(name)).ok()?).ok()?;
        Some(ModeDef {
            startsounds: p.0.get(&fnv("startsounds")).map(key_instances).unwrap_or_default(),
            pausegroups: p.0.get(&fnv("pausegroups")).map(key_instances).unwrap_or_default(),
        })
    }

    pub fn instrument(&self, name: &str) -> Option<Instrument> {
        let id = match name.strip_prefix('#') {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => fnv(name),
        };
        Instrument::parse(&String::from_utf8_lossy(&self.read(TYPE_INSTRUMENT, id).ok()?))
    }

    /// Seconds an SNR sample plays (header only), for headless voice ends.
    /// Uncompressed size of a record (no read).
    pub fn record_size(&self, type_id: u32, instance: u32) -> Option<u32> {
        self.records.get(&(type_id, instance)).map(|(_, e)| e.mem_size as u32)
    }

    pub fn sample_seconds(&self, sample: u32) -> Option<f32> {
        let b = self.read(TYPE_SNR, sample).ok()?;
        let h = crate::audio::parse_snr_header(&b)?;
        Some(h.num_samples as f32 / h.sample_rate.max(1) as f32)
    }

    /// Bundle ids of every installed `pdr` (diagnostics, tests).
    pub fn bundle_ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.records.keys().filter(|(t, _)| *t == TYPE_PDR).map(|(_, i)| *i).collect();
        v.sort_unstable();
        v
    }
}

fn key_instance(p: &prop::Property) -> Option<u32> {
    Some(u32::from_le_bytes(p.values.first()?.get(..4)?.try_into().ok()?))
}

/// Instance ids of a key (or key array) property; u32 arrays are big endian.
fn key_instances(p: &prop::Property) -> Vec<u32> {
    p.values
        .iter()
        .filter_map(|v| {
            if p.kind == 0x20 {
                Some(u32::from_le_bytes(v.get(..4)?.try_into().ok()?))
            } else if v.len() >= 4 {
                Some(u32::from_be_bytes(v.get(..4)?.try_into().ok()?))
            } else {
                None
            }
        })
        .collect()
}
