//! GMDL dependency inspection. The first word is a LE version; the next
//! word is a BE dependency count. Dependencies are not the model geometry.
//! Complete inline geometry and baked material decoding live in baked_model.

use std::collections::HashSet;
use std::path::Path;

use crate::dbpf;
use crate::decode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceKey {
    pub type_id: u32,
    pub group_id: u32,
    pub instance_id: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Gmdl {
    pub table_meshes: Vec<ResourceKey>,
    pub material_meshes: Vec<ResourceKey>,
    pub mesh_count_field: u32,
}

/// Read the native dependency table without treating the version as a count.
pub fn parse_mesh_table(blob: &[u8]) -> Option<Gmdl> {
    if blob.len() < 8 {
        return None;
    }
    let version = u32::from_le_bytes(blob[..4].try_into().ok()?);
    let count = u32::from_be_bytes(blob[4..8].try_into().ok()?) as usize;
    if !(5..=9).contains(&version) || count > 4096 || blob.len() < 12 + count * 12 {
        return None;
    }
    let mut g = Gmdl {
        mesh_count_field: u32::from_le_bytes(
            blob[8 + count * 12..12 + count * 12].try_into().ok()?,
        ),
        ..Default::default()
    };
    if g.mesh_count_field > 512 {
        return None;
    }
    for i in 0..count {
        let o = 8 + i * 12;
        let inst = u32::from_be_bytes([blob[o], blob[o + 1], blob[o + 2], blob[o + 3]]);
        let grp = u32::from_be_bytes([blob[o + 4], blob[o + 5], blob[o + 6], blob[o + 7]]);
        let typ = u32::from_be_bytes([blob[o + 8], blob[o + 9], blob[o + 10], blob[o + 11]]);
        if typ == 0 {
            break;
        }
        g.table_meshes.push(ResourceKey {
            type_id: typ,
            group_id: grp,
            instance_id: inst,
        });
    }
    Some(g)
}

/// Types that may legally appear in a mesh table (validation set).
pub const TABLE_KNOWN_TYPES: &[u32] = &[
    decode::TYPE_RW4_MODEL,
    decode::TYPE_RASTER,
    decode::TYPE_GMDL,
    decode::TYPE_PROP,
    decode::TYPE_ANIM,
    decode::TYPE_PNG,
    decode::TYPE_JPEG,
    decode::TYPE_AUDIO,
    decode::TYPE_LOCALE_TEXT,
    decode::TYPE_NAMEGEN,
];

#[derive(Debug, Clone)]
pub enum TableKind {
    MeshTable(Vec<ResourceKey>),
    Other { count_field: u32 },
}

/// Classify the versioned dependency header; malformed geometry counts are
/// kept out of the preview's resource traversal.
pub fn classify_table(blob: &[u8]) -> Option<TableKind> {
    if let Some(g) = parse_mesh_table(blob) {
        return Some(TableKind::MeshTable(g.table_meshes));
    }
    Some(TableKind::Other {
        count_field: u32::from_le_bytes(blob.get(..4)?.try_into().ok()?),
    })
}
/// Scan material blocks for LE (instance, type, group) triples validated
/// against the known index set. Only RW4-mesh refs are collected.
pub fn scan_material_mesh_refs(blob: &[u8], known: &HashSet<(u32, u32, u32)>) -> Vec<ResourceKey> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if blob.len() < 12 {
        return out;
    }
    for i in (0..blob.len().saturating_sub(11)).step_by(4) {
        let typ = u32::from_le_bytes([blob[i + 4], blob[i + 5], blob[i + 6], blob[i + 7]]);
        if typ != decode::TYPE_RW4_MODEL {
            continue;
        }
        let inst = u32::from_le_bytes([blob[i], blob[i + 1], blob[i + 2], blob[i + 3]]);
        let grp = u32::from_le_bytes([blob[i + 8], blob[i + 9], blob[i + 10], blob[i + 11]]);
        let key = (typ, grp, inst);
        if known.contains(&key) && seen.insert(key) {
            out.push(ResourceKey {
                type_id: typ,
                group_id: grp,
                instance_id: inst,
            });
        }
    }
    out
}

/// Build the (type, group, instance) set of every record in the install.
/// Index-only; no decompression.
pub fn build_index_set(install: &crate::spore_install::SporeInstall) -> HashSet<(u32, u32, u32)> {
    let mut set = HashSet::new();
    for pkg in &install.packages {
        if let Ok(index) = dbpf::read_package_index(pkg) {
            for e in &index.entries {
                set.insert((e.key.type_id, e.key.group_id, e.key.instance_id));
            }
        }
    }
    set
}

/// Find a record's package + entry by key.
pub fn find_record<'a>(
    install: &'a crate::spore_install::SporeInstall,
    key: &ResourceKey,
) -> Option<(&'a Path, dbpf::DbpfEntry)> {
    for pkg in &install.packages {
        if let Ok(index) = dbpf::read_package_index(pkg) {
            for e in &index.entries {
                if e.key.type_id == key.type_id
                    && e.key.group_id == key.group_id
                    && e.key.instance_id == key.instance_id
                {
                    return Some((pkg.as_path(), e.clone()));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn be_triple(inst: u32, grp: u32, typ: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&inst.to_be_bytes());
        b.extend_from_slice(&grp.to_be_bytes());
        b.extend_from_slice(&typ.to_be_bytes());
        b
    }

    #[test]
    fn mesh_table_parses() {
        let mut blob = 8u32.to_le_bytes().to_vec();
        blob.extend_from_slice(&2u32.to_be_bytes());
        blob.extend_from_slice(&be_triple(0x64028FE0, 0x41606100, 0x2F4E681B));
        blob.extend_from_slice(&be_triple(0xEC3B4B7E, 0x41606100, 0x2F4E681B));
        blob.extend_from_slice(&1u32.to_le_bytes());
        let g = parse_mesh_table(&blob).unwrap();
        assert_eq!(g.table_meshes.len(), 2);
        assert_eq!(g.table_meshes[0].instance_id, 0x64028FE0);
        assert_eq!(g.table_meshes[0].type_id, 0x2F4E681B);
        assert!(parse_mesh_table(&[0u8; 4]).is_none());
    }

    #[test]
    fn classify_accepts_and_rejects() {
        // Mesh-table variant (probe file shape).
        let mut blob = 8u32.to_le_bytes().to_vec();
        blob.extend_from_slice(&2u32.to_be_bytes());
        blob.extend_from_slice(&be_triple(0x64028FE0, 0x41606100, 0x2F4E681B));
        blob.extend_from_slice(&be_triple(0xEC3B4B7E, 0x41606100, 0x2F4E681B));
        blob.extend_from_slice(&1u32.to_le_bytes());
        match classify_table(&blob) {
            Some(TableKind::MeshTable(refs)) => assert_eq!(refs.len(), 2),
            other => panic!("expected MeshTable, got {:?}", other),
        }
        // Index-array variant: must NOT parse as mesh table.
        let mut other = 8u32.to_le_bytes().to_vec();
        other.extend_from_slice(&0u32.to_le_bytes());
        for v in [
            0x00030001u32,
            0x00040005,
            0x00060006,
            0x00070009,
            0x00080009,
            0x00090009,
            0x0007000Au32,
            0x00080009,
        ] {
            other.extend_from_slice(&v.to_le_bytes());
            other.extend_from_slice(&0u32.to_le_bytes());
            other.extend_from_slice(&0u32.to_le_bytes());
        }
        match classify_table(&other) {
            Some(TableKind::Other { count_field: 8 }) => {}
            other => panic!("expected Other, got {:?}", other),
        }
    }

    #[test]
    fn material_scan_validates() {
        let mut blob = vec![0xFFu8; 4];
        blob.extend_from_slice(&0x64028FE0u32.to_le_bytes());
        blob.extend_from_slice(&0x2F4E681Bu32.to_le_bytes());
        blob.extend_from_slice(&0x41606100u32.to_le_bytes());
        let mut known = HashSet::new();
        known.insert((0x2F4E681B, 0x41606100, 0x64028FE0));
        let refs = scan_material_mesh_refs(&blob, &known);
        assert_eq!(refs.len(), 1);
        // Unknown keys are rejected.
        assert!(scan_material_mesh_refs(&blob, &HashSet::new()).is_empty());
    }
}
