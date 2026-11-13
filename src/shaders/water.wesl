import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

import bevy_pbr::render::mesh_view_bindings::view;
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> parameters: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> coast: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var normal_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var normal_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var foam_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var foam_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var seabed_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var seabed_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var<uniform> native_layers: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var<uniform> shallow_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var<uniform> sky_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var<uniform> tuning: vec4<f32>;
// Planet mode: xyz planet centre, w water radius (0 = flat island sea).
@group(constants::MATERIAL_BIND_GROUP) @binding(112) var<uniform> planet: vec4<f32>;
// Water depth below the sea surface / coast.w, in world orientation.
@group(constants::MATERIAL_BIND_GROUP) @binding(113) var depth_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(114) var depth_sampler: sampler;
// Retail TerrainWater (planet_sky.rs fills these; sky[15].x = 1 enables it).
@group(constants::MATERIAL_BIND_GROUP) @binding(115) var<uniform> sky: array<vec4<f32>, 24>;
@group(constants::MATERIAL_BIND_GROUP) @binding(116) var scatter_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(117) var scatter_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(118) var pca0_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(119) var pca0_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(120) var pca1_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(121) var pca1_sampler: sampler;

const FACE_EDGE_UNITS: f32 = 785.3982;

fn scatter_dir(d: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(sky[6].xyz, d), dot(sky[7].xyz, d), dot(sky[8].xyz, d));
}
fn scatter_at(d: vec3<f32>) -> vec4<f32> {
    return textureSampleLevel(scatter_map, scatter_sampler, scatter_dir(d), 0.0);
}
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}
// GetInScatterTerminator.
fn terminator(n: vec3<f32>) -> f32 {
    let dp = dot(n, sky[0].xyz);
    let cutoff = sky[0].w;
    return select(max(0.0, dp + cutoff) / (2.0 * cutoff), 1.0, dp > cutoff);
}
// GetTerrainFogStrength: the path toward the camera inside the shell
// (R + thickness) beyond the fogParms.z minimum distance.
fn fog_strength(p: vec3<f32>, n: vec3<f32>, e: vec3<f32>, cam_dist: f32) -> f32 {
    let atmo = sky[3].x + sky[3].z;
    let b = dot(-e, p);
    let c = dot(p, p) - atmo * atmo;
    let t = -b + sqrt(max(b * b - c, 0.0));
    let dist = max(0.0, min(t, cam_dist) - sky[4].z);
    return (1.0 - exp(sky[4].x * dist)) * terminator(n);
}
// TerrainWaterVS undistort table (cube-face projection compensation).
fn mm(i: i32) -> f32 {
    switch i {
        case 0: { return 0.57735026; }
        case 1: { return 0.60290092; }
        case 2: { return 0.62773043; }
        case 3: { return 0.65094453; }
        case 4: { return 0.67149222; }
        case 5: { return 0.68824714; }
        case 6: { return 0.70014000; }
        default: { return 0.70632243; }
    }
}

// TerrainWaterVSMedium + TerrainWaterPSMedium (no shadow map: shadow = 1).
// The refraction render target is replaced by blending over the real sea
// floor: out = A + B * floor, emitted premultiplied (rgb = a A, alpha =
// a (1 - B)); the refraction target's alpha is 0 (TerrainBelowRefract
// writes oC0.w = 0), so the program's final lerp drops out.
// World units per planet unit: 1 on the stage planets, the Space planet
// view's scale there (sky[9].w; the TerrainState stays in planet units).
fn world_unit() -> f32 {
    return select(1.0, sky[9].w, sky[9].w > 0.0);
}

fn retail_water(in: VertexOutput) -> FragmentOutput {
    let centre = sky[2].xyz;
    let p = (in.world_position.xyz - centre) / world_unit();
    let n = normalize(p);
    let cam = (view.world_position.xyz - centre) / world_unit();
    let to_vert = p - cam;
    let cam_dist = length(to_vert);
    let e = to_vert / max(cam_dist, 1e-4);
    let sun = sky[0].xyz;
    let time = sky[9].x;
    let alt_range = sky[3].y;
    let uv = in.uv / FACE_EDGE_UNITS;

    // Vertex terms.
    let fog = fog_strength(p, n, e, cam_dist);
    let fresnel = 1.0 - dot(-e, n);
    let hf_fade = (1.0 - clamp(cam_dist / 1200.0, 0.0, 1.0)) * 0.95 + 0.05;
    let sun_drop = clamp(dot(n, sun) + 0.95, 0.0, 1.0);
    let cam_offset = mix(1.0, 10.0, clamp((length(cam) - sky[3].x - 100.0) / 500.0, 0.0, 1.0));
    let vertex_above = (sky[2].w - length(p) + cam_offset) / alt_range;
    var foam_wave = 1.0 - fract(vec2<f32>(time / 16.0) + vec2<f32>(0.0, 0.5));
    let wave_blend = foam_wave * (1.0 - foam_wave) * 1.75;
    foam_wave = foam_wave * 7.5;
    var undistort = normalize(vec3<f32>(uv.x - 0.5, uv.y - 0.5, -0.5));
    var entry = (1.0 - abs(fract(uv) * 2.0 - 1.0)) * 6.999;
    let frac_e = fract(entry);
    entry = floor(entry);
    let ex = i32(entry.x);
    let ey = i32(entry.y);
    let minmax = vec2<f32>(mix(mm(ex), mm(ex + 1), frac_e.x), mix(mm(ey), mm(ey + 1), frac_e.y));
    undistort = vec3<f32>((undistort.xy + minmax) / minmax, undistort.z);
    let anim = undistort.xy * 10.0 + sin(undistort.yx * 20.0 + vec2<f32>(time) * vec2<f32>(0.1, 0.18)) * 0.125;
    // ADAPTED tangent frame: the retail cross(N, +Y) degenerates at the stage
    // pole of this world frame.
    let helper = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.9);
    let tangent = normalize(cross(n, helper));
    let binormal = cross(tangent, n);

    // Pixel terms: PCADecompressTexture (pcawater.pcaw frame, PCAwater2_0/1).
    let t0 = textureSample(pca0_map, pca0_sampler, anim) * 2.0 - 1.0;
    let t1 = textureSample(pca1_map, pca1_sampler, anim) * 2.0 - 1.0;
    var pca = vec2<f32>(dot(t0, sky[10]) + dot(t1, sky[12]) + sky[14].x,
                        dot(t0, sky[11]) + dot(t1, sky[13]) + sky[14].z) * (2.0 / 255.0) - 1.0;
    pca = select(vec2<f32>(0.0), pca, sky[15].y > 0.5);
    let depth_units = textureSample(depth_cube, depth_sampler, n).r * sky[9].y;
    let water_depth = clamp(depth_units / alt_range, 0.0, 1.0);
    let alpha = clamp(min(water_depth, max(vertex_above, 0.0)) * 100.0, 0.0, 1.0);
    let bump = n + tangent * (pca.x * hf_fade * 1.5) + binormal * (pca.y * hf_fade * 1.5);
    let eye_dir = -e;
    let reflected = eye_dir - 2.0 * dot(eye_dir, bump) * bump;
    let w = clamp(fresnel - 0.5, 0.0, 1.0);
    let ndl = dot(sun, bump);
    let ndl_sat = clamp(ndl, 0.0, 1.0);
    let ndl_abs = abs(ndl);
    let middle = (1.0 - ndl_abs) * (1.0 - ndl_abs);
    let light = ndl_abs * 0.9 + ndl_sat * 0.25 + middle * 0.5;
    let foam = textureSample(foam_map, foam_sampler, uv * 16.0).r;
    let cut = textureSample(foam_map, foam_sampler, uv * 5.0).a;
    let wave_foam = foam * cut * wave_blend * (1.0 - clamp(abs(foam_wave - water_depth * 50.0), vec2<f32>(0.0), vec2<f32>(1.0)));
    let foam_i = (wave_foam.x + wave_foam.y) * native_layers.y;
    let spec_c = scatter_at(reflected);
    let spec = to_linear(spec_c.rgb) * spec_c.a * ndl_sat * 3.0;
    let s = scatter_at(-e);
    let s_lin = to_linear(s.rgb);
    let water_color = to_linear(sky[1].rgb);
    let a_term = (1.0 - fog) * (w * water_color * light + vec3<f32>(foam_i * light + foam_i) + spec)
        + fog * s_lin + s_lin * s.a * sun_drop;
    let b_term = (1.0 - w) * light * (1.0 - fog);
    var out: FragmentOutput;
    out.color = vec4<f32>(a_term * alpha, clamp(alpha * (1.0 - b_term), 0.0, 1.0));
    // OPENSPORE_WATER_DEBUG (development): 1 transparent, 2 specular, 3 fog, 4 B.
    let dbg = sky[15].z;
    if dbg > 0.5 && dbg < 1.5 { out.color = vec4<f32>(0.0); }
    if dbg > 1.5 && dbg < 2.5 { out.color = vec4<f32>(spec, 1.0); }
    if dbg > 2.5 && dbg < 3.5 { out.color = vec4<f32>(vec3<f32>(fog), 1.0); }
    if dbg > 3.5 { out.color = vec4<f32>(vec3<f32>(b_term * 0.5), 1.0); }
    return out;
}

// Shelf profile shared with terrain.rs (BASE_Y, SEA_Y, SHELF_DEPTH).
const BASE_Y: f32 = -0.6;
const SEA_Y: f32 = -0.9;
const SHELF_DEPTH: f32 = 1.6;

fn coast_depth(p: vec2<f32>, extent: f32, phase: f32) -> f32 {
    let radial_noise = sin(p.x * 0.31 + phase) * 1.05
        + cos(p.y * 0.27 - phase * 0.7) * 0.8
        + sin((p.x + p.y) * 0.16 + phase * 1.4) * 0.65;
    return extent * 0.86 + radial_noise - length(p);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if sky[15].x > 0.5 && planet.w > 0.0 {
        return retail_water(in);
    }
    var input = pbr_input_from_standard_material(in, is_front);
    let planet_mode = planet.w > 0.0;
    var p = in.world_position.xz;
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if planet_mode {
        // Planet water sphere: per-face uv in world units, normal = local up.
        p = in.uv;
        up = normalize(in.world_position.xyz - planet.xyz);
    }
    let planet_depth = textureSample(depth_cube, depth_sampler, up).r * coast.w;
    let t = parameters.x;
    // Adapted crossing projections of the installed normal, not PCA playback.
    let uv0 = p * 0.16 + vec2<f32>(t * 0.009, t * 0.004);
    let uv1 = vec2<f32>(-p.y, p.x) * 0.113 + vec2<f32>(-t * 0.006, t * 0.008);
    let n0 = textureSample(normal_map, normal_sampler, uv0).rg * 2.0 - 1.0;
    let n1 = textureSample(normal_map, normal_sampler, uv1).rg * 2.0 - 1.0;
    let ripple = (n0 + n1) * parameters.z * native_layers.x;
    input.N = normalize(vec3<f32>(ripple.x, 1.0, ripple.y));
    if planet_mode {
        let tangent = normalize(cross(up, select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(up.z) > 0.9)));
        let bitangent = cross(tangent, up);
        input.N = normalize(up + tangent * ripple.x + bitangent * ripple.y);
    }

    // Water depth over the island's generated shelf. Distant water (and
    // horizon islands) are treated as deep.
    let shore = coast_depth(p, coast.y, coast.x);
    let shelf = BASE_Y - SHELF_DEPTH * smoothstep(0.0, 1.0, (0.9 - shore) / 4.5);
    var depth = mix(8.0, max(SEA_Y - shelf, 0.0), coast.z);
    if planet_mode {
        depth = planet_depth;
    }
    let shallow = 1.0 - smoothstep(0.0, tuning.w, depth);

    // The authored WaterColor is the deep tint; shallow shelves turn
    // turquoise and let the installed seabed raster show through.
    let bed = textureSample(seabed_map, seabed_sampler, p * 0.12 + ripple * 0.08).rgb;
    let bed_tint = mix(vec3<f32>(1.0), bed * 2.4, native_layers.z * tuning.z * shallow * shallow);
    var water = mix(input.material.base_color.rgb, shallow_color.rgb, shallow) * bed_tint;

    // Original foamline raster: incoming lines along the waterline.
    var shore_u = atan2(p.y, p.x) * 11.0;
    if planet_mode {
        shore_u = (p.x + p.y) * 0.05;
    }
    let incoming_wave = depth * 2.2 - t * 0.17;
    let foam = textureSample(foam_map, foam_sampler, vec2<f32>(shore_u, incoming_wave)).r;
    let edge = 1.0 - smoothstep(0.0, 0.07, depth);
    let foam_band = (1.0 - smoothstep(0.05, 0.42, depth)) * smoothstep(0.0, 0.015, depth);
    let foam_amount = clamp(
        (smoothstep(0.3, 0.75, foam) * foam_band + edge * 0.55) * native_layers.y * coast.z * tuning.x,
        0.0, 1.0);
    water = mix(water, vec3<f32>(0.86, 0.94, 0.95), foam_amount);
    input.material.base_color = vec4<f32>(water, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(input);
    let direction = normalize(view.world_position.xyz - in.world_position.xyz);
    let fresnel = pow(1.0 - clamp(dot(input.N, direction), 0.0, 1.0), 5.0);
    // Preview environment tint, not an original reflection shader execution.
    out.color = vec4<f32>(mix(out.color.rgb, sky_color.rgb,
        (0.04 + fresnel * tuning.y) * (1.0 - foam_amount)), out.color.a);
    out.color = main_pass_post_lighting_processing(input, out.color);
    return out;
}
