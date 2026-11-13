// Cell spherical-harmonics lighting: the retail pixel fragments
// `shLightingSpec_CellEditorsNMap` (Creator, Spore_Graphics 0x40212002 instance 3,
// PS 25) and `shLightingSpec_BakedCreaturesNormalMap` (stage, PS 28: the same
// body plus the cel rim and the shadow adjust of the specular), 16 coefficients,
// kUseSH5 = 0, ported line by line. Spore shades in display (gamma) space, so
// the base colour is taken as display values, lit, and converted to linear
// again for the sRGB target (the unlit texel then reaches the screen unchanged,
// like the editor's gamma_material copies).
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
};
import bevy_pbr::render::mesh_view_bindings::view;

// shCoeffs[16]: the light PROP's coefficients times its exposure (xyz).
@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> sh: array<vec4<f32>, 16>;
// dirLightsWorld[0]: Spore-space direction towards the sun (xyz), colour * strength.
@group(constants::MATERIAL_BIND_GROUP) @binding(101)
var<uniform> sun: array<vec4<f32>, 2>;
// x gloss (blend of glossy / diffuse), y specE, z specStrength (materialParams
// w, x, y), w mode bits: 1 = the base colour holds display values, 2 = Cell
// stage axes.
@group(constants::MATERIAL_BIND_GROUP) @binding(102)
var<uniform> params: vec4<f32>;
// x bump scale (materialParams.z), y cel strength, z specular shadow adjust.
@group(constants::MATERIAL_BIND_GROUP) @binding(103)
var<uniform> extra: vec4<f32>;
// The spec / bump map: its alpha scales specStrength and gloss (specNMap.a).
@group(constants::MATERIAL_BIND_GROUP) @binding(104)
var spec_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105)
var spec_sampler: sampler;

const kSH_A1: f32 = 0.6666667;
const kSH_A2: f32 = 0.25;
const kCelOutlineThreshold: f32 = 0.25;
const kCelOutlineThresholdRange: f32 = 0.075;

fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    let low = color / 12.92;
    let high = pow((color + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, color <= vec3<f32>(0.04045));
}
fn linear_to_srgb(color: vec3<f32>) -> vec3<f32> {
    let c = max(color, vec3<f32>(0.0));
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(high, low, c <= vec3<f32>(0.0031308));
}

// Bevy world (Y up, the Creator's cell front towards -Z) to the Spore world the
// coefficients are authored in (Z up, front towards -Y): the proper rotation
// that maps the model's front -Y to -Z and up Z to Y, i.e. Spore (x, y, z) is
// Bevy (-x, z, y). (The retail frames put the sun highlight on the upper-left
// edge of the body, which this map reproduces.)
// The Cell stage looks straight down with screen-up = Bevy -Z, so Spore +Y
// (up the screen) is Bevy -Z: Spore (x, y, z) is Bevy (x, -z, y).
fn to_spore(v: vec3<f32>, stage: bool) -> vec3<f32> {
    if (stage) {
        return vec3<f32>(v.x, -v.z, v.y);
    }
    return vec3<f32>(-v.x, v.z, v.y);
}

// Bands 0..2 of the SH at x0, as SHDiffSpec9 / SHLightingNMap.
fn bands_0_1(x0: vec3<f32>) -> vec3<f32> {
    return sh[1].xyz * x0.y + sh[2].xyz * x0.z + sh[3].xyz * x0.x;
}
fn band_2(x0: vec3<f32>) -> vec3<f32> {
    let x1 = x0 * x0;
    let x2 = x0 * x0.yzx;
    return sh[4].xyz * x2.x + sh[5].xyz * x2.y + sh[6].xyz * (3.0 * x1.z - 1.0)
        + sh[7].xyz * x2.z + sh[8].xyz * (x1.x - x1.y);
}
// Band 3 (coefficients 9..15), added to the glossy part only (kSH_A3 = 0).
fn band_3(x0: vec3<f32>) -> vec3<f32> {
    let x1 = x0 * x0;
    return sh[9].xyz * (3.0 * x1.x - x1.y) * x0.y
        + sh[10].xyz * x1.x * x0.z
        + sh[11].xyz * (5.0 * x1.z - 1.0) * x0.y
        + sh[12].xyz * (5.0 * x1.z - 3.0) * x0.z
        + sh[13].xyz * (5.0 * x1.z - 1.0) * x0.x
        + sh[14].xyz * (x1.x - x1.y) * x0.z
        + sh[15].xyz * (x1.x - 3.0 * x1.y) * x0.x;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let input = pbr_input_from_standard_material(in, is_front);
    // alpha_discard: an opaque material writes alpha 1 (the atlases' alpha is data).
    let base = alpha_discard(input.material.flags, input.material.alpha_cutoff, input.material.base_color);
    let mode = u32(params.w + 0.5);
    let stage = (mode & 2u) != 0u;

    // materialParams.z scales the z of the normal map's normal before it is
    // normalised: a larger value flattens the bump (1.0 in every installed cell
    // material). Bevy has already applied the map at full strength.
    let geometric = normalize(input.world_normal);
    var bumped = normalize(input.N);
    if (extra.x != 1.0) {
        let along = dot(bumped, geometric);
        bumped = normalize(bumped - geometric * along + geometric * (along * extra.x));
    }
    let normal = to_spore(bumped, stage);
    // worldCameraDirection: the camera's forward axis.
    let forward = to_spore(-normalize(view.world_from_view[2].xyz), stage);
    let reflected = reflect(forward, normal);

    // specStrength and gloss are scaled by the alpha of the spec / bump map.
    var spec_alpha = 1.0;
    @if(VERTEX_UVS_A) {
        spec_alpha = textureSample(spec_texture, spec_sampler, in.uv).a;
    }
    let s = params.x * spec_alpha;
    let spec_e = params.y * extra.z;
    let spec_strength = params.z * spec_alpha * extra.z;

    // Diffuse: the cosine-lobe convolution of bands 0..2 at the normal.
    let diffuse = sh[0].xyz + bands_0_1(normal) * kSH_A1 + band_2(normal) * kSH_A2;
    // Glossy: the unconvolved bands 0..3 at the reflected normal.
    let glossy = sh[0].xyz + bands_0_1(reflected) + band_2(reflected) + band_3(reflected);

    var color = base.rgb;
    if ((mode & 1u) == 0u) {
        color = linear_to_srgb(color);
    }
    var lit = color * (diffuse * (1.0 - s) + glossy * s);
    let sun_dir = sun[0].xyz;
    let spec = pow(clamp(dot(reflected, sun_dir), 0.0, 1.0), spec_e);
    lit += spec * spec_strength * sun[1].xyz;
    // ApplyCel: darken the silhouettes (stage fragment, strength 0 = off).
    let n_dot_v = -dot(normal, forward);
    lit *= (1.0 - extra.y) + extra.y * smoothstep(kCelOutlineThreshold - kCelOutlineThresholdRange, kCelOutlineThreshold + kCelOutlineThresholdRange, n_dot_v);

    var out: FragmentOutput;
    out.color = vec4<f32>(srgb_to_linear(clamp(lit, vec3<f32>(0.0), vec3<f32>(1.0))), base.a);
    out.color = main_pass_post_lighting_processing(input, out.color);
    return out;
}
