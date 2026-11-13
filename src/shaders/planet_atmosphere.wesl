import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};
import bevy_pbr::render::mesh_view_bindings::view;

// ADAPTED atmosphere shell (stand-in for the undecoded TerrainAtmosphere
// program): additive glow in the palette AirColor that grows with the length
// of the view ray inside the shell (above the planet). Only back faces shade,
// so every pixel integrates its ray once, from inside or outside the shell.
@group(constants::MATERIAL_BIND_GROUP) @binding(100)
var<uniform> sphere: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101)
var<uniform> color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102)
var<uniform> params: vec4<f32>;

// Entry/exit distances along a unit ray (t1 < t0 when it misses).
fn span(o: vec3<f32>, d: vec3<f32>, r: f32) -> vec2<f32> {
    let b = dot(o, d);
    let c = dot(o, o) - r * r;
    let disc = b * b - c;
    if disc < 0.0 {
        return vec2<f32>(1.0, -1.0);
    }
    let s = sqrt(disc);
    return vec2<f32>(-b - s, -b + s);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var out: FragmentOutput;
    if is_front {
        out.color = vec4<f32>(0.0);
        return out;
    }
    let o = view.world_position.xyz - sphere.xyz;
    let d = normalize(in.world_position.xyz - view.world_position.xyz);
    let shell = span(o, d, params.y);
    let ground = span(o, d, sphere.w);
    var far = shell.y;
    if ground.y >= ground.x && ground.x > 0.0 {
        far = min(far, ground.x);
    }
    let path = max(far - max(shell.x, 0.0), 0.0);
    let reference = sqrt(max(params.y * params.y - sphere.w * sphere.w, 1.0));
    let glow = pow(clamp(path / reference, 0.0, 1.0), params.x) * color.a;
    out.color = vec4<f32>(color.rgb * glow, glow);
    return out;
}
