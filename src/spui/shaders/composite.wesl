// Shows the gamma-space SPUI surface over the scene.
//
// The surface holds colours exactly as the retail renderer would have blended
// them (raw, gamma-encoded values), premultiplied by the coverage that the
// layers accumulated over a transparent clear. The output surface is linear, so
// the straight colour is decoded and premultiplied again for the compositing
// blend state of the material.
import bevy_ui_render::ui_vertex_output::UiVertexOutput;

@group(1) @binding(0) var hud_texture: texture_2d<f32>;
@group(1) @binding(1) var hud_sampler: sampler;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let t = textureSample(hud_texture, hud_sampler, in.uv);
    if (t.a <= 0.0) {
        return vec4<f32>(0.0);
    }
    let straight = clamp(t.rgb / t.a, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(srgb_to_linear(straight) * t.a, t.a);
}
