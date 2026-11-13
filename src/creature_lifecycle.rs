//! Player nest, recovery and generations. The flow mirrors Creature-stage
//! return/rest/mate play; radii and cooldowns are OpenSpore tuning, not yet
//! measured against the original executable. Resting at the nest heals at the
//! retail healing rate (`creatureHealingHealthRecovery`, a fraction of the
//! maximum HP per second, see `creature_rules`).

use serde::{Deserialize, Serialize};

pub const HOME_RADIUS: f32 = 3.0;
/// Fraction of the maximum HP restored per second of rest at the nest.
pub const REST_HEALTH_FRACTION_PER_SECOND: f32 = crate::creature_rules::HEALING_HEALTH_RECOVERY;
pub const MATING_COOLDOWN: f32 = 8.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CreatureLifecycle {
    pub home: [f32; 3],
    pub generation: u32,
    pub deaths: u32,
    pub mating_cooldown: f32,
    pub mating: bool,
    pub resting: bool,
    pub mating_call_active: bool,
    pub mating_call_timer: f32,
    pub egg_incubating: bool,
    pub egg_timer: f32,
}

impl Default for CreatureLifecycle {
    fn default() -> Self {
        Self {
            home: [0.0, 0.7, 8.0],
            generation: 1,
            deaths: 0,
            mating_cooldown: 0.0,
            mating: false,
            resting: false,
            mating_call_active: false,
            mating_call_timer: 0.0,
            egg_incubating: false,
            egg_timer: 0.0,
        }
    }
}

impl CreatureLifecycle {
    pub fn at_home(&self, position: [f32; 3]) -> bool {
        if !position
            .iter()
            .chain(self.home.iter())
            .all(|v| v.is_finite())
        {
            return false;
        }
        let dx = position[0] - self.home[0];
        let dz = position[2] - self.home[2];
        // On the planet a home beyond the limb shares x, z with a spot in
        // front of it, not its height.
        dx * dx + dz * dz <= HOME_RADIUS * HOME_RADIUS
            && crate::creature_planet::same_side(position, self.home)
    }

    /// Emit an acoustic mating call that travels toward the home nest.
    pub fn emit_mating_call(&mut self, threatened: bool) -> Result<(), &'static str> {
        if threatened {
            return Err("Escape combat before calling for a mate");
        }
        self.mating_call_active = true;
        self.mating_call_timer = 12.0;
        Ok(())
    }

    /// Tick with simulation time only; returns the FRACTION of the maximum HP
    /// restored this tick. Movement, an active hostile encounter or leaving
    /// home immediately stops rest. Hunger is never refilled by rest, so a
    /// starving animal must still find food.
    pub fn tick(&mut self, dt: f32, position: [f32; 3], moving: bool, threatened: bool) -> f32 {
        if !dt.is_finite() || dt <= 0.0 {
            return 0.0;
        }
        self.mating_cooldown = (self.mating_cooldown - dt).max(0.0);
        if self.mating_call_active {
            self.mating_call_timer = (self.mating_call_timer - dt).max(0.0);
            if self.mating_call_timer <= 0.0 || self.at_home(position) {
                self.mating_call_active = false;
            }
        }
        if self.egg_incubating {
            self.egg_timer = (self.egg_timer - dt).max(0.0);
        }
        if moving || threatened || !self.at_home(position) || self.mating {
            self.resting = false;
        }
        if self.resting {
            REST_HEALTH_FRACTION_PER_SECOND * dt
        } else {
            0.0
        }
    }

    pub fn start_rest(&mut self, position: [f32; 3], threatened: bool) -> Result<(), &'static str> {
        if !self.at_home(position) {
            return Err("Return to your home nest to rest");
        }
        if threatened {
            return Err("Escape combat before resting");
        }
        if self.mating {
            return Err("Finish mating before resting");
        }
        self.resting = true;
        Ok(())
    }

    /// This is called before opening the Creature editor. Finishing the
    /// editor calls finish_mating once; repeated key presses cannot create
    /// extra generations or refresh the cooldown.
    pub fn begin_mating(
        &mut self,
        position: [f32; 3],
        threatened: bool,
        health_fraction: f32,
    ) -> Result<(), &'static str> {
        if !self.at_home(position) {
            return Err("Return to your home nest to mate and evolve");
        }
        if threatened {
            return Err("Escape combat before mating");
        }
        if !health_fraction.is_finite() || health_fraction <= 0.25 {
            return Err("Rest at your home nest before mating");
        }
        if self.mating_cooldown > 0.0 {
            return Err("Your mate is recovering; wait before mating again");
        }
        if self.mating {
            return Err("Mating is already in progress");
        }
        self.resting = false;
        self.mating = true;
        self.egg_incubating = true;
        self.egg_timer = 2.5;
        Ok(())
    }

    pub fn finish_mating(&mut self) -> bool {
        if !self.mating {
            return false;
        }
        self.mating = false;
        self.egg_incubating = false;
        self.generation = self.generation.saturating_add(1);
        self.mating_cooldown = MATING_COOLDOWN;
        true
    }

    /// Preserve the lineage/build, ecology and DNA. The stage caller handles
    /// health/hunger and moves the new player to this home position.
    pub fn respawn(&mut self) -> [f32; 3] {
        self.deaths = self.deaths.saturating_add(1);
        self.mating = false;
        self.resting = false;
        self.mating_call_active = false;
        self.egg_incubating = false;
        self.home
    }

    pub fn hud(&self, position: [f32; 3]) -> String {
        let dx = position[0] - self.home[0];
        let dz = position[2] - self.home[2];
        if self.at_home(position) {
            format!(
                "Home nest | H rest | M mate | generation {}{}",
                self.generation,
                if self.resting { " | resting" } else { "" }
            )
        } else if self.mating_call_active {
            format!(
                "Mate answered call! Return to nest {:.0}m | generation {}",
                (dx * dx + dz * dz).sqrt(),
                self.generation
            )
        } else {
            format!(
                "Home nest {:.0}m | return to rest and mate | generation {}",
                (dx * dx + dz * dz).sqrt(),
                self.generation
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_requires_home_and_safety_and_stops_on_movement() {
        let mut life = CreatureLifecycle::default();
        assert!(life.start_rest([10.0, 0.7, 8.0], false).is_err());
        assert!(life.start_rest(life.home, true).is_err());
        life.start_rest(life.home, false).unwrap();
        // Retail healing rate: 7.5% of the maximum HP per second.
        assert!((life.tick(0.5, life.home, false, false) - 0.0375).abs() < 1e-6);
        assert_eq!(life.tick(0.5, life.home, true, false), 0.0);
        assert!(!life.resting);
        assert_eq!(life.tick(f32::NAN, life.home, false, false), 0.0);
    }

    #[test]
    fn mating_is_a_single_generation_and_has_a_simulation_cooldown() {
        let mut life = CreatureLifecycle::default();
        assert!(life.begin_mating([20.0, 0.7, 8.0], false, 1.0).is_err());
        assert!(life.begin_mating(life.home, true, 1.0).is_err());
        assert!(life.begin_mating(life.home, false, 0.25).is_err());
        life.begin_mating(life.home, false, 1.0).unwrap();
        assert!(life.begin_mating(life.home, false, 1.0).is_err());
        assert!(life.finish_mating());
        assert!(!life.finish_mating());
        assert_eq!(life.generation, 2);
        assert!(life.begin_mating(life.home, false, 1.0).is_err());
        life.tick(MATING_COOLDOWN, life.home, false, false);
        assert!(life.begin_mating(life.home, false, 1.0).is_ok());
    }

    #[test]
    fn old_saves_load_and_respawn_preserves_home_and_generation() {
        let mut life: CreatureLifecycle = serde_json::from_str("{}").unwrap();
        assert_eq!(life, CreatureLifecycle::default());
        life.generation = 7;
        life.resting = true;
        assert_eq!(life.respawn(), [0.0, 0.7, 8.0]);
        assert_eq!((life.deaths, life.generation), (1, 7));
        assert!(!life.resting && !life.mating);
        let saved = serde_json::to_string(&life).unwrap();
        assert_eq!(
            serde_json::from_str::<CreatureLifecycle>(&saved).unwrap(),
            life
        );
    }

    #[test]
    fn mating_call_activates_and_egg_incubation_progresses() {
        let mut life = CreatureLifecycle::default();
        assert!(!life.mating_call_active);
        assert!(life.emit_mating_call(true).is_err());
        assert!(life.emit_mating_call(false).is_ok());
        assert!(life.mating_call_active);
        assert_eq!(life.mating_call_timer, 12.0);

        let hud_call = life.hud([15.0, 0.0, 20.0]);
        assert!(hud_call.contains("Mate answered call!"));

        // Ticking time away from home decrements timer
        life.tick(5.0, [15.0, 0.0, 20.0], false, false);
        assert!(life.mating_call_active);
        assert_eq!(life.mating_call_timer, 7.0);

        // Arriving home clears call
        life.tick(1.0, life.home, false, false);
        assert!(!life.mating_call_active);

        // Mating sets egg_incubating
        assert!(!life.egg_incubating);
        life.begin_mating(life.home, false, 1.0).unwrap();
        assert!(life.egg_incubating);
        assert_eq!(life.egg_timer, 2.5);

        life.finish_mating();
        assert!(!life.egg_incubating);
    }
}
