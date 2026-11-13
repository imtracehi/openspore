//! Native SkinPaint pass construction shared by the game and independent replay.
//! These are isolated part passes, not the complete procedural body atlas job.
use crate::native_packet::*;
type Result<T> = std::result::Result<T, String>;
static PACKETS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DRAWS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PIXELS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[derive(serde::Serialize)]
pub struct Counters {
    pub packets: u64,
    pub draws: u64,
    pub target_pixels: u64,
}
pub fn counters() -> Counters {
    use std::sync::atomic::Ordering::Relaxed;
    Counters {
        packets: PACKETS.load(Relaxed),
        draws: DRAWS.load(Relaxed),
        target_pixels: PIXELS.load(Relaxed),
    }
}
pub fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::args().any(|a| a == "--native-skinpaint"))
}
#[cfg(windows)]
pub fn execute(packet: &NativeRenderPacket) -> Result<Vec<u8>> {
    thread_local! {
        // A D3D9 device is used and destroyed on its creating thread. Bevy's
        // parallel asset work never shares the device or its hidden HWND.
        static DEVICE: std::cell::RefCell<Result<crate::native_d3d9::Renderer>> = std::cell::RefCell::new(
            crate::native_d3d9::adapters().and_then(|a| {
                let adapter=a.iter().find(|v|v.vendor==0x10de).or_else(||a.first()).ok_or("no HAL adapter")?;
                crate::native_d3d9::Renderer::new(adapter.ordinal)
            })
        );
    }
    DEVICE.with(|device| {
        let device = device.borrow();
        let renderer = device.as_ref().map_err(Clone::clone)?;
        let targets = renderer.replay(packet)?;
        use std::sync::atomic::Ordering::Relaxed;
        PACKETS.fetch_add(1, Relaxed);
        DRAWS.fetch_add(
            packet
                .commands
                .iter()
                .filter(|c| matches!(c, Command::Draw(_)))
                .count() as u64,
            Relaxed,
        );
        PIXELS.fetch_add(
            packet
                .targets
                .iter()
                .map(|t| t.width as u64 * t.height as u64)
                .sum(),
            Relaxed,
        );
        targets
            .last()
            .ok_or("native paint returned no target")?
            .rgba8()
    })
}
#[cfg(not(windows))]
pub fn execute(_packet: &NativeRenderPacket) -> Result<Vec<u8>> {
    Err("native SkinPaint requires Windows Direct3D 9".into())
}
fn texture(width: u32, height: u32, rgba: &[u8]) -> Result<Texture> {
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || rgba.len() as u64 != width as u64 * height as u64 * 4
    {
        return Err("invalid paint input dimensions".into());
    }
    Ok(Texture {
        width,
        height,
        format: 21,
        faces: vec![vec![Mip {
            row_pitch: width as usize * 4,
            bytes: rgba
                .chunks_exact(4)
                .flat_map(|v| [v[2], v[1], v[0], v[3]])
                .collect(),
        }]],
    })
}
pub fn shader_words(bytes: &[u8]) -> Result<Vec<u32>> {
    if crate::d3d9_shader::parse(bytes)?.stage != crate::d3d9_shader::Stage::Pixel {
        return Err("paint pass requires a pixel shader".into());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .collect())
}
pub fn fullscreen(
    width: u32,
    height: u32,
    pixel_shader: Vec<u32>,
    constants: Vec<[f32; 4]>,
    bindings: Vec<Binding>,
) -> Draw {
    let mut vertices = vec![];
    for (x, y, u, v) in [
        (-0.5, -0.5, 0.0f32, 0.0f32),
        (width as f32 - 0.5, -0.5, 1., 0.),
        (-0.5, height as f32 - 0.5, 0., 1.),
        (width as f32 - 0.5, height as f32 - 0.5, 1., 1.),
    ] {
        for val in [x, y, 0.0, 1.0] {
            vertices.extend_from_slice(&val.to_le_bytes());
        }
        vertices.extend_from_slice(&u32::MAX.to_le_bytes());
        vertices.extend_from_slice(&u32::MAX.to_le_bytes());
        for _ in 0..8 {
            vertices.extend_from_slice(&u.to_le_bytes());
            vertices.extend_from_slice(&v.to_le_bytes());
        }
    }
    let sampler_states = bindings
        .iter()
        .flat_map(|b| {
            [
                [b.sampler, 6, 1],
                [b.sampler, 5, 1],
                [b.sampler, 7, 2],
                [b.sampler, 1, 3],
                [b.sampler, 2, 3],
                [b.sampler, 3, 3],
                [b.sampler, 11, 0],
            ]
        })
        .collect();
    Draw {
        target: 0,
        viewport: [0, 0, width, height, 0, 1.0f32.to_bits()],
        scissor: None,
        vertex_shader: None,
        pixel_shader: Some(pixel_shader),
        fvf: Some(0x8c4),
        declaration: vec![],
        vertices,
        stride: 88,
        vertex_count: 4,
        indices: None,
        indexed_range: None,
        primitive_type: 5,
        primitive_count: 2,
        render_states: vec![
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
            [194, 0],
        ],
        sampler_states,
        texture_stage_states: vec![],
        bindings,
        constants: vec![Constants::Float {
            vertex: false,
            start: 0,
            bits: constants.into_iter().map(|v| v.map(f32::to_bits)).collect(),
        }],
    }
}
fn target(width: u32, height: u32) -> Target {
    Target {
        width,
        height,
        format: 21,
        depth_format: 0,
        surface_only: false,
    }
}
fn clear(target: usize) -> Command {
    Command::Clear {
        target,
        flags: 1,
        color: 0,
        depth_bits: 1.0f32.to_bits(),
        stencil: 0,
        viewport: None,
        rects: vec![],
        scissor: None,
    }
}
pub fn tint_packet(
    shader: Vec<u32>,
    width: u32,
    height: u32,
    diffuse: &[u8],
    mask_width: u32,
    mask_height: u32,
    mask: &[u8],
    palette: [[f32; 4]; 3],
) -> Result<NativeRenderPacket> {
    tint_texture_packet(
        shader,
        texture(width, height, diffuse)?,
        texture(mask_width, mask_height, mask)?,
        palette,
    )
}
pub fn tint_texture_packet(
    shader: Vec<u32>,
    diffuse: Texture,
    mask: Texture,
    palette: [[f32; 4]; 3],
) -> Result<NativeRenderPacket> {
    if diffuse.faces.len() != 1 || mask.faces.len() != 1 {
        return Err("part tint expects 2D native textures".into());
    }
    let width = diffuse.width;
    let height = diffuse.height;
    let mut draw = fullscreen(
        width,
        height,
        shader,
        palette.to_vec(),
        vec![
            Binding {
                sampler: 0,
                source: Some(TextureSource::Texture(0)),
            },
            Binding {
                sampler: 1,
                source: Some(TextureSource::Texture(1)),
            },
        ],
    );
    // The original preview's spatial tint sampling is linear. One-pixel hardware
    // probes cannot establish it; this path stays opt-in pending atlas capture.
    for [_, state, value] in &mut draw.sampler_states {
        if *state == 5 || *state == 6 {
            *value = 2;
        }
    }
    let packet = NativeRenderPacket {
        version: 1,
        label: "native isolated part tint".into(),
        targets: vec![target(width, height)],
        textures: vec![diffuse, mask],
        commands: vec![clear(0), Command::Draw(draw)],
    };
    packet.validate()?;
    Ok(packet)
}
pub fn normal_packet(
    normal_shader: Vec<u32>,
    copy_shader: Option<Vec<u32>>,
    width: u32,
    height: u32,
    source: &[u8],
    scale: f32,
    bump_height: f32,
) -> Result<NativeRenderPacket> {
    normal_texture_packet(
        normal_shader,
        copy_shader,
        texture(width, height, source)?,
        scale,
        bump_height,
    )
}
pub fn normal_texture_packet(
    normal_shader: Vec<u32>,
    copy_shader: Option<Vec<u32>>,
    source: Texture,
    scale: f32,
    bump_height: f32,
) -> Result<NativeRenderPacket> {
    if !scale.is_finite() || !bump_height.is_finite() {
        return Err("nonfinite native paint parameters".into());
    }
    if source.faces.len() != 1 {
        return Err("part normals expect a 2D native texture".into());
    }
    let width = source.width;
    let height = source.height;
    let mut targets = vec![target(width, height)];
    let mut commands = vec![];
    let input = if let Some(copy) = copy_shader {
        commands.push(clear(0));
        // Native R/specular write and B/scaled-height write into RGBA8. GPU
        // quantization happens before the normal shader samples the target.
        for (mask, value) in [(1, 1.0), (4, scale)] {
            let mut draw = fullscreen(
                width,
                height,
                copy.clone(),
                vec![[value, value, value, 1.0]],
                vec![Binding {
                    sampler: 0,
                    source: Some(TextureSource::Texture(0)),
                }],
            );
            draw.render_states.iter_mut().find(|s| s[0] == 168).unwrap()[1] = mask;
            commands.push(Command::Draw(draw));
        }
        targets.push(target(width, height));
        TextureSource::Target(0)
    } else {
        if scale != 1.0 {
            return Err("scaled normal bake needs original copy shader".into());
        }
        TextureSource::Texture(0)
    };
    let destination = targets.len() - 1;
    let height_value = if bump_height == 0.0 { 5.0 } else { bump_height };
    let mut draw = fullscreen(
        width,
        height,
        normal_shader,
        vec![[
            1.0 / width as f32,
            1.0 / height as f32,
            height_value / 3.0,
            0.0,
        ]],
        vec![Binding {
            sampler: 0,
            source: Some(input),
        }],
    );
    draw.target = destination;
    commands.push(clear(destination));
    commands.push(Command::Draw(draw));
    let packet = NativeRenderPacket {
        version: 1,
        label: "native isolated part normals".into(),
        targets,
        textures: vec![source],
        commands,
    };
    packet.validate()?;
    Ok(packet)
}
