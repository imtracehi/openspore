// The worlds of the solar-system view (space_planets/impostor.rs):
// planetImpostorVS / planetImpostorPS of the HLSL fragment pack 0x40212002
// (VS 117 / PS 124), lit by the star at the origin. The retail impostor
// samples four cube maps baked from the terrain; here the planet's colour
// atlas stands in for the diffuse cube (Sampler0), the mesh normal (the
// ground normal) for the normal cube (Sampler2), the atlas alpha (1 over
// water) for the specular channel of Sampler1 and the lava glow atlas for its
// emissive channel; the sphere normal arrives in the tangent attribute (the
// impostor's own sphere normal, In.texcoord<t3>). The in-scatter cube
// (Sampler3) is the world's BakeInScatterTextureShader cube at p 0.99 (the
// impostor bake 0xF9E490 step 3), looked up with mul(eyeVec, customParams):
// ADAPTED frame with +Z away from the star (the bake is symmetric around Z).
// Without a cube (flags.y 0) the earlier one-colour scatter stands in.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

// rgb: the scatter colour (display values), a: its additive part.
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> scatter: vec4<f32>;
// x: 1 when the glow atlas is bound.
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> flags: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var diffuse_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var diffuse_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var glow_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var glow_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var scatter_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var scatter_sampler: sampler;

// The scatter-cube direction of `d`: +Z away from the light `l`
// (planet_sky::scatter_frame).
fn scatter_dir(d: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let z = -l;
    let helper = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(z.y) > 0.95);
    let x = normalize(cross(helper, z));
    let y = cross(z, x);
    return vec3<f32>(dot(x, d), dot(y, d), dot(z, d));
}

fn to_display(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
}
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}
fn max3(c: vec3<f32>) -> f32 {
    return max(c.r, max(c.g, c.b));
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    // PS constants (as written in the program).
    let emissive_factor = 1.3;
    let spec_size = 50.0;
    let non_water_spec = 0.05;
    let cutoff = 0.2;
    let ambient_dot_offset = 0.65;
    let ambient_level = 0.2;
    let ambient = 0.9;

    let atlas = textureSample(diffuse_map, diffuse_sampler, in.uv);
    let diffuse = to_display(atlas.rgb);
    // specEmiss = saturate(Sampler1 + nonWaterSpec): r water specular, b
    // emissive. The 0.05 is added to every channel, so a world without glow
    // still lifts 0.05 x 1.3 of its diffuse colour over the night side.
    let spec = clamp(atlas.a + non_water_spec, 0.0, 1.0);
    var emissive_tex = 0.0;
    if flags.x > 0.5 {
        let glow = to_display(textureSample(glow_map, glow_sampler, in.uv).rgb);
        emissive_tex = clamp(max3(glow) / max(max3(diffuse), 1e-3), 0.0, 1.0);
    }
    let emissive = clamp(emissive_tex + non_water_spec, 0.0, 1.0);
    let n = normalize(in.world_normal);
    // Every orbit mesh carries the sphere normal as its tangent
    // (`space_planets::orbit_mesh`), so VERTEX_TANGENTS is always defined.
    let vertex_normal = normalize(in.world_tangent.xyz);
    let p = in.world_position.xyz;
    // VS: light at the world origin, eyeVec toward the camera, fog by the
    // impostor's normal.
    let l = normalize(-p);
    let eye = normalize(view.world_position.xyz - p);
    let cam_vector = -eye;
    let fog_strength = 0.5 * (1.0 - clamp(dot(eye, vertex_normal), 0.0, 1.0));

    let n_dot_l = dot(n, l);
    let r = normalize(l - 2.0 * dot(l, n) * n);
    let terminator = clamp((dot(vertex_normal, l) + cutoff) / (2.0 * cutoff), 0.0, 1.0);
    let dot_x = clamp(n_dot_l, 0.0, 1.0);
    let dot_y = clamp((-n_dot_l + ambient_dot_offset) / (1.0 + ambient_dot_offset), 0.0, 1.0);
    let night_light = dot_y * diffuse;
    let night_lum = dot(night_light, vec3<f32>(0.3, 0.59, 0.11));
    // nightLumBlend 0: the tinted night luminance.
    let ambient_tint = night_lum * vec3<f32>(0.7, 0.3, 2.3) * 0.5 * ambient;
    var color = ambient_tint + diffuse * (dot_x * terminator + ambient_level);
    color = mix(color, diffuse, emissive * emissive_factor);
    color = color + pow(clamp(dot(cam_vector, r), 0.0, 1.0), spec_size) * terminator * spec * vec3<f32>(1.0, 0.6, 0.06);
    var sc = scatter;
    if flags.y > 0.5 {
        // texCUBE(Sampler3, mul(eyeVec, trans)): eyeVec toward the camera.
        sc = textureSampleLevel(scatter_map, scatter_sampler, scatter_dir(eye, l), 0.0);
    }
    let s = sc * terminator;
    color = mix(color, s.rgb, fog_strength) + s.rgb * s.a;

    var out: FragmentOutput;
    out.color = vec4<f32>(to_linear(color), 1.0);
    return out;
}
