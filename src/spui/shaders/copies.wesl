// The copies of one image in a stroke / halo / outline kernel (`spui::outline`), drawn in one pass.
//
// The retail renderer draws the layer's image once per kernel copy: shifted by whole pixels and
// tinted `(alpha << 24) + rgb` (texture colour times the tint, texture alpha times the tint alpha),
// each copy blended over the previous one. The node of this material is the layer's rectangle
// grown by `margin` pixels on every side; for each copy the shader samples the image at the
// position shifted back by the copy's offset and accumulates the same "over" blend in premultiplied
// form, then returns the straight colour and the accumulated coverage for the normal alpha
// blend of the UI pipeline (which gives the same pixel as the sequence of single draws).
import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct Params {
    // Source region of the image in the texture (uv): min x, min y, max x, max y.
    src: vec4<f32>,
    // x: margin around the layer (pixels), y: number of copies.
    info: vec4<f32>,
    // Tint colour of the copies (rgb).
    color: vec4<f32>,
    // (dx, dy, alpha, unused) of each copy, in drawing order.
    taps: array<vec4<f32>, 120>,
};

@group(1) @binding(0) var layer_texture: texture_2d<f32>;
@group(1) @binding(1) var layer_sampler: sampler;
@group(1) @binding(2) var<uniform> params: Params;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let margin = params.info.x;
    let count = i32(params.info.y);
    // The layer's rectangle inside the node, and this pixel's centre in node pixels.
    let layer_size = max(in.size - vec2<f32>(2.0 * margin), vec2<f32>(1.0));
    let pos = in.uv * in.size;
    var acc_rgb = vec3<f32>(0.0);
    var acc_a = 0.0;
    for (var i = 0; i < count; i = i + 1) {
        let tap = params.taps[i];
        let q = (pos - vec2<f32>(margin) - tap.xy) / layer_size;
        if (q.x < 0.0 || q.y < 0.0 || q.x >= 1.0 || q.y >= 1.0) {
            continue;
        }
        let texel = textureSampleLevel(layer_texture, layer_sampler, mix(params.src.xy, params.src.zw, q), 0.0);
        let a = texel.a * tap.z;
        acc_rgb = texel.rgb * params.color.rgb * a + acc_rgb * (1.0 - a);
        acc_a = a + acc_a * (1.0 - a);
    }
    if (acc_a <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(acc_rgb / acc_a, acc_a);
}
