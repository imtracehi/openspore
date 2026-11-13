//! Sound components (effdir component type 0x05) of running effects.
//!
//! RETAIL data (`SoundEffect`, 28 bytes): `flags` 3D 1 / loop 2 / volume 4 /
//! stop-with-effect 8 / track 0x10, the soundProp or SNR id, the track id, the
//! location update period `invSample` (default 0.05 s: the `sample` /
//! `locationUpdateRate` option is a rate in Hz), a length and the volume. A
//! sound component is a block of its effect like any other: it starts with the
//! effect instance that owns it, so the effects reached through sequences, meta
//! particles and selection groups start (or skip) their sounds exactly when the
//! simulation instantiates them.
//!
//! The simulation (`sim.rs`) only reports: every update, each live sound child
//! pushes a [`LiveSound`] (world position of its block frame) into
//! [`SwarmSounds`]. Playing, attenuating and stopping them is the job of the
//! audio side (`effect_audio.rs`), which diffs the list from frame to frame: a
//! voice that disappears from the list has lost its effect instance.

use bevy::prelude::*;

/// One live sound component this frame.
#[derive(Clone, Copy, Debug)]
pub struct LiveSound {
    /// The root effect entity (not the nested instance).
    pub effect: Entity,
    /// Unique per sound child (stable while the child lives).
    pub voice: u64,
    /// `sound_flag` bits of the component.
    pub flags: u32,
    /// soundProp / SNR instance id.
    pub sound: u32,
    /// Component volume (1 unless `sound_flag::VOLUME`).
    pub volume: f32,
    /// Seconds between location updates of a 3D sound.
    pub period: f32,
    /// World position of the component's block frame.
    pub origin: Vec3,
    /// The owning instance is stopping (`SwarmEffect::stop`, a parent's stop).
    pub stopped: bool,
}

/// The sound components that were live in the latest simulation step.
#[derive(Resource, Default)]
pub struct SwarmSounds {
    pub live: Vec<LiveSound>,
}

impl SwarmSounds {
    /// Report one live sound (a pre-rolled instance reports it many times in a
    /// step: the last report wins).
    pub fn report(&mut self, sound: LiveSound) {
        match self.live.iter_mut().rev().take(64).find(|s| s.voice == sound.voice) {
            Some(slot) => *slot = sound,
            None => self.live.push(sound),
        }
    }
}

#[cfg(test)]
mod installed;

/// Default location update period of a 3D sound component (`invSample`).
pub const DEFAULT_PERIOD: f32 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_voice_is_reported_once_per_step() {
        let mut s = SwarmSounds::default();
        let base = LiveSound {
            effect: Entity::PLACEHOLDER,
            voice: 7,
            flags: 1,
            sound: 0xABCD,
            volume: 1.0,
            period: DEFAULT_PERIOD,
            origin: Vec3::ZERO,
            stopped: false,
        };
        s.report(base);
        s.report(LiveSound { origin: Vec3::X, ..base });
        s.report(LiveSound { voice: 8, ..base });
        assert_eq!(s.live.len(), 2);
        assert_eq!(s.live[0].origin, Vec3::X, "the later report replaces the earlier one");
    }
}
