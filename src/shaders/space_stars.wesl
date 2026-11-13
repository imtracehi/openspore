// Galaxy map star glows (space_galaxy/stars.rs): the retail `effectStarMaterial`
// pipeline of the Space effect directory 0xC1195706 (`sg_galaxy`). The effect
// fragments are in the HLSL pack 0x40212002 (space-visuals\re\frag\fragments.txt):
//   effectTransSetupVF               scale = size (the builder 0x700000F3 `effectStarShader`'s choice, not effectClipTransSetupVF's size * 0.5)
//   effectToNormalClipVF             clipPos = normalised clip position (w = 1)
//   effectScreenSpaceAxisClampedVF   axis = clamp(saturate(1 - clipPos.z) * scale,
//                                          minScaleRatio c0.z, maxScaleRatio c0.w); axis.x *= H / W
//   effectOffsetClipPosVF            clipPos.xy += axis * (uv - 0.5)  (v down)
//   effectDistanceFadeAdditiveVF     color *= saturate((fadeEnd - dist) / (fadeEnd - fadeStart))
//   effectAnimateTilesVF             two strip frames from id * 255 + time * animBlendRate (c0.x)
//   effectPS                         tex2D(map, uv) * color, blended SRCALPHA / ONE.
// The composition is the builder 0x700000F3 `effectStarShader` of the pack 0x40212004!3 (vs_3_0 / ps_3_0): VS
// effectSetupVF, effectTransSetupVF, effectDistanceFadeAdditiveVF, effectToNormalClipVF, effectAnimateTilesVF,
// effectScreenSpaceAxisClampedVF, effectOffsetClipPosVF, effectOutputAnimVF; PS effectInputAnimPF, effectAnimPS.
// Shader model 3: the colour is not clamped between the stages (HDR glow colours reach the pixel stage).
// Vertex data: position = the star (galaxy map space), uv = quad corner,
// uv_b = (population index + size factor / 4, strip frame id / 255), colour = glow colour (HDR) and
// alpha. The world is in galaxy units (retail units: the camera distances of the
// material registers apply directly).
import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_view_bindings::{view, globals},
};

// Rows 2k = (animBlendRate, stripLength, minScaleRatio, maxScaleRatio) and
// 2k + 1 = (fadeStart, fadeEnd, half the mean authored size, 0) of population k.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> table: array<vec4<f32>, 32>;
// x: dim (map filter); y, z, w reserved.
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> state: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var strip_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var strip_sampler: sampler;

// spaceNearClip / spaceFarClip of the galaxy camera (data values).
const NEAR: f32 = 0.05;
const FAR: f32 = 10000.0;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let pop = i32(floor(v.uv_b.x));
    // The size of this particle over the population's mean.
    let size_factor = fract(v.uv_b.x) * 4.0;
    let anim = table[pop * 2];
    let fade_row = table[pop * 2 + 1];
    let id = floor(v.uv_b.y * 255.0 + 0.5);
    let uv = v.uv;
    let centre = (world_from_local * vec4<f32>(v.position, 1.0)).xyz;
    // effectSetupVF: distance from the camera to the particle.
    let camera_dist = length(view.world_position.xyz - centre);
    // effectDistanceFadeAdditiveVF.
    let fade = clamp((fade_row.y - camera_dist) / (fade_row.y - fade_row.x), 0.0, 1.0);
    let clip = view.clip_from_world * vec4<f32>(centre, 1.0);
    let depth = -(view.view_from_world * vec4<f32>(centre, 1.0)).z;
    if fade <= 0.0 || clip.w <= 0.0 || depth <= 0.0 {
        // Nothing to draw: collapse the quad outside the clip volume.
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.color = vec4<f32>(0.0);
        out.uv = vec2<f32>(0.0);
        out.uv_b = vec2<f32>(0.0);
        out.world_position = vec4<f32>(0.0);
        return out;
    }
    let ndc = clip.xy / clip.w;
    // saturate(1 - clipPos.z) of a D3D perspective projection with the retail
    // near / far: n (f - d) / (d (f - n)).
    let one_minus_z = clamp(NEAR * (FAR - depth) / (depth * (FAR - NEAR)), 0.0, 1.0);
    let scale = fade_row.z * size_factor;
    let axis = clamp(one_minus_z * scale, anim.z, anim.w);
    let ratio = view.viewport.w / view.viewport.z;
    let offset = vec2<f32>(axis * ratio * (uv.x - 0.5), axis * (0.5 - uv.y));
    out.position = vec4<f32>((ndc + offset) * clip.w, clip.z, clip.w);
    // vs_3_0 / ps_3_0: no colour clamp between the stages (the fade scales all four channels).
    out.color = v.color * fade;
    // effectAnimateTilesVF: two strip cells blended by the animation.
    let t = id + globals.time * anim.x;
    let blend = fract(t);
    var tile = vec2<f32>(t - blend, t + 1.0 - blend) / anim.y;
    tile = fract(tile) + vec2<f32>(uv.y / anim.y);
    out.uv = vec2<f32>(uv.x, tile.x);
    out.uv_b = vec2<f32>(uv.x, tile.y);
    out.world_position = vec4<f32>(blend, 0.0, 0.0, 1.0);
    return out;
}

// Retail blends in gamma space (the back buffer is not sRGB); Bevy blends in linear space. A value g added in gamma
// space is decode(g) added in linear space over black (exponent 2.2) but about g over a mid-grey background (the
// map's colour layers): the glow halos would vanish over them. ADAPTED: a compromise exponent.
const BLEND_GAMMA: f32 = 1.5;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(BLEND_GAMMA));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let a = textureSample(strip_map, strip_sampler, in.uv);
    let b = textureSample(strip_map, strip_sampler, in.uv_b);
    // effectAnimPS: the two cells blended, times the vertex colour.
    let c = mix(a, b, in.world_position.x) * in.color;
    let rgb = clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let alpha = clamp(c.a, 0.0, 1.0);
    // SRCALPHA / ONE in gamma space: over black rgb * alpha, shown in linear.
    return vec4<f32>(to_linear(rgb * alpha * state.x), 0.0);
}
