import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};
import bevy_pbr::render::mesh_view_bindings::{view, lights};
import bevy_pbr::render::mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT;
import bevy_pbr::render::shadows::fetch_directional_shadow;

@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> parameters: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101)
var layers_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102)
var layers_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103)
var ramp_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104)
var ramp_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105)
var<uniform> beach_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106)
var<uniform> cliff_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107)
var detail_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108)
var detail_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(109)
var<uniform> tiling: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(110)
var<uniform> shaping: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(111)
var<uniform> surface: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(112)
var<uniform> grading: vec4<f32>;
// Planet mode (planet_render): xyz planet centre, w water radius (0 = the
// flat prototype island).
@group(constants::MATERIAL_BIND_GROUP) @binding(113)
var<uniform> planet: vec4<f32>;
// x: beach/wet band scale (beach thresholds are in world units), y: unused.
@group(constants::MATERIAL_BIND_GROUP) @binding(114)
var<uniform> planet_bands: vec4<f32>;

// Retail TerrainState (planet_sky.rs; sky[15].x = 1 on the stage planets):
// the in-scatter fog of TerrainAbove and the TerrainBelowRefract sea floor.
@group(constants::MATERIAL_BIND_GROUP) @binding(115)
var<uniform> sky: array<vec4<f32>, 24>;
@group(constants::MATERIAL_BIND_GROUP) @binding(116)
var scatter_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(117)
var scatter_sampler: sampler;
// terrainThemeBelow (seabed raster).
@group(constants::MATERIAL_BIND_GROUP) @binding(118)
var below_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(119)
var below_sampler: sampler;

fn scatter_at(d: vec3<f32>) -> vec4<f32> {
    let t = vec3<f32>(dot(sky[6].xyz, d), dot(sky[7].xyz, d), dot(sky[8].xyz, d));
    return textureSampleLevel(scatter_map, scatter_sampler, t, 0.0);
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
// GetTerrainFogStrength (shell R + thickness, fogParms).
fn fog_strength(p: vec3<f32>, n: vec3<f32>, e: vec3<f32>, cam_dist: f32) -> f32 {
    let atmo = sky[3].x + sky[3].z;
    let b = dot(-e, p);
    let c = dot(p, p) - atmo * atmo;
    let t = -b + sqrt(max(b * b - c, 0.0));
    let dist = max(0.0, min(t, cam_dist) - sky[4].z);
    return (1.0 - exp(sky[4].x * dist)) * terminator(n);
}

// Retail terrain lighting (HLSL fragments 0x40212002: TerrainLightingLib
// EvaluateVertexLighting, TerrainPixelLib EvaluatePixelLighting,
// TerrainAboveSynthVS/PSMedium, TerrainBelowRefractPS). Slots 16..23 of the
// TerrainState (planet_sky.rs): 16 nightLightTint (w = kNightLumBlend), 17..19
// day / dusk / night light colour, 20..22 day / dusk / night shadow colour,
// 23 duskDawnStartEnd (scale, scale, bias, bias; 0xF98910). All values are
// display (gamma) values like the retail framebuffer.
// (day, dusk, night) weights of a sphere direction (EvaluateVertexLighting).
fn light_blend(n: vec3<f32>) -> vec3<f32> {
    let ndl = dot(n, sky[0].xyz);
    let dn = clamp(vec2<f32>(ndl, -ndl) * sky[23].xy + sky[23].zw, vec2<f32>(0.0), vec2<f32>(1.0));
    return vec3<f32>(dn.x, 1.0 - max(dn.x, dn.y), dn.y);
}
fn blend_light(b: vec3<f32>) -> vec3<f32> {
    return b.x * sky[17].rgb + b.y * sky[18].rgb + b.z * sky[19].rgb;
}
fn blend_shadow(b: vec3<f32>) -> vec3<f32> {
    return b.x * sky[20].rgb + b.y * sky[21].rgb + b.z * sky[22].rgb;
}
// EvaluatePixelLighting: bendLightAtDusk 0.35, abs / sat / mid 0.9 / 0.2 / 0.5,
// shadowAmount 0.2, night luminance x nightLightTint by nightLumBlend.
fn pixel_lighting(normal: vec3<f32>, diffuse: vec3<f32>, shadow: f32, b: vec3<f32>) -> vec3<f32> {
    let shadow_color = blend_shadow(b);
    let delta = blend_light(b) - shadow_color;
    let sun = mix(sky[0].xyz, normal, b.y * 0.35);
    let ndl = dot(normal, sun);
    let ndl_abs = abs(ndl);
    let middle = (1.0 - ndl_abs) * (1.0 - ndl_abs);
    var strength = ndl_abs * 0.9 + clamp(ndl, 0.0, 1.0) * 0.2 + middle * 0.5;
    strength = strength * mix(1.0, shadow, 0.2);
    let light = (shadow_color + delta * strength) * diffuse * strength;
    let night = dot(light, vec3<f32>(0.5)) * sky[16].rgb;
    return mix(light, night, sky[16].w * b.z);
}
fn to_display(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
}
// The retail shadow-map term (applyShadowMapTerrain) from Bevy's cascades
// of the shadow-casting sun (ADAPTED: Bevy's PCF instead of the retail
// Fourier-basis maps; 1 = lit).
fn sun_shadow(world: vec4<f32>, n: vec3<f32>, frag: vec2<f32>) -> f32 {
    let view_z = dot(vec4<f32>(view.view_from_world[0].z, view.view_from_world[1].z,
        view.view_from_world[2].z, view.view_from_world[3].z), world);
    for (var i: u32 = 0u; i < lights.n_directional_lights; i = i + 1u) {
        if (lights.directional_lights[i].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            return fetch_directional_shadow(i, world, n, view_z, frag);
        }
    }
    return 1.0;
}

const SEA_Y: f32 = -0.9;
// AboveDetail2's constant red channel, paired with the low-tiling sample by
// the native program but dropped when the rasters are packed.
const DETAIL_CONSTANT: f32 = 0.518;
// Mean of the beach_generic luminance raster.
const BEACH_MEAN: f32 = 0.574;
const SAND: vec3<f32> = vec3<f32>(0.60, 0.47, 0.27);

// Same seed-warped coastline as terrain::coast_depth (positive inland).
fn coast_depth(p: vec2<f32>, extent: f32, phase: f32) -> f32 {
    let radial_noise = sin(p.x * 0.31 + phase) * 1.05
        + cos(p.y * 0.27 - phase * 0.7) * 0.8
        + sin((p.x + p.y) * 0.16 + phase * 1.4) * 0.65;
    return extent * 0.86 + radial_noise - length(p);
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var input = pbr_input_from_standard_material(in, is_front);
    let world = in.world_position.xyz;
    let planet_mode = planet.w > 0.0;
    // On the planet the detail tiles per cube face (uv = face uv x ~785
    // units, the native per-face tiling), elevation is measured above the
    // water sphere and slopes against the local up.
    var p = world.xz;
    var up = vec3<f32>(0.0, 1.0, 0.0);
    var sea_height = world.y - SEA_Y;
    // Space planet view: the planet is drawn at planet_bands.y world units
    // per planet unit; heights, distances and the rock projection below are
    // then measured in planet units (0 = the stages' 1:1 planet, unchanged).
    let world_unit = select(1.0, planet_bands.y, planet_bands.y > 0.0);
    var tri = world;
    if planet_mode {
        p = in.uv;
        up = normalize(world - planet.xyz);
        sea_height = (length(world - planet.xyz) - planet.w) / world_unit;
        if planet_bands.y > 0.0 {
            tri = (world - planet.xyz) / world_unit;
        }
    }
    let phase = parameters.x;
    let normal = normalize(in.world_normal);
    let camera_distance = distance(world, view.world_position.xyz) / world_unit;

    // Biome noise (biome_noise_modifier) at two scales breaks the color ramp
    // into patches, standing in for the native munged control maps.
    let noise_macro = textureSample(layers_map, layers_sampler,
        p * 0.011 + vec2<f32>(phase * 0.11, phase * 0.17)).b;
    let noise_mid = textureSample(layers_map, layers_sampler,
        p * 0.047 + vec2<f32>(phase * 0.23, phase * 0.05)).b;

    // Elevation control for the installed color ramp.
    let elevation = sea_height / shaping.y;
    let control = clamp(elevation + (noise_macro - 0.5) * shaping.z
        + (noise_mid - 0.5) * shaping.z * 0.45, 0.004, 0.996);
    let ramp = textureSample(ramp_map, ramp_sampler, vec2<f32>(control, 0.5)).rgb;
    var ground = select(input.material.base_color.rgb, ramp, surface.w > 0.5);
    ground = mix(ground, vec3<f32>(luminance(ground)), grading.y);

    // Native TerrainAbove detail: two samples of the packed raster at high
    // and low tiling, blended by the ramp control value. The sum averages
    // ~1.0; the preview widens its contrast for the close land camera.
    let a = textureSample(detail_map, detail_sampler, p * tiling.x
        + vec2<f32>(phase * 0.19, phase * 0.31));
    let b = textureSample(detail_map, detail_sampler, p * tiling.y
        + vec2<f32>(phase * 0.07, phase * 0.13));
    let c = textureSample(detail_map, detail_sampler, p * tiling.w
        + vec2<f32>(phase * 0.41, phase * 0.29));
    let native_detail = mix(a.g + b.r, a.r + DETAIL_CONSTANT, control);
    let near_fade = 1.0 - smoothstep(5.0, 24.0, camera_distance);
    // AboveDetail1 (intertiling) adds fine grain only near the camera.
    let near = ((c.b - 0.5) + (c.a - 0.5) * 0.5) * surface.y * near_fade;
    let detail_amount = clamp(parameters.z, 0.0, 1.0);
    let variation = (native_detail - 1.0) * shaping.w;
    let detail = max(1.0 + (variation + near) * detail_amount, 0.25);

    // Dark broad detail reads as bare soil patches: tint toward the cliff
    // earth color, keyed to the low-tiling sample so patches stay large.
    let soil = cliff_color.rgb * vec3<f32>(0.85, 0.8, 0.75);
    let dirt = clamp((0.5 - b.r) * 5.0 + (0.5 - noise_mid) * 0.8, 0.0, 1.0)
        * grading.x * detail_amount;
    ground = mix(ground, mix(ground, soil, 0.6), dirt);
    var albedo = ground * detail;

    // Bump the lighting normal with the same detail height so the ground
    // reads as textured under the low sun. Finite differences of offset
    // samples avoid the 2x2-quad artifacts of screen-space derivatives.
    let texel = 2.0 / 512.0;
    let high_uv = p * tiling.x + vec2<f32>(phase * 0.19, phase * 0.31);
    let low_uv = p * tiling.y + vec2<f32>(phase * 0.07, phase * 0.13);
    let ax = textureSample(detail_map, detail_sampler, high_uv + vec2<f32>(texel, 0.0));
    let az = textureSample(detail_map, detail_sampler, high_uv + vec2<f32>(0.0, texel));
    let bx = textureSample(detail_map, detail_sampler, low_uv + vec2<f32>(texel, 0.0));
    let bz = textureSample(detail_map, detail_sampler, low_uv + vec2<f32>(0.0, texel));
    let high_h = mix(a.g, a.r, control);
    let high_dx = (mix(ax.g, ax.r, control) - high_h) / texel * tiling.x;
    let high_dz = (mix(az.g, az.r, control) - high_h) / texel * tiling.x;
    let low_dx = (bx.r - b.r) / texel * tiling.y * (1.0 - control);
    let low_dz = (bz.r - b.r) / texel * tiling.y * (1.0 - control);
    let bump_fade = (1.0 - smoothstep(12.0, 45.0, camera_distance)) * surface.x * detail_amount;
    var tangent = vec3<f32>(1.0, 0.0, 0.0);
    var bitangent = vec3<f32>(0.0, 0.0, 1.0);
    if planet_mode {
        tangent = normalize(cross(up, select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(up.z) > 0.9)));
        bitangent = cross(tangent, up);
    }
    let slope_vector = (tangent * (high_dx + low_dx * 4.0) + bitangent * (high_dz + low_dz * 4.0)) * bump_fade;
    let shading_normal = normalize(normal - (slope_vector - normal * dot(slope_vector, normal)));

    // Native cliff brush: cliff_base luminance times the palette CliffColor,
    // blended between kMinCliffGradient and kMaxCliffGradient. Triplanar
    // projection keeps the rock from stretching on steep faces.
    let cliff_noise = (noise_mid - 0.5) * 0.1;
    let slope_cliff = 1.0 - smoothstep(shaping.x, parameters.y, dot(normal, up) + cliff_noise);
    var weights = pow(abs(normal), vec3<f32>(4.0));
    weights = weights / (weights.x + weights.y + weights.z);
    let rock = textureSample(layers_map, layers_sampler, tri.zy * tiling.z).r * weights.x
        + textureSample(layers_map, layers_sampler, p * tiling.z).r * weights.y
        + textureSample(layers_map, layers_sampler, tri.xy * tiling.z).r * weights.z;
    // Height-blend: raised rock texels surface first, so partial slopes show
    // crisp rock patches instead of a soft green/orange mix.
    let cliff = smoothstep(0.35, 0.65, slope_cliff + (rock - 0.55) * 0.9 * slope_cliff * (1.0 - slope_cliff) * 4.0);
    let rock_shade = pow(clamp(0.55 + (rock - 0.55) * grading.w, 0.05, 1.0), 2.2) * 2.4;
    let cliff_albedo = cliff_color.rgb * rock_shade * mix(1.0, detail, 0.5);

    // Native beach brush: flat BeachColor near the waterline, here textured
    // with beach_generic and pulled toward sand for a readable shoreline.
    let island_coast = coast_depth(p, parameters.w, phase);
    let coast_beach = 1.0 - smoothstep(0.4, 2.4, island_coast + (noise_mid - 0.5) * 1.6);
    let band = max(planet_bands.x, 1.0);
    let height_beach = 1.0 - smoothstep(0.12 * band, 0.5 * band, sea_height + (noise_mid - 0.5) * 0.3 * band);
    let beach = select(height_beach, coast_beach, parameters.w > 0.0) * (1.0 - cliff);
    let grain = textureSample(layers_map, layers_sampler, p * 0.21).g;
    var sand = mix(beach_color.rgb, SAND, surface.z) * (1.0 + (grain - BEACH_MEAN) * 4.0);
    sand = sand * mix(1.0, detail, 0.35);
    // Wet sand darkens right at the waterline.
    let wet = 1.0 - smoothstep(0.0, 0.14 * band, sea_height);
    sand = sand * mix(1.0, 0.62, wet);

    albedo = mix(albedo, sand, clamp(beach, 0.0, 1.0));
    albedo = mix(albedo, cliff_albedo, clamp(cliff, 0.0, 1.0));
    input.material.base_color = vec4<f32>(albedo * grading.z, input.material.base_color.a);
    input.N = shading_normal;
    // TerrainBelowRefract: the sea floor takes the theme's seabed raster
    // (kBelowTexTiling 3 per face, INFERRED uv) under the same lighting.
    // Also on the Space planet view (sky[15].x = 1 there since space-visuals:
    // positions are measured in planet units, `world_unit`).
    let retail = sky[15].x > 0.5 && planet_mode;
    let under = retail && sea_height < 0.0;
    // The seabed raster shares the face uv of the height / normal maps
    // (TerrainBelowRefractPS: uv * (1 - 1/256) + 0.5/256): one copy per face.
    let bed = textureSample(below_map, below_sampler, p / 785.3982 * (1.0 - 1.0 / 256.0) + 0.5 / 256.0).rgb;
    if under {
        input.material.base_color = vec4<f32>(bed * grading.z, input.material.base_color.a);
    }
    var out: FragmentOutput;
    // Retail land / sea-floor lighting (sky[15].w = 1): the day / dusk / night
    // terrain colours by the sun angle at each point, computed in display
    // values and converted to linear once at the end (as the water).
    // Also on the Space planet view (space_planets::sky: the same TerrainState
    // with the in-scatter fog and sea floor, in planet units).
    let retail_light = planet_mode && sky[15].w > 0.5;
    if !retail_light {
        out.color = apply_pbr_lighting(input);
    }
    if retail_light {
        let pos = (world - sky[2].xyz) / world_unit;
        let cam = (view.world_position.xyz - sky[2].xyz) / world_unit;
        let cam_dist = length(pos - cam);
        let e = (pos - cam) / max(cam_dist, 1e-4);
        // lightBlend from the sphere direction (the VS `normal`), the map
        // normal = the terrain's geometric normal.
        let b = light_blend(up);
        // The shadow cascades belong to the stages' sun (none in Space).
        let shadow = select(1.0, sun_shadow(in.world_position, normal, in.position.xy), retail && planet_bands.y <= 0.0);
        // The land's palette ramp is full scale, the retail colour map half
        // (sky[9].z, planet_light.rs); the seabed raster is used as installed.
        let diffuse = to_display(input.material.base_color.rgb) * select(sky[9].z, 1.0, under);
        if under {
            // TerrainBelowRefractPS: texNormal *= shadow 0.4 + 0.6, no shadow
            // term, then the depth cue over waterDepthLevel 60 toward the
            // luminance x waterColor and toward waterColor by depth / 200 +
            // 0.7 fresnel^2 (the water's own fog is applied by the surface).
            let lit = pixel_lighting(normal * (shadow * 0.4 + 0.6), diffuse, 1.0, b);
            let water_color = sky[1].rgb;
            let depth = -sea_height;
            let cue = 1.0 - clamp(depth / 60.0, 0.0, 1.0);
            let bottom = dot(lit, vec3<f32>(0.333)) * water_color;
            var c = mix(bottom, lit, cue);
            let fresnel = 1.0 - dot(-e, up);
            c = mix(c, water_color, clamp(depth * 0.5 / 100.0 + fresnel * fresnel * 0.7, 0.0, 1.0));
            var c_lin = to_linear(c);
            if sky[15].z > 0.5 && sky[15].z < 1.5 { c_lin = vec3<f32>(clamp(depth / 60.0, 0.0, 1.0)); }
            out.color = vec4<f32>(c_lin, 1.0);
        } else {
            // TerrainAboveSynthPSMedium: pixel lighting, detailing (pinch 3,
            // intensity 0.3 by the map normal against the view), then
            // lerp(light, scatter, fog) + scatter x a x sunDrop.
            var lit = pixel_lighting(normal, diffuse, shadow, b);
            let tndotv = dot(normal, -e);
            lit = mix(lit, lit * clamp(tndotv * 3.0, 0.0, 1.0), 0.3);
            if retail {
                let fog = fog_strength(pos, up, e, cam_dist);
                let s = scatter_at(-e);
                let sun_drop = clamp(dot(up, sky[0].xyz) + 0.95, 0.0, 1.0);
                out.color = vec4<f32>(to_linear(mix(lit, s.rgb, fog) + s.rgb * s.a * sun_drop), 1.0);
            } else {
                out.color = vec4<f32>(to_linear(lit), 1.0);
            }
        }
    } else if retail {
        let pos = (world - sky[2].xyz) / world_unit;
        let cam = (view.world_position.xyz - sky[2].xyz) / world_unit;
        let cam_dist = length(pos - cam);
        let e = (pos - cam) / max(cam_dist, 1e-4);
        if under {
            // Depth cue over waterDepthLevel 60 toward the luminance x
            // waterColor, then toward waterColor by depth / 200 + 0.7 fresnel^2.
            let water_color = to_linear(sky[1].rgb);
            let depth = -sea_height;
            let cue = 1.0 - clamp(depth / 60.0, 0.0, 1.0);
            let bottom = dot(out.color.rgb, vec3<f32>(0.333)) * water_color;
            var c = mix(bottom, out.color.rgb, cue);
            let fresnel = 1.0 - dot(-e, up);
            c = mix(c, water_color, clamp(depth * 0.5 / 100.0 + fresnel * fresnel * 0.7, 0.0, 1.0));
            if sky[15].z > 0.5 && sky[15].z < 1.5 { c = vec3<f32>(clamp(depth / 60.0, 0.0, 1.0)); }
            out.color = vec4<f32>(c, out.color.a);
        } else {
            // TerrainAbove fog: lerp(light, scatter, fog) + scatter x a x sunDrop.
            let fog = fog_strength(pos, up, e, cam_dist);
            let s = scatter_at(-e);
            let s_lin = to_linear(s.rgb);
            let sun_drop = clamp(dot(up, sky[0].xyz) + 0.95, 0.0, 1.0);
            out.color = vec4<f32>(mix(out.color.rgb, s_lin, fog) + s_lin * s.a * sun_drop, out.color.a);
        }
    }
    out.color = main_pass_post_lighting_processing(input, out.color);
    return out;
}
