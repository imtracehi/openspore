//! Retail land lighting of the planet stages (`notes/planet-look-notes.md`
//! section 9): the day / dusk / night terrain colours by the sun angle at
//! each surface point, and the day / transition / night weights of the model
//! lighting that follow the same sun.
//!
//! Original data / code (VERIFIED, ModAPI march2017 addresses):
//! - PlanetMaterial 0x0243AD2B!0xD985994C, read by 0xFBE7A0 into
//!   cTerrainStateMgr: kDayLightColor (+0xC8), kDuskLightColor (+0xE0),
//!   kNightLightColor (+0xF8), kDayShadowColor (+0xD4), kDuskShadowColor
//!   (+0xEC), kNightShadowColor (+0x104), kDuskDawnStartEnd (+0x110),
//!   kNightLightingTint (+0xB4) whose w is replaced by kNightLumBlend (+0xC0).
//!   UpdateFrame 0xFBF540 copies them unscaled into the TerrainState;
//!   PrepareShaderData 0xF98910 turns the angles into scale / bias pairs:
//!   day = sat((N.L - cos 70) / (cos 50 - cos 70)), night = sat((-N.L - cos 90)
//!   / (cos 60 - cos 90)) = sat(-2 N.L), and vSunDir.w = cos 60.
//! - Shader maths (HLSL fragments, pack group 0x40212002): TerrainLightingLib
//!   EvaluateVertexLighting (dusk = 1 - max(day, night), blended light /
//!   shadow colours), TerrainPixelLib EvaluatePixelLighting (light bent toward
//!   the normal by 0.35 x dusk, strength |N.L| 0.9 + sat(N.L) 0.2 + (1 -
//!   |N.L|)^2 0.5, x lerp(1, shadow, 0.2), (shadow + delta x s) x diffuse x s,
//!   night luminance x tint by tint.w x night), TerrainAboveSynthPSMedium
//!   (detailing: lerp(c, c x sat(3 N.V), 0.3)), TerrainBelowRefractPS
//!   (normal x (shadow 0.4 + 0.6), no shadow term).
//! - Model lighting 0x7061E0 (per object, from the lighting state
//!   lighting_properties~!planet 0x40200100!0x5F848353 loaded by 0x70AF40):
//!   d = sun . object up; d > 0: day = sat((d - planetDayStart) /
//!   planetDayRange), SH = lerp(transition, day, day); else night = sat((-d -
//!   planetNightStart) / planetNightRange), SH = (1 - night) transition +
//!   night x night set. The SH sets (0x70A620) are the atmosphere lobes plus
//!   planetSunBoost / planetTransitionBoost / planetNightBoost (rgb + phase),
//!   x planetAtmosphere, saturation planetSaturation.
//!
//! ADAPTED: models keep Bevy's PBR lights (no SH pipeline): the sun's colour
//! and the ambient brightness follow the retail terrain light curve of flat
//! ground at the stage origin (so models keep the noon balance with the land
//! all day) tinted by the VERIFIED object weights and boost colours.
use crate::{asset_catalog::ResourceIndex, world_catalog::PLANET_MATERIALS};
use bevy::prelude::*;

/// The land's diffuse scale for the retail lighting (sky slot 9 z). INFERRED:
/// the retail colour map is authored at half scale (planet_color_ramps_* stay
/// below ~0.5 and the 2x day light brings them back: EvaluatePixelLighting
/// gives 2.02 x diffuse on flat ground at noon); OpenSpore's stand-in ramp
/// (the palette elevation colours) is full scale, so it is halved.
/// `OS_RETAIL_DIFFUSE` overrides it (calibration).
pub fn land_diffuse_scale() -> f32 {
    crate::world_presentation::tune("RETAIL_DIFFUSE", 0.5)
}

/// The planet's terrain lighting constants (display values).
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct TerrainLighting {
    /// kNightLightingTint rgb, w = kNightLumBlend.
    pub night_tint: [f32; 4],
    /// kDay / kDusk / kNightLightColor.
    pub light: [[f32; 3]; 3],
    /// kDay / kDusk / kNightShadowColor.
    pub shadow: [[f32; 3]; 3],
    /// kDuskDawnStartEnd (degrees): day end, dusk end, night start, night full.
    pub dusk_dawn: [f32; 4],
}

impl Default for TerrainLighting {
    /// The installed PlanetMaterial values (VERIFIED dump).
    fn default() -> Self {
        Self {
            night_tint: [0.43, 0.49, 1.0, 0.25],
            light: [[1.75; 3], [1.7; 3], [1.75; 3]],
            shadow: [[0.85; 3], [0.95; 3], [0.65; 3]],
            dusk_dawn: [50.0, 70.0, 60.0, 90.0],
        }
    }
}

const K_DAY_LIGHT: u32 = 0x87C2473F;
const K_DUSK_LIGHT: u32 = 0x7CB9E01A;
const K_NIGHT_LIGHT: u32 = 0xDAFA9ED3;
const K_DAY_SHADOW: u32 = 0x1331B443;
const K_DUSK_SHADOW: u32 = 0xE5AC96BC;
const K_NIGHT_SHADOW: u32 = 0x1CE679BF;
const K_DUSK_DAWN: u32 = 0x54B6D2B6;
const K_NIGHT_TINT: u32 = 0x031E1D79;
const K_NIGHT_LUM_BLEND: u32 = 0xA14AD299;

impl TerrainLighting {
    /// Read PlanetMaterial like 0xFBE7A0 (missing fields keep the defaults).
    pub fn load(index: &ResourceIndex) -> Self {
        let mut out = Self::default();
        let Ok(list) = index.properties(PLANET_MATERIALS) else {
            return out;
        };
        let v3 = |id: u32, slot: &mut [f32; 3]| {
            if let Some(v) = list.0.get(&id).and_then(|p| p.vector3()).filter(|v| v.iter().all(|c| c.is_finite())) {
                *slot = v;
            }
        };
        v3(K_DAY_LIGHT, &mut out.light[0]);
        v3(K_DUSK_LIGHT, &mut out.light[1]);
        v3(K_NIGHT_LIGHT, &mut out.light[2]);
        v3(K_DAY_SHADOW, &mut out.shadow[0]);
        v3(K_DUSK_SHADOW, &mut out.shadow[1]);
        v3(K_NIGHT_SHADOW, &mut out.shadow[2]);
        if let Some(v) = list.0.get(&K_DUSK_DAWN).and_then(|p| p.vector4()).filter(|v| v.iter().all(|c| c.is_finite())) {
            out.dusk_dawn = v;
        }
        if let Some(v) = list.0.get(&K_NIGHT_TINT).and_then(|p| p.vector4()) {
            out.night_tint[..3].copy_from_slice(&v[..3]);
        }
        if let Some(v) = list.0.get(&K_NIGHT_LUM_BLEND).and_then(|p| p.number()) {
            out.night_tint[3] = v;
        }
        out
    }

    /// duskDawnStartEnd as PrepareShaderData 0xF98910 uploads it: (day scale,
    /// night scale, day bias, night bias).
    pub fn dusk_dawn_terms(&self) -> [f32; 4] {
        let c = self.dusk_dawn.map(|d| d.to_radians().cos());
        let day = 1.0 / (c[0] - c[1]);
        let night = 1.0 / (c[2] - c[3]);
        [day, night, -day * c[1], -night * c[3]]
    }

    /// (day, dusk, night) weights of a point whose up vector makes `ndl` with
    /// the sun (EvaluateVertexLighting).
    pub fn blend(&self, ndl: f32) -> [f32; 3] {
        let t = self.dusk_dawn_terms();
        let day = (ndl * t[0] + t[2]).clamp(0.0, 1.0);
        let night = (-ndl * t[1] + t[3]).clamp(0.0, 1.0);
        [day, 1.0 - day.max(night), night]
    }

    /// TerrainState slots 16..23 (terrain.wesl).
    pub fn slots(&self) -> [Vec4; 8] {
        let v = |c: [f32; 3]| Vec4::new(c[0], c[1], c[2], 0.0);
        [
            Vec4::from_array(self.night_tint),
            v(self.light[0]),
            v(self.light[1]),
            v(self.light[2]),
            v(self.shadow[0]),
            v(self.shadow[1]),
            v(self.shadow[2]),
            Vec4::from_array(self.dusk_dawn_terms()),
        ]
    }

    /// EvaluatePixelLighting of flat ground (normal = up) with diffuse 1 and
    /// no shadow, for a sun at `ndl` with the up vector.
    pub fn flat(&self, ndl: f32) -> [f32; 3] {
        let b = self.blend(ndl);
        let mix3 = |c: &[[f32; 3]; 3], i: usize| b[0] * c[0][i] + b[1] * c[1][i] + b[2] * c[2][i];
        // sunDir = lerp(sun, normal, 0.35 dusk): N.L' = ndl (1 - k) + k.
        let k = b[1] * 0.35;
        let n_l = ndl * (1.0 - k) + k;
        let a = n_l.abs();
        let s = a * 0.9 + n_l.clamp(0.0, 1.0) * 0.2 + (1.0 - a) * (1.0 - a) * 0.5;
        let light: [f32; 3] = std::array::from_fn(|i| {
            let shadow = mix3(&self.shadow, i);
            (shadow + (mix3(&self.light, i) - shadow) * s) * s
        });
        let lum = (light[0] + light[1] + light[2]) * 0.5;
        let blend = self.night_tint[3] * b[2];
        std::array::from_fn(|i| light[i] + (lum * self.night_tint[i] - light[i]) * blend)
    }
}

/// TerrainState of the Space planet view (`space_planets::planet_view`): the
/// retail terrain lighting only (slot 15 w; no in-scatter fog, water or sea
/// floor there yet), with the system's star at the scene origin as the sun.
/// `OPENSPORE_RETAIL_LIGHT=0` keeps the Bevy-lit stand-in.
pub fn space_view_sky(world: &mut World, centre: Vec3) -> crate::planet_sky::SkyState {
    let mut s = [Vec4::ZERO; crate::planet_sky::SKY_SLOTS];
    if std::env::var("OPENSPORE_RETAIL_LIGHT").is_ok_and(|v| v == "0") {
        return s;
    }
    if !world.contains_resource::<TerrainLighting>() {
        let lighting =
            world.get_resource::<crate::Originals>().map(|o| TerrainLighting::load(&o.resource_index)).unwrap_or_default();
        world.insert_resource(lighting);
    }
    s[0] = (-centre).normalize_or(Vec3::Y).extend(0.5);
    s[2] = centre.extend(0.0);
    s[9] = Vec4::new(0.0, 0.0, land_diffuse_scale(), 0.0);
    s[15] = Vec4::new(0.0, 0.0, 0.0, 1.0);
    s[16..24].copy_from_slice(&world.resource::<TerrainLighting>().slots());
    s
}

/// The planet lighting state's day / night terms of the model lighting
/// (lighting_properties~!planet, VERIFIED dump and loader 0x70AF40).
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectLighting {
    pub day_start: f32,
    pub day_range: f32,
    pub night_start: f32,
    pub night_range: f32,
    /// planetSunBoost / planetTransitionBoost / planetNightBoost (rgb, phase).
    pub boosts: [[f32; 4]; 3],
}

impl Default for ObjectLighting {
    fn default() -> Self {
        Self {
            day_start: 0.15,
            day_range: 0.2,
            night_start: 0.15,
            night_range: 0.25,
            boosts: [[0.45, 0.4, 0.25, 0.3], [0.7, 0.65, 0.55, 0.0], [0.7, 0.65, 0.75, -0.25]],
        }
    }
}

impl ObjectLighting {
    /// (day, transition, night) weights of 0x7061E0 for d = sun . up.
    pub fn weights(&self, d: f32) -> [f32; 3] {
        if d > 0.0 {
            let day = ((d - self.day_start) / self.day_range).clamp(0.0, 1.0);
            [day, 1.0 - day, 0.0]
        } else {
            let night = ((-d - self.night_start) / self.night_range).clamp(0.0, 1.0);
            [0.0, 1.0 - night, night]
        }
    }
}

/// The model sun of the stage at a sun direction (local frame, +Y up at the
/// stage origin): (colour factor relative to the tuned noon light, intensity
/// factor). ADAPTED (see the module comment): intensity = the retail flat
/// terrain brightness relative to noon; colour = that light's tint x the boost
/// colours by the object weights, normalised to keep the intensity.
pub fn model_light(terrain: &TerrainLighting, objects: &ObjectLighting, d: f32) -> ([f32; 3], f32) {
    let lit = terrain.flat(d);
    let noon = terrain.flat(1.0);
    let lum = |c: [f32; 3]| c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
    let ratio = (lum(lit) / lum(noon).max(1e-4)).max(0.0);
    let w = objects.weights(d);
    let boost = |k: usize| {
        let b = objects.boosts[k];
        let m = b[0].max(b[1]).max(b[2]).max(1e-4);
        [b[0] / m, b[1] / m, b[2] / m]
    };
    let warm = boost(1);
    let night = boost(2);
    let tint: [f32; 3] = std::array::from_fn(|i| {
        let terrain_tint = lit[i] / lum(lit).max(1e-4);
        terrain_tint * (w[0] + w[1] * warm[i] + w[2] * night[i])
    });
    let t = lum(tint).max(1e-4);
    (tint.map(|c| c / t), ratio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dusk_dawn_terms_match_prepare_shader_data() {
        let t = TerrainLighting::default().dusk_dawn_terms();
        let (c50, c70) = (50f32.to_radians().cos(), 70f32.to_radians().cos());
        assert!((t[0] - 1.0 / (c50 - c70)).abs() < 1e-4);
        // night scale 1 / (cos 60 - cos 90) = 2, bias -2 cos 90 = 0.
        assert!((t[1] - 2.0).abs() < 1e-4 && t[3].abs() < 1e-4);
        assert!((t[2] + c70 / (c50 - c70)).abs() < 1e-4);
    }

    #[test]
    fn blend_bands_follow_the_sun_angle() {
        let l = TerrainLighting::default();
        assert_eq!(l.blend(1.0), [1.0, 0.0, 0.0]);
        let horizon = l.blend(0.0);
        assert!(horizon[0] == 0.0 && (horizon[1] - 1.0).abs() < 1e-6 && horizon[2].abs() < 1e-6);
        let midnight = l.blend(-1.0);
        assert!(midnight[2] == 1.0 && midnight[1] == 0.0);
        // Half night when the sun is 15 deg below the horizon (N.L = -0.25).
        assert!((l.blend(-0.25)[2] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn flat_ground_is_brightest_at_noon_and_darkest_at_dusk() {
        let l = TerrainLighting::default();
        let noon = l.flat(1.0);
        // (0.85 + 0.9 x 1.1) x 1.1 = 2.024.
        assert!((noon[0] - 2.024).abs() < 1e-3);
        let dusk = l.flat(0.0)[1];
        let night = l.flat(-1.0);
        assert!(dusk < night[1] && night[1] < noon[1]);
        // The night blends 25 % toward the blue luminance.
        assert!(night[2] > night[0]);
    }

    #[test]
    fn object_weights_follow_the_lighting_state() {
        let o = ObjectLighting::default();
        assert_eq!(o.weights(1.0), [1.0, 0.0, 0.0]);
        assert_eq!(o.weights(0.1), [0.0, 1.0, 0.0]);
        let w = o.weights(0.25);
        assert!((w[0] - 0.5).abs() < 1e-5);
        let n = o.weights(-0.275);
        assert!((n[2] - 0.5).abs() < 1e-5 && (n[1] - 0.5).abs() < 1e-5);
        let (_, noon) = model_light(&TerrainLighting::default(), &o, 1.0);
        assert!((noon - 1.0).abs() < 1e-5);
    }

    #[test]
    #[ignore = "requires the installed game"]
    fn installed_planet_material_lighting_matches_the_defaults() {
        let Ok(install) = crate::spore_install::discover() else { return };
        let index = ResourceIndex::load(&install.packages);
        assert_eq!(TerrainLighting::load(&index), TerrainLighting::default());
    }
}
