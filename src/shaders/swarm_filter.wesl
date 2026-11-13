// Swarm screen-filter pass: one full-screen draw per filter of a `filterChain`
// screen effect (see swarm_fx/filters.rs). Every filter kind of the effect
// format that the Cell/Space/cinematic chains use is one branch of `fragment`;
// the maths follow the pixel shaders the game ships for them (the
// `Filter*Shader` programs of the compiled shader pack, disassembled with
// tools/d3d9dis.py), including their gamma-space arithmetic.
//
// Inputs: tex_a / tex_b / tex_c (meaning per kind), params. All colour maths
// happen in gamma space: textures flagged sRGB are decoded by the hardware to
// linear and converted back here; the destination is converted the other way.
import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

struct Params {
    // x: kind, y: flags (see FLAG_*), z, w unused.
    head: vec4<u32>,
    p0: vec4<f32>,
    p1: vec4<f32>,
    p2: vec4<f32>,
    // xy: texel size (1/width, 1/height) of tex_a, zw: of the destination.
    texel: vec4<f32>,
}

@group(0) @binding(0) var samp: sampler;
@group(0) @binding(1) var tex_a: texture_2d<f32>;
@group(0) @binding(2) var tex_b: texture_2d<f32>;
@group(0) @binding(3) var tex_c: texture_2d<f32>;
@group(0) @binding(4) var<uniform> params: Params;

const FLAG_A_SRGB: u32 = 1u;
const FLAG_B_SRGB: u32 = 2u;
const FLAG_C_SRGB: u32 = 4u;
const FLAG_DST_SRGB: u32 = 8u;
const FLAG_ALPHA: u32 = 16u;
const FLAG_MASK: u32 = 32u;
const FLAG_REPLACE: u32 = 64u;
const FLAG_INVERT: u32 = 128u;

const KIND_COPY: u32 = 0u;
const KIND_COMPRESS: u32 = 1u;
const KIND_ADD: u32 = 2u;
const KIND_COLORIZE: u32 = 3u;
const KIND_BLUR1D: u32 = 6u;
const KIND_DISTORT: u32 = 10u;
const KIND_MULTIPLY: u32 = 13u;
const KIND_LEAK: u32 = 16u;

fn to_gamma(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn fetch_a(uv: vec2<f32>) -> vec4<f32> {
    var c = textureSampleLevel(tex_a, samp, uv, 0.0);
    if (params.head.y & FLAG_A_SRGB) != 0u {
        c = vec4<f32>(to_gamma(c.rgb), c.a);
    }
    return c;
}

fn fetch_b(uv: vec2<f32>) -> vec4<f32> {
    var c = textureSampleLevel(tex_b, samp, uv, 0.0);
    if (params.head.y & FLAG_B_SRGB) != 0u {
        c = vec4<f32>(to_gamma(c.rgb), c.a);
    }
    return c;
}

fn fetch_c(uv: vec2<f32>) -> vec4<f32> {
    var c = textureSampleLevel(tex_c, samp, uv, 0.0);
    if (params.head.y & FLAG_C_SRGB) != 0u {
        c = vec4<f32>(to_gamma(c.rgb), c.a);
    }
    return c;
}

fn luma(c: vec3<f32>) -> f32 {
    // The Colorize shader's weights.
    return dot(c, vec3<f32>(0.23, 0.66, 0.11));
}

fn channel(c: vec4<f32>, index: f32) -> f32 {
    let i = u32(clamp(index, 0.0, 3.0));
    if i == 0u { return c.r; }
    if i == 1u { return c.g; }
    if i == 2u { return c.b; }
    return c.a;
}

fn filter_color(uv: vec2<f32>) -> vec4<f32> {
    let kind = params.head.x;
    let flags = params.head.y;
    if kind == KIND_COPY {
        // FilterCopyWithParams*: rgb * colour, alpha = the alpha parameter
        // (the pass blends onto the destination with it).
        let s = fetch_a(uv);
        var a = s.a;
        if (flags & FLAG_ALPHA) != 0u {
            a = params.p0.w;
        }
        if (flags & FLAG_MASK) != 0u {
            var m = channel(fetch_b(uv), params.p1.x);
            if (flags & FLAG_INVERT) != 0u {
                m = 1.0 - m;
            }
            a = a * m;
        }
        return vec4<f32>(s.rgb * params.p0.rgb, a);
    }
    if kind == KIND_COMPRESS {
        // p0: bias, scale, mono. ADAPTED from the parameter names.
        var s = fetch_a(uv);
        var rgb = s.rgb;
        if params.p0.z > 0.5 {
            rgb = vec3<f32>(luma(rgb));
        }
        return vec4<f32>(clamp(rgb * params.p0.y + params.p0.x, vec3<f32>(0.0), vec3<f32>(1.0)), s.a);
    }
    if kind == KIND_ADD {
        // p0: sourceMul, addMul, maskChannel; p1.xy tile.
        let s = fetch_a(uv);
        let t = fetch_b(uv * params.p1.xy);
        if (flags & FLAG_REPLACE) != 0u {
            // FilterAddWithReplaceShader.
            return vec4<f32>(mix(s.rgb * params.p0.x, t.rgb * params.p0.y, t.a), 1.0);
        }
        if (flags & FLAG_MASK) != 0u {
            // FilterAddWithMaskShader.
            let m = channel(fetch_c(uv), params.p0.z);
            return vec4<f32>(s.rgb * params.p0.x * (1.0 - m) + t.rgb * params.p0.y * m, 1.0);
        }
        // FilterAddShader.
        return vec4<f32>(s.rgb * params.p0.x + t.rgb * params.p0.y, s.a);
    }
    if kind == KIND_COLORIZE {
        // FilterColorizeShader: lerp(src, colour * luminance(src), strength).
        let s = fetch_a(uv);
        return vec4<f32>(mix(s.rgb, params.p0.rgb * luma(s.rgb), params.p0.w), s.a);
    }
    if kind == KIND_BLUR1D {
        // FilterBlur1DShader: four taps at -1.5, -0.5, 0.5, 1.5 texels along
        // the direction p0.xy.
        let stride = params.p0.xy * params.texel.xy;
        var sum = fetch_a(uv + stride * -1.5);
        sum += fetch_a(uv + stride * -0.5);
        sum += fetch_a(uv + stride * 0.5);
        sum += fetch_a(uv + stride * 1.5);
        return sum * 0.25;
    }
    if kind == KIND_DISTORT {
        // FilterDistortShader: the distorter's xy (decoded to -1..1) shifts the
        // lookup by p0.x texels, fading out toward the screen border.
        let nuv = uv * params.p1.xy + params.p1.zw;
        let n = fetch_b(nuv).xy * 2.0 - 1.0;
        let centred = (uv - 0.5) * 2.0;
        let radius = clamp(length(centred), 0.0, 1.0);
        let shifted = uv + n * params.texel.xy * params.p0.x;
        return fetch_a(mix(shifted, uv, radius));
    }
    if kind == KIND_MULTIPLY {
        // FilterMulShaderUseTexture: (tex * texMul * tex.a + texBias) *
        // (src * srcMul + srcBias). p0: srcMul, texMul, srcBias, texBias.
        let s = fetch_a(uv);
        let t = fetch_b(uv * params.p1.xy);
        let tex = t.rgb * params.p0.y * t.a + params.p0.w;
        let src = s.rgb * params.p0.x + params.p0.z;
        return vec4<f32>(tex * src, 1.0);
    }
    if kind == KIND_LEAK {
        // DOFBlurLeakShader: sampler0 (a, premultiplied by its own alpha)
        // over sampler1 (b).
        let blurred = fetch_a(uv);
        let scene = fetch_b(uv);
        return vec4<f32>(blurred.rgb * blurred.a + scene.rgb * (1.0 - blurred.a), 1.0);
    }
    // Pass-through.
    return fetch_a(uv);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var c = filter_color(in.uv);
    if (params.head.y & FLAG_DST_SRGB) != 0u {
        c = vec4<f32>(to_linear(c.rgb), c.a);
    }
    return c;
}
