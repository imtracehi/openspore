// The atmosphere of the solar-system worlds (space_planets/impostor.rs):
// planetAtmoImpostorVS (VS 118) / planetAtmoImpostorPerPixelLightingPS
// (PS 125) of the HLSL fragment pack 0x40212002 on the "planetAtmoImpostor"
// model (a sphere 1.2 x the planet), lit by the star at the origin. The
// vertex terms run per pixel here; only the back faces are drawn (dpView is
// 0 on the near side anyway).
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

// Sampler0: the world's baked in-scatter cube (the planet impostor's +0x890).
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var scatter_map: texture_cube<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var scatter_sampler: sampler;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

// The scatter-cube direction of `d`: +Z away from the light `l` (ADAPTED
// frame for customParams, planet_sky::scatter_frame).
fn scatter_dir(d: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let z = -l;
    let helper = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(z.y) > 0.95);
    let x = normalize(cross(helper, z));
    let y = cross(z, x);
    return vec3<f32>(dot(x, d), dot(y, d), dot(z, d));
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let vert_pos = in.world_position.xyz;
    let world_normal = normalize(in.world_normal);
    // VS 118: eyeVec = normalize(camPos - vertPos), lightVec =
    // normalize(-vertPos), dpView = smoothstep(0.1, 1, N . -eye), shadow =
    // sat(L . N + 1), t1.x = sat(dpView x shadow) x 2.
    let eye = normalize(view.world_position.xyz - vert_pos);
    let light = normalize(-vert_pos);
    let dp_view = smoothstep(0.1, 1.0, dot(world_normal, -eye));
    let shadow = clamp(dot(light, world_normal) + 1.0, 0.0, 1.0);
    let alpha = clamp(dp_view * shadow, 0.0, 1.0) * 2.0;
    // PS 125.
    let scatter = textureSampleLevel(scatter_map, scatter_sampler, scatter_dir(eye, light), 0.0);
    let hack_off = select(0.0, 1.0, dot(scatter, scatter) > 0.0);
    let color = scatter.rgb * shadow + scatter.rgb * scatter.a;

    var out: FragmentOutput;
    out.color = vec4<f32>(to_linear(color), clamp(alpha * hack_off, 0.0, 1.0));
    return out;
}
