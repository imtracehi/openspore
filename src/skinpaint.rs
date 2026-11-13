//! Executes installed tint and bump-to-normal pixel programs for original
//! part textures. Whole-creature body/brush/atlas composition remains separate.
use crate::{
    asset_catalog::ResourceIndex, dbpf::ResourceKey, shader_pack, shader_vm::PixelKernel,
    texture::Pixels,
};
pub const APP_PAINT_DEFAULTS: ResourceKey = ResourceKey {
    instance_id: 0x08ea246b,
    group_id: 0,
    type_id: crate::asset_catalog::PROP,
};
#[derive(Debug, Clone, PartialEq)]
pub struct PaintParameters {
    pub source: Option<ResourceKey>,
    pub bump_height: f32,
    pub part_bump_scale: f32,
}
impl Default for PaintParameters {
    fn default() -> Self {
        Self {
            source: None,
            bump_height: 5.,
            part_bump_scale: 1.,
        }
    }
}
impl PaintParameters {
    fn from_properties(properties: &crate::prop::PropertyList) -> Result<Self, String> {
        let float = |id: u32, fallback: f32| -> Result<f32, String> {
            let Some(p) = properties.0.get(&id) else {
                return Ok(fallback);
            };
            if p.kind != 13 || p.array || p.values.len() != 1 {
                return Err(format!("paint setting {id:08X} is not a scalar float"));
            }
            let value = p
                .number()
                .filter(|v| v.is_finite())
                .ok_or("invalid native paint setting")?;
            Ok(if value == 0. { fallback } else { value })
        };
        Ok(Self {
            source: Some(APP_PAINT_DEFAULTS),
            bump_height: float(0x0c19db33, 5.)?,
            part_bump_scale: float(0x1c76d9b5, 1.)?,
        })
    }
    fn load(index: &ResourceIndex) -> Result<Self, String> {
        if index.records.contains_key(&APP_PAINT_DEFAULTS) {
            Self::from_properties(&index.properties(APP_PAINT_DEFAULTS)?)
        } else {
            Ok(Self::default())
        }
    }
}
#[derive(Debug, Clone)]
pub struct NativePainter {
    pub source: ResourceKey,
    pub pack: String,
    pub kernel: PixelKernel,
    pub normal_kernel: PixelKernel,
    pub parameters: PaintParameters,
    pub tint_bytecode: Vec<u32>,
    pub normal_bytecode: Vec<u32>,
    pub copy_bytecode: Vec<u32>,
}
pub fn load(index: &ResourceIndex) -> Result<NativePainter, String> {
    let parameters = PaintParameters::load(index)?;
    let mut keys: Vec<_> = index
        .records
        .keys()
        .copied()
        .filter(|k| k.type_id == shader_pack::TYPE_ID && k.group_id == shader_pack::GROUP_COMPILED)
        .collect();
    keys.sort_by_key(|k| k.instance_id);
    let mut choices = vec![];
    for key in keys {
        let pack = shader_pack::parse(&index.read(key)?)?;
        let normal = pack
            .standards
            .iter()
            .find(|s| s.header.id == 0x700000b2)
            .ok_or("installed skpBumpToNormal shader missing")?;
        if normal.entries.len() != 1 {
            return Err("ambiguous native bump-to-normal profile".into());
        }
        let normal_entry = &normal.entries[0];
        if normal_entry.pixel_uniforms.len() != 1
            || normal_entry.pixel_uniforms[0].data_index != 0x206
            || normal_entry.pixel_uniforms[0].register != 0
            || normal_entry.pixel_uniforms[0].register_size != 1
        {
            return Err("unrecognized native normal-conversion uniform bindings".into());
        }
        let normal_kernel = PixelKernel::compile(&crate::d3d9_shader::parse(&normal_entry.pixel)?)?;
        let normal_bytecode = crate::native_paint::shader_words(&normal_entry.pixel)?;
        let copy = pack
            .standards
            .iter()
            .find(|s| s.header.id == 0x700000af)
            .ok_or("installed native copy shader missing")?;
        if copy.entries.len() != 1 {
            return Err("ambiguous native copy shader profile".into());
        }
        let copy_bytecode = crate::native_paint::shader_words(&copy.entries[0].pixel)?;
        for shader in pack.standards.iter().filter(|s| s.header.id == 0x700000ad) {
            for entry in &shader.entries {
                if entry.pixel_uniforms.len() != 1
                    || entry.pixel_uniforms[0].data_index != 0x206
                    || entry.pixel_uniforms[0].register != 0
                    || entry.pixel_uniforms[0].register_size != 3
                {
                    return Err("unrecognized native SkinPaint uniform bindings".into());
                }
                let kernel = PixelKernel::compile(&crate::d3d9_shader::parse(&entry.pixel)?)?;
                choices.push(NativePainter {
                    source: key,
                    pack: pack.name.clone(),
                    kernel,
                    normal_kernel: normal_kernel.clone(),
                    parameters: parameters.clone(),
                    tint_bytecode: crate::native_paint::shader_words(&entry.pixel)?,
                    normal_bytecode: normal_bytecode.clone(),
                    copy_bytecode: copy_bytecode.clone(),
                });
            }
        }
    }
    choices.sort_by(|a, b| {
        a.pack
            .cmp(&b.pack)
            .then(a.kernel.version.cmp(&b.kernel.version))
    });
    choices
        .into_iter()
        .next()
        .ok_or("installed skpSplatTint shader missing".into())
}
/// Base-mip linear sampling. The texture values remain in native UNORM space;
/// no speculative sRGB conversion is inserted into the original shader math.
fn sample(p: &Pixels, uv: [f32; 2]) -> [f32; 4] {
    let x = uv[0] * p.width as f32 - 0.5;
    let y = uv[1] * p.height as f32 - 0.5;
    let at = |x: i32, y: i32| {
        let x = x.clamp(0, p.width as i32 - 1) as usize;
        let y = y.clamp(0, p.height as i32 - 1) as usize;
        let at = (y * p.width as usize + x) * 4;
        std::array::from_fn::<_, 4, _>(|i| p.rgba[at + i] as f32 / 255.)
    };
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let a = at(ix, iy);
    let tx = x - ix as f32;
    let ty = y - iy as f32;
    if tx == 0. && ty == 0. {
        return a;
    }
    let b = at(ix + 1, iy);
    let c = at(ix, iy + 1);
    let d = at(ix + 1, iy + 1);
    std::array::from_fn(|i| {
        let top = a[i] + (b[i] - a[i]) * tx;
        let bottom = c[i] + (d[i] - c[i]) * tx;
        top + (bottom - top) * ty
    })
}
fn validate(p: &Pixels) -> Result<(), String> {
    if p.width == 0
        || p.height == 0
        || p.width > 4096
        || p.height > 4096
        || p.rgba.len() != p.width as usize * p.height as usize * 4
    {
        return Err("invalid SkinPaint texture dimensions".into());
    }
    Ok(())
}
impl NativePainter {
    pub fn native_part_normal(
        &self,
        index: &ResourceIndex,
        key: ResourceKey,
    ) -> Result<Pixels, String> {
        let source = crate::native_texture::decode(&index.read(key)?, None)?;
        let width = source.width;
        let height = source.height;
        let packet = crate::native_paint::normal_texture_packet(
            self.normal_bytecode.clone(),
            Some(self.copy_bytecode.clone()),
            source,
            self.parameters.part_bump_scale,
            self.parameters.bump_height,
        )?;
        self.gpu_pixels(packet, width, height)
    }
    pub fn native_part_tint(
        &self,
        index: &ResourceIndex,
        diffuse: ResourceKey,
        mask: ResourceKey,
        colors: [[u8; 3]; 3],
    ) -> Result<Pixels, String> {
        let diffuse = crate::native_texture::decode(&index.read(diffuse)?, None)?;
        let mask = crate::native_texture::decode(&index.read(mask)?, None)?;
        let width = diffuse.width;
        let height = diffuse.height;
        let palette = colors.map(|c| {
            [
                c[0] as f32 / 255.0,
                c[1] as f32 / 255.0,
                c[2] as f32 / 255.0,
                0.0,
            ]
        });
        let packet = crate::native_paint::tint_texture_packet(
            self.tint_bytecode.clone(),
            diffuse,
            mask,
            palette,
        )?;
        self.gpu_pixels(packet, width, height)
    }
    /// Isolated part bake: native texture-1 blue write, then normal conversion.
    /// Painter-local part/spec scales initialize to 1 in CreateTextures.
    /// Whole-creature atlas, body brushes and authored atlas placements remain separate.
    pub fn part_normal(&self, packed: &Pixels) -> Result<Pixels, String> {
        validate(packed)?;
        if crate::native_paint::enabled() {
            let packet = crate::native_paint::normal_packet(
                self.normal_bytecode.clone(),
                Some(self.copy_bytecode.clone()),
                packed.width,
                packed.height,
                &packed.rgba,
                self.parameters.part_bump_scale,
                self.parameters.bump_height,
            )?;
            return self.gpu_pixels(packet, packed.width, packed.height);
        }
        let mut scaled = packed.clone();
        for p in scaled.rgba.chunks_exact_mut(4) {
            p[2] = (p[2] as f32 * self.parameters.part_bump_scale)
                .round()
                .clamp(0., 255.) as u8;
        }
        self.normal_map(&scaled, self.parameters.bump_height)
    }
    /// The native job binds (1/width, 1/height, bumpHeight/3, 0).
    /// A zero bump-height setting selects the observed native fallback of 5.
    /// Input B is height; the original program copies input R (specular) to A.
    pub fn normal_map(&self, packed: &Pixels, bump_height: f32) -> Result<Pixels, String> {
        validate(packed)?;
        if !bump_height.is_finite() {
            return Err("nonfinite native bump height".into());
        }
        let height = if bump_height == 0. { 5. } else { bump_height };
        let constants = [[
            1. / packed.width as f32,
            1. / packed.height as f32,
            height / 3.,
            0.,
        ]];
        if crate::native_paint::enabled() {
            let packet = crate::native_paint::normal_packet(
                self.normal_bytecode.clone(),
                None,
                packed.width,
                packed.height,
                &packed.rgba,
                1.0,
                height,
            )?;
            return self.gpu_pixels(packet, packed.width, packed.height);
        }
        let mut out = packed.clone();
        for y in 0..packed.height {
            for x in 0..packed.width {
                let uv = [
                    (x as f32 + 0.5) / packed.width as f32,
                    (y as f32 + 0.5) / packed.height as f32,
                    0.,
                    1.,
                ];
                let value =
                    self.normal_kernel
                        .run(&[uv], &[uv], &constants, |sampler, coordinate| {
                            debug_assert_eq!(sampler, 0);
                            sample(packed, coordinate)
                        })?;
                let at = ((y * packed.width + x) * 4) as usize;
                for i in 0..4 {
                    out.rgba[at + i] = (value[i].clamp(0., 1.) * 255.).round() as u8;
                }
            }
        }
        Ok(out)
    }
    pub fn tint(
        &self,
        diffuse: &Pixels,
        mask: &Pixels,
        colors: [[u8; 3]; 3],
    ) -> Result<Pixels, String> {
        validate(diffuse)?;
        validate(mask)?;
        let constants = colors.map(|c| {
            [
                c[0] as f32 / 255.,
                c[1] as f32 / 255.,
                c[2] as f32 / 255.,
                0.,
            ]
        });
        if crate::native_paint::enabled() {
            let packet = crate::native_paint::tint_packet(
                self.tint_bytecode.clone(),
                diffuse.width,
                diffuse.height,
                &diffuse.rgba,
                mask.width,
                mask.height,
                &mask.rgba,
                constants,
            )?;
            return self.gpu_pixels(packet, diffuse.width, diffuse.height);
        }
        let mut out = diffuse.clone();
        for y in 0..diffuse.height {
            for x in 0..diffuse.width {
                let uv = [
                    (x as f32 + 0.5) / diffuse.width as f32,
                    (y as f32 + 0.5) / diffuse.height as f32,
                ];
                // SM2 declares t1; SM3 declares v0. Both consume the same UV.
                let inputs = [[uv[0], uv[1], 0., 1.]; 2];
                let pixel = self
                    .kernel
                    .run(&inputs, &inputs, &constants, |sampler, uv| {
                        sample(if sampler == 0 { diffuse } else { mask }, uv)
                    })?;
                let at = (y * diffuse.width + x) as usize * 4;
                for c in 0..4 {
                    out.rgba[at + c] = (pixel[c].clamp(0., 1.) * 255.).round() as u8;
                }
            }
        }
        Ok(out)
    }
    /// Keep the existing flat body-paint approximation after native part
    /// tinting. This is explicitly not full procedural SkinPaint/atlas parity.
    pub fn preview_part(
        &self,
        diffuse: &Pixels,
        mask: &Pixels,
        colors: [[u8; 3]; 3],
    ) -> Result<Pixels, String> {
        let mut out = self.tint(diffuse, mask, colors)?;
        if crate::native_paint::enabled() {
            return Ok(out);
        }
        for (pixel, authored) in out
            .rgba
            .chunks_exact_mut(4)
            .zip(diffuse.rgba.chunks_exact(4))
        {
            let alpha = authored[3] as f32 / 255.;
            for c in 0..3 {
                pixel[c] =
                    (pixel[c] as f32 * alpha + colors[0][c] as f32 * (1. - alpha)).round() as u8;
            }
            pixel[3] = 255;
        }
        Ok(out)
    }
    fn gpu_pixels(
        &self,
        packet: crate::native_packet::NativeRenderPacket,
        width: u32,
        height: u32,
    ) -> Result<Pixels, String> {
        // An explicitly requested native path may never silently become a CPU
        // approximation through the preview caller's optional-texture fallback.
        let rgba = crate::native_paint::execute(&packet).unwrap_or_else(|error| {
            eprintln!("Native SkinPaint failed: {error}");
            std::process::exit(1)
        });
        Ok(Pixels {
            width,
            height,
            rgba,
        })
    }
}
pub fn load_at_startup(mut originals: bevy::prelude::ResMut<crate::Originals>) {
    match load(&originals.resource_index) {
        Ok(painter) => {
            bevy::log::info!(
                "original part pixel shaders: {} tint {:08X}, normals {:08X}, bump height {}, part scale {}",
                painter.pack,
                painter.kernel.version,
                painter.normal_kernel.version,
                painter.parameters.bump_height,
                painter.parameters.part_bump_scale,
            );
            originals.native_painter = Some(painter)
        }
        Err(e) => {
            if crate::native_paint::enabled() {
                eprintln!("Native SkinPaint unavailable: {e}");
                std::process::exit(1);
            }
            bevy::log::warn!("native part painting unavailable: {e}")
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Reads an oracle fixture from `tests/fixtures` at run time. The public repository leaves these
    /// captures of the original code out, so the test is skipped when the file is missing.
    fn fixture(name: &str) -> Option<serde_json::Value> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        match std::fs::read_to_string(&path) {
            Ok(text) => Some(serde_json::from_str(&text).unwrap()),
            Err(_) => {
                eprintln!("skipped: fixture {} is not present", path.display());
                None
            }
        }
    }
    #[test]
    fn native_settings_keep_zero_fallbacks_and_reject_invalid_values() {
        use crate::prop::{Property, PropertyList};
        let mut properties = PropertyList::default();
        let scalar = |value: f32| Property {
            kind: 13,
            array: false,
            values: vec![value.to_be_bytes().to_vec()],
        };
        properties.0.insert(0x0c19db33, scalar(0.));
        properties.0.insert(0x1c76d9b5, scalar(0.));
        let settings = PaintParameters::from_properties(&properties).unwrap();
        assert_eq!((settings.bump_height, settings.part_bump_scale), (5., 1.));
        properties.0.insert(0x1c76d9b5, scalar(0.8));
        assert_eq!(
            PaintParameters::from_properties(&properties)
                .unwrap()
                .part_bump_scale,
            0.8
        );
        properties.0.insert(0x0c19db33, scalar(f32::NAN));
        assert!(PaintParameters::from_properties(&properties).is_err());
        let mut array = scalar(5.);
        array.array = true;
        properties.0.insert(0x0c19db33, array);
        assert!(PaintParameters::from_properties(&properties).is_err());
    }
    #[test]
    #[ignore = "installed original settings and part texture census"]
    fn inspect_installed_bump_inputs_and_settings() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut settings = vec![];
        for key in index
            .records
            .keys()
            .filter(|k| k.type_id == crate::asset_catalog::PROP)
        {
            let Ok(props) = index
                .read(*key)
                .and_then(|bytes| crate::prop::parse(&bytes))
            else {
                continue;
            };
            for id in [0x0c19db33, 0x1c76d9b5] {
                if let Some(value) = props.0.get(&id) {
                    settings.push(serde_json::json!({"key":crate::resource_audit::key_json(*key),"property":format!("{id:08X}"),"kind":value.kind,"value":value.number()}));
                }
            }
        }
        let catalog = crate::asset_catalog::PartCatalog::from_index(&index);
        let keys: std::collections::HashSet<_> =
            catalog.parts.iter().filter_map(|p| p.spec_bump).collect();
        let mut inputs = vec![];
        for key in keys {
            let bytes = index.read(key).unwrap();
            let pixels = crate::texture::decode_rw4(&bytes).expect("authored spec/bump raster");
            inputs.push(serde_json::json!({"key":crate::resource_audit::key_json(key),"width":pixels.width,"height":pixels.height}));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({"settings":settings,"inputs":inputs}))
                .unwrap()
        );
        assert!(!inputs.is_empty());
    }
    #[test]
    #[ignore = "installed original normal-conversion programs; expected spatial pixels measured on D3D9 hardware"]
    fn spatial_normal_conversion_matches_original_gpu_stencils() {
        let Some(fixture) = fixture("skinpaint-spatial-normal-d3d9.json") else {
            return;
        };
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut maximum = 0;
        let mut pixels = 0;
        for profile in fixture["profiles"].as_array().unwrap() {
            let pack_key = index
                .records
                .keys()
                .find(|k| {
                    k.type_id == shader_pack::TYPE_ID
                        && k.group_id == shader_pack::GROUP_COMPILED
                        && shader_pack::parse(&index.read(**k).unwrap()).unwrap().name
                            == profile["pack"].as_str().unwrap()
                })
                .unwrap();
            let pack = shader_pack::parse(&index.read(*pack_key).unwrap()).unwrap();
            let entry = &pack
                .standards
                .iter()
                .find(|s| s.header.id == 0x700000b2)
                .unwrap()
                .entries[0];
            let mut painter = load(&index).unwrap();
            painter.normal_kernel =
                PixelKernel::compile(&crate::d3d9_shader::parse(&entry.pixel).unwrap()).unwrap();
            for case in profile["cases"].as_array().unwrap() {
                let source = Pixels {
                    width: profile["width"].as_u64().unwrap() as u32,
                    height: profile["height"].as_u64().unwrap() as u32,
                    rgba: case["source_rgba"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap() as u8)
                        .collect(),
                };
                painter.parameters.bump_height = case["bump_height"].as_f64().unwrap() as f32;
                painter.parameters.part_bump_scale =
                    case["part_bump_scale"].as_f64().unwrap() as f32;
                let actual = painter.part_normal(&source).unwrap();
                for (i, value) in actual.rgba.iter().enumerate() {
                    let expected = case["output_rgba"][i].as_u64().unwrap() as u8;
                    maximum = maximum.max(value.abs_diff(expected));
                    assert!(
                        value.abs_diff(expected) <= 1,
                        "{} {} byte{i}: Rust{value} vs GPU{expected}",
                        profile["pack"],
                        case["name"]
                    );
                }
                for (normal, authored) in
                    actual.rgba.chunks_exact(4).zip(source.rgba.chunks_exact(4))
                {
                    assert_eq!(
                        normal[3], authored[0],
                        "native alpha keeps specular, not opacity"
                    );
                }
                pixels += source.width * source.height;
            }
        }
        assert_eq!(pixels, 2048);
        println!("{pixels} independent spatial GPU pixels; maximum RGBA8 difference {maximum}");
    }
    fn check_gpu_fixture(fixture: serde_json::Value) {
        let shader_id = u32::from_str_radix(fixture["shader_id"].as_str().unwrap(), 16).unwrap();
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let mut maximum_difference = 0;
        let mut measured_cases = 0;
        for profile in fixture["profiles"].as_array().unwrap() {
            let mut found = vec![];
            for key in index.records.keys().filter(|k| {
                k.type_id == shader_pack::TYPE_ID && k.group_id == shader_pack::GROUP_COMPILED
            }) {
                let pack = shader_pack::parse(&index.read(*key).unwrap()).unwrap();
                if pack.name != profile["pack"].as_str().unwrap() {
                    continue;
                }
                for shader in pack.standards.iter().filter(|s| s.header.id == shader_id) {
                    for entry in &shader.entries {
                        let program = crate::d3d9_shader::parse(&entry.pixel).unwrap();
                        found.push((PixelKernel::compile(&program).unwrap(), program));
                    }
                }
            }
            assert_eq!(
                found.len(),
                1,
                "oracle profile must identify one original program"
            );
            let (kernel, program) = &found[0];
            assert_eq!(
                format!("0x{:08X}", kernel.version),
                profile["shader_version"].as_str().unwrap()
            );
            let color: [f32; 4] =
                std::array::from_fn(|i| fixture["vertex_color"][i].as_f64().unwrap() as f32);
            let uv: [f32; 4] =
                std::array::from_fn(|i| fixture["texture_coordinate"][i].as_f64().unwrap() as f32);
            let mut inputs = [color; 16];
            // SM2 v0/v1 are the explicit vertex colours. SM3 declarations
            // place TEXCOORD and COLOR semantics into indexed v registers.
            if program.major == 3 {
                for declaration in program.instructions.iter().filter(|i| i.opcode == 31) {
                    let token = declaration.parameters[1];
                    if crate::d3d9_shader::register_type(token) == 1 {
                        let register = crate::d3d9_shader::register_number(token) as usize;
                        inputs[register] = match declaration.parameters[0] & 15 {
                            5 => uv,
                            10 => color,
                            usage => panic!("unbound oracle input semantic {usage}"),
                        };
                    }
                }
            }
            for case in fixture["cases"].as_array().unwrap() {
                let measured = profile["outputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|v| v["name"] == case["name"])
                    .unwrap();
                let rgba = |field: &str| {
                    std::array::from_fn::<_, 4, _>(|i| {
                        case[field][i].as_f64().unwrap() as f32 / 255.
                    })
                };
                let constants: Vec<[f32; 4]> = case["palette"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| std::array::from_fn(|i| row[i].as_f64().unwrap() as f32))
                    .collect();
                let actual = kernel
                    .run(&inputs, &[uv; 8], &constants, |s, _| {
                        rgba(if s == 0 { "inputRGBA8" } else { "tintRGBA8" })
                    })
                    .unwrap();
                for i in 0..4 {
                    let actual = (actual[i].clamp(0., 1.) * 255.).round() as u8;
                    let expected = measured["outputRGBA8"][i].as_u64().unwrap() as u8;
                    let difference = actual.abs_diff(expected);
                    maximum_difference = maximum_difference.max(difference);
                    assert!(
                        difference <= 1,
                        "{} {} channel{i}: CPU{actual} vs original D3D9{expected}",
                        profile["pack"],
                        case["name"]
                    );
                }
                measured_cases += 1;
            }
        }
        assert_eq!(measured_cases, 48);
        println!("{shader_id:08X}: 48 independent D3D9 oracle cases across four native profiles; maximum RGBA8 difference {maximum_difference}");
    }
    #[test]
    #[ignore = "requires installed original shader packs; expected pixels measured independently with Direct3D9"]
    fn installed_tint_matches_independent_d3d9_gpu_pixels() {
        let Some(fixture) = fixture("skinpaint-d3d9-oracle.json") else {
            return;
        };
        check_gpu_fixture(fixture);
    }
    #[test]
    #[ignore = "requires installed original shader packs; independent GPU cases for AO and bump-to-normal"]
    fn installed_auxiliary_skinpaint_matches_independent_d3d9_gpu_pixels() {
        let Some(fixtures) = fixture("skinpaint-auxiliary-d3d9-oracle.json") else {
            return;
        };
        assert_eq!(fixtures["families"].as_array().unwrap().len(), 2);
        for fixture in fixtures["families"].as_array().unwrap() {
            check_gpu_fixture(fixture.clone());
        }
    }
    #[test]
    #[ignore = "requires installed Spore shader packs"]
    fn all_native_tint_profiles_execute_consistently() {
        let install = crate::spore_install::discover().unwrap();
        let index = ResourceIndex::load(&install.packages);
        let painter = load(&index).unwrap();
        let mut baseline: Option<[f32; 4]> = None;
        let mut profiles = 0;
        for key in index.records.keys().filter(|k| {
            k.type_id == shader_pack::TYPE_ID && k.group_id == shader_pack::GROUP_COMPILED
        }) {
            let pack = shader_pack::parse(&index.read(*key).unwrap()).unwrap();
            for shader in pack.standards.iter().filter(|s| s.header.id == 0x700000ad) {
                for entry in &shader.entries {
                    let kernel =
                        PixelKernel::compile(&crate::d3d9_shader::parse(&entry.pixel).unwrap())
                            .unwrap();
                    let actual = kernel
                        .run(
                            &[[0.5, 0.5, 0., 1.]; 2],
                            &[[0.5, 0.5, 0., 1.]; 2],
                            &[
                                [0.4, 0.7, 0.2, 0.],
                                [0.7, 0.2, 0.5, 0.],
                                [0.2, 0.3, 0.8, 0.],
                            ],
                            |s, _| {
                                if s == 0 {
                                    [0.6, 0.4, 0.2, 0.75]
                                } else {
                                    [0.3, 0.5, 0.2, 1.]
                                }
                            },
                        )
                        .unwrap();
                    if let Some(expected) = baseline {
                        for i in 0..4 {
                            assert!((actual[i] - expected[i]).abs() < 1e-5);
                        }
                    } else {
                        baseline = Some(actual)
                    }
                    assert!(
                        (actual[3] - 0.75).abs() < 1e-6,
                        "native tint keeps diffuse alpha"
                    );
                    profiles += 1;
                }
            }
        }
        assert_eq!(profiles, 4);
        let diffuse = Pixels {
            width: 1,
            height: 1,
            rgba: vec![153, 102, 51, 191],
        };
        let mask = Pixels {
            width: 1,
            height: 1,
            rgba: vec![77, 128, 51, 255],
        };
        let painted = painter
            .tint(
                &diffuse,
                &mask,
                [[102, 179, 51], [179, 51, 128], [51, 77, 204]],
            )
            .unwrap();
        assert_eq!(painted.rgba[3], 191);
        assert_ne!(painted.rgba[..3], diffuse.rgba[..3]);
        assert!(painter
            .tint(
                &Pixels {
                    width: 0,
                    height: 1,
                    rgba: vec![]
                },
                &mask,
                [[0; 3]; 3]
            )
            .is_err());
    }
}
