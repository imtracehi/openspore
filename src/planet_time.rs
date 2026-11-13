//! Planet time of day (`notes/planet-re-notes.md` 1F, 3A, 3B): day length,
//! phases, the sun as seen from a point of the spinning planet, and the
//! day / dusk / night light weights of the terrain lighting.
//!
//! Original data (VERIFIED): `animations~!GlobalTuning` defaultDayLength
//! 960 s and the per-stage hourly speed tables (Creature = Tribe, CityCiv =
//! Space); `cTimeOfDay::SetDayDuration` 0xBC29A0 phase boundaries dawn 0.208,
//! day 0.297, dusk 0.708, night 0.792; SetTimeOfDay 0xBC2DC0: local time 0.5
//! = noon with the sun overhead; Creature starts at creatureStartTimeOfDay 7.0;
//! PlanetMaterial kDuskDawnStartEnd (50, 70, 60, 90 deg) and the day / dusk /
//! night light colours (kDayLightColor 1.75, kDuskLightColor 1.7,
//! kNightLightColor 1.75 grey, kNightLightingTint (0.43, 0.49, 1.0),
//! kNightLumBlend 0.25).
//! ADAPTED: the stage origin is treated as equatorial with no axial tilt (the
//! planet's spin axis relative to the origin is not traced), so the sun climbs
//! 15 degrees per hour from 06:00 to noon; the sun's azimuth plane is the
//! preview's authored one; the night keeps a dim bluish light from a moon-like
//! direction instead of the undecoded night terrain program.
use bevy::math::Vec3;

/// `defaultDayLength` (seconds per full day at multiplier 1).
pub const DAY_LENGTH: f32 = 960.0;
/// Phase boundaries as fractions of the day (0xBC29A0).
pub const DAWN: f32 = 0.208;
pub const DAY: f32 = 0.297;
pub const DUSK: f32 = 0.708;
pub const NIGHT: f32 = 0.792;
/// creatureStartTimeOfDay (hours).
pub const CREATURE_START_HOUR: f32 = 7.0;
/// kDuskDawnStartEnd (degrees from the local zenith).
pub const DAY_END_DEG: f32 = 50.0;
pub const DUSK_END_DEG: f32 = 70.0;
pub const NIGHT_START_DEG: f32 = 60.0;
pub const NIGHT_FULL_DEG: f32 = 90.0;
pub const NIGHT_TINT: [f32; 3] = [0.43, 0.49, 1.0];
pub const NIGHT_LUM_BLEND: f32 = 0.25;

/// Hourly clock multipliers (index = hour 0..=24).
pub const CREATURE_TRIBE_HOURLY: [f32; 25] = [
    0.25, 0.75, 2.0, 3.0, 8.0, 6.0, 4.0, 2.0, 1.0, 0.75, 0.75, 0.25, 0.25, 0.25, 0.75, 0.75, 1.0,
    2.0, 4.0, 6.0, 8.0, 3.0, 2.0, 0.75, 0.25,
];
pub const CIV_SPACE_HOURLY: [f32; 25] = [
    0.25, 0.5, 1.0, 1.0, 2.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.25, 0.25, 0.25, 0.5, 0.5, 1.0, 1.0,
    1.0, 1.0, 2.0, 1.0, 1.0, 0.5, 0.25,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Night,
    Dawn,
    Day,
    Dusk,
}

/// Phase of a day fraction (0..1, 0.5 = noon).
pub fn phase(fraction: f32) -> Phase {
    let f = fraction.rem_euclid(1.0);
    if f < DAWN || f >= NIGHT {
        Phase::Night
    } else if f < DAY {
        Phase::Dawn
    } else if f < DUSK {
        Phase::Day
    } else {
        Phase::Dusk
    }
}

/// Sun elevation (radians) at a local hour: -90 deg at midnight, 0 at 06:00
/// and 18:00, 90 at noon (ADAPTED equatorial origin).
pub fn sun_elevation(hour: f32) -> f32 {
    let h = hour.rem_euclid(24.0);
    (90.0 - (h - 12.0).abs() * 15.0).to_radians()
}

/// Direction toward the sun in a local frame with +Y = up; the sun rises on
/// the `azimuth` side (radians from +Z toward +X) and sets opposite.
pub fn sun_direction(hour: f32, azimuth: f32) -> Vec3 {
    let e = sun_elevation(hour);
    let h = hour.rem_euclid(24.0);
    // Morning on the azimuth side, afternoon on the far side.
    let side = if h < 12.0 { 1.0 } else { -1.0 };
    let horizontal = Vec3::new(azimuth.sin(), 0.0, azimuth.cos()) * side;
    (horizontal * e.cos().abs() + Vec3::Y * e.sin()).normalize_or(Vec3::Y)
}

/// (day, dusk, night) weights from the cosine of the sun's zenith angle,
/// after kDuskDawnStartEnd: full day within 50 deg of the zenith, dusk to
/// 70 deg, night fading in from 60 to 90 deg. Sums to 1.
pub fn light_weights(cos_zenith: f32) -> (f32, f32, f32) {
    let ramp = |x: f32, a: f32, b: f32| ((x - a) / (b - a)).clamp(0.0, 1.0);
    let day = ramp(
        cos_zenith,
        DUSK_END_DEG.to_radians().cos(),
        DAY_END_DEG.to_radians().cos(),
    );
    let night = 1.0
        - ramp(
            cos_zenith,
            NIGHT_FULL_DEG.to_radians().cos(),
            NIGHT_START_DEG.to_radians().cos(),
        );
    let night = night.min(1.0 - day);
    (day, (1.0 - day - night).max(0.0), night)
}

/// Light colour multiplier (relative to day = 1) for the weights: dusk is
/// warm at 1.7/1.75, night is the grey 1.75 light blended toward the night
/// tint by kNightLumBlend and dimmed (ADAPTED, the night program is not
/// decoded).
pub fn light_color(weights: (f32, f32, f32)) -> [f32; 3] {
    let (day, dusk, night) = weights;
    let dusk_c = [1.7 / 1.75, 0.8 * 1.7 / 1.75, 0.62 * 1.7 / 1.75];
    let night_c = NIGHT_TINT.map(|t| (1.0 - NIGHT_LUM_BLEND + NIGHT_LUM_BLEND * t) * t * 0.55);
    [0, 1, 2].map(|i| day + dusk * dusk_c[i] + night * night_c[i])
}

/// The light direction used for shading: the sun while it is up (at least a
/// few degrees above the horizon), else a moon-like light opposite it.
pub fn shading_direction(hour: f32, azimuth: f32) -> Vec3 {
    let sun = sun_direction(hour, azimuth);
    let min = 8f32.to_radians().sin();
    if sun.y >= min {
        sun
    } else if sun.y > -min {
        Vec3::new(sun.x, min, sun.z).normalize()
    } else {
        // Moon: the antipode of the sun, kept above 20 degrees.
        let moon = -sun;
        let y = moon.y.max(20f32.to_radians().sin());
        Vec3::new(moon.x, y, moon.z).normalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_follow_set_day_duration() {
        assert_eq!(phase(0.0), Phase::Night);
        assert_eq!(phase(5.5 / 24.0), Phase::Dawn);
        assert_eq!(phase(0.5), Phase::Day);
        assert_eq!(phase(17.5 / 24.0), Phase::Dusk);
        assert_eq!(phase(20.0 / 24.0), Phase::Night);
        assert_eq!(phase(CREATURE_START_HOUR / 24.0), Phase::Dawn);
    }

    #[test]
    fn the_sun_climbs_to_noon_and_sets() {
        assert!(sun_elevation(12.0) > 89f32.to_radians());
        assert!(sun_elevation(6.0).abs() < 1e-5 && sun_elevation(18.0).abs() < 1e-5);
        assert!(sun_elevation(0.0) < -89f32.to_radians());
        let morning = sun_direction(9.0, 0.3);
        let evening = sun_direction(15.0, 0.3);
        assert!((morning.y - evening.y).abs() < 1e-5);
        assert!(morning.x * evening.x < 0.0, "rises and sets on opposite sides");
        for h in [0.0, 3.0, 6.5, 12.0, 19.0, 23.9] {
            let d = shading_direction(h, 0.3);
            assert!((d.length() - 1.0).abs() < 1e-4 && d.y > 0.1, "{h}: {d}");
        }
    }

    #[test]
    fn light_weights_blend_day_dusk_night() {
        for deg in [0.0f32, 30.0, 50.0, 60.0, 65.0, 70.0, 80.0, 90.0, 120.0] {
            let (d, k, n) = light_weights(deg.to_radians().cos());
            assert!((d + k + n - 1.0).abs() < 1e-5 && d >= 0.0 && k >= 0.0 && n >= 0.0, "{deg}");
        }
        assert_eq!(light_weights(1.0), (1.0, 0.0, 0.0));
        let (_, _, night) = light_weights(-0.5);
        assert_eq!(night, 1.0);
        let c = light_color(light_weights(-1.0));
        assert!(c[2] > c[0], "night is blue");
        assert_eq!(light_color((1.0, 0.0, 0.0)), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn hourly_tables_match_global_tuning() {
        assert_eq!(CREATURE_TRIBE_HOURLY[4], 8.0);
        assert_eq!(CIV_SPACE_HOURLY[20], 2.0);
        assert_eq!(CREATURE_TRIBE_HOURLY.len(), 25);
    }
}
