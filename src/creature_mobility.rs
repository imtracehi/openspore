//! Player movement abilities for Creature stage.
//!
//! Ability values are the retail per-level tables (`creature_rules`, notes
//! C2C): sprint and stealth are timed abilities with a duration, a recharge
//! time and a refresh, a sprint adds `globalCreatureSprintSpeedBoost` to the
//! gear speed, stealth narrows the distance at which creatures notice the
//! player, jump height is capped at `creatureMaxJumpHeight` and a glide allows
//! two flaps per level. A glide use is one flap (VERIFIED, see [`CreatureMobility`]):
//! it needs a flap left, spends it, and landing gives the flaps back by glide
//! level. Gravity and what one flap does to the body are OpenSpore adaptations
//! (the physics scale of the original is not traced). The retail manual
//! documents Space to jump/flap, Shift to sprint, and Z to toggle sneak.

use bevy::prelude::*;

use crate::asset_catalog::Capabilities;
use crate::creature_rules as rules;

/// ADAPTED: world gravity of the jump arc; the retail physics scale is not
/// traced. The jump velocity is derived from the retail height instead.
pub const GRAVITY: f32 = 12.0;
/// ADAPTED: seconds of glide one retail flap gives (the flap animation and the
/// body's physics were not traced; 2 flaps per glide level, so the airtime
/// matches the earlier 1.4 s per level).
pub const GLIDE_SECONDS_PER_FLAP: f32 = 0.7;
/// ADAPTED: a flap is refused this long after take-off. Retail refuses a glide
/// use while the jump clip (0x5485A4C) or the previous flap clip (0x77300FF) is
/// still playing (0xD1EC49..0xD1EC6B); the clip lengths were not decoded.
pub const FLAP_AFTER_JUMP: f32 = 0.25;
/// ADAPTED: no retail slow-down for sneaking was found in the code read.
pub const SNEAK_SPEED_MULTIPLIER: f32 = 0.72;
const GLIDE_GRAVITY_MULTIPLIER: f32 = 0.28;
const MIN_GLIDE_FALL_SPEED: f32 = 1.4;
const MAX_GLIDE_FALL_SPEED: f32 = 2.6;

/// Runtime-only state; it is recreated on Creature stage entry, so existing
/// save files need no migration. `height` is measured from the terrain-relative
/// foot baseline and consumed by terrain grounding.
///
/// `sprint_seconds` and `stealth_seconds` are the seconds still available of
/// each timed ability; they refill while the ability is off at
/// `duration / recharge` per second (retail 0xC1AC30), and after use the
/// ability waits its `refresh` (`*_cooldown`).
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct CreatureMobility {
    pub height: f32,
    pub vertical_velocity: f32,
    pub sprint_seconds: f32,
    pub sprint_cooldown: f32,
    /// Glide seconds left: `flaps_left` x [`GLIDE_SECONDS_PER_FLAP`] plus the
    /// running flap (kept for the HUD / runtime dump; the flaps are the state).
    pub glide_seconds: f32,
    /// Flaps the avatar may still use in the air (`cCreatureGameData::
    /// mNumFlapsAllowed`, +8 of the singleton 0x169E370): a glide use needs one
    /// left and spends it (0xD1EC3F..0xD1EC80); landing sets it to the glide
    /// level's count (`AfterGlideFinish` 0xD2E580, called at 0xD441CE).
    pub flaps_left: u32,
    /// Seconds left of the flap that is running, and the wait after take-off.
    pub flap_seconds: f32,
    pub flap_wait: f32,
    pub sprinting: bool,
    pub sneaking: bool,
    pub gliding: bool,
    pub stealth_seconds: f32,
    pub stealth_cooldown: f32,
    /// Seconds before the next jump (`JUMP_REFRESH`).
    pub jump_cooldown: f32,
    /// This frame's attack was made while stealthed (damage x1.5 for bite and
    /// strike); set by `on_attack`, cleared by the next `update`.
    pub stealth_strike: bool,
}

impl Default for CreatureMobility {
    fn default() -> Self {
        Self {
            height: 0.0,
            vertical_velocity: 0.0,
            sprint_seconds: Self::sprint_capacity(5),
            sprint_cooldown: 0.0,
            glide_seconds: Self::glide_capacity(5),
            flaps_left: rules::glide_flaps(5),
            flap_seconds: 0.0,
            flap_wait: 0.0,
            sprinting: false,
            sneaking: false,
            gliding: false,
            stealth_seconds: Self::stealth_capacity(5),
            stealth_cooldown: 0.0,
            jump_cooldown: 0.0,
            stealth_strike: false,
        }
    }
}

impl CreatureMobility {
    /// Attacking ends stealth. The attack itself still counts as made from
    /// stealth (`stealth_strike`); call this after `update` in the frame of the
    /// attack key so the damage code sees it.
    pub fn on_attack(&mut self) {
        if self.sneaking {
            self.stealth_strike = true;
            self.sneaking = false;
            self.stealth_cooldown = rules::STEALTH_REFRESH;
        }
    }

    pub fn reset_for_respawn(&mut self, abilities: Capabilities) {
        self.height = 0.0;
        self.vertical_velocity = 0.0;
        self.sprinting = false;
        self.sneaking = false;
        self.gliding = false;
        self.stealth_strike = false;
        self.sprint_seconds = Self::sprint_capacity(abilities.sprint);
        self.sprint_cooldown = 0.0;
        self.glide_seconds = Self::glide_capacity(abilities.glide);
        self.flaps_left = rules::glide_flaps(abilities.glide);
        self.flap_seconds = 0.0;
        self.flap_wait = 0.0;
        self.stealth_seconds = Self::stealth_capacity(abilities.sneak);
        self.stealth_cooldown = 0.0;
        self.jump_cooldown = 0.0;
    }

    /// Sprint duration of a sprint level (retail table; 0 without the ability).
    pub fn sprint_capacity(rank: u8) -> f32 {
        rules::sprint(rank).map_or(0.0, |a| a.duration)
    }

    pub fn stealth_capacity(rank: u8) -> f32 {
        rules::stealth(rank).map_or(0.0, |a| a.duration)
    }

    pub fn glide_capacity(rank: u8) -> f32 {
        rules::glide_flaps(rank) as f32 * GLIDE_SECONDS_PER_FLAP
    }

    /// Speed added while sprinting (`globalCreatureSprintSpeedBoost`, additive).
    pub fn sprint_speed_bonus(&self) -> f32 {
        if self.sprinting {
            rules::SPRINT_SPEED_BOOST
        } else {
            0.0
        }
    }

    pub fn sneak_speed_multiplier(&self) -> f32 {
        if self.sneaking {
            SNEAK_SPEED_MULTIPLIER
        } else {
            1.0
        }
    }

    /// Passive NPC notice radius. A non-stealthed actor keeps the ordinary
    /// radius; stealth narrows it to the level's detection range (16, 12, 8, 4
    /// or 2 for levels 1 to 5), never widening it.
    pub fn npc_acquisition_radius(&self, base: f32, sneak_rank: u8) -> f32 {
        match (self.sneaking, rules::stealth_range(sneak_rank)) {
            (true, Some((range, _min))) => base.min(range),
            _ => base,
        }
    }

    /// Compact, read-only text for the shared Creature HUD.
    pub fn hud_summary(&self, abilities: Capabilities) -> String {
        let jump = if abilities.jump > 0 {
            format!("Jump {}", abilities.jump.min(5))
        } else {
            "Jump locked".into()
        };
        let glide = if abilities.glide > 0 {
            format!(
                "Glide {} {}/{} flaps",
                abilities.glide.min(5),
                self.flaps_left,
                rules::glide_flaps(abilities.glide)
            )
        } else {
            "Glide locked".into()
        };
        let sprint = if abilities.sprint > 0 {
            let state = if self.sprint_cooldown > 0.0 {
                format!("cooldown {:.0}s", self.sprint_cooldown.ceil())
            } else if self.sprinting {
                "on".into()
            } else {
                "ready".into()
            };
            format!(
                "Sprint {} {} {}/{}s",
                abilities.sprint.min(5),
                state,
                self.sprint_seconds.ceil() as u32,
                Self::sprint_capacity(abilities.sprint).ceil() as u32
            )
        } else {
            "Sprint locked".into()
        };
        let sneak = if abilities.sneak > 0 {
            format!(
                "Sneak {} {}",
                abilities.sneak.min(5),
                if self.sneaking {
                    "on".to_string()
                } else if self.stealth_cooldown > 0.0 {
                    format!("cooldown {:.0}s", self.stealth_cooldown.ceil())
                } else {
                    "off".to_string()
                }
            )
        } else {
            "Sneak locked".into()
        };
        format!("{jump} | {glide} | {sprint} | {sneak}")
    }

    /// Consume movement controls and advance vertical/ability state. A fresh
    /// jump is accepted only from the ground; holding Space in air can only
    /// glide with a positive glide rank and remaining stamina.
    pub fn update(&mut self, keys: &ButtonInput<KeyCode>, abilities: Capabilities, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        self.stealth_strike = false;
        let jump_rank = abilities.jump.min(5);
        let sprint_rank = abilities.sprint.min(5);
        let glide_rank = abilities.glide.min(5);
        let sneak_rank = abilities.sneak.min(5);
        self.sprint_seconds = self
            .sprint_seconds
            .clamp(0.0, Self::sprint_capacity(sprint_rank));
        self.flaps_left = self.flaps_left.min(rules::glide_flaps(glide_rank));
        self.stealth_seconds = self
            .stealth_seconds
            .clamp(0.0, Self::stealth_capacity(sneak_rank));

        if sprint_rank == 0 || self.sprint_seconds <= 0.0 {
            self.sprinting = false;
        }
        if sneak_rank == 0 {
            self.sneaking = false;
        }
        if keys.just_pressed(KeyCode::ShiftLeft) || keys.just_pressed(KeyCode::ShiftRight) {
            if self.sprinting {
                // Ending a sprint starts the ability's refresh wait.
                self.sprinting = false;
                self.sprint_cooldown = rules::SPRINT_REFRESH;
            } else if sprint_rank > 0 && self.sprint_seconds > 0.0 && self.sprint_cooldown <= 0.0 {
                self.sprinting = true;
            }
        }
        if keys.just_pressed(KeyCode::KeyZ) && sneak_rank > 0 {
            if self.sneaking {
                self.sneaking = false;
                self.stealth_cooldown = rules::STEALTH_REFRESH;
            } else if self.stealth_seconds > 0.0 && self.stealth_cooldown <= 0.0 {
                self.sneaking = true;
            }
        }

        let mut grounded = self.height <= 1e-4;
        if grounded {
            self.height = 0.0;
            self.vertical_velocity = 0.0;
            self.gliding = false;
            // Landing: `AfterGlideFinish` gives the flaps of the glide level back.
            self.flaps_left = rules::glide_flaps(glide_rank);
            self.flap_seconds = 0.0;
        }
        self.jump_cooldown = (self.jump_cooldown - dt).max(0.0);
        if grounded
            && jump_rank > 0
            && self.jump_cooldown <= 0.0
            && keys.just_pressed(KeyCode::Space)
        {
            self.vertical_velocity = (2.0 * GRAVITY * rules::jump_height(jump_rank)).sqrt();
            self.jump_cooldown = rules::JUMP_REFRESH;
            self.flap_wait = FLAP_AFTER_JUMP;
            self.gliding = false;
            grounded = false;
        }

        // Timed abilities: while on they use up their seconds, while off the
        // used time is regained at duration / recharge per second.
        self.sprint_cooldown = (self.sprint_cooldown - dt).max(0.0);
        if self.sprinting {
            self.sprint_seconds = (self.sprint_seconds - dt).max(0.0);
            if self.sprint_seconds <= 0.0 {
                self.sprinting = false;
                self.sprint_cooldown = rules::SPRINT_REFRESH;
            }
        } else if let Some(sprint) = rules::sprint(sprint_rank) {
            self.sprint_seconds = (self.sprint_seconds + sprint.recovery_per_second() * dt)
                .min(sprint.duration);
        }
        self.stealth_cooldown = (self.stealth_cooldown - dt).max(0.0);
        if self.sneaking {
            self.stealth_seconds = (self.stealth_seconds - dt).max(0.0);
            if self.stealth_seconds <= 0.0 {
                self.sneaking = false;
                self.stealth_cooldown = rules::STEALTH_REFRESH;
            }
        } else if let Some(stealth) = rules::stealth(sneak_rank) {
            self.stealth_seconds = (self.stealth_seconds + stealth.recovery_per_second() * dt)
                .min(stealth.duration);
        }

        if grounded {
            self.glide_seconds = self.flaps_left as f32 * GLIDE_SECONDS_PER_FLAP;
            self.height = 0.0;
            return;
        }

        // A glide use in the air is one flap: it needs a flap left and spends it
        // (VERIFIED 0xD1EC3F..0xD1EC80); a held key flaps again when the last
        // flap has run out (ADAPTED: retail needs a new press per use).
        self.flap_wait = (self.flap_wait - dt).max(0.0);
        let wants = glide_rank > 0 && keys.pressed(KeyCode::Space);
        if wants && self.flap_seconds <= 0.0 && self.flap_wait <= 0.0 && self.flaps_left > 0 {
            self.flaps_left -= 1;
            self.flap_seconds = GLIDE_SECONDS_PER_FLAP;
        }
        self.gliding = wants && self.flap_seconds > 0.0;
        if self.gliding {
            self.flap_seconds = (self.flap_seconds - dt).max(0.0);
        }
        self.glide_seconds = self.flaps_left as f32 * GLIDE_SECONDS_PER_FLAP + self.flap_seconds;
        let gravity = if self.gliding {
            GRAVITY * GLIDE_GRAVITY_MULTIPLIER
        } else {
            GRAVITY
        };
        self.vertical_velocity -= gravity * dt;
        if self.gliding {
            let rank_fraction = (glide_rank.saturating_sub(1) as f32 / 4.0).clamp(0.0, 1.0);
            let fall_limit = MAX_GLIDE_FALL_SPEED
                - (MAX_GLIDE_FALL_SPEED - MIN_GLIDE_FALL_SPEED) * rank_fraction;
            self.vertical_velocity = self.vertical_velocity.max(-fall_limit);
        }
        self.height += self.vertical_velocity * dt;
        if self.height <= 0.0 {
            self.height = 0.0;
            self.vertical_velocity = 0.0;
            self.gliding = false;
            // The flaps come back on the next pass (the grounded branch above).
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_requires_rank_and_cannot_be_retriggered_airborne() {
        let mut mobility = CreatureMobility::default();
        let mut input = ButtonInput::default();
        input.press(KeyCode::Space);
        mobility.update(&input, Capabilities::default(), 0.016);
        assert_eq!(mobility.height, 0.0);

        mobility.update(
            &input,
            Capabilities {
                jump: 2,
                ..default()
            },
            0.016,
        );
        assert!(mobility.height > 0.0);
        let initial_velocity = mobility.vertical_velocity;
        input.clear();
        input.press(KeyCode::Space);
        mobility.update(
            &input,
            Capabilities {
                jump: 5,
                ..default()
            },
            0.016,
        );
        assert!(mobility.vertical_velocity < initial_velocity);
        assert!(mobility.height > 0.0);
    }

    #[test]
    fn jump_apex_matches_the_retail_height_and_is_capped_at_3_5() {
        // A level-5 jump peaks at creatureMaxJumpHeight (3.5); lower levels less.
        let apex = |jump: u8| {
            let mut mobility = CreatureMobility::default();
            let mut input = ButtonInput::default();
            input.press(KeyCode::Space);
            let ability = Capabilities { jump, ..default() };
            mobility.update(&input, ability, 0.001);
            input.clear();
            let mut top = mobility.height;
            for _ in 0..4000 {
                mobility.update(&input, ability, 0.001);
                top = top.max(mobility.height);
                if mobility.height == 0.0 {
                    break;
                }
            }
            top
        };
        let (low, high) = (apex(1), apex(5));
        assert!((high - 3.5).abs() < 0.05, "level 5 apex {high}");
        assert!((low - 0.7).abs() < 0.05, "level 1 apex {low}");
        assert!(high <= rules::MAX_JUMP_HEIGHT + 0.05);
    }

    #[test]
    fn jumps_wait_for_the_retail_refresh() {
        let mut mobility = CreatureMobility::default();
        let ability = Capabilities {
            jump: 3,
            ..default()
        };
        let mut input = ButtonInput::default();
        input.press(KeyCode::Space);
        mobility.update(&input, ability, 0.016);
        assert!(mobility.height > 0.0);
        assert_eq!(mobility.jump_cooldown, rules::JUMP_REFRESH);
        // Land instantly and press again inside the 0.5 s refresh: no jump.
        mobility.height = 0.0;
        mobility.vertical_velocity = 0.0;
        input.release(KeyCode::Space);
        input.clear();
        input.press(KeyCode::Space);
        mobility.update(&input, ability, 0.016);
        assert_eq!(mobility.height, 0.0);
        mobility.jump_cooldown = 0.0;
        input.release(KeyCode::Space);
        input.clear();
        input.press(KeyCode::Space);
        mobility.update(&input, ability, 0.016);
        assert!(mobility.height > 0.0);
    }

    #[test]
    fn glide_brakes_fall_only_while_held_and_each_flap_is_spent() {
        let mut mobility = CreatureMobility {
            height: 3.0,
            vertical_velocity: -3.0,
            flaps_left: 1,
            ..default()
        };
        let ability = Capabilities {
            glide: 1,
            ..default()
        };
        let mut input = ButtonInput::default();
        input.press(KeyCode::Space);
        mobility.update(&input, ability, 0.016);
        assert!(mobility.gliding);
        assert_eq!(mobility.flaps_left, 0, "the use spent the one flap");
        assert!(mobility.vertical_velocity > -3.0);
        // The flap runs its seconds, then there is nothing to glide with.
        let mut seconds = 0.016;
        while mobility.gliding && seconds < 5.0 {
            mobility.update(&input, ability, 0.016);
            seconds += 0.016;
        }
        assert!((seconds - GLIDE_SECONDS_PER_FLAP).abs() < 0.05, "{seconds}");
        mobility.height = mobility.height.max(2.0);
        mobility.update(&input, ability, 0.016);
        assert!(!mobility.gliding, "no flap left: a held key does nothing");
    }

    #[test]
    fn a_flap_waits_for_the_takeoff_and_landing_gives_the_flaps_back() {
        let ability = Capabilities { jump: 3, glide: 2, ..default() };
        let mut mobility = CreatureMobility::default();
        let mut input = ButtonInput::default();
        input.press(KeyCode::Space);
        mobility.update(&input, ability, 0.016);
        assert!(mobility.height > 0.0);
        assert_eq!(mobility.flaps_left, 4, "level 2 allows 4 flaps and the jump spends none");
        assert!(!mobility.gliding, "the take-off is not a flap");
        // Held through the wait: the first flap starts, spending one of four.
        for _ in 0..30 {
            mobility.update(&input, ability, 0.016);
        }
        assert_eq!(mobility.flaps_left, 3);
        // Back on the ground the count is the glide level's again.
        input.release(KeyCode::Space);
        input.clear();
        mobility.height = 0.0;
        mobility.vertical_velocity = 0.0;
        mobility.update(&input, ability, 0.016);
        assert_eq!(mobility.flaps_left, rules::glide_flaps(2));
        assert_eq!(mobility.glide_seconds, 4.0 * GLIDE_SECONDS_PER_FLAP);
    }

    #[test]
    fn glide_stamina_is_two_flaps_per_level() {
        for level in 1..=5u8 {
            assert_eq!(rules::glide_flaps(level), 2 * level as u32);
            assert!(
                (CreatureMobility::glide_capacity(level)
                    - 2.0 * level as f32 * GLIDE_SECONDS_PER_FLAP)
                    .abs()
                    < 1e-5
            );
        }
        assert_eq!(CreatureMobility::glide_capacity(0), 0.0);
    }

    #[test]
    fn sprint_and_sneak_toggle_only_with_authored_ranks_and_sneak_hides_notice() {
        let mut mobility = CreatureMobility::default();
        mobility.sprint_seconds = CreatureMobility::sprint_capacity(2);
        let mut input = ButtonInput::default();
        input.press(KeyCode::ShiftLeft);
        input.press(KeyCode::KeyZ);
        mobility.update(
            &input,
            Capabilities {
                sprint: 2,
                sneak: 3,
                ..default()
            },
            0.0,
        );
        assert!(mobility.sprinting && mobility.sneaking);
        // The sprint boost is additive: +3.0 on top of the gear speed.
        assert_eq!(mobility.sprint_speed_bonus(), rules::SPRINT_SPEED_BOOST);
        assert!(mobility.npc_acquisition_radius(9.0, 3) < 9.0);
        mobility.on_attack();
        assert!(!mobility.sneaking, "attacking immediately breaks stealth");
        assert!(mobility.stealth_strike, "but the attack counts as a sneak attack");
        mobility.update(&ButtonInput::default(), Capabilities::default(), 0.016);
        assert!(!mobility.stealth_strike, "the flag lasts one frame");

        let mut locked = CreatureMobility::default();
        locked.update(&input, Capabilities::default(), 0.0);
        assert!(!locked.sprinting && !locked.sneaking);
        assert_eq!(locked.sprint_speed_bonus(), 0.0);
    }

    #[test]
    fn sprint_lasts_the_retail_duration_then_waits_its_refresh_and_recharges() {
        // Level 1: 10 s of sprint, 120 s to recharge fully, 25 s refresh.
        let ability = Capabilities {
            sprint: 1,
            ..default()
        };
        let mut mobility = CreatureMobility::default();
        let mut input = ButtonInput::default();
        input.press(KeyCode::ShiftLeft);
        mobility.update(&input, ability, 0.0);
        assert!(mobility.sprinting);
        assert_eq!(mobility.sprint_seconds, 10.0);
        input.clear();
        let mut elapsed: f32 = 0.0;
        while mobility.sprinting && elapsed < 30.0 {
            mobility.update(&input, ability, 0.05);
            elapsed += 0.05;
        }
        assert!((elapsed - 10.0).abs() < 0.2, "sprinted {elapsed} s");
        assert_eq!(mobility.sprint_seconds, 0.0);
        assert!((mobility.sprint_cooldown - 25.0).abs() < 0.2);
        // No restart during the refresh, even with energy.
        mobility.sprint_seconds = 5.0;
        input.press(KeyCode::ShiftLeft);
        mobility.update(&input, ability, 0.05);
        assert!(!mobility.sprinting, "refresh wait");
        // Resting regains duration / recharge = 1/12 s per second.
        input.clear();
        let mut rest = CreatureMobility {
            sprint_seconds: 0.0,
            ..default()
        };
        for _ in 0..1200 {
            rest.update(&input, ability, 0.1);
        }
        assert!((rest.sprint_seconds - 10.0).abs() < 0.01, "full after 120 s: {}", rest.sprint_seconds);
        let mut half = CreatureMobility {
            sprint_seconds: 0.0,
            ..default()
        };
        for _ in 0..600 {
            half.update(&input, ability, 0.1);
        }
        assert!((half.sprint_seconds - 5.0).abs() < 0.05, "60 s -> half: {}", half.sprint_seconds);
        // A level-5 sprint lasts 30 s and recharges in 60 s.
        assert_eq!(CreatureMobility::sprint_capacity(5), 30.0);
        assert_eq!(CreatureMobility::sprint_capacity(0), 0.0);
    }

    #[test]
    fn stopping_a_sprint_early_also_starts_the_refresh() {
        let ability = Capabilities {
            sprint: 3,
            ..default()
        };
        let mut mobility = CreatureMobility::default();
        let mut input = ButtonInput::default();
        input.press(KeyCode::ShiftLeft);
        mobility.update(&input, ability, 0.0);
        assert!(mobility.sprinting);
        input.release(KeyCode::ShiftLeft);
        input.clear();
        input.press(KeyCode::ShiftLeft);
        mobility.update(&input, ability, 0.0);
        assert!(!mobility.sprinting);
        assert_eq!(mobility.sprint_cooldown, rules::SPRINT_REFRESH);
    }

    #[test]
    fn stealth_is_a_timed_ability_that_narrows_the_notice_radius() {
        let ability = Capabilities {
            sneak: 1,
            ..default()
        };
        let mut mobility = CreatureMobility::default();
        let mut input = ButtonInput::default();
        input.press(KeyCode::KeyZ);
        mobility.update(&input, ability, 0.0);
        assert!(mobility.sneaking);
        assert_eq!(mobility.stealth_seconds, 10.0, "level 1 stealth lasts 10 s");
        input.clear();
        let mut elapsed: f32 = 0.0;
        while mobility.sneaking && elapsed < 30.0 {
            mobility.update(&input, ability, 0.05);
            elapsed += 0.05;
        }
        assert!((elapsed - 10.0).abs() < 0.2, "stealth lasted {elapsed} s");
        assert!(mobility.stealth_cooldown > 24.0);
        // Detection range per level: 16, 12, 8, 4, 2 (never widening the radius).
        for (level, range) in [(1u8, 16.0f32), (2, 12.0), (3, 8.0), (4, 4.0), (5, 2.0)] {
            let sneaking = CreatureMobility {
                sneaking: true,
                ..default()
            };
            assert_eq!(sneaking.npc_acquisition_radius(20.0, level), range);
            assert_eq!(sneaking.npc_acquisition_radius(1.0, level), 1.0);
            assert_eq!(CreatureMobility::default().npc_acquisition_radius(20.0, level), 20.0);
        }
        assert_eq!(CreatureMobility::stealth_capacity(5), 20.0);
    }

    #[test]
    fn exhausting_sprint_stops_it_and_enforces_a_cooldown() {
        let mut mobility = CreatureMobility {
            sprinting: true,
            sprint_seconds: 0.01,
            ..default()
        };
        mobility.update(
            &ButtonInput::default(),
            Capabilities {
                sprint: 1,
                ..default()
            },
            0.02,
        );
        assert!(!mobility.sprinting);
        assert!(mobility.sprint_cooldown > 0.0);
        assert_eq!(mobility.sprint_seconds, 0.0);
    }

    #[test]
    fn malformed_delta_cannot_create_vertical_motion() {
        let mut mobility = CreatureMobility::default();
        mobility.update(
            &ButtonInput::default(),
            Capabilities {
                jump: 4,
                ..default()
            },
            f32::NAN,
        );
        assert_eq!(mobility.height, 0.0);
        assert_eq!(mobility.vertical_velocity, 0.0);
    }
}
