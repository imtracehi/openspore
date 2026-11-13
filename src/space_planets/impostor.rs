//! The retail impostor lighting of the solar-system worlds (helper
//! `space-final2`; `notes/space-visuals-notes.md` "Planet impostors",
//! `notes/space-final2-notes.md` G2): planetImpostorVS / PS
//! (`shaders/space_impostor.wesl`) on the orbit bodies instead of the PBR
//! material: terminator cutoff 0.2, the blue night tint (0.7, 0.3, 2.3) x 0.5
//! x 0.9 of the night luminance, ambient level 0.2, emissive x 1.3, the
//! specular highlight pow(camera . R, 50) over water in (1, 0.6, 0.06) and the
//! rim fog 0.5 (1 - eye . N) toward the scatter colour.
//!
//! The atmosphere impostor and the baked scatter cube (helper `space-final4`,
//! `notes/space-final4-notes.md` A1-A3, VERIFIED code / source):
//! * The impostor bake 0xFA8800 builds two models per world: "planetImpostor"
//!   (scale 1.0, textures +0x884 diffuse, +0x888 spec / emissive, +0x88C
//!   normal, +0x890 scatter as Sampler0..3) and "planetAtmoImpostor" (scale
//!   0x3F99999A = 1.2, the same scatter cube +0x890 as its Sampler0,
//!   property list "PlanetImpostorAtmosphere").
//! * The scatter cube is BakeInScatterTextureShader's (0xF9D960, the six
//!   faces into sTerrainBakeInScatterTexture, then each face copied into
//!   +0x890) run by the impostor step 3 of 0xF9E490 after UpdateFrame
//!   0xFBF540 with the game viewer: the lobes at p = sat((|cam| - R) /
//!   kSkyScaleDist) x 0.6 + 0.39, which is 0.99 for the far solar-view camera
//!   (INFERRED: the solar camera is always beyond R + 100).
//! * planetAtmoImpostorVS (VS 118) / PerPixelLightingPS (PS 125): the light
//!   at the origin, eyeVec toward the camera, dpView = smoothstep(0.1, 1,
//!   N . -eye) (only the far side of the shell shows), shadow = sat(L . N +
//!   1); colour = scatter x shadow + scatter x scatter.a, alpha = sat(dpView x
//!   shadow) x 2 x hackOff (0 for a black scatter cube: airless worlds).
//!
//! VERIFIED: the programs, the model scales and the bake of the scatter cube.
//! ADAPTED: the diffuse cube is the planet's colour atlas (alpha = water for
//! the specular channel), the emissive channel the lava glow atlas and the
//! normal cube the mesh's ground normals; the cube lookup frame (customParams
//! of both programs, not traced) puts the baked sun lobe away from the star
//! like `planet_sky::scatter_frame`, which only matters through its z axis
//! (the bake is symmetric around it); the lobe colour follows the planet
//! view's choice (the look's air colour unless `OPENSPORE_PLANET_SKY=1`);
//! the atmosphere's blend state is not traced (alpha blending).
//! `OPENSPORE_SPACE_IMPOSTOR=0` keeps the earlier PBR material and halo.
use bevy::{
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

pub type ImpostorMaterial = ExtendedMaterial<StandardMaterial, PlanetImpostor>;
pub type AtmosphereImpostorMaterial = ExtendedMaterial<StandardMaterial, AtmosphereImpostor>;

/// 0xFA8800: the "planetAtmoImpostor" model scale.
pub const ATMOSPHERE_SCALE: f32 = 1.2;
/// The lobes' altitude row of the solar-view bake (p of 0xFBF540 with the
/// camera beyond R + kSkyScaleDist).
pub const BAKE_P: f32 = 0.99;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct PlanetImpostor {
    /// rgb: the scatter colour (display values), a: its additive part (when
    /// no cube is bound).
    #[uniform(100)]
    pub scatter: Vec4,
    /// x: 1 when `glow` is bound, y: 1 when `scatter_cube` is the baked cube.
    #[uniform(101)]
    pub flags: Vec4,
    #[texture(102)]
    #[sampler(103)]
    pub diffuse: Handle<Image>,
    #[texture(104)]
    #[sampler(105)]
    pub glow: Option<Handle<Image>>,
    /// Sampler3: the baked in-scatter cube.
    #[texture(106, dimension = "cube")]
    #[sampler(107)]
    pub scatter_cube: Handle<Image>,
}

impl MaterialExtension for PlanetImpostor {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/space_impostor.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// planetAtmoImpostorPerPixelLightingPS: Sampler0 = the scatter cube.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct AtmosphereImpostor {
    #[texture(100, dimension = "cube")]
    #[sampler(101)]
    pub scatter_cube: Handle<Image>,
}

impl MaterialExtension for AtmosphereImpostor {
    fn fragment_shader() -> ShaderRef {
        "embedded://openspore/shaders/space_atmo_impostor.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// The child of an orbit body that draws its atmosphere impostor.
#[derive(Component)]
pub struct OrbitAtmosphereImpostor;

/// Whether the impostor program draws the orbit bodies.
pub fn enabled() -> bool {
    !std::env::var("OPENSPORE_SPACE_IMPOSTOR").is_ok_and(|v| v.trim() == "0")
}

/// The earlier ADAPTED scatter colour of a world (used when no cube can be
/// baked): its air colour, by the atmosphere thickness
/// (kAtmThicknessByAtmosphereScore, 0 airless .. 1.75), at most 0.6; no
/// additive part.
pub fn scatter_for(look: &super::Look) -> Vec4 {
    let k = (look.thickness / 1.75).clamp(0.0, 1.0) * 0.6;
    Vec4::new(look.air[0] * k, look.air[1] * k, look.air[2] * k, 0.0)
}

/// The world's scatter cube (0xF9D960 at `BAKE_P`): the lobes of its sky
/// tables with the planet view's colour choice.
pub fn bake_scatter(tables: &crate::planet_sky::SkyTables, inputs: &crate::planet_sky::SkyInputs, air: [f32; 3]) -> Image {
    let table_sky = std::env::var("OPENSPORE_PLANET_SKY").is_ok_and(|v| v == "1");
    let lobes = tables.lobes(inputs, BAKE_P, (!table_sky).then_some(air));
    crate::planet_sky::scatter_cube(&lobes)
}

/// Whether a baked cube is black (the PS 125 `hackOff`: no atmosphere).
pub fn cube_is_black(image: &Image) -> bool {
    image.data.as_ref().is_none_or(|d| d.iter().all(|&b| b == 0))
}

/// The material of a body.
pub fn material(
    look: &super::Look,
    diffuse: Handle<Image>,
    glow: Option<Handle<Image>>,
    scatter_cube: Option<Handle<Image>>,
) -> ImpostorMaterial {
    ImpostorMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            ..default()
        },
        extension: PlanetImpostor {
            scatter: scatter_for(look),
            flags: Vec4::new(
                if glow.is_some() { 1.0 } else { 0.0 },
                if scatter_cube.is_some() { 1.0 } else { 0.0 },
                0.0,
                0.0,
            ),
            diffuse,
            glow,
            scatter_cube: scatter_cube.unwrap_or(crate::planet_sky::NEUTRAL_CUBE),
        },
    }
}

/// The atmosphere impostor's material: the back faces of the 1.2 x sphere,
/// blended.
pub fn atmosphere_material(scatter_cube: Handle<Image>) -> AtmosphereImpostorMaterial {
    AtmosphereImpostorMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            fog_enabled: false,
            cull_mode: Some(bevy::render::render_resource::Face::Front),
            alpha_mode: AlphaMode::Blend,
            ..default()
        },
        extension: AtmosphereImpostor { scatter_cube },
    }
}

/// Register the materials (render apps).
pub fn build(app: &mut App) {
    app.add_plugins(MaterialPlugin::<ImpostorMaterial>::default());
    app.add_plugins(MaterialPlugin::<AtmosphereImpostorMaterial>::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airless_worlds_scatter_nothing() {
        let mut look = crate::space_planets::Look {
            ramp: vec![[0.5; 3]; 4],
            beach: [0.5; 3],
            cliff: [0.5; 3],
            water: [0.1, 0.2, 0.5],
            fluid: crate::space_planets::Fluid::Water,
            water_level: 0.0,
            air: [0.5, 0.7, 1.0],
            thickness: 0.0,
            home: false,
            climate: (0.5, 0.5),
            life_column: 0.0,
        };
        assert_eq!(scatter_for(&look), Vec4::ZERO);
        look.thickness = 1.75;
        let s = scatter_for(&look);
        assert!((s.z - 0.6).abs() < 1e-6 && s.w == 0.0);
    }

    /// The retail bake through the installed sky tables: a thick atmosphere's
    /// cube is lit; at A 0 the sunset table still weighs the lobes (0.71 at
    /// p 0.99), so OpenSpore keeps airless worlds black itself (ADAPTED).
    #[test]
    #[ignore = "reads the installed game"]
    fn the_scatter_cube_bakes_from_the_sky_tables() {
        let Ok(install) = crate::spore_install::discover() else {
            return;
        };
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let tables = crate::planet_sky::SkyTables::load(&index, 0, 0).expect("sky tables");
        let inputs = |a| crate::planet_sky::SkyInputs { temperature: 0.5, atmosphere: a, atmosphere_type: 0, water_type: 0 };
        let lobes = tables.lobes(&inputs(0.0), BAKE_P, None);
        assert!((lobes.sunset - 0.706).abs() < 0.01, "{lobes:?}");
        let thick = crate::planet_sky::scatter_cube(&tables.lobes(&inputs(0.8), BAKE_P, None));
        assert!(!cube_is_black(&thick));
    }
}
