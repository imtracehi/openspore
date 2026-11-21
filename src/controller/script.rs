//! Scripted controller input for quick captures, so controller play can be
//! checked without a pad: a virtual pad is connected through Bevy's raw
//! gamepad events (the path a real XInput pad takes) and driven per frame of
//! each stage's warm-up.
//!
//! - `--pad BUTTON@FRAME` taps a button (pressed on FRAME, released on the
//!   next); `--pad BUTTON@FIRST-LAST` holds it. Buttons: a b x y lb rb lt rt
//!   view menu ls rs up down left right.
//! - `--stick l|r:X,Y@FIRST-LAST` holds a stick at (X, Y), each -1..1
//!   (+Y = up).

use bevy::input::gamepad::{
    GamepadAxis, GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
    RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::prelude::*;

use super::Pad;

/// The virtual pad of a scripted capture.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ScriptPad {
    pub entity: Entity,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Button { pad: Pad, first: usize, last: usize },
    Stick { right: bool, at: Vec2, first: usize, last: usize },
}

fn frames(at: &str) -> Option<(usize, usize)> {
    match at.split_once('-') {
        Some((first, last)) => Some((first.parse().ok()?, last.parse().ok()?)),
        None => {
            let first: usize = at.parse().ok()?;
            Some((first, first + 1))
        }
    }
}

/// The script steps in the command line.
pub fn parse(args: &[String]) -> Vec<Step> {
    let mut steps = Vec::new();
    for pair in args.windows(2) {
        match pair[0].as_str() {
            "--pad" => {
                let Some((name, at)) = pair[1].split_once('@') else { continue };
                if let (Some(pad), Some((first, last))) = (Pad::parse(name), frames(at)) {
                    steps.push(Step::Button { pad, first, last });
                }
            }
            "--stick" => {
                let Some((spec, at)) = pair[1].split_once('@') else { continue };
                let Some((side, xy)) = spec.split_once(':') else { continue };
                let Some((x, y)) = xy.split_once(',') else { continue };
                let (Ok(x), Ok(y), Some((first, last))) = (x.parse::<f32>(), y.parse::<f32>(), frames(at)) else {
                    continue;
                };
                steps.push(Step::Stick {
                    right: side.eq_ignore_ascii_case("r"),
                    at: Vec2::new(x, y).clamp(Vec2::splat(-1.0), Vec2::ONE),
                    first,
                    last,
                });
            }
            _ => {}
        }
    }
    steps
}

/// The raw events of `steps` for one frame.
pub fn events(entity: Entity, steps: &[Step], frame: usize) -> Vec<RawGamepadEvent> {
    let mut out = Vec::new();
    for step in steps {
        match *step {
            Step::Button { pad, first, last } => {
                let value = if frame == first {
                    1.0
                } else if frame == last {
                    0.0
                } else {
                    continue;
                };
                out.push(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                    entity,
                    pad.button(),
                    value,
                )));
            }
            Step::Stick { right, at, first, last } => {
                let value = if frame == first {
                    at
                } else if frame == last {
                    Vec2::ZERO
                } else {
                    continue;
                };
                let (x, y) = if right {
                    (GamepadAxis::RightStickX, GamepadAxis::RightStickY)
                } else {
                    (GamepadAxis::LeftStickX, GamepadAxis::LeftStickY)
                };
                out.push(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(entity, x, value.x)));
                out.push(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(entity, y, value.y)));
            }
        }
    }
    out
}

/// Connects the virtual pad (first call) and writes this frame's events.
/// Called by the quick-capture loop before each update; does nothing
/// without `--pad` / `--stick`.
pub fn capture_step(app: &mut App, args: &[String], frame: usize) {
    let steps = parse(args);
    if steps.is_empty() {
        return;
    }
    let world = app.world_mut();
    let entity = match world.get_resource::<ScriptPad>() {
        Some(pad) => pad.entity,
        None => {
            let entity = world.spawn_empty().id();
            world.insert_resource(ScriptPad { entity });
            // Bevy's gilrs backend sends a connection this way (the raw
            // event only mirrors it).
            world
                .resource_mut::<Messages<GamepadConnectionEvent>>()
                .write(GamepadConnectionEvent::new(
                    entity,
                    GamepadConnection::Connected {
                        name: "OpenSpore script pad".into(),
                        vendor_id: None,
                        product_id: None,
                    },
                ));
            return;
        }
    };
    let mut messages = world.resource_mut::<Messages<RawGamepadEvent>>();
    for event in events(entity, &steps, frame) {
        messages.write(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn script_parses_taps_holds_and_sticks() {
        let steps = parse(&args(&["x", "--pad", "a@30", "--pad", "rb@10-40", "--stick", "l:0,1@20-80", "--pad", "bogus@3"]));
        assert_eq!(
            steps,
            vec![
                Step::Button { pad: Pad::A, first: 30, last: 31 },
                Step::Button { pad: Pad::RB, first: 10, last: 40 },
                Step::Stick { right: false, at: Vec2::new(0.0, 1.0), first: 20, last: 80 },
            ]
        );
    }

    #[test]
    fn events_press_then_release() {
        let entity = Entity::PLACEHOLDER;
        let steps = vec![Step::Button { pad: Pad::A, first: 2, last: 5 }];
        assert_eq!(events(entity, &steps, 1).len(), 0);
        let down = events(entity, &steps, 2);
        assert!(matches!(&down[..], [RawGamepadEvent::Button(b)] if b.value == 1.0));
        let up = events(entity, &steps, 5);
        assert!(matches!(&up[..], [RawGamepadEvent::Button(b)] if b.value == 0.0));
    }
}
