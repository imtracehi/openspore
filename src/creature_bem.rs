//! Binary editor models (`.bem`, type 0x1A99B06B; `notes/limb-re-notes.md`
//! section 5, VERIFIED): the serialized `Editors::cEditorResource` of a
//! creation (ModAPI `cEditorResource.h`). The rigid (exo) limb assemblies of
//! the Creature Creator ship only this file (no `.creaturedata`), so their
//! blocks are read from here.
//!
//! Format (VERIFIED against `xlimb_exo_l_01.bem` and `x_leg_09.bem`, whose
//! `.creaturedata` holds the same blocks): the bytes are run-length coded,
//! `FE nn` = the previous byte `nn` more times and `FE 00` = a literal 0xFE.
//! Decoded: magic 0x48657ED3, version 5, block count, the properties up to
//! offset 0x94, then one 0x1D8-byte `cEditorResourceBlock` per block.

use bevy::prelude::*;

/// `cEditorResource::RESOURCE_TYPE` (`.bem`).
pub const TYPE: u32 = 0x1A99_B06B;
pub const MAGIC: u32 = 0x4865_7ED3;
/// Decoded offset of the first block (version 5).
pub const BLOCKS_AT: usize = 0x94;
/// `sizeof(cEditorResourceBlock)`.
pub const BLOCK_SIZE: usize = 0x1D8;

/// One block (`cEditorResourceBlock`), creature space (Spore, Z up).
#[derive(Clone, Debug, PartialEq)]
pub struct BemBlock {
    pub group: u32,
    pub instance: u32,
    pub parent: i32,
    pub symmetric: i32,
    pub scale: f32,
    pub position: Vec3,
    /// Rows of the orientation (the block's local axes in creature space).
    pub orientation: [Vec3; 3],
    pub user_orientation: [Vec3; 3],
    pub snapped: bool,
    pub asymmetric: bool,
    /// `mLimbType`: 0 / 1 skinned limb segment, 2 rigid limb segment, 3 not a
    /// limb (`notes/skin-re-notes.md` 3.1).
    pub limb_type: i32,
    pub muscle_scale: f32,
    /// The ball-connector scale at this block's joint.
    pub ball_scale: f32,
    /// Deform handle (channel, weight) pairs.
    pub handles: Vec<(u32, f32)>,
}

impl BemBlock {
    pub fn is_limb(&self) -> bool {
        (0..=2).contains(&self.limb_type)
    }
}

/// Undo the run-length coding.
pub fn decode_rle(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 3);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0xFE && i + 1 < bytes.len() && !out.is_empty() {
            let n = bytes[i + 1] as usize;
            if n == 0 {
                out.push(0xFE);
            } else {
                let last = *out.last().unwrap();
                out.extend(std::iter::repeat(last).take(n));
            }
            i += 2;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

fn f32_at(d: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

fn vec_at(d: &[u8], o: usize) -> Vec3 {
    Vec3::new(f32_at(d, o), f32_at(d, o + 4), f32_at(d, o + 8))
}

/// Parse a `.bem` resource (raw, still run-length coded).
pub fn parse(bytes: &[u8]) -> Result<Vec<BemBlock>, String> {
    let d = decode_rle(bytes);
    if d.len() < BLOCKS_AT || u32_at(&d, 0) != MAGIC {
        return Err("not a .bem".into());
    }
    let count = u32_at(&d, 8) as usize;
    if count > 1024 || d.len() < BLOCKS_AT + count * BLOCK_SIZE {
        return Err(format!(".bem: {count} blocks in {} decoded bytes", d.len()));
    }
    Ok((0..count)
        .map(|i| {
            let b = BLOCKS_AT + i * BLOCK_SIZE;
            let rows = |o: usize| [vec_at(&d, b + o), vec_at(&d, b + o + 12), vec_at(&d, b + o + 24)];
            let handles = (0..(u32_at(&d, b + 0x90) as usize).min(8))
                .map(|k| (u32_at(&d, b + 0xB4 + 4 * k), f32_at(&d, b + 0x94 + 4 * k)))
                .collect();
            BemBlock {
                group: u32_at(&d, b),
                instance: u32_at(&d, b + 4),
                parent: u32_at(&d, b + 8) as i32,
                symmetric: u32_at(&d, b + 0xC) as i32,
                scale: f32_at(&d, b + 0x10),
                position: vec_at(&d, b + 0x14),
                orientation: rows(0x38),
                user_orientation: rows(0x5C),
                snapped: d[b + 0x80] != 0,
                asymmetric: d[b + 0x81] != 0,
                limb_type: u32_at(&d, b + 0x84) as i32,
                muscle_scale: f32_at(&d, b + 0x88),
                ball_scale: f32_at(&d, b + 0x8C),
                handles,
            }
        })
        .collect())
}

/// A limb chain of a `.bem` assembly: the segment blocks root first and the
/// end block (a foot / hand: the non-limb child of the last segment).
pub fn limb_chain(blocks: &[BemBlock]) -> Option<(Vec<usize>, Option<usize>)> {
    let is_limb = |i: usize| blocks.get(i).is_some_and(BemBlock::is_limb);
    let root = (0..blocks.len()).find(|&i| is_limb(i) && (blocks[i].parent < 0 || !is_limb(blocks[i].parent as usize)))?;
    let mut chain = vec![root];
    while let Some(next) = (0..blocks.len()).find(|&i| is_limb(i) && blocks[i].parent == *chain.last().unwrap() as i32) {
        if chain.contains(&next) {
            break;
        }
        chain.push(next);
    }
    let last = *chain.last().unwrap() as i32;
    let end = (0..blocks.len()).find(|&i| !is_limb(i) && blocks[i].parent == last);
    Some((chain, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_length_coding() {
        assert_eq!(decode_rle(&[1, 0xFE, 3, 2]), vec![1, 1, 1, 1, 2]);
        assert_eq!(decode_rle(&[0xA5, 0xFE, 0x00, 0x6B]), vec![0xA5, 0xFE, 0x6B], "FE 00 is a literal");
        assert_eq!(decode_rle(&[0, 0xFE, 0x0D, 0x80, 0x3F]).len(), 16);
    }

    /// The skinned `x_leg_09.bem` copy in the notes decodes to the same blocks
    /// as its `.creaturedata` (VERIFIED in skin-re 2.5: thigh, shin, foot).
    #[test]
    fn x_leg_09_bem_matches_its_creature_data() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../notes/skin-re");
        let (Ok(bem), Ok(cd)) = (std::fs::read(root.join("x_leg_09.bem")), std::fs::read(root.join("x_leg_09.creaturedata"))) else { return };
        let blocks = parse(&bem).expect("x_leg_09.bem");
        assert_eq!(blocks.len(), 3);
        let data = crate::creature_data::parse(&cd).expect("creaturedata");
        for b in &data.rigblocks {
            let bb = b.bounding_box_bits().map(f32::from_bits);
            println!("creaturedata block {:08X}: scale {} bbox {:?}", b.instance_id(), f32::from_bits(b.scale_bits()), bb);
        }
        let records = crate::creature_skin::records_from_creature_data(&data);
        let (chain, end) = limb_chain(&blocks).expect("chain");
        assert_eq!(chain.len(), 2);
        assert!(end.is_some());
        for (k, &i) in chain.iter().enumerate() {
            let b = &blocks[i];
            println!("bem block {i}: {:08X} parent {} limb type {} pos {} muscle {} ball {}", b.instance, b.parent, b.limb_type, b.position, b.muscle_scale, b.ball_scale);
            assert_eq!(b.limb_type, 1, "tentacle-type skinned segment");
            let r = records.iter().filter(|r| r.kind == crate::creature_skin::BlockKind::Limb).nth(k).unwrap();
            assert!(b.position.distance(r.position) < 1e-3, "{} vs {}", b.position, r.position);
            assert!((b.muscle_scale - r.muscle_scale).abs() < 1e-4);
        }
    }

    /// Installed data: the 24 exo assemblies are `.bem`-only rigid chains
    /// (limb type 2) of installed segment parts ending in a foot / hand.
    #[test]
    fn installed_exo_assemblies_decode() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        if index.records.is_empty() {
            return;
        }
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let (mut ok, mut mixed) = (0, 0);
        for side in ["a", "l"] {
            for n in 1..=12 {
                let item = crate::world_objects::name_id(&format!("limb_assembly_exo_{side}_{n:02}"));
                let Ok(p) = index.properties(crate::cell_editor_data::prop_key(crate::asset_catalog::CREATURE_PARTS, item)) else { continue };
                let Some(file) = crate::cell_editor_data::key_of(&p, crate::creature_limbs::ASSEMBLY_FILE) else { continue };
                let key = crate::dbpf::ResourceKey { type_id: TYPE, group_id: file.group_id, instance_id: file.instance_id };
                let Ok(bytes) = index.read(key) else {
                    println!("exo_{side}_{n:02}: no .bem");
                    continue;
                };
                let blocks = parse(&bytes).expect("bem");
                let (chain, end) = limb_chain(&blocks).expect("chain");
                let names: Vec<String> = chain
                    .iter()
                    .map(|&i| catalog.by_definition_in(crate::asset_catalog::CREATURE_PARTS, blocks[i].instance).map_or(format!("{:08X}?", blocks[i].instance), |d| d.name.clone()))
                    .collect();
                let end_name = end.and_then(|e| catalog.by_definition_in(crate::asset_catalog::CREATURE_PARTS, blocks[e].instance)).map(|d| d.name.clone());
                println!(
                    "exo_{side}_{n:02}: {} blocks, segments {names:?} types {:?} balls {:?} handles {:?}, end {end_name:?}",
                    blocks.len(),
                    chain.iter().map(|&i| blocks[i].limb_type).collect::<Vec<_>>(),
                    chain.iter().map(|&i| blocks[i].ball_scale).collect::<Vec<_>>(),
                    chain.iter().map(|&i| blocks[i].handles.len()).collect::<Vec<_>>(),
                );
                if !chain.iter().all(|&i| blocks[i].limb_type == 2) {
                    mixed += 1;
                }
                assert!(chain.iter().any(|&i| blocks[i].limb_type == 2), "rigid segments");
                assert!(names.iter().all(|n| !n.ends_with('?')), "installed segment parts");
                ok += 1;
            }
        }
        println!("{ok} exo assemblies, {mixed} mixing skinned and rigid segments");
        assert_eq!(ok, 24, "every exo assembly decodes");
    }
}
