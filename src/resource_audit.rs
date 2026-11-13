//! Read-only installed-data diagnostics through the runtime ResourceIndex.
use crate::{asset_catalog::ResourceIndex, dbpf::ResourceKey, prop};

pub fn key_json(key: ResourceKey) -> serde_json::Value {
    serde_json::json!({
        "group":format!("{:08X}",key.group_id),
        "instance":format!("{:08X}",key.instance_id),
        "type":format!("{:08X}",key.type_id),
    })
}

/// Headers of every effective native bit-image resource, without assuming a
/// texture-container format or interpreting packed color/height channels.
pub fn run_world_images() {
    let result = crate::spore_install::discover().map(|install| {
        let index=ResourceIndex::load(&install.packages);
        let mut keys:Vec<_>=index.records.keys().copied().filter(|k|
            crate::bitmap::supported(k.type_id) || k.type_id == 0x03E421F0).collect();
        keys.sort_by_key(|k|(k.type_id,k.group_id,k.instance_id));
        let mut rows=vec![];let mut errors=vec![];let mut decoded=0;
        for key in keys {match index.read(key) {
            Ok(bytes)=>{
                let header=&bytes[..bytes.len().min(32)];
                let le:Vec<_>=header.chunks_exact(4).map(|b|u32::from_le_bytes(b.try_into().unwrap())).collect();
                let be:Vec<_>=header.chunks_exact(4).map(|b|u32::from_be_bytes(b.try_into().unwrap())).collect();
                let parsed = match crate::bitmap::parse(key.type_id,&bytes) {
                    Ok(bitmap) => { decoded += 1; serde_json::json!({"width":bitmap.width,"height":bitmap.height,"channels":bitmap.channel_count,"pixel_bytes":bitmap.data.len(),"preview_bytes":bitmap.rgba8().len(),"unknown":bitmap.unknown,"trailing_bytes":bitmap.trailing.len()}) },
                    Err(e) => { errors.push(serde_json::json!({"key":key_json(key),"error":e})); serde_json::Value::Null },
                };
                rows.push(serde_json::json!({"key":key_json(key),"bytes":bytes.len(),"header_le":le,"header_be":be,"decoded":parsed}));
            },Err(e)=>errors.push(serde_json::json!({"key":key_json(key),"error":e})),
        }}
        serde_json::json!({"images":rows,"decoded_images":decoded,"errors":errors,"index_errors":index.errors})
    });
    match result {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

pub fn dump_properties(ids: &[u32]) -> Result<serde_json::Value, String> {
    if ids.is_empty() {
        return Err("provide at least one hexadecimal property ID".into());
    }
    let install = crate::spore_install::discover()?;
    let index = ResourceIndex::load(&install.packages);
    let mut keys: Vec<_> = index
        .records
        .keys()
        .copied()
        .filter(|k| k.type_id == crate::asset_catalog::PROP)
        .collect();
    keys.sort_by_key(|k| (k.group_id, k.instance_id));
    let mut rows = vec![];
    let mut errors = vec![];
    for key in &keys {
        let properties = match index.read(*key).and_then(|b| prop::parse(&b)) {
            Ok(p) => p,
            Err(e) => {
                errors.push(serde_json::json!({"key":key_json(*key),"error":e}));
                continue;
            }
        };
        if !ids.iter().any(|id| properties.0.contains_key(id)) {
            continue;
        }
        let mut fields = serde_json::Map::new();
        // Include the entire matching definition: linked world resources and
        // transforms are needed to reproduce the config, not just the selector.
        for (id, property) in properties.0 {
            let values: Vec<_> = property.values.iter().map(|value| {
                let scalar = prop::Property { kind: property.kind, array: false, values:vec![value.clone()] };
                if let Some(k) = scalar.key() { key_json(k) }
                else if let Some(v) = scalar.vector3() { serde_json::json!(v) }
                else if let Some(v) = scalar.color_rgb() { serde_json::json!(v) }
                else if let Some(v) = scalar.vector4() { serde_json::json!(v) }
                else if let Some(v) = scalar.number() { serde_json::json!(v) }
                else if let Some((table, instance, text)) = scalar.text() {
                    serde_json::json!({"table":format!("{table:08X}"),"instance":format!("{instance:08X}"),"text":text})
                } else { serde_json::json!({"bytes":value.len(),"hex":value.iter().map(|b| format!("{b:02X}")).collect::<String>()}) }
            }).collect();
            fields.insert(
                format!("{id:08X}"),
                serde_json::json!({"kind":property.kind,"array":property.array,"values":values}),
            );
        }
        let package = index.records[key]
            .0
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        rows.push(serde_json::json!({"key":key_json(*key),"package":package,"properties":fields}));
    }
    Ok(
        serde_json::json!({"scanned":keys.len(),"matches":rows,"errors":errors,"index_errors":index.errors}),
    )
}

pub fn run_properties() {
    let args: Vec<_> = std::env::args().collect();
    let result = args
        .iter()
        .position(|a| a == "--dump-properties")
        .and_then(|i| args.get(i + 1))
        .ok_or_else(|| "--dump-properties requires comma-separated hexadecimal IDs".to_string())
        .and_then(|value| {
            value
                .split(',')
                .map(|id| {
                    u32::from_str_radix(
                        id.trim().trim_start_matches("0x").trim_start_matches("0X"),
                        16,
                    )
                    .map_err(|_| format!("invalid property ID {id}"))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .and_then(|ids| dump_properties(&ids));
    match result {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
