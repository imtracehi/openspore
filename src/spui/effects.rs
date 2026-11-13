//! Visibility effects of SPUI windows: `Fade`, `Inflate` and `Glide`.
//!
//! These window procedures (`BiStateEffect` family, trigger 1 = "on") are the
//! pop-in/pop-out animations of retail panels and callouts: while a window is
//! shown it fades from transparent to opaque, inflates from `Scale` to full
//! size about its centre and glides in from `Offset` away from its place; all
//! three run backwards when it is hidden. `Time` is the duration of each effect.
//!
//! VERIFIED (retail `UTFWin::BiStateEffect`, march2017 addresses): the effect
//! keeps a state `p` that moves at `1 / Time` per second towards its target
//! (the update, message 0xC of `HandleUIMessage` 0x9636A0, switch case at
//! 0x963927) and hands `envelope(p)` to the effect's `func84h(window, value)`
//! (Fade 0x96EFE0 sets the shade alpha to `(1 - value) * 255`, so `p` is 1
//! while the window is hidden and runs down to 0 as it appears). The
//! envelope is selected by `InterpolationType` (`Effect.h`): 0 Linear and
//! 2 DampedSpring pass through 0x963410 unchanged (the spring is stepped by
//! the update instead, below), 1 EaseInEaseOut is the trapezoid velocity
//! profile of 0x963410 with `SetEase` (0x963610) clamping both ease fractions
//! to 0..1 and scaling them to a sum of at most 1:
//! `k = 1 / (2 - in - out)`; `t < in`: `k t^2 / in`; `t < 1 - out`:
//! `k (2 t - in)`; else `1 - k (1 - t)^2 / out`. DampedSpring (0x963948 ..
//! 0x963AC0): the frame time is clamped to 0.02 .. 0.2 s and split into steps
//! of at most 0.02 s; each step is the Verlet update
//! `x' = x + c (x - prev) - 100 (x - target) h^2 / Time` with
//! `c = max(0.1, 1 - Damping * dt)`, finishing when |x - prev| + |x - target|
//! < 0.001 (OpenSpore steps it at the 0.02 s tick of a 60 Hz frame).
//!
//! ADAPTED: the other effect classes (Rotate, Modulate, Material, Cascade, ...)
//! are not executed here, and a damped spring is cut off at `Time` (the
//! layout's visibility clock stops there; retail lets it ring until it settles).

use super::model::{EffectKind, Layout, Window, WinProc};

/// Trigger value of an effect that follows the window's visibility.
const TRIGGER_ON: u32 = 1;

/// `InterpolationType` of `UTFWin/Effect.h`.
const EASE_IN_EASE_OUT: u32 = 1;
const DAMPED_SPRING: u32 = 2;

/// Step of the spring simulation (0x1440E2C = 0.02 s) and the clamp of the
/// frame time handed to it (0x13EC45C = 0.2 s, 0x1440E2C).
const SPRING_STEP: f32 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Timing {
    time: f32,
    interpolation: u32,
    ease_in: f32,
    ease_out: f32,
    damping: f32,
}

impl Timing {
    /// `BiStateEffect::SetEase` (0x963610): both fractions are clamped to
    /// 0..1 and scaled down to a sum of at most 1.
    fn ease(&self) -> (f32, f32) {
        let (a, b) = (self.ease_in.clamp(0.0, 1.0), self.ease_out.clamp(0.0, 1.0));
        let sum = a + b;
        if sum > 1.0 {
            (a / sum, b / sum)
        } else {
            (a, b)
        }
    }

    /// The envelope of the effect state `t` (0x963410): the value the effect
    /// applies for a state `t` between 0 and 1.
    fn envelope(&self, t: f32) -> f32 {
        if self.interpolation != EASE_IN_EASE_OUT || t == 0.0 || t == 1.0 {
            return t;
        }
        let (ease_in, ease_out) = self.ease();
        if ease_in + ease_out == 0.0 {
            return t;
        }
        let k = 1.0 / (2.0 - ease_in - ease_out);
        if ease_in > t {
            k / ease_in * t * t
        } else if 1.0 - ease_out > t {
            (2.0 * t - ease_in) * k
        } else {
            1.0 - k / ease_out * (1.0 - t) * (1.0 - t)
        }
    }

    /// The damped spring (0x963948 ..): the state after `elapsed` seconds of
    /// moving from 0 to 1, stepped at the frame tick.
    fn spring(&self, elapsed: f32) -> f32 {
        if self.time < 1.0e-3 {
            return 1.0;
        }
        let k = 1.0 / self.time;
        let c = (1.0 - self.damping * SPRING_STEP).max(0.1);
        let (mut x, mut prev) = (0.0f32, 0.0f32);
        let mut left = elapsed.max(0.0);
        while left > 0.0 {
            let h = left.min(SPRING_STEP);
            let next = x + (x - prev) * c - 100.0 * (x - 1.0) * k * h * h;
            prev = x;
            x = next;
            left -= h;
            if (x - prev).abs() + (x - 1.0).abs() < 1.0e-3 {
                return 1.0;
            }
        }
        x
    }

    /// Progress after `elapsed` seconds of the window being shown: 0 hidden,
    /// 1 fully shown. Retail's state `p` runs 1 -> 0 while the window appears
    /// and the effect applies `1 - envelope(p)`.
    fn progress(&self, elapsed: f32) -> f32 {
        let t = (elapsed / self.time.max(1e-4)).clamp(0.0, 1.0);
        if self.interpolation == DAMPED_SPRING {
            // ADAPTED: the layout's visibility clock stops at `Time`, retail's
            // spring keeps ringing until it settles; the window rests at `Time`.
            return if elapsed >= self.time { 1.0 } else { self.spring(elapsed) };
        }
        1.0 - self.envelope(1.0 - t)
    }
}

/// The visibility effects of one window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowEffects {
    fade: Option<Timing>,
    inflate: Option<(f32, Timing)>,
    glide: Option<([f32; 2], Timing)>,
    /// Longest effect time: the window counts as fully shown after this long.
    pub duration: f32,
}

/// How a window looks at some point of its effects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Multiplies the window's colour alpha (children inherit it).
    pub alpha: f32,
    /// Scale about the window centre (children scale with it).
    pub scale: f32,
    /// Displacement from the window's place in pixels.
    pub offset: [f32; 2],
}

impl Pose {
    pub const REST: Pose = Pose {
        alpha: 1.0,
        scale: 1.0,
        offset: [0.0, 0.0],
    };
}

impl WindowEffects {
    /// The visibility effects declared by a window, if any.
    pub fn of(window: &Window) -> Option<Self> {
        let mut out = WindowEffects::default();
        for proc in &window.procs {
            let WinProc::Effect { bi, kind } = proc else {
                continue;
            };
            if bi.trigger != TRIGGER_ON || bi.time <= 0.0 {
                continue;
            }
            let timing = Timing {
                time: bi.time,
                interpolation: bi.interpolation,
                ease_in: bi.ease_in,
                ease_out: bi.ease_out,
                damping: bi.damping,
            };
            match kind {
                EffectKind::Fade if out.fade.is_none() => out.fade = Some(timing),
                EffectKind::Inflate { scale } if out.inflate.is_none() => out.inflate = Some((*scale, timing)),
                EffectKind::Glide { offset } if out.glide.is_none() => out.glide = Some((*offset, timing)),
                _ => continue,
            }
            out.duration = out.duration.max(bi.time);
        }
        (out.duration > 0.0).then_some(out)
    }

    /// Pose after the window has been on for `elapsed` seconds (`0` = hidden).
    pub fn pose(&self, elapsed: f32) -> Pose {
        let mut pose = Pose::REST;
        if let Some(t) = &self.fade {
            pose.alpha = t.progress(elapsed);
        }
        if let Some((from, t)) = &self.inflate {
            pose.scale = from + (1.0 - from) * t.progress(elapsed);
        }
        if let Some((offset, t)) = &self.glide {
            let rest = 1.0 - t.progress(elapsed);
            pose.offset = [offset[0] * rest, offset[1] * rest];
        }
        pose
    }
}

/// Effects of every window of a layout (index = window id).
pub fn collect(layout: &Layout) -> Vec<Option<WindowEffects>> {
    layout.windows.iter().map(WindowEffects::of).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spui::model::BiState;

    fn effect(kind: EffectKind, time: f32, trigger: u32) -> WinProc {
        WinProc::Effect {
            bi: BiState {
                time,
                trigger,
                interpolation: 0,
                ease_in: 0.0,
                ease_out: 0.0,
                damping: 0.0,
            },
            kind,
        }
    }

    fn window(procs: Vec<WinProc>) -> Window {
        Window {
            id: 0,
            element: 0,
            class: 0,
            kind: crate::spui::model::WindowKind::Window,
            parent: None,
            children: vec![],
            flags: 3,
            control_id: 0,
            command_id: 0,
            area: [0.0; 4],
            caption: None,
            fill_color: 0,
            shade_color: 0xFFFF_FFFF,
            text_style: 0,
            drawable: None,
            procs,
            extra: Default::default(),
        }
    }

    #[test]
    fn callout_pop_in_fades_and_inflates() {
        // The retail callout root: Fade 0.2 s and Inflate (scale 0.1) 0.3 s.
        let w = window(vec![
            effect(EffectKind::Fade, 0.2, 1),
            effect(EffectKind::Inflate { scale: 0.1 }, 0.3, 1),
        ]);
        let fx = WindowEffects::of(&w).expect("effects");
        assert!((fx.duration - 0.3).abs() < 1e-6);
        assert_eq!(fx.pose(0.0).alpha, 0.0);
        assert!((fx.pose(0.0).scale - 0.1).abs() < 1e-6);
        let mid = fx.pose(0.1);
        assert!((mid.alpha - 0.5).abs() < 1e-6);
        assert!(mid.scale > 0.1 && mid.scale < 1.0);
        assert_eq!(fx.pose(0.3), Pose::REST);
        assert_eq!(fx.pose(5.0), Pose::REST);
    }

    #[test]
    fn glide_starts_displaced_and_ends_at_rest() {
        let w = window(vec![effect(EffectKind::Glide { offset: [0.0, 100.0] }, 0.4, 1)]);
        let fx = WindowEffects::of(&w).unwrap();
        assert_eq!(fx.pose(0.0).offset, [0.0, 100.0]);
        assert!((fx.pose(0.2).offset[1] - 50.0).abs() < 1e-4);
        assert_eq!(fx.pose(0.4).offset, [0.0, 0.0]);
    }

    #[test]
    fn other_triggers_and_effect_kinds_are_not_visibility_effects() {
        let w = window(vec![
            effect(EffectKind::Fade, 0.3, 2),
            effect(EffectKind::Rotate { axis: [0.0, 1.0, 0.0], angle: 180.0 }, 0.0, 1),
            effect(EffectKind::Fade, 0.0, 1),
        ]);
        assert!(WindowEffects::of(&w).is_none());
    }

    fn timing(interpolation: u32, ease_in: f32, ease_out: f32) -> Timing {
        Timing { time: 1.0, interpolation, ease_in, ease_out, damping: 0.1 }
    }

    #[test]
    fn linear_and_ease_envelopes_follow_the_retail_formula() {
        // Linear passes the state through.
        assert_eq!(timing(0, 0.3, 0.3).envelope(0.4), 0.4);
        // Ease: a trapezoid velocity profile, continuous at its joints.
        let e = timing(1, 0.25, 0.25);
        let k = 1.0 / 1.5;
        assert!((e.envelope(0.125) - k * 0.125 * 0.125 / 0.25).abs() < 1e-6);
        assert!((e.envelope(0.5) - (2.0 * 0.5 - 0.25) * k).abs() < 1e-6);
        assert!((e.envelope(0.25 - 1e-4) - e.envelope(0.25 + 1e-4)).abs() < 1e-3);
        assert!((e.envelope(0.5) - 0.5).abs() < 1e-6, "symmetric ease ends at the middle");
        assert!((e.envelope(0.9) + e.envelope(0.1) - 1.0).abs() < 1e-6);
        // SetEase scales fractions whose sum exceeds 1.
        let big = timing(1, 1.0, 1.0);
        assert_eq!(big.ease(), (0.5, 0.5));
        assert_eq!(timing(1, 2.0, -1.0).ease(), (1.0, 0.0));
        // Ends are fixed and no ease means linear.
        assert_eq!((e.progress(0.0), e.progress(1.0)), (0.0, 1.0));
        assert_eq!(timing(1, 0.0, 0.0).progress(0.25), 0.25);
        // Retail's state runs 1 -> 0 while the window appears and the effect
        // applies `1 - envelope(p)`: an ease-in shapes the END of the
        // appearance (it arrives slowly), an ease-out its start.
        assert!(timing(1, 0.5, 0.0).progress(0.2) > 0.2);
        assert!(timing(1, 0.0, 0.5).progress(0.2) < 0.2);
    }

    #[test]
    fn damped_spring_settles_on_the_target() {
        let s = Timing { time: 0.4, interpolation: 2, ease_in: 0.0, ease_out: 0.0, damping: 0.5 };
        assert_eq!(s.progress(0.0), 0.0);
        assert!((s.progress(5.0) - 1.0).abs() < 1e-6);
        let mid = (0..40).map(|i| s.progress(i as f32 * 0.02)).fold(0.0f32, f32::max);
        assert!(mid > 0.0);
    }
}
