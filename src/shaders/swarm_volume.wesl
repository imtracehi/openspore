// Swarm volume slices (component type 0x28): port of the retail `volumeFire`
// pixel fragment (HLSL fragment pack 0x40212002!3, PS 114; builder 0x7000009F)
// with its `mNoise` 3D noise (Blum Blum Shub hashed slices of the 2D noise
// texture, modulus 61) at one octave. Vertex data (swarm_fx/volume.rs):
// uv = uvw.xy, uv_b.x = uvw.z, color = (noise position xyz, mTime).
import bevy_pbr::render::forward_io::VertexOutput;

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var fire_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var fire_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var noise_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var noise_sampler: sampler;
// mNoiseScale.mFrequencyMag (shader data 0x205): xyz frequency, w magnitude.
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var<uniform> frequency_mag: vec4<f32>;

const MODULUS: f32 = 61.0;

// mNoiseHash(z, ramp, modulus): hashes z and z + 1 (HLSL fmod = WGSL %).
fn noise_hash(z: f32, ramp: f32) -> vec2<f32> {
    let n = (vec2<f32>(z * ramp) + vec2<f32>(0.0, ramp)) % MODULUS;
    return (n * n) % MODULUS;
}

// mNoise(float3 pos, sampler): two hashed rows of the noise texture blended
// across the integer lattice of z.
fn m_noise(pos: vec3<f32>) -> f32 {
    let split = modf(pos.z);
    let frac_arg = split.fract;
    let hash = noise_hash(split.whole, 3.0) / MODULUS;
    let p = pos.xy / MODULUS;
    // The sampler has no mips (MIPFILTER none): level 0.
    let a = textureSampleLevel(noise_texture, noise_sampler, vec2<f32>(p.x, p.y + hash.x), 0.0).xy * 2.0 - 1.0;
    let b = textureSampleLevel(noise_texture, noise_sampler, vec2<f32>(p.x, p.y + hash.y), 0.0).xy * 2.0 - 1.0;
    return mix(a.x + a.y * frac_arg, b.x + b.y * (frac_arg - 1.0), smoothstep(0.0, 1.0, frac_arg));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv3 = vec3<f32>(in.uv, in.uv_b.x);
    let pos = in.color.xyz;
    let m_time = in.color.w;
    let u = length(uv3.xy * 2.0 - 1.0);
    let noise_pos = vec3<f32>(pos.xy * frequency_mag.xy, m_time * 0.6 - pos.z * frequency_mag.z);
    // Turbulence(noisePos, noiseTexture, 1): a single octave.
    let f_noise = frequency_mag.w * m_noise(noise_pos);
    let v = 1.0 - (uv3.z + uv3.z * f_noise);
    var c = textureSampleLevel(fire_texture, fire_sampler, vec2<f32>(u, v), 0.0);
    // D3DTADDRESS_BORDER with the default (transparent black) border colour.
    if u > 1.0 || v < 0.0 || v > 1.0 {
        c = vec4<f32>(0.0);
    }
    // SRCBLEND = SRCALPHA, DESTBLEND = ONE: premultiplied add with zero alpha.
    return vec4<f32>(c.rgb * c.a, 0.0);
}
