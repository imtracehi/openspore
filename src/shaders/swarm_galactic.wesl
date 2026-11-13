// Pixel stage of the galactic sprite programs (galacticSpinTilt 0x70000120, galacticStar 0x70000121,
// galacticZpoleTilt 0x70000122; the same four instructions in all three, `space-shaders/dumps/galactic_sm3.txt`):
//   texld_pp r2, v0.xy, s0          the strip at the first tile coordinate
//   texld_pp r1, v0.zw, s0          the strip at the second
//   lrp_pp r0, v1.x, r1, r2         blended by the frame fraction
//   mul_pp oC0, r0, v2              times the vertex colour (rgb and alpha)
// blended SRCALPHA / ONE (the compiled states of the three materials): result = dst + rgb * alpha, in gamma
// space. The vertex stage ran on the CPU (swarm_fx/galactic.rs): uv = the first tile coordinate, uv_b the
// second, the tile blend rides in the tangent's w, colour = the program's oT3 (alpha is signed: a negative
// alpha subtracts).
import bevy_pbr::render::forward_io::VertexOutput;

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var galactic_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var galactic_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let a = textureSample(galactic_texture, galactic_sampler, in.uv);
    let b = textureSample(galactic_texture, galactic_sampler, in.uv_b);
    let c = mix(a, b, in.world_tangent.w) * in.color;
    // Gamma-space product, shown in linear (the sprites are lit over black space, where this is exact).
    let g = c.rgb * clamp(c.a, -1.0, 1.0);
    let linear_rgb = sign(g) * pow(abs(g), vec3<f32>(2.2));
    // Premultiplied add (the engine's additive blend): zero alpha adds rgb.
    return vec4<f32>(linear_rgb, 0.0);
}
