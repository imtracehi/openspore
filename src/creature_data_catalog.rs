//! Original creature anatomy inventory. Errors and graph diagnostics remain
//! visible; an undecodable creature must never become a placeholder record.
use crate::{asset_catalog::ResourceIndex, creature_data, parity, resource_audit};
use serde_json::{json, Value};
use std::path::Path;

pub fn audit(index: &ResourceIndex) -> Value {
    let mut keys: Vec<_> = index
        .records
        .keys()
        .filter(|key| key.type_id == creature_data::TYPE)
        .copied()
        .collect();
    keys.sort_by_key(|k| (k.group_id, k.instance_id));
    let mut records = vec![];
    let mut errors = vec![];
    let mut rigblocks = 0;
    let mut capabilities = 0;
    let mut animations = 0;
    for key in keys {
        let result = index.read(key).and_then(|bytes| {
            let data = creature_data::parse(&bytes)?;
            if data.original_layout()? != bytes {
                return Err("original bytes did not round-trip".into());
            }
            Ok((bytes, data))
        });
        match result {
            Ok((bytes, data)) => {
                rigblocks += data.rigblocks.len();
                capabilities += data.capability_ids.len();
                animations += data.animation_ids.len();
                let anatomy = data.validate_anatomy().err();
                let block_views:Vec<_>=data.rigblocks.iter().map(|b|json!({"index":b.index(),"parent_index":b.parent_index(),"symmetric_index":b.symmetric_index(),
                    "flags":b.flags(),"type":b.block_type(),"capability_index":b.capability_index(),"capability_count":b.capability_count(),"morph_index":b.morph_index(),
                    "group_id":b.group_id(),"instance_id":b.instance_id(),"position_bits":b.position_bits(),"orientation_bits":b.orientation_bits(),
                    "bounding_box_bits":b.bounding_box_bits(),"scale_bits":b.scale_bits()})).collect();
                records.push(json!({"key":resource_audit::key_json(key),"package":index.records[&key].0,"source_sha256":parity::sha256(&bytes),"source_bytes":bytes.len(),
                    "lossless":true,"model_type":data.model_type(),"anatomy_diagnostic":anatomy,"rigblock_views":block_views,"data":data}));
            }
            Err(error) => errors.push(json!({"key":resource_audit::key_json(key),"error":error})),
        }
    }
    json!({"version":1,"kind":"InstalledCreatureDataInventory","decoded_records":records.len(),"rigblocks":rigblocks,"capabilities":capabilities,"animation_values":animations,
        "records":records,"errors":errors,"index_errors":index.errors,
        "limitation":"Native anatomy bytes decoded and retained. This is not the CRT creation graph, generated body mesh, animation binding or a scene parity comparison."})
}
pub fn run(output: &Path, copied_package: Option<&Path>) -> Result<(), String> {
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let parent = output
        .parent()
        .ok_or("creature data output needs a parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if !parent.starts_with(&target) {
        return Err("original creature data exports must stay under ignored target".into());
    }
    let output = parent.join(output.file_name().ok_or("missing output filename")?);
    // Refuse an existing symlink/junction leaf that could escape the output
    // root even though its parent passed canonicalization.
    if output.exists()
        && !output
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&target)
    {
        return Err("creature data output resolves outside ignored target".into());
    }
    let install = crate::spore_install::discover()?;
    let mut packages = install.packages;
    if let Some(package) = copied_package {
        let package = package.canonicalize().map_err(|e| e.to_string())?;
        if !package.starts_with(&target) {
            return Err("profile cache inspection requires a copy below ignored target".into());
        }
        packages.push(package);
    }
    let index = ResourceIndex::load(&packages);
    let report = audit(&index);
    std::fs::write(
        output,
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("Original creature data: {} records, {} rig blocks, {} capability entries, {} animation values; {} errors",report["decoded_records"],report["rigblocks"],report["capabilities"],report["animation_values"],report["errors"].as_array().unwrap().len());
    if !report["errors"].as_array().unwrap().is_empty() || !index.errors.is_empty() {
        return Err("original creature data inventory has decode/index errors; see report".into());
    }
    if report["decoded_records"] == 0 {
        return Err("packages contain no native creature data records; inspect a copied original GraphicsCache.package".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires installed original Spore packages"]
    fn installed_creature_data_retains_all_original_bytes() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let report = super::audit(&index);
        assert!(index.errors.is_empty(), "{:?}", index.errors);
        assert!(
            report["errors"].as_array().unwrap().is_empty(),
            "{}",
            report["errors"]
        );
        assert!(report["decoded_records"].as_u64().unwrap() > 0);
        println!(
            "{} installed records; {} rig blocks; all original bytes preserved",
            report["decoded_records"], report["rigblocks"]
        );
    }
    #[test]
    #[ignore = "requires target/creature-data-native-056/GraphicsCache.package read-only snapshot"]
    fn copied_native_dbbf_cache_has_bounded_index_and_retrievable_resources() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/creature-data-native-056/GraphicsCache.package");
        let index = crate::dbpf::read_package_index(&path).unwrap();
        assert!(index.header.is_dbbf);
        assert!(!index.entries.is_empty());
        let mut total = 0;
        for entry in &index.entries {
            let bytes = crate::dbpf::read_entry_bytes(&path, entry).unwrap();
            assert_eq!(bytes.len(), entry.mem_size as usize);
            total += bytes.len();
        }
        println!(
            "Copied DBBF cache: {} original resources; {total} uncompressed bytes retrieved",
            index.entries.len()
        );
    }
}
