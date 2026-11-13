import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};
import bevy_pbr::render::mesh_view_bindings::view;

// TerrainAtmosphere (TerrainAtmosphereShader 0x70000015, HLSL fragments
// TerrainAtmosphereVS / "pixel shader Main: Atmosphere" of the fragment pack
// 0x40212002). The vertex maths run per pixel here; the shell is the back
// faces of a sphere of radius R + thickness around the planet centre.
// sky slots: see planet_sky.rs (SKY_SLOTS).
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: array<vec4<f32>, 24>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var scatter_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var scatter_sampler: sampler;
// atmosphere_packed_curves: 1 x 64, R = illum curve, G = view-angle curve.
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var curves_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var curves_sampler: sampler;

fn scatter_dir(d: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(sky[6].xyz, d), dot(sky[7].xyz, d), dot(sky[8].xyz, d));
}

fn curves(x: f32) -> vec4<f32> {
    return textureSampleLevel(curves_map, curves_sampler, vec2<f32>(0.5, clamp(x, 0.0, 1.0)), 0.0);
}

// Retail colours are display values; Bevy blends in linear space.
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;
    let centre = sky[2].xyz;
    let sun = sky[0].xyz;
    let cutoff = sky[0].w;
    let up = normalize(in.world_position.xyz - centre);
    let shell = up * (sky[3].x + sky[3].z);
    // Planet units (sky[9].w = world units per planet unit on the Space
    // planet view, 0 on the stages).
    let cam = (view.world_position.xyz - centre) / select(1.0, sky[9].w, sky[9].w > 0.0);
    let to_vert = normalize(shell - cam);
    // illum = (clamp(N.sun + cutoff - cos(5), lo, hi) - lo) / (hi - lo); the
    // HLSL takes cos(5) in radians (0.283662).
    let lo = sky[5].x;
    let hi = sky[5].y;
    var illum = dot(sun, up) + cutoff - cos(5.0);
    illum = (clamp(illum, lo, hi) - lo) / (hi - lo);
    let dp_view = clamp(dot(to_vert, up), 0.0, 1.0);
    let ascore = sky[4].w;
    let scatter = textureSampleLevel(scatter_map, scatter_sampler, scatter_dir(-to_vert), 0.0);
    let illum_c = curves(illum).r;
    var normal_c = curves(dp_view).g;
    let opacity = illum_c * ascore * 2.0;
    var ext = normal_c * ascore;
    ext = ext * ext;
    ext = ext * ext;
    normal_c = min(1.0, normal_c + ext);
    let color = scatter.rgb * illum_c + scatter.rgb * scatter.a;
    out.color = vec4<f32>(to_linear(color), clamp(normal_c * opacity, 0.0, 1.0));
    return out;
}
