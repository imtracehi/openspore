import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> center_time: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101)
var<uniform> atmosphere: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102)
var cloud_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103)
var cloud_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(104)
var<uniform> cloud_controls: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105)
var<uniform> zenith: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106)
var<uniform> horizon: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107)
var<uniform> sun: vec4<f32>;

const TAU: f32 = 6.2831853;

fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    let low = color / 12.92;
    let high = pow((color + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, color <= vec3<f32>(0.04045));
}

fn hash4(n: u32) -> vec4<f32> {
    var v = vec4<u32>(n, n * 1664525u + 1013904223u, n ^ 0x9E3779B9u, n * 22695477u + 1u);
    v = v * 1664525u + 1013904223u;
    v.x += v.y * v.w; v.y += v.z * v.x; v.z += v.x * v.y; v.w += v.y * v.z;
    v = v ^ (v >> vec4<u32>(16u));
    v.x += v.y * v.w; v.y += v.z * v.x; v.z += v.x * v.y; v.w += v.y * v.z;
    return vec4<f32>(v & vec4<u32>(0xFFFFu)) / 65535.0;
}

// One ring of original cloud_lit sprites scattered in azimuth cells. The
// sprites stay upright like the original sky billboards; the scattering,
// drift and haze fade are preview mappings.
fn cloud_layer(
    direction: vec3<f32>,
    layer: u32,
    cells: f32,
    low: f32,
    high: f32,
    scale: f32,
    haze: vec3<f32>,
    sky_in: vec3<f32>,
) -> vec3<f32> {
    var sky = sky_in;
    let altitude = asin(clamp(direction.y, -1.0, 1.0));
    let drift = center_time.w * 0.0009 * (1.0 + f32(layer) * 0.6);
    let azimuth = fract(atan2(direction.z, direction.x) / TAU + 0.5 + drift);
    let base = floor(azimuth * cells);
    let horizontal = max(cos(altitude), 0.05);
    for (var k: i32 = -1; k <= 1; k++) {
        let cell = base + f32(k);
        let wrapped = u32((cell + cells) % cells);
        let h = hash4(wrapped * 7919u + layer * 104729u + 17u);
        let present = select(0.0, 1.0, h.x < cloud_controls.y);
        let center_azimuth = (cell + 0.5 + (h.y - 0.5) * 0.55) / cells;
        let center_altitude = asin(mix(low, high, h.z * h.z));
        let half_width = (0.45 + 0.55 * h.w) / cells * TAU * 0.5 * scale;
        let half_height = half_width * 0.66;
        let dx = (fract(azimuth - center_azimuth + 0.5) - 0.5) * TAU * horizontal;
        let dy = altitude - center_altitude;
        var local = vec2<f32>(dx / half_width, dy / half_height);
        if fract(h.y * 13.0) > 0.5 { local.x = -local.x; }
        let uv = vec2<f32>(local.x * 0.5 + 0.5, 0.5 - local.y * 0.5);
        let inside = select(0.0, 1.0, all(abs(local) < vec2<f32>(1.0)));
        let source = textureSampleLevel(cloud_map, cloud_sampler, uv, 0.0);
        // The authored cloud is lit white with a cool, darker base. Keep it
        // bright and let distant clouds sink into the horizon haze.
        var color = source.rgb * vec3<f32>(1.08, 1.08, 1.12);
        let distance_fade = 1.0 - smoothstep(0.0, 0.32, center_altitude);
        color = mix(color, haze * 1.05, 0.55 * distance_fade);
        let amount = source.a * present * inside * atmosphere.a
            * smoothstep(-0.005, 0.03, direction.y);
        sky = mix(sky, color, clamp(amount, 0.0, 1.0));
    }
    return sky;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var input = pbr_input_from_standard_material(in, is_front);
    let direction = normalize(in.world_position.xyz - center_time.xyz);
    let altitude = clamp(direction.y, 0.0, 1.0);

    // Interpret installed AirColor through the preview's sRGB convention.
    // Zenith and horizon are derived from it on the CPU; the gradient shape,
    // sun halo and haze band are preview approximations of the atmosphere.
    let air = srgb_to_linear(atmosphere.rgb);
    let haze = mix(horizon.rgb, air, horizon.w);
    var sky = mix(horizon.rgb, zenith.rgb, pow(altitude, zenith.w));
    // Fade into the distance-fog color right at the horizon so the fogged sea
    // and far islands meet the sky without a seam.
    sky = mix(haze, sky, smoothstep(0.0, 0.07, direction.y));

    let toward_sun = max(dot(direction, normalize(sun.xyz)), 0.0);
    let halo = pow(toward_sun, 6.0) * 0.22 + pow(toward_sun, 48.0) * 0.5;
    sky += vec3<f32>(1.0, 0.93, 0.78) * halo * sun.w;
    sky = mix(sky, vec3<f32>(4.0, 3.8, 3.4), smoothstep(0.9993, 0.99965, toward_sun));

    // The cloud_lit RW4 raster is original source pixels. Two scattered rings
    // keep big cloud banks low and smaller puffs higher in the dome.
    sky = cloud_layer(direction, 0u, cloud_controls.x, cloud_controls.z,
        cloud_controls.z + (cloud_controls.w - cloud_controls.z) * 0.35, 0.95, haze, sky);
    sky = cloud_layer(direction, 1u, cloud_controls.x * 0.75, cloud_controls.z + 0.08,
        cloud_controls.w, 0.7, haze, sky);

    input.material.base_color = vec4<f32>(sky, 1.0);
    var out: FragmentOutput;
    // Skip PBR lighting so the atmosphere cannot be shadowed or dimmed by the
    // scene's directional lights. Keep Bevy's camera post-processing path.
    out.color = vec4<f32>(sky, 1.0);
    out.color = main_pass_post_lighting_processing(input, out.color);
    return out;
}
