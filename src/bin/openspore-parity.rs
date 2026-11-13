//! Small standalone native replay/reference tool, independent of Bevy's renderer.
#[path = "../creature_data.rs"]
mod creature_data;
#[path = "../d3d9_shader.rs"]
mod d3d9_shader;
#[cfg(windows)]
#[path = "../native_d3d9.rs"]
mod native_d3d9;
#[path = "../native_packet.rs"]
mod native_packet;
#[path = "../native_paint.rs"]
mod native_paint;
#[path = "../native_texture.rs"]
mod native_texture;
#[path = "../parity.rs"]
mod parity;
#[path = "../parity_ledger.rs"]
mod parity_ledger;
use native_packet::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, String>;
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        .map_err(|e| format!("{}: {e}", path.display()))
}
fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
#[cfg(windows)]
fn write_rgba_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .map_err(|e| e.to_string())?
        .write_image_data(rgba)
        .map_err(|e| e.to_string())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedCapture {
    version: u32,
    kind: String,
    complete: bool,
    frame: u64,
    draw_calls: usize,
    recorded_draws: usize,
    present_target: usize,
    width: u32,
    height: u32,
    format: i32,
    device: CapturedDevice,
    issues: Vec<String>,
    buffer_shadow_reads: usize,
    #[serde(default)]
    target_readbacks: Vec<usize>,
    scope: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturedDevice {
    description: String,
    ordinal: u32,
    vendor_id: u32,
    device_id: u32,
    behavior_flags: u32,
}
#[cfg(windows)]
fn replay_capture(root: &Path, out: &Path) -> Result<bool> {
    ignored_output(out)?;
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if !root.starts_with(&target) {
        return Err("capture inputs must remain under ignored target".into());
    }
    let capture: OwnedCapture = read(&root.join("capture-0000.json"))?;
    if capture.version != 1
        || capture.kind != "OwnedD3D9CaptureProbe"
        || !capture.complete
        || !capture.issues.is_empty()
        || capture.frame != 0
        || capture.draw_calls == 0
        || capture.draw_calls != capture.recorded_draws
        || capture.scope != "validated draw subset; not an original-game scenario"
    {
        return Err("owned draw capture is incomplete or has an unsupported identity; no native-scene claim is permitted".into());
    }
    let packet_path = root.join("capture-0000.packet.json");
    let packet: NativeRenderPacket = read(&packet_path)?;
    packet.validate()?;
    if packet.version >= 4
        && capture.target_readbacks != (0..packet.targets.len()).collect::<Vec<_>>()
    {
        return Err("capture omitted or reordered intermediate surface readbacks".into());
    }
    if packet
        .commands
        .iter()
        .filter(|c| matches!(c, Command::Draw(_)))
        .count()
        != capture.recorded_draws
    {
        return Err("capture draw count differs from its packet".into());
    }
    let desc = packet
        .targets
        .get(capture.present_target)
        .ok_or("capture target missing")?;
    if desc.width != capture.width
        || desc.height != capture.height
        || desc.format != capture.format
        || capture.format != 21
    {
        return Err("owned reference capture target differs or is not native A8R8G8B8".into());
    }
    let adapter = native_d3d9::adapters()?
        .into_iter()
        .find(|a| a.ordinal == capture.device.ordinal)
        .ok_or("captured adapter unavailable")?;
    if adapter.description != capture.device.description
        || adapter.vendor != capture.device.vendor_id
        || adapter.device != capture.device.device_id
        || capture.device.behavior_flags != 34
    {
        return Err("captured device/behavior differs from native replay".into());
    }
    let expected_raw = std::fs::read(root.join("owned-frame.raw")).map_err(|e| e.to_string())?;
    let observed_raw = std::fs::read(root.join("capture-0000.raw")).map_err(|e| e.to_string())?;
    if expected_raw != observed_raw {
        return Err("observer readback differs from independently rendered owned frame".into());
    }
    let renderer = native_d3d9::Renderer::new(capture.device.ordinal)?;
    let buffers = renderer.replay(&packet)?;
    let buffer = buffers
        .get(capture.present_target)
        .ok_or("native replay omitted capture target")?;
    if expected_raw.len() != capture.width as usize * capture.height as usize * 4 {
        return Err("capture raw byte count invalid".into());
    }
    std::fs::write(out.join("replay.raw"), &buffer.bytes).map_err(|e| e.to_string())?;
    let identity = parity::sha256(&std::fs::read(packet_path).map_err(|e| e.to_string())?);
    let mut expected = parity::ScenarioTrace {
        version: 1,
        manifest_sha256: identity.clone(),
        scenario: format!("owned D3D9 observed draw replay/{identity}"),
        origin: parity::Origin::ShaderProbe,
        isolation: None,
        frames: vec![],
    };
    let mut actual = expected.clone();
    let rgba: Vec<_> = expected_raw
        .chunks_exact(4)
        .flat_map(|b| [b[2], b[1], b[0], b[3]])
        .collect();
    let replay_rgba = buffer.rgba8()?;
    write_rgba_png(
        &out.join("reference.png"),
        capture.width,
        capture.height,
        &rgba,
    )?;
    write_rgba_png(
        &out.join("replay.png"),
        capture.width,
        capture.height,
        &replay_rgba,
    )?;
    expected
        .frames
        .push(frame(0, rgba, capture.width, capture.height));
    actual
        .frames
        .push(frame(0, replay_rgba, capture.width, capture.height));
    let mut surface_reports = vec![];
    for &id in &capture.target_readbacks {
        let desc = &packet.targets[id];
        let independent = std::fs::read(root.join(format!("owned-target-{id:04}.raw")))
            .map_err(|e| e.to_string())?;
        let observed = std::fs::read(root.join(format!("capture-0000.target-{id:04}.raw")))
            .map_err(|e| e.to_string())?;
        let replayed = &buffers[id];
        let (row, rows) = surface_layout(desc.format, desc.width, desc.height)?;
        if independent.len() != row * rows || independent != observed {
            return Err(format!(
                "independent/observer surface {id} differs or has invalid size"
            ));
        }
        std::fs::write(out.join(format!("target-{id:04}.raw")), &replayed.bytes)
            .map_err(|e| e.to_string())?;
        let expected_rgba: Vec<_> = independent
            .chunks_exact(4)
            .flat_map(|b| [b[2], b[1], b[0], b[3]])
            .collect();
        let actual_rgba = replayed.rgba8()?;
        if id != capture.present_target {
            let name = format!("target/{id}");
            let image = parity::Image {
                width: desc.width,
                height: desc.height,
                rgba8: expected_rgba.clone(),
            };
            expected.frames[0].images.insert(name.clone(), image);
            actual.frames[0].images.insert(
                name,
                parity::Image {
                    width: desc.width,
                    height: desc.height,
                    rgba8: actual_rgba.clone(),
                },
            );
        }
        write_rgba_png(
            &out.join(format!("target-{id:04}-reference.png")),
            desc.width,
            desc.height,
            &expected_rgba,
        )?;
        write_rgba_png(
            &out.join(format!("target-{id:04}-replay.png")),
            desc.width,
            desc.height,
            &actual_rgba,
        )?;
        surface_reports.push(serde_json::json!({"target":id,"width":desc.width,"height":desc.height,"source_sha256":parity::sha256(&independent),"observer_sha256":parity::sha256(&observed),"replay_sha256":parity::sha256(&replayed.bytes),"raw_equal":independent==replayed.bytes}));
    }
    let report = parity::compare(&expected, &actual)?;
    write(&out.join("expected.json"), &expected)?;
    write(&out.join("actual.json"), &actual)?;
    write(&out.join("comparison.json"), &report)?;
    write(
        &out.join("capture-replay.json"),
        &serde_json::json!({"version":1,"kind":"OwnedCaptureReplay","packet_sha256":identity,"draws":capture.recorded_draws,
        "buffer_shadow_reads":capture.buffer_shadow_reads,"source_raw_sha256":parity::sha256(&expected_raw),"observer_raw_sha256":parity::sha256(&observed_raw),
        "replay_raw_sha256":parity::sha256(&buffer.bytes),"passed":report.passed,"surfaces":surface_reports,"compared_pixels":report.compared_pixels,"origin":"ShaderProbe","original_game_launched":false}),
    )?;
    println!("Captured {} draws; {} exact replay pixels; passed {} (owned fixture, not an original-game scene)",capture.recorded_draws,report.compared_pixels,report.passed);
    Ok(report.passed)
}
fn ignored_output(path: &Path) -> Result<()> {
    let target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    let output = path.canonicalize().map_err(|e| e.to_string())?;
    if !output.starts_with(&target) {
        return Err("original shader packets/readbacks must stay in this repository's ignored target directory".into());
    }
    Ok(())
}
#[cfg(windows)]
fn choose_adapter() -> Result<u32> {
    let adapters = native_d3d9::adapters()?;
    adapters
        .iter()
        .find(|a| a.vendor == 0x10de)
        .or_else(|| adapters.first())
        .map(|a| a.ordinal)
        .ok_or("no D3D9 HAL adapter available".into())
}
#[derive(Deserialize)]
struct Oracle {
    shader_file: PathBuf,
    adapter: String,
    vendor_id: u32,
    device_id: u32,
    cases: Vec<OracleCase>,
}
#[derive(Deserialize)]
struct OracleCase {
    name: String,
    #[serde(rename = "inputRGBA8")]
    input: [u8; 4],
    #[serde(rename = "tintRGBA8")]
    tint: [u8; 4],
    palette: [[f32; 4]; 3],
    #[serde(rename = "outputRGBA8")]
    output: [u8; 4],
}
fn quad(width: u32, height: u32, uv: [f32; 2]) -> Vec<u8> {
    let mut bytes = vec![];
    for (x, y) in [
        (-0.5, -0.5),
        (width as f32 - 0.5, -0.5),
        (-0.5, height as f32 - 0.5),
        (width as f32 - 0.5, height as f32 - 0.5),
    ] {
        for v in [x, y, 0.0, 1.0] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        for _ in 0..8 {
            for v in uv {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    bytes
}
fn probe_packet(shader: Vec<u32>, case: &OracleCase) -> NativeRenderPacket {
    let textures = [case.input, case.tint]
        .into_iter()
        .map(|v| Texture {
            width: 1,
            height: 1,
            format: 21,
            faces: vec![vec![Mip {
                row_pitch: 4,
                bytes: vec![v[2], v[1], v[0], v[3]],
            }]],
        })
        .collect();
    let states = vec![
        [7, 0],
        [14, 0],
        [22, 1],
        [27, 0],
        [137, 0],
        [28, 0],
        [52, 0],
        [174, 0],
        [136, 0],
        [168, 15],
    ];
    let samplers = (0..2)
        .flat_map(|stage| {
            [
                [stage, 6, 1],
                [stage, 5, 1],
                [stage, 7, 2],
                [stage, 1, 3],
                [stage, 2, 3],
                [stage, 3, 3],
                [stage, 11, 0],
            ]
        })
        .collect();
    NativeRenderPacket {
        version: 1,
        label: case.name.clone(),
        targets: vec![Target {
            width: 1,
            height: 1,
            format: 21,
            depth_format: 0,
            surface_only: false,
        }],
        textures,
        commands: vec![
            Command::Clear {
                target: 0,
                flags: 1,
                color: 0,
                depth_bits: 1.0f32.to_bits(),
                stencil: 0,
                viewport: None,
                rects: vec![],
                scissor: None,
            },
            Command::Draw(Draw {
                target: 0,
                viewport: [0, 0, 1, 1, 0, 1.0f32.to_bits()],
                scissor: None,
                vertex_shader: None,
                pixel_shader: Some(shader),
                fvf: Some(0x8c4),
                declaration: vec![],
                vertices: quad(1, 1, [0.5, 0.5]),
                stride: 88,
                vertex_count: 4,
                indices: None,
                indexed_range: None,
                primitive_type: 5,
                primitive_count: 2,
                render_states: states,
                sampler_states: samplers,
                texture_stage_states: vec![],
                bindings: vec![
                    Binding {
                        sampler: 0,
                        source: Some(TextureSource::Texture(0)),
                    },
                    Binding {
                        sampler: 1,
                        source: Some(TextureSource::Texture(1)),
                    },
                ],
                constants: vec![Constants::Float {
                    vertex: false,
                    start: 0,
                    bits: case.palette.map(|p| p.map(f32::to_bits)).to_vec(),
                }],
            }),
        ],
    }
}
fn frame(tick: u64, rgba8: Vec<u8>, width: u32, height: u32) -> parity::Frame {
    parity::Frame {
        tick,
        delta_bits: 0,
        inputs: serde_json::Value::Null,
        rng: serde_json::Value::Null,
        state: serde_json::Value::Null,
        animation: serde_json::Value::Null,
        audio: serde_json::Value::Null,
        images: BTreeMap::from([(
            "final".into(),
            parity::Image {
                width,
                height,
                rgba8,
            },
        )]),
    }
}
fn quad_declaration(first_uv_3d: bool) -> Vec<VertexElement> {
    let mut decl = vec![
        VertexElement {
            stream: 0,
            offset: 0,
            kind: 3,
            method: 0,
            usage: 9,
            usage_index: 0,
        },
        VertexElement {
            stream: 0,
            offset: 16,
            kind: 4,
            method: 0,
            usage: 10,
            usage_index: 0,
        },
        VertexElement {
            stream: 0,
            offset: 20,
            kind: 4,
            method: 0,
            usage: 10,
            usage_index: 1,
        },
    ];
    for i in 0..8u8 {
        decl.push(VertexElement {
            stream: 0,
            offset: 24 + 8 * i as u16 + if first_uv_3d && i > 0 { 4 } else { 0 },
            kind: if first_uv_3d && i == 0 { 2 } else { 1 },
            method: 0,
            usage: 5,
            usage_index: i,
        });
    }
    decl
}
/// Hardware resource checks with unambiguous authored synthetic endpoint colors.
/// These exercise the backend; they do not establish original-game scene parity.
#[cfg(windows)]
fn resources(shader_path: &Path, out: &Path, adapter: u32) -> Result<bool> {
    ignored_output(out)?;
    let copy = native_paint::shader_words(&std::fs::read(shader_path).map_err(|e| e.to_string())?)?;
    let renderer = native_d3d9::Renderer::new(adapter)?;
    let mut expected = parity::ScenarioTrace {
        version: 1,
        manifest_sha256: parity::sha256(
            &serde_json::to_vec(&renderer.adapter).map_err(|e| e.to_string())?,
        ),
        scenario: "D3D9 resource validation (synthetic textures)".into(),
        origin: parity::Origin::ShaderProbe,
        isolation: None,
        frames: vec![],
    };
    let mut actual = expected.clone();
    let colors = [
        [0, 255, 0, 255],
        [255, 0, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
    ];
    let mut mips = vec![];
    for (level, color) in [0x07e0u16, 0xf800, 0x001f, 0xffff].into_iter().enumerate() {
        let width = 8u32 >> level;
        let (row, rows) = surface_layout(0x31545844, width, width)?;
        let block = [color as u8, (color >> 8) as u8, 0, 0, 0, 0, 0, 0];
        mips.push(Mip {
            row_pitch: row,
            bytes: (0..row * rows / 8).flat_map(|_| block).collect(),
        });
    }
    for level in 0..4 {
        let mut draw = native_paint::fullscreen(
            8,
            8,
            copy.clone(),
            vec![[1.; 4]],
            vec![Binding {
                sampler: 0,
                source: Some(TextureSource::Texture(0)),
            }],
        );
        draw.sampler_states.push([0, 9, level]);
        draw.render_states.extend([[7, 1], [14, 1], [23, 4]]);
        if level >= 2 {
            draw.fvf = None;
            draw.declaration = quad_declaration(false);
        }
        if level % 2 == 1 {
            draw.indices = Some(IndexData {
                bits: 16,
                bytes: [0u16, 1, 2, 2, 1, 3]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            });
            draw.primitive_type = 4;
        }
        let packet = NativeRenderPacket {
            version: 1,
            label: format!("compressed mip {level}, depth and native declaration/index draw"),
            targets: vec![Target {
                width: 8,
                height: 8,
                format: 21,
                depth_format: 75,
                surface_only: false,
            }],
            textures: vec![Texture {
                width: 8,
                height: 8,
                format: 0x31545844,
                faces: vec![mips.clone()],
            }],
            commands: vec![
                Command::Clear {
                    target: 0,
                    flags: 3,
                    color: 0,
                    depth_bits: 1.0f32.to_bits(),
                    stencil: 0,
                    viewport: None,
                    rects: vec![],
                    scissor: None,
                },
                Command::Draw(draw),
            ],
        };
        write(&out.join(format!("mip-{level}.json")), &packet)?;
        let pixels = renderer.replay(&packet)?[0].rgba8()?;
        expected.frames.push(frame(
            level as u64,
            (0..64).flat_map(|_| colors[level as usize]).collect(),
            8,
            8,
        ));
        actual.frames.push(frame(level as u64, pixels, 8, 8));
    }
    // Minimal SM2 cubemap sampling probe, built from D3D9 token encodings. The
    // cube inputs and expected face colors are synthetic; no EA cube shader is claimed.
    let cube_shader = vec![
        0xffff0200, 0x0200001f, 0x80000000, 0xb00f0000, 0x0200001f, 0x98000000, 0xa00f0800,
        0x03000042, 0x800f0000, 0xb0e40000, 0xa0e40800, 0x02000001, 0x800f0800, 0x80e40000, 0xffff,
    ];
    let cube_colors = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 0, 255, 255],
        [255, 255, 0, 255],
        [0, 255, 255, 255],
    ];
    let directions = [
        [1.0, 0., 0.],
        [-1., 0., 0.],
        [0., 1., 0.],
        [0., -1., 0.],
        [0., 0., 1.],
        [0., 0., -1.],
    ];
    for (face, direction) in directions.into_iter().enumerate() {
        let mut draw = native_paint::fullscreen(
            1,
            1,
            cube_shader.clone(),
            vec![[1.; 4]],
            vec![Binding {
                sampler: 0,
                source: Some(TextureSource::Texture(0)),
            }],
        );
        let mut vertices = vec![];
        for vertex in draw.vertices.chunks_exact(88) {
            vertices.extend_from_slice(&vertex[..24]);
            for value in direction {
                vertices.extend_from_slice(&f32::to_le_bytes(value));
            }
            vertices.extend_from_slice(&vertex[32..]);
        }
        draw.vertices = vertices;
        draw.stride = 92;
        draw.fvf = None;
        draw.declaration = quad_declaration(true);
        let packet = NativeRenderPacket {
            version: 1,
            label: format!("synthetic cube face {face}"),
            targets: vec![Target {
                width: 1,
                height: 1,
                format: 21,
                depth_format: 0,
                surface_only: false,
            }],
            textures: vec![Texture {
                width: 1,
                height: 1,
                format: 21,
                faces: cube_colors
                    .iter()
                    .map(|c| {
                        vec![Mip {
                            row_pitch: 4,
                            bytes: vec![c[2], c[1], c[0], c[3]],
                        }]
                    })
                    .collect(),
            }],
            commands: vec![
                Command::Clear {
                    target: 0,
                    flags: 1,
                    color: 0,
                    depth_bits: 1.0f32.to_bits(),
                    stencil: 0,
                    viewport: None,
                    rects: vec![],
                    scissor: None,
                },
                Command::Draw(draw),
            ],
        };
        write(&out.join(format!("cube-{face}.json")), &packet)?;
        let pixels = renderer.replay(&packet)?[0].rgba8()?;
        expected
            .frames
            .push(frame(face as u64 + 4, cube_colors[face].to_vec(), 1, 1));
        actual.frames.push(frame(face as u64 + 4, pixels, 1, 1));
    }
    write(&out.join("expected.json"), &expected)?;
    write(&out.join("actual.json"), &actual)?;
    let report = parity::compare(&expected, &actual)?;
    write(&out.join("comparison.json"), &report)?;
    println!(
        "Resource validation: {} frames, {} exact pixels, passed {}",
        report.compared_frames, report.compared_pixels, report.passed
    );
    Ok(report.passed)
}
#[derive(Deserialize)]
struct SpatialFixture {
    profiles: Vec<SpatialProfile>,
}
#[derive(Deserialize)]
struct SpatialProfile {
    pack: String,
    adapter: String,
    width: u32,
    height: u32,
    shader_sha256: String,
    copy_shader_sha256: String,
    cases: Vec<SpatialCase>,
}
#[derive(Deserialize)]
struct SpatialCase {
    name: String,
    bump_height: f32,
    part_bump_scale: f32,
    source_rgba: Vec<u8>,
    output_rgba: Vec<u8>,
}
#[cfg(windows)]
fn spatial(path: &Path, out: &Path, adapter: u32) -> Result<bool> {
    ignored_output(out)?;
    let fixture: SpatialFixture = read(path)?;
    let folder = path.parent().ok_or("fixture has no parent folder")?;
    let renderer = native_d3d9::Renderer::new(adapter)?;
    let mut passed = true;
    let mut pixels = 0;
    for profile in fixture.profiles {
        if profile.adapter != renderer.adapter.description {
            return Err("spatial fixture and replay adapters differ".into());
        }
        let normal = std::fs::read(folder.join(format!(
            "normal/shader-blobs/skpBumpToNormalShader-{}.bin",
            profile.pack
        )))
        .map_err(|e| e.to_string())?;
        let copy = std::fs::read(folder.join(format!(
            "copy/shader-blobs/skpSplatCopyTexShader-{}.bin",
            profile.pack
        )))
        .map_err(|e| e.to_string())?;
        if !parity::sha256(&normal).eq_ignore_ascii_case(&profile.shader_sha256)
            || !parity::sha256(&copy).eq_ignore_ascii_case(&profile.copy_shader_sha256)
        {
            return Err("spatial original shader fingerprint mismatch".into());
        }
        let mut expected = parity::ScenarioTrace {
            version: 1,
            manifest_sha256: parity::sha256(&std::fs::read(path).map_err(|e| e.to_string())?),
            scenario: format!("original-spatial-normal:{}", profile.pack),
            origin: parity::Origin::ShaderProbe,
            isolation: None,
            frames: vec![],
        };
        let mut actual = expected.clone();
        let profile_out = out.join(&profile.pack);
        std::fs::create_dir_all(&profile_out).map_err(|e| e.to_string())?;
        for (i, case) in profile.cases.iter().enumerate() {
            let mut packet = native_paint::normal_packet(
                native_paint::shader_words(&normal)?,
                if case.part_bump_scale == 1.0 {
                    None
                } else {
                    Some(native_paint::shader_words(&copy)?)
                },
                profile.width,
                profile.height,
                &case.source_rgba,
                case.part_bump_scale,
                case.bump_height,
            )?;
            packet.label = case.name.clone();
            let file = profile_out.join(format!("packet-{i:02}.json"));
            write(&file, &packet)?;
            let packet: NativeRenderPacket = read(&file)?;
            let result = renderer.replay(&packet)?;
            expected.frames.push(frame(
                i as u64,
                case.output_rgba.clone(),
                profile.width,
                profile.height,
            ));
            actual.frames.push(frame(
                i as u64,
                result.last().ok_or("no normal output")?.rgba8()?,
                profile.width,
                profile.height,
            ));
        }
        write(&profile_out.join("expected.json"), &expected)?;
        write(&profile_out.join("actual.json"), &actual)?;
        let report = parity::compare(&expected, &actual)?;
        pixels += report.compared_pixels;
        passed &= report.passed;
        write(&profile_out.join("comparison.json"), &report)?;
        println!(
            "{}: {} spatial pixels, exact {}",
            profile.pack, report.compared_pixels, report.passed
        );
    }
    println!("Spatial total: {pixels} pixels, exact {passed}");
    Ok(passed)
}
#[cfg(windows)]
fn oracle(path: &Path, out: &Path, adapter: u32) -> Result<bool> {
    ignored_output(out)?;
    let oracle: Oracle = read(path)?;
    let mut renderer = native_d3d9::Renderer::new(adapter)?;
    if renderer.adapter.vendor != oracle.vendor_id
        || renderer.adapter.device != oracle.device_id
        || renderer.adapter.description != oracle.adapter
    {
        return Err("oracle and replay adapters differ".into());
    }
    let shader = std::fs::read(&oracle.shader_file).map_err(|e| e.to_string())?;
    d3d9_shader::parse(&shader)?;
    let words: Vec<u32> = shader
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    let identity = parity::sha256(&std::fs::read(path).map_err(|e| e.to_string())?);
    let mut expected = parity::ScenarioTrace {
        version: 1,
        manifest_sha256: identity,
        scenario: format!("original-shader-probe:{}", parity::sha256(&shader)),
        origin: parity::Origin::ShaderProbe,
        isolation: None,
        frames: vec![],
    };
    let mut actual = expected.clone();
    for (i, case) in oracle.cases.iter().enumerate() {
        let packet = probe_packet(words.clone(), case);
        packet.validate()?;
        let file = out.join(format!("packet-{i:02}.json"));
        write(&file, &packet)?;
        // Replay the serialized boundary rather than the in-memory producer.
        let packet: NativeRenderPacket = read(&file)?;
        let result = renderer.replay(&packet)?;
        let rgba = result[0].rgba8()?;
        expected
            .frames
            .push(frame(i as u64, case.output.to_vec(), 1, 1));
        actual.frames.push(frame(i as u64, rgba, 1, 1));
    }
    write(&out.join("expected.json"), &expected)?;
    write(&out.join("actual.json"), &actual)?;
    let report = parity::compare(&expected, &actual)?;
    write(&out.join("comparison.json"), &report)?;
    // Verify resource teardown and rebuild through two real D3D9 Reset calls.
    let last = oracle.cases.last().ok_or("empty oracle")?;
    let packet = probe_packet(words, last);
    let before = renderer.replay(&packet)?[0].rgba8()?;
    renderer.reset(1920, 1080)?;
    let resized = renderer.replay(&packet)?[0].rgba8()?;
    renderer.reset(1, 1)?;
    let restored = renderer.replay(&packet)?[0].rgba8()?;
    let reset_passed = before == resized && before == restored;
    write(
        &out.join("adapter-reset.json"),
        &serde_json::json!({"adapter":renderer.adapter,"reset_sizes":[[1920,1080],[1,1]],"passed":reset_passed}),
    )?;
    println!(
        "{}: {} cases, exact pixels {}, device reset {}",
        oracle.shader_file.display(),
        oracle.cases.len(),
        report.passed,
        reset_passed
    );
    Ok(report.passed && reset_passed)
}
#[cfg(windows)]
fn replay(path: &Path, out: &Path, adapter: u32) -> Result<()> {
    ignored_output(out)?;
    let packet: NativeRenderPacket = read(path)?;
    let renderer = native_d3d9::Renderer::new(adapter)?;
    let targets = renderer.replay(&packet)?;
    for (i, target) in targets.iter().enumerate() {
        std::fs::write(out.join(format!("target-{i:02}.raw")), &target.bytes)
            .map_err(|e| e.to_string())?;
        if target.format == 21 {
            let file = std::fs::File::create(out.join(format!("target-{i:02}.png")))
                .map_err(|e| e.to_string())?;
            let mut encoder = png::Encoder::new(file, target.width, target.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .map_err(|e| e.to_string())?
                .write_image_data(&target.rgba8()?)
                .map_err(|e| e.to_string())?;
        }
    }
    write(
        &out.join("replay.json"),
        &serde_json::json!({"packet_sha256":parity::sha256(&std::fs::read(path).map_err(|e|e.to_string())?),"adapter":renderer.adapter,"targets":targets.iter().map(|t|serde_json::json!({"width":t.width,"height":t.height,"format":t.format,"row_pitch":t.row_pitch,"sha256":parity::sha256(&t.bytes)})).collect::<Vec<_>>()}),
    )?;
    println!(
        "Replayed {} commands into {} native targets",
        packet.commands.len(),
        targets.len()
    );
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestConfig {
    profile: String,
    executable: PathBuf,
    package_roots: Vec<PathBuf>,
    preferences: Vec<PathBuf>,
    adapter: u32,
    locale: String,
    settings: serde_json::Value,
}
fn package_files(root: &Path, result: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            package_files(&entry.path(), result)?;
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("package"))
        {
            result.push(entry.path());
        }
    }
    Ok(())
}
#[cfg(windows)]
fn manifest(config: &Path, out: &Path) -> Result<()> {
    let config: ManifestConfig = read(config)?;
    let adapter = native_d3d9::adapters()?
        .into_iter()
        .find(|a| a.ordinal == config.adapter)
        .ok_or("manifest adapter unavailable")?;
    let mut packages = vec![];
    for root in config.package_roots {
        package_files(&root, &mut packages)?;
    }
    packages.sort();
    packages.dedup();
    let manifest = parity::ReferenceManifest {
        version: 1,
        profile: config.profile,
        executable: parity::fingerprint(&config.executable)?,
        packages: packages
            .iter()
            .map(|p| parity::fingerprint(p))
            .collect::<Result<_>>()?,
        preferences: config
            .preferences
            .iter()
            .map(|p| parity::fingerprint(p))
            .collect::<Result<_>>()?,
        adapter: serde_json::to_value(adapter).map_err(|e| e.to_string())?,
        locale: config.locale,
        settings: config.settings,
    };
    let id = manifest.identity()?;
    write(out, &manifest)?;
    println!(
        "Reference {id}: {} packages fingerprinted",
        manifest.packages.len()
    );
    Ok(())
}
fn run() -> Result<bool> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        #[cfg(windows)] Some("replay-capture") if args.len()==3 => replay_capture(Path::new(&args[1]),Path::new(&args[2])),
        Some("compression-oracle") if args.len()==3 => {
            let output=Path::new(&args[2]);ignored_output(output.parent().ok_or("report needs a parent")?)?;
            let report=creature_data::compare_compression(read(Path::new(&args[1]))?)?;
            write(output,&report)?;println!("Compared {} native compressed-buffer cases, {} exact bytes; passed {}",report.compared_cases,report.compared_bytes,report.passed);Ok(report.passed)
        },
        Some("creature-data") if args.len()==3 => {
            let output=Path::new(&args[2]);ignored_output(output.parent().ok_or("output needs a parent")?)?;
            let bytes=std::fs::read(&args[1]).map_err(|e|e.to_string())?;
            let data=creature_data::parse(&bytes)?;
            if data.original_layout()?!=bytes {return Err("creature data losslessness check failed".into());}
            write(output,&data)?;
            println!("Native creature data v{}: {} rig blocks, {} capabilities, {} animation values; anatomy {:?}",data.version,data.rigblocks.len(),data.capability_ids.len(),data.animation_ids.len(),data.validate_anatomy());Ok(true)
        },
        #[cfg(windows)] Some("resources") if args.len()==3 || args.len()==4 => resources(Path::new(&args[1]),Path::new(&args[2]),if args.len()==4 {args[3].parse().map_err(|_|"invalid adapter ordinal")?} else {choose_adapter()?}),
        Some("texture") if args.len()==3 => {
            let source=Path::new(&args[1]);let output=Path::new(&args[2]);
            ignored_output(output.parent().ok_or("texture output needs a parent folder")?)?;
            let texture=native_texture::decode(&std::fs::read(source).map_err(|e|e.to_string())?,None)?;
            write(output,&texture)?;println!("Native {}x{}, {} faces, {} mips",texture.width,texture.height,texture.faces.len(),texture.faces[0].len());Ok(true)
        },
        Some("ledger-check") if args.len()==2 => {
            let path=Path::new(&args[1]);let ledger:parity_ledger::Ledger=read(path)?;
            ledger.validate(path.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new(".")))?;
            println!("Verified {} feature states and their native evidence",ledger.features.len());Ok(true)
        },
        #[cfg(windows)] Some("spatial") if args.len()==3 || args.len()==4 => spatial(Path::new(&args[1]),Path::new(&args[2]),if args.len()==4 {args[3].parse().map_err(|_|"invalid adapter ordinal")?} else {choose_adapter()?}),
        #[cfg(windows)] Some("adapters") if args.len()==1 => { println!("{}",serde_json::to_string_pretty(&native_d3d9::adapters()?).map_err(|e|e.to_string())?); Ok(true) },
        #[cfg(windows)] Some("manifest") if args.len()==3 => { manifest(Path::new(&args[1]),Path::new(&args[2]))?; Ok(true) },
        #[cfg(windows)] Some("oracle") if args.len()==3 || args.len()==4 => oracle(Path::new(&args[1]),Path::new(&args[2]),if args.len()==4 {args[3].parse().map_err(|_|"invalid adapter ordinal")?} else {choose_adapter()?}),
        #[cfg(windows)] Some("replay") if args.len()==3 || args.len()==4 => {replay(Path::new(&args[1]),Path::new(&args[2]),if args.len()==4 {args[3].parse().map_err(|_|"invalid adapter ordinal")?} else {choose_adapter()?})?; Ok(true)},
        Some("compare") if args.len()==4 => {
            let report=parity::compare(&read(Path::new(&args[1]))?,&read(Path::new(&args[2]))?)?;
            write(Path::new(&args[3]),&report)?; println!("{}",serde_json::to_string_pretty(&report).map_err(|e|e.to_string())?); Ok(report.passed)
        },
        Some("repeatability") if args.len()==5 => {let traces=args[1..4].iter().map(|a|read(Path::new(a))).collect::<Result<Vec<_>>>()?;write(Path::new(&args[4]),&parity::repeatability(&traces)?)?; Ok(true)},
        Some("isolation-check") if args.len()==2 => {let receipt:parity::ProfileIsolation=read(Path::new(&args[1]))?; receipt.validate()?;println!("Isolation receipt paths and startup sequence validated; receipt authenticity requires observer verification.");Ok(true)},
        _ => Err("Usage: openspore-parity adapters | manifest CONFIG OUTPUT | oracle GPU_FIXTURE OUTPUT_DIR [ADAPTER] | spatial FIXTURE OUTPUT_DIR [ADAPTER] | resources COPY_SHADER OUTPUT_DIR [ADAPTER] | texture INPUT OUTPUT_JSON | creature-data INPUT OUTPUT_JSON | compression-oracle FIXTURE REPORT | replay PACKET OUTPUT_DIR [ADAPTER] | replay-capture OWNED_CAPTURE_ROOT OUTPUT_DIR | compare EXPECTED ACTUAL REPORT | repeatability ORIGINAL1 ORIGINAL2 ORIGINAL3 REPORT | isolation-check RECEIPT | ledger-check LEDGER".into()),
    }
}
fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(2),
        Err(e) => {
            eprintln!("OpenSpore parity: {e}");
            std::process::exit(1)
        }
    }
}
