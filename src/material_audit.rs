//! Census of original RW4 material bindings using the runtime readers.
use crate::{
    asset_catalog::{ResourceIndex, RW4},
    material_state,
    resource_audit::key_json,
    rw4,
};
use std::collections::BTreeMap;

pub fn audit(index: &ResourceIndex) -> serde_json::Value {
    let mut keys: Vec<_> = index
        .records
        .keys()
        .copied()
        .filter(|k| k.type_id == RW4)
        .collect();
    keys.sort_by_key(|k| (k.group_id, k.instance_id));
    let mut rows = vec![];
    let mut errors = vec![];
    let mut renderers = BTreeMap::<String, usize>::new();
    let mut record_count = 0;
    let mut state_count = 0;
    let mut link_count = 0;
    for key in &keys {
        let bytes = match index.read(*key) {
            Ok(b) => b,
            Err(e) => {
                errors.push(serde_json::json!({"key":key_json(*key),"error":e}));
                continue;
            }
        };
        let sections = match rw4::sections(&bytes) {
            Ok(s) => s,
            Err(e) => {
                errors.push(serde_json::json!({"key":key_json(*key),"error":e}));
                continue;
            }
        };
        let material_sections: Vec<_> = sections
            .iter()
            .enumerate()
            .filter(|(_, s)| s.0 == rw4::T_COMPILED_STATE)
            .collect();
        if material_sections.is_empty() {
            continue;
        }
        record_count += 1;
        let links = match rw4::material_links(&bytes) {
            Ok(l) => l,
            Err(e) => {
                errors.push(serde_json::json!({"key":key_json(*key),"error":e}));
                vec![]
            }
        };
        link_count += links.len();
        for (section, (_, payload)) in material_sections {
            let state = match material_state::parse(payload) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(
                        serde_json::json!({"key":key_json(*key),"section":section,"error":e}),
                    );
                    continue;
                }
            };
            state_count += 1;
            let renderer = format!("{:08X}", state.renderer_id);
            *renderers.entry(renderer.clone()).or_default() += 1;
            let mut textures = vec![];
            for slot in &state.texture_slots {
                let reference = slot.raster_reference;
                let target = if reference >> 22 == 0 {
                    sections.get(reference as usize)
                } else {
                    None
                };
                let decoded = if target.is_some_and(|s| s.0 == 0x20003) {
                    crate::texture::decode_raster(&bytes, reference as usize)
                } else {
                    None
                };
                textures.push(serde_json::json!({"sampler":slot.sampler,"reference":format!("{reference:08X}"),"section_type":target.map(|s|format!("{:08X}",s.0)),"decoded_dimensions":decoded.map(|p|[p.width,p.height]),"stage_mask":format!("{:08X}",slot.stage_mask),"stage_states":slot.stage_states,"sampler_mask":format!("{:08X}",slot.sampler_mask),"sampler_states":slot.sampler_states}));
            }
            let meshes: Vec<_> = links
                .iter()
                .filter(|l| l.state_sections.contains(&section))
                .map(|l| l.mesh_section)
                .collect();
            rows.push(serde_json::json!({"key":key_json(*key),"section":section,"renderer":renderer,"primitive":state.primitive,"flags":state.flags,"field_14":state.field_14,"mesh_sections":meshes,"textures":textures,"uniforms":state.shader_data.iter().map(|u|serde_json::json!({"index":u.index,"offset":u.offset,"words":u.words,"object_reference":u.object_reference})).collect::<Vec<_>>(),"render_states":state.render_states,"material_color":state.material_color,"ambient_color":state.ambient_color,"trailing_bytes":state.trailing.len()}));
        }
    }
    serde_json::json!({"rw4_records":keys.len(),"material_records":record_count,"decoded_states":state_count,"mesh_links":link_count,"renderers":renderers,"states":rows,"errors":errors,"index_errors":index.errors})
}
pub fn run() {
    match crate::spore_install::discover() {
        Ok(install) => println!(
            "{}",
            serde_json::to_string_pretty(&audit(&ResourceIndex::load(&install.packages))).unwrap()
        ),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
