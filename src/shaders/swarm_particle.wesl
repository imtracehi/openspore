// Swarm particle sprite: texture x vertex colour, with the effdir
// `effectAnimMaterial` strip cross-fade. Vertex data (see swarm_fx/render.rs):
// uv = frame A, uv_b = (blend factor, step to the next strip frame along the
// strip axis).
import bevy_pbr::render::forward_io::VertexOutput;

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var particle_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var particle_sampler: sampler;
// x: 0 alpha blend, 1 additive, 2 multiply; y: alpha cut-off; z: 1 when the
// texture's frame strip runs along V; w: alpha exponent. Spore blends in
// gamma space; Bevy blends in linear space, where a light sprite at alpha a
// over black shows as a^(1/2.2) brighter. Raising alpha to 2.2 reproduces the
// gamma-space result exactly over black and closely over the dark water.
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> params: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let step = select(vec2<f32>(in.uv_b.y, 0.0), vec2<f32>(0.0, in.uv_b.y), params.z > 0.5);
    let a = textureSample(particle_texture, particle_sampler, in.uv);
    let b = textureSample(particle_texture, particle_sampler, in.uv + step);
    var c = mix(a, b, in.uv_b.x) * in.color;
    c.a = pow(clamp(c.a, 0.0, 1.0), max(params.w, 0.01));
    if params.y > 0.0 && c.a < params.y {
        discard;
    }
    if params.x > 0.5 && params.x < 1.5 {
        // Premultiplied blending with zero alpha adds rgb * a.
        return vec4<f32>(c.rgb * c.a, 0.0);
    }
    if params.x >= 1.5 {
        // Multiply: blend toward white where transparent.
        return vec4<f32>(mix(vec3<f32>(1.0), c.rgb, c.a), c.a);
    }
    return c;
}
