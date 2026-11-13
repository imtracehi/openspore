// Weather cloud puffs of the Space planet view (space_planets/clouds.rs):
// cloudVanillaHighVS / cloudVanillaHighPS of the HLSL fragment pack 0x40212002
// (material 0x5995959E, shader 0x213D3024 of the planet effect directory:
// customParams c0 = (animBlendRate 0.3, stripLength 8, topFadeAmount 1)).
// The particle simulation is ADAPTED (clouds.rs); the per-vertex maths below
// are the retail program's, in planet units.
// Vertex data: position = puff site (planet-local), normal = drift (planet
// units / s), uv = quad corner, uv_b = (particle id, life phase), colour =
// (life seconds, size factor, terrain height above R, 1).
import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
};

// The planet view's TerrainState (planet_sky.rs slots; 0 sun, 2 centre +
// water radius, 3 sphereParms, 4 fogParms, 9 x time / w world unit, 16 night tint).
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sky: array<vec4<f32>, 24>;
// 0: customParams c0 (animBlendRate, stripLength, topFadeAmount, -);
// 1: cloud colour rgb, temperature score; 2: alpha curve keys 0..3 count;
// 3: size curve (4 keys).
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> params: array<vec4<f32>, 4>;
// The cloud component's alpha curve (16 keys).
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> alpha_keys: array<vec4<f32>, 4>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var side_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var side_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var top_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var top_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var scatter_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var scatter_sampler: sampler;

fn ramp(lo: f32, hi: f32, v: f32) -> f32 {
    return clamp((v - lo) / (hi - lo), 0.0, 1.0);
}

fn alpha_key(i: i32) -> f32 {
    let k = clamp(i, 0, 15);
    return alpha_keys[k / 4][k % 4];
}

// Swarm curves: keys spread evenly over the life (ADAPTED sampling).
fn alpha_curve(t: f32) -> f32 {
    let n = i32(params[2].x);
    if n <= 1 {
        return alpha_key(0);
    }
    let x = clamp(t, 0.0, 1.0) * f32(n - 1);
    let i = i32(floor(x));
    return mix(alpha_key(i), alpha_key(min(i + 1, n - 1)), x - f32(i));
}

fn size_curve(t: f32) -> f32 {
    let x = clamp(t, 0.0, 1.0) * 3.0;
    let i = i32(floor(x));
    let a = params[3][min(i, 3)];
    let b = params[3][min(i + 1, 3)];
    return mix(a, b, x - f32(i));
}

// sunset() of the cloud programs.
fn sunset(cos_sun: f32, night_tint: vec3<f32>) -> vec3<f32> {
    let white = vec3<f32>(1.0) * 1.3;
    let yellow = vec3<f32>(1.0, 1.0, 0.5) * 1.2;
    let orange = vec3<f32>(1.0, 0.7, 0.3);
    let red = vec3<f32>(1.0, 0.5, 0.5);
    let b0 = mix(yellow, white, ramp(0.6, 1.0, cos_sun));
    let b1 = mix(orange, b0, ramp(0.2, 0.6, cos_sun));
    let b2 = mix(red, b1, ramp(-0.2, 0.2, cos_sun));
    return mix(night_tint, b2, ramp(-1.0, -0.2, cos_sun));
}

fn terminator(n: vec3<f32>) -> f32 {
    let dp = dot(n, sky[0].xyz);
    let cutoff = sky[0].w;
    return select(max(0.0, dp + cutoff) / (2.0 * cutoff), 1.0, dp > cutoff);
}

// GetTerrainFogStrength (planet units).
fn fog_strength(p: vec3<f32>, n: vec3<f32>, e: vec3<f32>, cam_dist: f32) -> f32 {
    let atmo = sky[3].x + sky[3].z;
    let b = dot(-e, p);
    let c = dot(p, p) - atmo * atmo;
    let t = -b + sqrt(max(b * b - c, 0.0));
    let dist = max(0.0, min(t, cam_dist) - sky[4].z);
    return (1.0 - exp(sky[4].x * dist)) * terminator(n);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let unit = select(1.0, sky[9].w, sky[9].w > 0.0);
    let time = sky[9].x;
    let anim_blend_rate = params[0].x;
    let strip_length = params[0].y;
    let top_fade_amount = params[0].z;
    let uv = v.uv;
    let id = v.uv_b.x;
    let life = max(v.color.x, 0.1);
    // ADAPTED particle life: each puff cycles through its life, drifting.
    let age = fract(time / life + v.uv_b.y);
    var site = v.position + v.normal * age * life;
    site = normalize(site) * length(v.position);
    var color = vec4<f32>(params[1].rgb, alpha_curve(age));
    // Planet units, the planet centre at the origin (the retail terrain frame).
    let centre = sky[2].xyz;
    let local_cam = (view.world_position.xyz - centre) / unit;
    let rot_site = (world_from_local * vec4<f32>(site, 1.0)).xyz;
    let p = (rot_site - centre) / unit;
    let npos = normalize(p);
    let to_camera = local_cam - p;
    let camera_dist = length(to_camera);
    let to_cam_n = to_camera / max(camera_dist, 1e-4);
    let top_blend = clamp(dot(npos, to_cam_n), 0.0, 1.0);
    color.a = color.a * ramp(1.0, 0.25, top_blend * top_fade_amount);
    color.a = color.a * ramp(150.0, 1000.0, camera_dist * (11.0 - 10.0 * top_fade_amount));
    let pdotv = dot(npos, normalize(local_cam));
    let near_fade = select(0.0, 1.0, 350.0 < camera_dist);
    let back_fade = select(0.0, 1.0, -0.2 < pdotv);
    // The authored size is the particle width (`In.normal.w * 2`).
    var scale = vec2<f32>(size_curve(age) * v.color.y) * back_fade * near_fade;
    scale = scale * min(color.a * 60.0, 1.0);
    // OPENSPORE_CLOUD_DEBUG (development): every puff opaque and full size.
    if params[0].w > 0.5 {
        color.a = 1.0;
        scale = vec2<f32>(size_curve(age) * v.color.y);
    }
    let glow0 = clamp(sky[2].w - sky[3].x - v.color.z, 0.0, 1.0);
    let cloud_height = id * 40.0 - 10.0;
    // The program adds the height to every component (kept as authored).
    var pos = p + vec3<f32>(cloud_height);
    if cloud_height <= 20.0 {
        scale = scale * 0.7;
        pos = pos - npos * 20.0;
    } else {
        scale.x = scale.x * mix(0.9, 0.6, top_blend);
        scale.y = scale.y * mix(0.3, 0.6, top_blend);
        pos = pos + npos * 10.0;
    }
    // cameraFaceUp: the quad turns about the view axis to keep the planet's
    // up on screen.
    let right = view.world_from_view[0].xyz;
    let cam_up = view.world_from_view[1].xyz;
    let dot_up = dot(npos, cam_up);
    let dot_tangent = dot(npos, right);
    let proj_up = normalize(dot_up * cam_up + dot_tangent * right + vec3<f32>(1e-6, 0.0, 0.0));
    let proj_tangent = normalize(-dot_tangent * cam_up + dot_up * right + vec3<f32>(0.0, 1e-6, 0.0));
    let corner = pos + proj_tangent * scale.x * (uv.x - 0.5) + proj_up * scale.y * (0.5 - uv.y);
    // tileUV: two strip cells blended by the animation.
    let t = id * 255.0 + time * anim_blend_rate;
    let blend = fract(t);
    var tile = vec2<f32>(t - blend, t + 1.0 - blend) / strip_length;
    tile = fract(tile) + vec2<f32>(uv.y / strip_length);
    let sun = sky[0].xyz;
    let cos_sun = clamp(dot(npos, sun), 0.0, 1.0);
    let bottom = sunset(cos_sun, sky[16].rgb);
    // volumeLightQuad.
    var rgb = mix(color.rgb, color.rgb * bottom, uv.y * (1.0 - top_blend));
    let qn = normalize(corner - p + vec3<f32>(1e-6));
    let i0 = clamp(dot(sun, qn) * 1.42, 0.0, 1.0);
    let intensity = mix(i0, cos_sun, top_blend * top_blend);
    rgb = mix(rgb, rgb * intensity, 0.25);
    let camera_height_fade = clamp((length(local_cam) - sky[3].x - 100.0) / 100.0, 0.0, 1.0);
    color.a = color.a * mix(2.5, 1.0, camera_height_fade);
    let glow = glow0 * clamp(params[1].w - 0.85, 0.0, 1.0) * 7.0;
    color.a = color.a * select(1.1, 0.3, cloud_height > 20.0);
    let e = normalize(corner - local_cam);
    let fog = fog_strength(corner, npos, e, length(corner - local_cam)) * 0.6;
    let world = centre + corner * unit;
    out.world_position = vec4<f32>(world, 1.0);
    out.position = position_world_to_clip(world);
    out.color = vec4<f32>(rgb, color.a);
    out.uv = vec2<f32>(uv.x, tile.x);
    out.uv_b = vec2<f32>(uv.x, tile.y);
    // (tileAnimBlend, topBlend, glowHeight, fog) and the view vector.
    out.world_normal = vec3<f32>(blend, top_blend, uv.y * glow);
    out.world_tangent = vec4<f32>(-e, fog);
    return out;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn scatter_at(d: vec3<f32>) -> vec4<f32> {
    let t = vec3<f32>(dot(sky[6].xyz, d), dot(sky[7].xyz, d), dot(sky[8].xyz, d));
    return textureSampleLevel(scatter_map, scatter_sampler, t, 0.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let blend = in.world_normal.x;
    let top_blend = in.world_normal.y;
    let y = in.world_normal.z;
    let fog = in.world_tangent.w;
    let top = mix(textureSample(top_map, top_sampler, in.uv), textureSample(top_map, top_sampler, in.uv_b), blend);
    let side = mix(textureSample(side_map, side_sampler, in.uv), textureSample(side_map, side_sampler, in.uv_b), blend);
    var diffuse = mix(side, top, top_blend) * in.color;
    // Lava worlds: the cloud bottoms glow (lavaGlowColor, intensity 24).
    let y4 = y * y * y * y;
    diffuse = vec4<f32>(diffuse.rgb + y4 * diffuse.a * 24.0 * (1.0 - top_blend) * vec3<f32>(1.0, 0.3, 0.0), diffuse.a);
    // Scatter fog (foggingCPU.w taken as 1, INFERRED).
    let s = scatter_at(in.world_tangent.xyz);
    let rgb = mix(diffuse.rgb, s.rgb, fog) + s.rgb * s.a;
    return vec4<f32>(to_linear(rgb), clamp(diffuse.a, 0.0, 1.0));
}
