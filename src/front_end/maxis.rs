//! The installed Maxis creations Sporepedia lists: every creature, building,
//! vehicle and spaceship the install describes with a `pollen_metadata`
//! record (Spore_Content.package: 759 creatures in group 40626200, 310
//! buildings 40636200, 285 vehicles 40646200, 91 UFOs 40656200).
//!
//! VERIFIED (installed data; the format is SporeModder-FX `PollenMetadata`,
//! versions up to 13, big-endian): version, asset id (u64), asset key (type,
//! group, instance), parent key, parent id (v10+), original parent id (v12+),
//! time created (u64), time downloaded (v9+; v10 stores `FFFFFFFF` for none),
//! then either literal UTF-16 author / name / description (flag 0) or one
//! locale table with the author / name / description string ids (the Maxis
//! creations: table 04DC2F94, author id 1 = "Maxis"), the authors list, tags,
//! shareable flag and consequence traits. Beside each record the install has
//! the creation's own 128 x 128 PNG (same group and instance, type
//! 2F7D0004): the card image retail shows. The creations' editable files
//! (`.crt` / `.bld` / `.vcl` / `.ufo`) are not installed, only their baked
//! models, so these entries cannot be opened in a creator.

use std::{
    collections::HashMap,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::{
    asset_catalog::ResourceIndex,
    creation_records::{BLD, UFO, VCL},
    dbpf::{DbpfEntry, ResourceKey},
};

pub const METADATA_TYPE: u32 = 0x030B_DEE3;
pub const CRT: u32 = 0x2B97_8C46;
pub const PNG_TYPE: u32 = 0x2F7D_0004;

/// A decoded `pollen_metadata` record.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub version: i32,
    pub asset_id: u64,
    /// (type, group, instance) of the creation file.
    pub asset_key: (u32, u32, u32),
    pub time_created: u64,
    pub author: Text,
    pub name: Text,
    pub description: Text,
    pub tags: Vec<String>,
}

/// A literal string or a locale-table reference.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Text {
    #[default]
    None,
    Literal(String),
    Localized { table: u32, id: u32 },
}

struct Reader<'a> {
    b: &'a [u8],
    o: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Option<u32> {
        let v = u32::from_be_bytes(self.b.get(self.o..self.o + 4)?.try_into().ok()?);
        self.o += 4;
        Some(v)
    }
    fn u64(&mut self) -> Option<u64> {
        Some(((self.u32()? as u64) << 32) | self.u32()? as u64)
    }
    fn utf16(&mut self, chars: usize) -> Option<String> {
        let bytes = self.b.get(self.o..self.o + chars.checked_mul(2)?)?;
        self.o += chars * 2;
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(String::from_utf16_lossy(&units))
    }
    fn ascii(&mut self, len: usize) -> Option<String> {
        let bytes = self.b.get(self.o..self.o + len)?;
        self.o += len;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
    fn len(&mut self) -> Option<usize> {
        let n = self.u32()? as usize;
        (n <= 0x10000).then_some(n)
    }
}

pub fn parse_metadata(bytes: &[u8]) -> Option<Metadata> {
    let mut r = Reader { b: bytes, o: 0 };
    let version = r.u32()? as i32;
    if !(0..=13).contains(&version) {
        return None;
    }
    let asset_id = r.u64()?;
    let asset_key = (r.u32()?, r.u32()?, r.u32()?);
    let _parent_key = (r.u32()?, r.u32()?, r.u32()?);
    if version >= 10 {
        r.u64()?;
    }
    if version >= 12 {
        r.u64()?;
    }
    let time_created = r.u64()?;
    if version == 10 {
        if r.u32()? != 0xFFFF_FFFF {
            r.u32()?;
        }
    } else if version >= 9 {
        r.u64()?;
    }
    let (author, name, description) = if r.u32()? == 0 {
        r.u64()?;
        let a = r.len()?;
        let author = r.utf16(a)?;
        let n = r.len()?;
        let name = r.utf16(n)?;
        let d = r.len()?;
        let description = r.utf16(d)?;
        (Text::Literal(author), Text::Literal(name), Text::Literal(description))
    } else {
        let table = r.u32()?;
        let at = |id: u32| if id == 0xFFFF_FFFF { Text::None } else { Text::Localized { table, id } };
        (at(r.u32()?), at(r.u32()?), at(r.u32()?))
    };
    // Authors (v2+ a list), tags (literal unless v13 says localized).
    if version >= 2 {
        for _ in 0..r.len()? {
            let n = r.len()?;
            r.ascii(n)?;
        }
    } else if version >= 1 {
        let n = r.len()?;
        r.ascii(n)?;
    }
    let mut tags = Vec::new();
    let localized_tags = version >= 13 && r.u32()? != 0;
    if !localized_tags {
        if let Some(count) = r.len() {
            for _ in 0..count {
                let Some(n) = r.len() else { break };
                let Some(tag) = r.utf16(n) else { break };
                tags.push(tag);
            }
        }
    }
    Some(Metadata { version, asset_id, asset_key, time_created, author, name, description, tags })
}

/// One installed creation for Sporepedia.
#[derive(Clone, Debug, PartialEq)]
pub struct InstalledCreation {
    /// Index into `pedia::CATEGORIES`.
    pub category: usize,
    pub name: String,
    pub author: String,
    pub description: String,
    /// The metadata's tags (the dialog's Tags box).
    pub tags: Vec<String>,
    /// The creation's PNG (card image), if installed.
    pub png: Option<ResourceKey>,
    pub key: (u32, u32, u32),
    pub order: usize,
}

/// Plant and cell creation types (`flr` / `cll`, reg_type.txt).
pub const FLR: u32 = 0x438F_6347;
pub const CLL: u32 = 0x3D97_A8E4;

pub fn category_of(type_id: u32) -> Option<usize> {
    match type_id {
        CRT => Some(0),
        BLD => Some(1),
        VCL => Some(2),
        UFO => Some(3),
        FLR => Some(4),
        CLL => Some(5),
        _ => None,
    }
}

/// The catalog, built on a worker thread the first time Sporepedia opens.
#[derive(Default)]
pub struct Catalog {
    pub state: Arc<Mutex<Option<Vec<InstalledCreation>>>>,
    pub started: bool,
}

impl Catalog {
    /// Start building (once); the entries once they are there.
    pub fn poll(&mut self, index: &ResourceIndex) -> Option<Vec<InstalledCreation>> {
        if !self.started && !index.records.is_empty() {
            self.started = true;
            let mut jobs = jobs(index);
            let state = self.state.clone();
            std::thread::spawn(move || {
                let rigid = std::mem::take(&mut jobs.rigid);
                if !classified() {
                    let classes = classify(rigid);
                    if let Ok(mut m) = MODEL_TYPES.lock() {
                        *m = Some(classes);
                    }
                }
                let list = build(jobs);
                if let Ok(mut s) = state.lock() {
                    *s = Some(list);
                }
            });
        }
        self.state.lock().ok().and_then(|s| s.clone())
    }
}

/// What the worker reads: metadata records, locale tables, the PNG keys.
pub struct Jobs {
    metadata: Vec<(ResourceKey, PathBuf, DbpfEntry)>,
    tables: HashMap<u32, Vec<(PathBuf, DbpfEntry)>>,
    pngs: std::collections::HashSet<ResourceKey>,
    /// The building, vehicle and UFO card PNGs (their `modeltype` is read, see [`model_type_of`]).
    rigid: Vec<(ResourceKey, PathBuf, DbpfEntry)>,
}

pub fn jobs(index: &ResourceIndex) -> Jobs {
    let mut metadata = Vec::new();
    let mut tables: HashMap<u32, Vec<(PathBuf, DbpfEntry)>> = HashMap::new();
    let mut pngs = std::collections::HashSet::new();
    let mut rigid = Vec::new();
    for (key, (path, entry)) in &index.records {
        match key.type_id {
            METADATA_TYPE => metadata.push((*key, path.clone(), entry.clone())),
            crate::spui::view::LOCALE_TABLE_TYPE => tables.entry(key.instance_id).or_default().push((path.clone(), entry.clone())),
            PNG_TYPE if (0x4061_6200..=0x4066_6200).contains(&key.group_id) => {
                pngs.insert(*key);
                if (0x4063_6200..=0x4065_6200).contains(&key.group_id) {
                    rigid.push((*key, path.clone(), entry.clone()));
                }
            }
            _ => {}
        }
    }
    Jobs { metadata, tables, pngs, rigid }
}

/// `modeltype` of the installed building, vehicle and UFO creations, by card
/// PNG key: the hash of the editor class name (`BuildingCityHall`,
/// `VehicleMilitaryLand`, ...) that the creation file carries. The new-game
/// shoppers filter by it (their filters' `browserFilterData` is the class name:
/// `CityHall` -> `BuildingCityHall`, `MilitaryLand` / `EconomicLand` /
/// `CulturalLand` -> `Vehicle<Purpose>Land`; installed group CC489C6F). Filled by
/// the catalog worker before the catalog is published.
static MODEL_TYPES: Mutex<Option<HashMap<ResourceKey, u32>>> = Mutex::new(None);

/// The `modeltype` of an installed creation's card PNG (`None` until the catalog is built,
/// or when the file has none).
pub fn model_type_of(png: ResourceKey) -> Option<u32> {
    MODEL_TYPES.lock().ok()?.as_ref()?.get(&png).copied()
}

/// Whether the classification has run.
pub fn classified() -> bool {
    MODEL_TYPES.lock().ok().is_some_and(|m| m.is_some())
}

/// `<modeltype>0x...</modeltype>` of a creation file.
pub fn model_type_in(xml: &str) -> Option<u32> {
    let start = xml.find("<modeltype>")? + "<modeltype>".len();
    let text = xml[start..].split('<').next()?.trim();
    u32::from_str_radix(text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text), 16).ok()
}

/// Read the creation file of each card PNG and take its `modeltype` (a pool of threads: each PNG is
/// decoded from its pixels' low bits, `png_creation`).
pub fn classify(rigid: Vec<(ResourceKey, PathBuf, DbpfEntry)>) -> HashMap<ResourceKey, u32> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    let chunk = rigid.len().div_ceil(threads).max(1);
    let mut out = HashMap::new();
    std::thread::scope(|scope| {
        let workers: Vec<_> = rigid
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    let records: Vec<(PathBuf, DbpfEntry)> = part.iter().map(|(_, p, e)| (p.clone(), e.clone())).collect();
                    let blobs = read_batch(&records);
                    part.iter()
                        .zip(blobs)
                        .filter_map(|((key, _, _), blob)| {
                            let decoded = super::png_creation::decode_png(&blob?)?;
                            Some((*key, model_type_in(&decoded.model)?))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for worker in workers {
            if let Ok(found) = worker.join() {
                out.extend(found);
            }
        }
    });
    out
}

/// Read many records of one package with one open file, in file order.
fn read_batch(records: &[(PathBuf, DbpfEntry)]) -> Vec<Option<Vec<u8>>> {
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by(|a, b| (&records[*a].0, records[*a].1.offset).cmp(&(&records[*b].0, records[*b].1.offset)));
    let mut out = vec![None; records.len()];
    let mut file: Option<(PathBuf, std::fs::File)> = None;
    for i in order {
        let (path, entry) = &records[i];
        if file.as_ref().map(|(p, _)| p != path).unwrap_or(true) {
            file = std::fs::File::open(path).ok().map(|f| (path.clone(), f));
        }
        let Some((_, f)) = file.as_mut() else { continue };
        let mut raw = vec![0u8; entry.disk_size as usize];
        if f.seek(SeekFrom::Start(entry.offset)).is_err() || f.read_exact(&mut raw).is_err() {
            continue;
        }
        let bytes = if entry.is_compressed && raw.len() >= 5 && (raw[0] == 0x10 || raw[0] == 0x50) && raw[1] == 0xFB {
            crate::dbpf::qfs_decompress(&raw).ok()
        } else {
            Some(raw)
        };
        out[i] = bytes;
    }
    out
}

pub fn build(jobs: Jobs) -> Vec<InstalledCreation> {
    let records: Vec<(PathBuf, DbpfEntry)> = jobs.metadata.iter().map(|(_, p, e)| (p.clone(), e.clone())).collect();
    let blobs = read_batch(&records);
    let mut parsed: Vec<(ResourceKey, Metadata)> = Vec::new();
    for ((key, _, _), blob) in jobs.metadata.iter().zip(blobs) {
        if let Some(m) = blob.as_deref().and_then(parse_metadata) {
            if category_of(m.asset_key.0).is_some() {
                parsed.push((*key, m));
            }
        }
    }
    // The locale tables the records name (first group wins, like SpuiAssets).
    let mut strings: HashMap<u32, HashMap<u32, String>> = HashMap::new();
    let wanted: std::collections::HashSet<u32> = parsed
        .iter()
        .flat_map(|(_, m)| [&m.author, &m.name, &m.description])
        .filter_map(|t| match t {
            Text::Localized { table, .. } => Some(*table),
            _ => None,
        })
        .collect();
    for table in wanted {
        let Some(list) = jobs.tables.get(&table) else { continue };
        let mut map = HashMap::new();
        for bytes in read_batch(list).into_iter().flatten() {
            for (id, text) in crate::decode::parse_locale_table(&bytes) {
                map.entry(id).or_insert(text);
            }
        }
        strings.insert(table, map);
    }
    let text = |t: &Text| match t {
        Text::None => String::new(),
        Text::Literal(s) => s.clone(),
        Text::Localized { table, id } => strings.get(table).and_then(|m| m.get(id)).cloned().unwrap_or_default(),
    };
    // Installed order: by asset id (the time sort of the Maxis creations).
    parsed.sort_by_key(|(k, m)| (m.asset_id, k.instance_id));
    let mut out = Vec::new();
    for (order, (key, m)) in parsed.into_iter().enumerate() {
        let name = text(&m.name);
        if name.trim().is_empty() {
            continue;
        }
        let png = ResourceKey { group_id: key.group_id, instance_id: key.instance_id, type_id: PNG_TYPE };
        out.push(InstalledCreation {
            category: category_of(m.asset_key.0).unwrap_or(0),
            name,
            author: text(&m.author),
            description: text(&m.description),
            tags: m.tags.clone(),
            png: jobs.pngs.contains(&png).then_some(png),
            key: m.asset_key,
            order: order + 1,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The installed record of "anticipaste" (40626200!36BC4FC2.030BDEE3).
    const ANTICIPASTE: &str = "0000000a000000000000016a2b978c464062620036bc4fc2ffffffffffffffffffffffffffffffffffffffff00000000000000000000000000000000ffffffff04dc2f9400000001e16217e0ffffffff00000001000000237461673a73706f72652e636f6d2c323030363a417373656d626c6564436f6e74656e7400000000000000000000000000";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn maxis_metadata_names_come_from_the_locale_table() {
        let m = parse_metadata(&hex(ANTICIPASTE)).expect("parses");
        assert_eq!(m.version, 10);
        assert_eq!(m.asset_id, 0x16A);
        assert_eq!(m.asset_key, (CRT, 0x4062_6200, 0x36BC_4FC2));
        assert_eq!(m.author, Text::Localized { table: 0x04DC_2F94, id: 1 });
        assert_eq!(m.name, Text::Localized { table: 0x04DC_2F94, id: 0xE162_17E0 });
        assert_eq!(m.description, Text::None);
        assert_eq!(category_of(m.asset_key.0), Some(0));
    }

    #[test]
    fn literal_metadata_reads_utf16_strings() {
        // version 13, literal author / name / description, no authors, no tags.
        let mut b = Vec::new();
        b.extend(13u32.to_be_bytes());
        b.extend(7u64.to_be_bytes());
        for v in [BLD, 1, 2, 0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF] {
            b.extend(v.to_be_bytes());
        }
        b.extend(u64::MAX.to_be_bytes()); // parent id
        b.extend(u64::MAX.to_be_bytes()); // original parent id
        b.extend(1234u64.to_be_bytes()); // created
        b.extend(0u64.to_be_bytes()); // downloaded
        b.extend(0u32.to_be_bytes()); // literal
        b.extend(9u64.to_be_bytes()); // author id
        for s in ["Me", "Hut", ""] {
            b.extend((s.len() as u32).to_be_bytes());
            for u in s.encode_utf16() {
                b.extend(u.to_le_bytes());
            }
        }
        b.extend(0u32.to_be_bytes()); // authors
        b.extend(0u32.to_be_bytes()); // tags not localized
        b.extend(0u32.to_be_bytes()); // no tags
        let m = parse_metadata(&b).expect("parses");
        assert_eq!((m.name, m.author, m.time_created), (Text::Literal("Hut".into()), Text::Literal("Me".into()), 1234));
    }

    #[test]
    fn the_model_type_is_read_from_the_creation_file() {
        assert_eq!(model_type_in("<sporemodel><properties><modeltype>0x9ea3031a</modeltype></properties>"), Some(0x9EA3_031A));
        assert_eq!(model_type_in("<sporemodel><properties></properties>"), None);
    }

    /// Every installed building / vehicle / UFO card PNG yields its creation's class (needs the game).
    #[test]
    #[ignore = "reads the installed Spore_Content package"]
    fn installed_rigid_creations_are_classified_by_model_type() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        let started = std::time::Instant::now();
        let classes = classify(jobs(&index).rigid);
        let elapsed = started.elapsed();
        let count = |name: &str| classes.values().filter(|t| **t == crate::world_objects::name_id(name)).count();
        println!("{} classified in {elapsed:?}: city halls {}, military land {}, economic land {}, cultural land {}", classes.len(), count("BuildingCityHall"), count("VehicleMilitaryLand"), count("VehicleEconomicLand"), count("VehicleCulturalLand"));
        assert!(classes.len() > 600, "{} of the 686 buildings, vehicles and UFOs", classes.len());
        assert!(count("BuildingCityHall") > 0 && count("VehicleMilitaryLand") > 0 && count("VehicleEconomicLand") > 0 && count("VehicleCulturalLand") > 0);
    }

    /// The installed catalog (needs the game).
    #[test]
    #[ignore = "reads the installed Spore_Content package"]
    fn installed_maxis_creations_are_named() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        let list = build(jobs(&index));
        let creatures = list.iter().filter(|c| c.category == 0).count();
        assert!(creatures > 700, "{creatures} creatures");
        let (plants, cells) = (list.iter().filter(|c| c.category == 4).count(), list.iter().filter(|c| c.category == 5).count());
        assert!(plants > 100 && cells > 30, "{plants} plants, {cells} cells");
        assert!(list.iter().all(|c| !c.name.is_empty() && c.png.is_some()));
        assert!(list.iter().any(|c| c.author == "Maxis"));
    }
}
