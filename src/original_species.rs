//! Strict link from an installed creature display model to its native editor
//! anatomy and the exact part definitions named by that anatomy.
//!
//! Creature abilities are derived only from those native rigblock definition
//! keys. This module deliberately does not synthesize a body/spine or choose a
//! visually similar creation recipe when an original reference is missing.

use crate::{
    asset_catalog::{Capabilities, PartCatalog, PartDefinition, ResourceIndex, PROP},
    baked_model,
    creature_data::{self, CreatureData, RigBlock},
    dbpf::ResourceKey,
    decode, parity,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Installed creature editor anatomy resource group (`CreatureModels`).
pub const CREATURE_ANATOMY_GROUP: u32 = 0x4062_6200;

#[derive(Debug, Clone)]
pub struct ResolvedPart {
    /// Original rigblock bytes and all native transform/morph/capability fields.
    pub block: RigBlock,
    /// The PROP key is copied directly from the rigblock's group/instance pair.
    pub definition: ResourceKey,
    pub model: ResourceKey,
    pub name: String,
    pub abilities: Capabilities,
}

#[derive(Debug, Clone)]
pub struct OriginalSpecies {
    /// The exact GMDL requested by the caller and parsed for display.
    pub display_model: ResourceKey,
    /// The matching installed creaturedata source: same instance, native
    /// CreatureModels group, and the creaturedata type.
    pub anatomy_key: ResourceKey,
    pub display_model_sha256: String,
    pub anatomy_sha256: String,
    /// Unmodified native metadata, including body/build transforms, capability
    /// tags, morph indices and animation IDs/values/weights.
    pub native: CreatureData,
    /// One entry per native rigblock in original order. Repeated definitions
    /// remain repeated because their separate transforms/morphs matter.
    pub parts: Vec<ResolvedPart>,
    /// Max-merged installed abilities from the exact referenced definitions.
    pub capabilities: Capabilities,
}

/// Resolve a displayed original creature GMDL to its same-instance native
/// creaturedata and exact part definitions. Any absent, malformed, ambiguous,
/// or unresolved source is an error; callers may then retain their existing
/// prototype fallback without receiving guessed original metadata.
pub fn resolve(
    index: &ResourceIndex,
    catalog: &PartCatalog,
    display_model: ResourceKey,
) -> Result<OriginalSpecies, String> {
    if display_model.type_id != decode::TYPE_GMDL
        || display_model.group_id != baked_model::CREATURE_GROUP
    {
        return Err(format!(
            "expected creature GMDL key, got {:08X}!{:08X}.{:08X}",
            display_model.group_id, display_model.instance_id, display_model.type_id
        ));
    }
    let display_bytes = index.read(display_model)?;
    let display = baked_model::parse(&display_bytes).map_err(|e| {
        format!(
            "creature GMDL {:08X} is not displayable: {e}",
            display_model.instance_id
        )
    })?;
    if display.pieces.is_empty() {
        return Err(format!(
            "creature GMDL {:08X} has no parsed geometry",
            display_model.instance_id
        ));
    }

    let anatomy_key = ResourceKey {
        type_id: creature_data::TYPE,
        group_id: CREATURE_ANATOMY_GROUP,
        instance_id: display_model.instance_id,
    };
    let anatomy_bytes = index.read(anatomy_key).map_err(|_| {
        format!(
            "creature GMDL {:08X} has no same-instance native anatomy at {:08X}!{:08X}.{:08X}",
            display_model.instance_id,
            anatomy_key.group_id,
            anatomy_key.instance_id,
            anatomy_key.type_id
        )
    })?;
    let native = creature_data::parse(&anatomy_bytes)
        .map_err(|e| format!("native anatomy {:08X}: {e}", anatomy_key.instance_id))?;
    resolve_verified_sources(
        display_model,
        &display_bytes,
        anatomy_key,
        &anatomy_bytes,
        native,
        catalog,
    )
}

/// Resolve already-identified source bytes. Kept separate so the strict
/// identity/definition rules can be tested without an installed package.
fn resolve_verified_sources(
    display_model: ResourceKey,
    display_bytes: &[u8],
    anatomy_key: ResourceKey,
    anatomy_bytes: &[u8],
    native: CreatureData,
    catalog: &PartCatalog,
) -> Result<OriginalSpecies, String> {
    if display_model.type_id != decode::TYPE_GMDL
        || display_model.group_id != baked_model::CREATURE_GROUP
    {
        return Err("display source is not a creature GMDL".into());
    }
    if anatomy_key.type_id != creature_data::TYPE
        || anatomy_key.group_id != CREATURE_ANATOMY_GROUP
        || anatomy_key.instance_id != display_model.instance_id
    {
        return Err("native anatomy key does not match the displayed creature GMDL".into());
    }
    native.validate_anatomy().map_err(|e| {
        format!(
            "invalid native anatomy {:08X}: {e}",
            anatomy_key.instance_id
        )
    })?;

    let mut parts = Vec::with_capacity(native.rigblocks.len());
    let mut unresolved = BTreeMap::new();
    let mut ambiguous = BTreeMap::new();
    for block in &native.rigblocks {
        let definition = ResourceKey {
            type_id: PROP,
            group_id: block.group_id(),
            instance_id: block.instance_id(),
        };
        let mut matches = catalog.parts.iter().filter(|part| part.key == definition);
        let Some(part) = matches.next() else {
            unresolved.insert(
                (
                    definition.group_id,
                    definition.instance_id,
                    definition.type_id,
                ),
                definition,
            );
            continue;
        };
        if matches.next().is_some() {
            ambiguous.insert(
                (
                    definition.group_id,
                    definition.instance_id,
                    definition.type_id,
                ),
                definition,
            );
            continue;
        }
        parts.push(resolved_part(block.clone(), definition, part));
    }
    if !unresolved.is_empty() || !ambiguous.is_empty() {
        let list = |keys: &BTreeMap<(u32, u32, u32), ResourceKey>| {
            keys.iter()
                .map(|(_, k)| format!("{:08X}!{:08X}.{:08X}", k.group_id, k.instance_id, k.type_id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut problems = Vec::new();
        if !unresolved.is_empty() {
            problems.push(format!(
                "unresolved rigblock definitions [{}]",
                list(&unresolved)
            ));
        }
        if !ambiguous.is_empty() {
            problems.push(format!(
                "ambiguous rigblock definitions [{}]",
                list(&ambiguous)
            ));
        }
        return Err(problems.join("; "));
    }
    if parts.len() != native.rigblocks.len() {
        return Err("not every native rigblock resolved to one part definition".into());
    }

    let mut capabilities = Capabilities::default();
    for part in &parts {
        capabilities.merge(part.abilities);
    }
    Ok(OriginalSpecies {
        display_model,
        anatomy_key,
        display_model_sha256: parity::sha256(display_bytes),
        anatomy_sha256: parity::sha256(anatomy_bytes),
        native,
        parts,
        capabilities,
    })
}

fn resolved_part(block: RigBlock, definition: ResourceKey, part: &PartDefinition) -> ResolvedPart {
    ResolvedPart {
        block,
        definition,
        model: part.model,
        name: part.name.clone(),
        abilities: part.abilities,
    }
}

fn key_json(key: ResourceKey) -> Value {
    json!({
        "type": format!("{:08X}", key.type_id),
        "group": format!("{:08X}", key.group_id),
        "instance": format!("{:08X}", key.instance_id),
    })
}

fn capabilities_json(a: Capabilities) -> Value {
    json!({
        "herbivore": a.herbivore, "carnivore": a.carnivore,
        "health": a.health, "speed": a.speed, "bite": a.bite,
        "strike": a.strike, "charge": a.charge, "spit": a.spit,
        "sing": a.sing, "dance": a.dance, "charm": a.charm, "pose": a.pose,
        "jump": a.jump, "sprint": a.sprint, "glide": a.glide, "sneak": a.sneak,
        "mouth": a.mouth, "eye": a.eye, "foot": a.foot, "grasper": a.grasper,
        "cell_flagella": a.cell_flagella, "cell_cilia": a.cell_cilia,
        "cell_jet": a.cell_jet, "cell_spike": a.cell_spike,
        "cell_poison": a.cell_poison, "cell_electric": a.cell_electric,
    })
}

/// Build an installed-data evidence report. This is intentionally a census,
/// not an NPC fallback policy: every source key, missing pair, unresolved part
/// definition and strict-resolution error remains explicit.
pub fn audit(index: &ResourceIndex, catalog: &PartCatalog) -> Value {
    let mut anatomy_keys: Vec<_> = index
        .records
        .keys()
        .filter(|key| key.type_id == creature_data::TYPE && key.group_id == CREATURE_ANATOMY_GROUP)
        .copied()
        .collect();
    anatomy_keys.sort_by_key(|k| k.instance_id);
    let mut display_keys: Vec<_> = index
        .records
        .keys()
        .filter(|key| {
            key.type_id == decode::TYPE_GMDL && key.group_id == baked_model::CREATURE_GROUP
        })
        .copied()
        .collect();
    display_keys.sort_by_key(|k| k.instance_id);

    let display_by_instance: BTreeMap<_, _> = display_keys
        .iter()
        .map(|key| (key.instance_id, *key))
        .collect();
    let anatomy_by_instance: BTreeMap<_, _> = anatomy_keys
        .iter()
        .map(|key| (key.instance_id, *key))
        .collect();
    let mut matched_pairs = Vec::new();
    let mut resolved_species = Vec::new();
    let mut unresolved_definitions = BTreeMap::new();
    let mut ambiguous_definitions = BTreeMap::new();
    let mut resolution_errors = Vec::new();
    let mut resolved_count = 0;

    for anatomy_key in &anatomy_keys {
        let Some(display_key) = display_by_instance.get(&anatomy_key.instance_id).copied() else {
            continue;
        };
        matched_pairs.push(
            json!({"display_gmdl":key_json(display_key),"native_anatomy":key_json(*anatomy_key)}),
        );
        let bytes = match index.read(*anatomy_key) {
            Ok(bytes) => bytes,
            Err(error) => {
                resolution_errors.push(json!({"anatomy":key_json(*anatomy_key),"error":error}));
                continue;
            }
        };
        let native = match creature_data::parse(&bytes) {
            Ok(native) => native,
            Err(error) => {
                resolution_errors.push(json!({"anatomy":key_json(*anatomy_key),"error":error}));
                continue;
            }
        };
        for block in &native.rigblocks {
            let definition = ResourceKey {
                type_id: PROP,
                group_id: block.group_id(),
                instance_id: block.instance_id(),
            };
            match catalog
                .parts
                .iter()
                .filter(|part| part.key == definition)
                .count()
            {
                0 => {
                    unresolved_definitions.insert(
                        (
                            definition.group_id,
                            definition.instance_id,
                            definition.type_id,
                        ),
                        definition,
                    );
                }
                n if n > 1 => {
                    ambiguous_definitions.insert(
                        (
                            definition.group_id,
                            definition.instance_id,
                            definition.type_id,
                        ),
                        definition,
                    );
                }
                _ => {}
            }
        }
        match resolve(index, catalog, display_key) {
            Ok(species) => {
                resolved_count += 1;
                resolved_species.push(json!({
                    "display_gmdl": key_json(species.display_model),
                    "native_anatomy": key_json(species.anatomy_key),
                    "display_model_sha256": species.display_model_sha256,
                    "anatomy_sha256": species.anatomy_sha256,
                    "model_type": format!("{:08X}", species.native.model_type()),
                    "capabilities": capabilities_json(species.capabilities),
                    "native_capability_tags": species.native.capability_ids,
                    "native_capability_levels": species.native.capability_levels,
                    "animation_ids": species.native.animation_ids,
                    "animation_value_bits": species.native.animation_value_bits,
                    "animation_weight_bits": species.native.animation_weight_bits,
                    "parts": species.parts.iter().map(|part| json!({
                        "block_index": part.block.index(),
                        "block_type": part.block.block_type(),
                        "definition": key_json(part.definition),
                        "model": key_json(part.model),
                        "name": part.name,
                        "morph_index": part.block.morph_index(),
                        "scale_bits": part.block.scale_bits(),
                        "capability_index": part.block.capability_index(),
                        "capability_count": part.block.capability_count(),
                    })).collect::<Vec<_>>(),
                }));
            }
            Err(error) => resolution_errors.push(json!({
                "display_gmdl":key_json(display_key),
                "native_anatomy":key_json(*anatomy_key),
                "error":error,
            })),
        }
    }
    let unmatched_anatomy: Vec<_> = anatomy_keys
        .iter()
        .filter(|key| !display_by_instance.contains_key(&key.instance_id))
        .copied()
        .map(key_json)
        .collect();
    let unmatched_display_gmdls: Vec<_> = display_keys
        .iter()
        .filter(|key| !anatomy_by_instance.contains_key(&key.instance_id))
        .copied()
        .map(key_json)
        .collect();
    let unresolved_definitions: Vec<_> =
        unresolved_definitions.into_values().map(key_json).collect();
    let ambiguous_definitions: Vec<_> = ambiguous_definitions.into_values().map(key_json).collect();

    json!({
        "version": 1,
        "kind": "OriginalSpeciesSourceLinkAudit",
        "rules": {
            "display_gmdl_group": format!("{:08X}", baked_model::CREATURE_GROUP),
            "native_anatomy_group": format!("{:08X}", CREATURE_ANATOMY_GROUP),
            "native_anatomy_type": format!("{:08X}", creature_data::TYPE),
            "anatomy_instance_must_equal_gmdl_instance": true,
            "rigblock_definition_key": "PROP type + native rigblock group_id + native rigblock instance_id",
            "capability_merge": "per-field maximum across exact resolved PartCatalog definitions",
        },
        "counts": {
            "display_gmdls": display_keys.len(),
            "native_anatomy_records": anatomy_keys.len(),
            "same_instance_pairs": matched_pairs.len(),
            "strictly_resolved_pairs": resolved_count,
            "unmatched_display_gmdls": unmatched_display_gmdls.len(),
            "unmatched_anatomy_records": unmatched_anatomy.len(),
            "unresolved_definition_keys": unresolved_definitions.len(),
            "ambiguous_definition_keys": ambiguous_definitions.len(),
            "resolution_errors": resolution_errors.len(),
        },
        "matched_pairs": matched_pairs,
        "resolved_species": resolved_species,
        "unmatched_display_gmdls": unmatched_display_gmdls,
        "unmatched_anatomy_records": unmatched_anatomy,
        "unresolved_definitions": unresolved_definitions,
        "ambiguous_definitions": ambiguous_definitions,
        "resolution_errors": resolution_errors,
        "part_catalog_errors": catalog.errors,
        "index_errors": index.errors,
        "limitation": "Resolved abilities are the installed PartCatalog capability properties of the exact native rigblock definitions. Native creature tuning and game behavior remain an adaptation; no anatomy or missing definition is guessed.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(type_id: u32, group_id: u32, instance_id: u32) -> ResourceKey {
        ResourceKey {
            type_id,
            group_id,
            instance_id,
        }
    }

    fn one_block_data(group: u32, instance: u32) -> CreatureData {
        let mut raw = vec![0; 140];
        raw[0..2].copy_from_slice(&0i16.to_le_bytes());
        raw[4..6].copy_from_slice(&(-1i16).to_le_bytes());
        raw[6..8].copy_from_slice(&(-1i16).to_le_bytes());
        raw[0x0e..0x10].copy_from_slice(&(-1i16).to_le_bytes());
        raw[0x84..0x88].copy_from_slice(&group.to_le_bytes());
        raw[0x88..0x8c].copy_from_slice(&instance.to_le_bytes());
        CreatureData {
            version: 9,
            properties: vec![0; 128],
            encoded_properties: vec![],
            rigblocks: vec![RigBlock { raw_bytes: raw }],
            capability_ids: vec![],
            capability_levels: vec![],
            animation_ids: vec![],
            animation_value_bits: vec![],
            animation_weight_bits: vec![],
            field_110: 0,
            effects: vec![],
            consumed_bytes: 0,
            trailing_bytes: vec![],
        }
    }

    fn test_part(definition: ResourceKey, abilities: Capabilities) -> PartDefinition {
        PartDefinition {
            key: definition,
            model: key(
                crate::asset_catalog::RW4,
                definition.group_id + 1,
                definition.instance_id,
            ),
            name: "fixture part".into(),
            is_null: false,
            price: 0,
            unlock_level: 0,
            min_scale: 0.5,
            max_scale: 2.0,
            default_scale: 1.0,
            model_scale: 1.0,
            model_offset: [0.0; 3],
            model_rotation: [0.0; 3],
            diffuse: None,
            tint: None,
            spec_bump: None,
            abilities,
        }
    }

    #[test]
    fn exact_native_rigblock_key_drives_part_identity_and_abilities() {
        let model = key(decode::TYPE_GMDL, baked_model::CREATURE_GROUP, 0x1234);
        let anatomy = key(creature_data::TYPE, CREATURE_ANATOMY_GROUP, 0x1234);
        let definition = key(PROP, crate::asset_catalog::CREATURE_PARTS, 0x5678);
        let catalog = PartCatalog {
            parts: vec![test_part(
                definition,
                Capabilities {
                    bite: 2,
                    speed: 3,
                    ..Default::default()
                },
            )],
            errors: vec![],
        };
        let resolved = resolve_verified_sources(
            model,
            b"gmdl fixture",
            anatomy,
            b"anatomy fixture",
            one_block_data(definition.group_id, definition.instance_id),
            &catalog,
        )
        .unwrap();
        assert_eq!(resolved.parts.len(), 1);
        assert_eq!(resolved.parts[0].definition, definition);
        assert_eq!(resolved.parts[0].model.instance_id, definition.instance_id);
        assert_eq!(
            (resolved.capabilities.bite, resolved.capabilities.speed),
            (2, 3)
        );
        assert_eq!(resolved.anatomy_key, anatomy);
        assert_eq!(resolved.display_model, model);
    }

    #[test]
    fn refuses_wrong_source_pair_and_unresolved_definition_instead_of_guessing() {
        let model = key(decode::TYPE_GMDL, baked_model::CREATURE_GROUP, 0x1234);
        let wrong_anatomy = key(creature_data::TYPE, CREATURE_ANATOMY_GROUP, 0x1235);
        let data = one_block_data(crate::asset_catalog::CREATURE_PARTS, 0x5678);
        let empty = PartCatalog::default();
        assert!(resolve_verified_sources(
            model,
            b"gmdl",
            wrong_anatomy,
            b"anatomy",
            data.clone(),
            &empty
        )
        .unwrap_err()
        .contains("does not match"));
        let anatomy = key(creature_data::TYPE, CREATURE_ANATOMY_GROUP, 0x1234);
        assert!(
            resolve_verified_sources(model, b"gmdl", anatomy, b"anatomy", data, &empty)
                .unwrap_err()
                .contains("40626000!00005678.00B1B104")
        );
    }

    #[test]
    #[ignore = "requires installed original Spore packages; writes an ignored target audit"]
    fn installed_creature_gmdls_link_to_same_instance_anatomy_and_exact_definitions() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let catalog = PartCatalog::from_index(&index);
        let report = audit(&index, &catalog);
        let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/original-species-063/audit.json");
        std::fs::create_dir_all(output.parent().unwrap()).unwrap();
        std::fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        assert!(index.errors.is_empty(), "{:?}", index.errors);
        assert_eq!(
            report["counts"]["same_instance_pairs"].as_u64(),
            Some(report["matched_pairs"].as_array().unwrap().len() as u64)
        );
        assert!(report["counts"]["native_anatomy_records"].as_u64().unwrap() > 0);
        println!("Installed original species audit: {}", output.display());
        println!(
            "{}",
            serde_json::to_string_pretty(&report["counts"]).unwrap()
        );
    }
}
