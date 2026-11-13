// The ice / lava "seas" of the Space planet view (space_planets/fluid.rs):
// TerrainBelowIceVS/PS and TerrainBelowLavaVS/PS of the HLSL fragment pack
// 0x40212002 (TerrainBelowIceShader 0x70000018 / TerrainBelowLavaShader
// 0x70000019), drawn on the water sphere like the retail programs (the
// vertex at the water surface point). The per-face height texture of the
// retail renderer (Sampler1) is a height cube here (ADAPTED: only its .x,
// the height, is known; the ice program's other channels read as 0).
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: array<vec4<f32>, 24>;
// x: 1 ice, 2 lava.
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> mode: vec4<f32>;
// (h + 1) / 2 of the terrain under each world direction.
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var height_cube: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var height_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var scatter_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var scatter_sampler: sampler;
// ice_ramp 0x3C9C0255 / lava_ramp 0xA3973EA2.
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var ramp_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var ramp_sampler: sampler;
// ice_lowfrequency 0x3CEF83D5 (ice Sampler5) / lava_detail 0x08C05AD9 (lava Sampler5).
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var noise_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var noise_sampler: sampler;
// The terrain detail raster (ice Sampler3) / the theme's below raster (lava Sampler3).
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var detail_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var detail_sampler: sampler;
// PCAwater2_0 / _1 (the lava's PCA normals, Sampler4 / Sampler6).
@group(constants::MATERIAL_BIND_GROUP) @binding(112) var pca0_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(113) var pca0_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(114) var pca1_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(115) var pca1_sampler: sampler;

const FACE_EDGE_UNITS: f32 = 785.3982;
// kAboveDetailLowTiling (illumParms.w) and the ice program's waterScale.
const DETAIL_LOW_TILING: f32 = 6.0;
const ICE_WATER_SCALE: f32 = 30.0;

fn world_unit() -> f32 {
    return select(1.0, sky[9].w, sky[9].w > 0.0);
}
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}
fn scatter_at(d: vec3<f32>) -> vec4<f32> {
    let t = vec3<f32>(dot(sky[6].xyz, d), dot(sky[7].xyz, d), dot(sky[8].xyz, d));
    return textureSampleLevel(scatter_map, scatter_sampler, t, 0.0);
}
fn terminator(n: vec3<f32>) -> f32 {
    let dp = dot(n, sky[0].xyz);
    let cutoff = sky[0].w;
    return select(max(0.0, dp + cutoff) / (2.0 * cutoff), 1.0, dp > cutoff);
}
fn fog_strength(p: vec3<f32>, n: vec3<f32>, e: vec3<f32>, cam_dist: f32) -> f32 {
    let atmo = sky[3].x + sky[3].z;
    let b = dot(-e, p);
    let c = dot(p, p) - atmo * atmo;
    let t = -b + sqrt(max(b * b - c, 0.0));
    let dist = max(0.0, min(t, cam_dist) - sky[4].z);
    return (1.0 - exp(sky[4].x * dist)) * terminator(n);
}
// PCADecompressTexture (/ 255): the x and z components (pcaTexture registers
// c0 / c2 of basis 0, c4 / c6 of basis 1, the mean).
fn pca_xz(uv: vec2<f32>) -> vec2<f32> {
    let t0 = textureSample(pca0_map, pca0_sampler, uv) * 2.0 - 1.0;
    let t1 = textureSample(pca1_map, pca1_sampler, uv) * 2.0 - 1.0;
    return vec2<f32>(dot(t0, sky[10]) + dot(t1, sky[12]) + sky[14].x,
                     dot(t0, sky[11]) + dot(t1, sky[13]) + sky[14].z) / 255.0;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let unit = world_unit();
    let centre = sky[2].xyz;
    let p = (in.world_position.xyz - centre) / unit;
    let n = normalize(p);
    let cam = (view.world_position.xyz - centre) / unit;
    let to_vert = p - cam;
    let cam_dist = length(to_vert);
    let e = to_vert / max(cam_dist, 1e-4);
    let sun = sky[0].xyz;
    let uv = in.uv / FACE_EDGE_UNITS;
    let fog = fog_strength(p, n, e, cam_dist);
    let sun_drop = clamp(dot(n, sun) + 0.95, 0.0, 1.0);
    let h01 = textureSample(height_cube, height_sampler, n).r;
    let alt_range = sky[3].y;
    let water_height = sky[2].w - sky[3].x;
    let s = scatter_at(-e);
    var color: vec3<f32>;
    if mode.x < 1.5 {
        // TerrainBelowIcePS: three noise octaves -> ice ramp, ice / snow mask,
        // cube specular, then the terrain fog.
        let far = vec4<f32>(h01 - 0.5, 0.0, 0.0, 0.0);
        var mid = textureSample(noise_map, noise_sampler, uv * ICE_WATER_SCALE) - vec4<f32>(0.5);
        mid.z = -mid.z;
        // The land's detail raster (linear packed data).
        let near = textureSample(detail_map, detail_sampler, uv * DETAIL_LOW_TILING) - vec4<f32>(0.5);
        var lookups = far * vec4<f32>(-0.1, 1.0, 1.5, 1.0) + mid * vec4<f32>(0.0, 1.0, 0.1, 0.1)
            + near * vec4<f32>(2.0, -0.1, 0.1, 0.1) + vec4<f32>(0.5);
        lookups = clamp(lookups, vec4<f32>(0.0), vec4<f32>(1.0));
        var ice = textureSample(ramp_map, ramp_sampler, vec2<f32>(0.0, lookups.x)).rgb;
        // ADAPTED: the per-face normal map is the sphere normal here.
        let smoothed = n;
        let ndl = clamp(dot(smoothed, normalize(sun)), 0.0, 1.0);
        ice = ice * (1.0 - lookups.z * 0.9);
        let eye_dir = -e;
        let r = eye_dir - 2.0 * dot(eye_dir, smoothed) * smoothed;
        let spec = scatter_at(r);
        ice = ice + spec.rgb * spec.a * ndl * lookups.z * 3.0;
        color = mix(ice, s.rgb, fog) + s.a * s.rgb * sun_drop;
    } else {
        // TerrainBelowLavaPS: incandescence ramp by low / mid / high frequency
        // and the shallows, over the lava detail.
        let height = alt_range * (h01 * 2.0 - 1.0);
        let lava_depth = clamp((water_height - height) / alt_range, 0.0, 1.0) - 0.1;
        let coast = clamp(1.0 - lava_depth * 13.0, 0.0, 1.0);
        let pca_a = pca_xz(uv * 5.0);
        let pca_b = pca_xz(uv * 7.0);
        // The below raster is bound sRGB (shared with the sea floor): back to
        // the display value the program reads.
        let below = pow(max(textureSample(detail_map, detail_sampler, uv).rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
        let d = textureSample(noise_map, noise_sampler, uv * 4.0);
        let cracks = d.a * 2.0;
        let detail = d.rgb * 0.6;
        var lookup = 1.0;
        lookup = lookup - (below.r - 0.5) * 2.5;
        lookup = lookup - pca_a.x * 2.5;
        lookup = lookup + pca_b.y * 1.75;
        lookup = lookup - coast;
        lookup = lookup + cracks * coast;
        lookup = lookup + sin(sky[9].x * 0.25) * 0.05;
        let inc = textureSample(ramp_map, ramp_sampler, vec2<f32>(0.0, clamp(lookup, 0.0, 1.0))).rgb;
        let diffuse = mix(detail, inc, inc.r);
        color = mix(diffuse, s.rgb, fog) + s.rgb * s.a * sun_drop;
    }
    var out: FragmentOutput;
    out.color = vec4<f32>(to_linear(color), 1.0);
    return out;
}
