// grassVanilla (builder 0x700000E4): grassVanillaVS + TerrainLightingLib and
// grassVanillaPS from the HLSL fragment pack 0x40212002 (planet_grass.rs).
// Vertex data: position = blade root, normal = up, tangent = heading,
// uv = quad corner, uv_b = (particle id, size), colour = particle colour
// (distribute colour x Color map, display values).
import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
};

// The stage planet's TerrainState (planet_sky.rs slots; 0 sun, 2 centre,
// 9 x time, 16..23 terrain lighting).
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sky: array<vec4<f32>, 24>;
// customParams c0 = (startFade, endFade, tilt deg, startFadeIn),
// c1 = (startScale, endScale, scaleFactor, endFadeIn).
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> custom: array<vec4<f32>, 2>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var strip: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var strip_sampler: sampler;

const PI: f32 = 3.1415926;

// EvaluateVertexLighting + TerrainVertexLighting (stock terrain lighting at
// the blade root: the sphere direction is the normal).
fn grass_lighting(n: vec3<f32>) -> vec4<f32> {
    let ndl0 = dot(n, sky[0].xyz);
    let dn = clamp(vec2<f32>(ndl0, -ndl0) * sky[23].xy + sky[23].zw, vec2<f32>(0.0), vec2<f32>(1.0));
    let b = vec3<f32>(dn.x, 1.0 - max(dn.x, dn.y), dn.y);
    let light = b.x * sky[17].rgb + b.y * sky[18].rgb + b.z * sky[19].rgb;
    let shadow = b.x * sky[20].rgb + b.y * sky[21].rgb + b.z * sky[22].rgb;
    let sun = mix(sky[0].xyz, n, b.y * 0.35);
    let ndl = dot(n, sun);
    let a = abs(ndl);
    let s = a * 0.9 + clamp(ndl, 0.0, 1.0) * 0.2 + (1.0 - a) * (1.0 - a) * 0.5;
    return vec4<f32>(mix(shadow, light, s) * s, b.z);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let root = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0)).xyz;
    let up = normalize((world_from_local * vec4<f32>(v.normal, 0.0)).xyz);
    let tangent = normalize((world_from_local * vec4<f32>(v.tangent.xyz, 0.0)).xyz);
    let corner = v.uv;
    let id = v.uv_b.x;
    let size = v.uv_b.y;
    let start_fade = custom[0].x;
    let end_fade = custom[0].y;
    let tilt = custom[0].z * PI / 180.0;
    let start_scale = custom[1].x;
    let end_scale = custom[1].y;
    let scale_factor = custom[1].z;
    let camera_distance = length(root - view.world_position.xyz);
    var scaled = size * 1.5;
    // ADAPTED: no trampling (theta = the random variance).
    let theta = id * tilt;
    // grassColorFadeAndAO (incoming colour x 2).
    var color = v.color * 2.0;
    var fade = clamp((end_fade - camera_distance) / (end_fade - start_fade), 0.0, 1.0);
    fade = fade * (id * 0.5 + 0.5);
    color.a = color.a * fade;
    let uv_height2 = 1.25 - corner.y * corner.y;
    color = vec4<f32>(mix(color.rgb, color.rgb * uv_height2, 0.5 * fade), color.a);
    // grassFadeScale.
    scaled = scaled * mix(1.0, scale_factor, clamp((camera_distance - start_scale) / (end_scale - start_scale), 0.0, 1.0));
    scaled = scaled * (fade * 0.5 + 0.5);
    // grassAnimate (INFERRED phase: the blade id).
    var uv = corner;
    uv.x = uv.x + sin(sky[9].x + 10.0 * id) * (1.0 - uv.y) * 0.1;
    let lit = grass_lighting(normalize(root - sky[2].xyz));
    let night_lum = sky[16].w * lit.w;
    // grassPositionRotated.
    let at = cross(up, tangent);
    let n = up * cos(theta) + at * sin(theta);
    let pos = root + tangent * scaled * (uv.x - 0.5) + n * scaled * (1.0 - uv.y);
    out.world_position = vec4<f32>(pos, 1.0);
    out.position = position_world_to_clip(pos);
    // grassCell: the strip's 8 cells by id.
    out.uv = vec2<f32>(corner.x, (corner.y + floor(id * 7.999)) / 8.0);
    out.color = vec4<f32>(color.rgb * fade * lit.rgb, color.a);
    // c2 rides in the normal slot, the night blend in uv_b.
    out.world_normal = color.rgb * (1.0 - fade) * lit.rgb;
    out.uv_b = vec2<f32>(night_lum, 0.0);
    out.world_tangent = vec4<f32>(tangent, 1.0);
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var diffuse = textureSample(strip, strip_sampler, in.uv);
    diffuse.a = diffuse.a * select(0.0, 1.0, diffuse.a > 0.3);
    let night = in.uv_b.x;
    let n1 = sky[16].rgb * night * 0.5;
    let light = diffuse * in.color + vec4<f32>(in.world_normal, 0.0);
    let rgb = vec3<f32>(dot(light.rgb, n1)) + light.rgb * (1.0 - night);
    if light.a < 64.5 / 255.0 {
        discard;
    }
    // Display values -> Bevy's linear pipeline.
    return vec4<f32>(pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(2.2)), 1.0);
}
