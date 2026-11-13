//! Generalized ANIM curves recovered from Steam GA snapshot 005BF880.
//! This evaluates authored goals; selection, IK and locomotion stay separate.
use crate::animation::{Channel, InfoKey, Interpolator, Keys, Sample};

/// Both native rotation branches are available. The runtime global selecting
/// them was not captured; the caller must choose a branch.
#[derive(Debug, Clone, Copy, Default)]
pub enum RotationCurve {
    #[default]
    Spherical,
    Normalized,
}
struct Segment<'a> {
    times: &'a [InfoKey],
    indices: [usize; 4],
    basis: [f32; 4],
    t: f32,
    period: f32,
}
impl Segment<'_> {
    fn scalar(&self, values: [f32; 4], outgoing: &Interpolator, incoming: &Interpolator) -> f32 {
        let [p, a, b, q] = self.indices.map(|i| self.times[i].frame);
        let before = if a < p {
            self.period - p as f32
        } else {
            (a - p) as f32
        };
        let span = (b - a) as f32;
        let after = if q < b { q as f32 } else { (q - b) as f32 };
        let [vp, va, vb, vq] = values;
        // Native byte offsets use lower.previous for the outgoing tangent and
        // upper.next for the incoming tangent. Modes are masked to two bits.
        // Mode 2 is zero; mode 3 is the explicit slope, despite public labels.
        let tangent = |mode: u8, factor: f32, automatic: f32| match mode & 3 {
            0 => automatic,
            1 => vb - va,
            2 => 0.,
            _ => factor * span,
        };
        let ta = tangent(
            outgoing.previous_mode,
            outgoing.previous_factor,
            (vb - vp) / (before + span) * span,
        );
        let tb = tangent(
            incoming.next_mode,
            incoming.next_factor,
            (vq - va) / (span + after) * span,
        );
        let [ha, hb, hta, htb] = self.basis;
        va * ha + vb * hb + tb * htb + ta * hta
    }
}
fn four<T>(keys: &[T], indices: [usize; 4]) -> Result<[&T; 4], String> {
    let at = |i| {
        keys.get(i)
            .ok_or_else(|| "ANIM value/time stream mismatch".to_string())
    };
    Ok([
        at(indices[0])?,
        at(indices[1])?,
        at(indices[2])?,
        at(indices[3])?,
    ])
}
fn dot(a: [f32; 4], b: [f32; 4]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}
/// Native 005BCB80, including extrapolation and its 0.0001 linear threshold.
fn slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let d = dot(a, b).clamp(-1., 1.);
    let sign = if d < 0. { -1. } else { 1. };
    let d = d.abs();
    let (wa, wb) = if 1. - d <= 0.0001 {
        (1. - t, t)
    } else {
        let angle = (d as f64).acos();
        let inverse = 1. / angle.sin();
        (
            (((1. - t as f64) * angle).sin() * inverse) as f32,
            ((t as f64 * angle).sin() * inverse) as f32,
        )
    };
    std::array::from_fn(|i| a[i] * (wa * sign) + b[i] * wb)
}
fn normalized_blend(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let a = a.map(|v| v * (1. - t));
    let b = b.map(|v| v * t);
    let sign = if dot(a, b) < 0. { -1. } else { 1. };
    let q = std::array::from_fn(|i| a[i] + b[i] * sign);
    let length = dot(q, q).sqrt();
    if length == 0. {
        q
    } else {
        q.map(|v| v / length)
    }
}
fn rotation(
    values: [[f32; 4]; 4],
    outgoing: u8,
    incoming: u8,
    t: f32,
    curve: RotationCurve,
) -> [f32; 4] {
    let [p, a, b, q] = values;
    let p = match outgoing & 3 {
        1 => slerp(a, b, -1.),
        2 => b,
        _ => p,
    };
    let q = match incoming & 3 {
        1 => slerp(b, a, -1.),
        2 => a,
        _ => q,
    };
    let u = slerp(p, a, t + 1.);
    let w = slerp(b, q, t - 1.);
    let blend = |a, b, t| match curve {
        RotationCurve::Spherical => slerp(a, b, t),
        RotationCurve::Normalized => normalized_blend(a, b, t),
    };
    let v = blend(a, b, t);
    let x = blend(u, v, (t + 1.) * 0.5);
    let y = blend(v, w, t * 0.5);
    blend(x, y, t)
}

/// Native neighbor selection, Hermite tangents, weights and quaternion curves.
/// Cyclic neighbors skip duplicated endpoint keys. Direct endpoint copies retain
/// authored values, including weights; interior weights follow native clamps.
pub fn sample(
    ch: &Channel,
    frame: f32,
    length_frames: f32,
    cyclic: bool,
    curve: RotationCurve,
) -> Result<Sample, String> {
    if !frame.is_finite() || !length_frames.is_finite() || length_frames < 0. {
        return Err("invalid authored ANIM sample time or length".into());
    }
    let Some(times) = ch
        .components
        .iter()
        .find_map(|c| {
            if let Keys::Info(t) = &c.keys {
                Some(t)
            } else {
                None
            }
        })
        .filter(|t| !t.is_empty())
    else {
        return Ok(Sample::default());
    };
    if times.windows(2).any(|w| w[0].frame > w[1].frame) {
        return Err("ANIM key times decrease".into());
    }
    let n = times.len();
    let upper = times.partition_point(|k| k.frame as f32 <= frame);
    let b = upper.min(n - 1);
    let a = upper.saturating_sub(1).min(n - 1);
    let p = if a > 0 {
        a - 1
    } else if cyclic {
        (a + n - (2 % n)) % n
    } else {
        a
    };
    let q = if b < n - 1 {
        b + 1
    } else if cyclic {
        (b + 2) % n
    } else {
        b
    };
    let span = (times[b].frame - times[a].frame) as f32;
    let t = if span > 0. {
        (frame - times[a].frame as f32) / span
    } else {
        0.
    };
    let t2 = t * t;
    let t3 = t2 * t;
    let segment = Segment {
        times,
        indices: [p, a, b, q],
        basis: [
            (1. - 3. * t2) + 2. * t3,
            3. * t2 - 2. * t3,
            (t - 2. * t2) + t3,
            t3 - t2,
        ],
        t,
        period: length_frames - 1.,
    };
    let mut out = Sample::default();
    for component in &ch.components {
        if component.flags & 0x10 == 0 {
            continue;
        }
        match &component.keys {
            Keys::Position(keys) => {
                let k = four(keys, segment.indices)?;
                let value = if a == b {
                    k[1].position
                } else {
                    std::array::from_fn(|axis| {
                        segment.scalar(
                            std::array::from_fn(|i| k[i].position[axis]),
                            &k[1].interpolators[axis],
                            &k[2].interpolators[axis],
                        )
                    })
                };
                let weight = if a == b {
                    k[1].weight
                } else {
                    segment
                        .scalar(
                            std::array::from_fn(|i| k[i].weight),
                            &k[1].interpolators[3],
                            &k[2].interpolators[3],
                        )
                        .clamp(0., 1.)
                };
                out.position = Some((value, weight, component.flags));
            }
            Keys::Rotation(keys) => {
                let k = four(keys, segment.indices)?;
                if k.iter().any(|k| {
                    !k.rotation.iter().all(|v| v.is_finite()) || dot(k.rotation, k.rotation) < 1e-12
                }) {
                    return Err("invalid quaternion in authored ANIM rotation track".into());
                }
                let value = if a == b {
                    k[1].rotation
                } else {
                    rotation(
                        std::array::from_fn(|i| k[i].rotation),
                        k[1].previous_mode,
                        k[2].next_mode,
                        segment.t,
                        curve,
                    )
                };
                let weight = if a == b {
                    k[1].weight
                } else {
                    segment
                        .scalar(
                            std::array::from_fn(|i| k[i].weight),
                            &k[1].weight_interpolator,
                            &k[2].weight_interpolator,
                        )
                        .clamp(0., 1.)
                };
                out.rotation = Some((value, weight, component.flags));
            }
            Keys::Deform(keys) => {
                let k = four(keys, segment.indices)?;
                let scalar = |axis: usize| {
                    let values =
                        std::array::from_fn(|i| if axis == 0 { k[i].value } else { k[i].weight });
                    if a == b {
                        values[1]
                    } else {
                        let value = segment.scalar(
                            values,
                            &k[1].interpolators[axis],
                            &k[2].interpolators[axis],
                        );
                        if component.flags & 0x20 != 0 {
                            value
                        } else {
                            value.clamp(0., 1.)
                        }
                    }
                };
                out.deforms.push((component.id, scalar(0), scalar(1)));
            }
            _ => {}
        }
    }
    if out
        .position
        .as_ref()
        .is_some_and(|(p, w, _)| !p.iter().all(|v| v.is_finite()) || !w.is_finite())
        || out
            .rotation
            .as_ref()
            .is_some_and(|(q, w, _)| !q.iter().all(|v| v.is_finite()) || !w.is_finite())
        || out
            .deforms
            .iter()
            .any(|(_, v, w)| !v.is_finite() || !w.is_finite())
    {
        return Err("authored ANIM curve overflow".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{Component, DeformKey, PositionKey, RotationKey};
    /// Reads an oracle fixture from `tests/fixtures` at run time. The public repository leaves these
    /// captures of the original code out, so the test is skipped when the file is missing.
    fn fixture(name: &str) -> Option<serde_json::Value> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        match std::fs::read_to_string(&path) {
            Ok(text) => Some(serde_json::from_str(&text).unwrap()),
            Err(_) => {
                eprintln!("skipped: fixture {} is not present", path.display());
                None
            }
        }
    }
    #[test]
    fn matches_independent_native_x86_curve_readbacks() {
        let Some(fixture) = fixture("animation-curves-x86.json") else {
            return;
        };
        let mut values_checked = 0;
        let mut maximum_difference = 0f32;
        for case in fixture["cases"].as_array().unwrap() {
            let frames: Vec<InfoKey> = case["frames"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| InfoKey {
                    frame: v.as_u64().unwrap() as u32,
                    ..Default::default()
                })
                .collect();
            let values = case["values"].as_array().unwrap();
            let weights = case["weights"].as_array().unwrap();
            let interpolator = Interpolator {
                next_mode: case["next_mode"].as_u64().unwrap() as u8,
                previous_mode: case["previous_mode"].as_u64().unwrap() as u8,
                next_factor: case["next_factor"].as_f64().unwrap() as f32,
                previous_factor: case["previous_factor"].as_f64().unwrap() as f32,
                ..Default::default()
            };
            let kind = case["kind"].as_str().unwrap();
            let (flags, keys) = match kind {
                "position" => (
                    0x11,
                    Keys::Position(
                        values
                            .iter()
                            .enumerate()
                            .map(|(i, v)| PositionKey {
                                position: std::array::from_fn(|axis| {
                                    (v.as_f64().unwrap() * (axis + 1) as f64) as f32
                                }),
                                weight: weights[i].as_f64().unwrap() as f32,
                                interpolators: [interpolator; 4],
                            })
                            .collect(),
                    ),
                ),
                "rotation" => (
                    0x12,
                    Keys::Rotation(
                        values
                            .iter()
                            .enumerate()
                            .map(|(i, v)| RotationKey {
                                rotation: std::array::from_fn(|axis| {
                                    v[axis].as_f64().unwrap() as f32
                                }),
                                weight: weights[i].as_f64().unwrap() as f32,
                                next_mode: interpolator.next_mode,
                                previous_mode: interpolator.previous_mode,
                                weight_interpolator: interpolator,
                                raw: vec![],
                            })
                            .collect(),
                    ),
                ),
                _ => (
                    if kind == "deform_unclamped" {
                        0x33
                    } else {
                        0x13
                    },
                    Keys::Deform(
                        values
                            .iter()
                            .enumerate()
                            .map(|(i, v)| DeformKey {
                                value: v.as_f64().unwrap() as f32,
                                weight: weights[i].as_f64().unwrap() as f32,
                                interpolators: [interpolator; 2],
                            })
                            .collect(),
                    ),
                ),
            };
            let component = |flags, index, keys| Component {
                flags,
                authored_flags: flags,
                id: index,
                authored_id: index,
                index,
                reserved: [0; 12],
                keys,
            };
            let channel = Channel {
                components: vec![
                    component(0, 0, Keys::Info(frames)),
                    component(flags, flags & 15, keys),
                ],
                name: "synthetic native oracle".into(),
                movement_flags: 0,
                primary: Default::default(),
                secondary: Default::default(),
                bind_flags: 0,
                reserved: vec![],
            };
            let curve = if case["normalized_rotation"].as_bool().unwrap() {
                RotationCurve::Normalized
            } else {
                RotationCurve::Spherical
            };
            let actual = sample(
                &channel,
                case["frame"].as_f64().unwrap() as f32,
                case["length_frames"].as_f64().unwrap() as f32,
                case["loop"].as_bool().unwrap(),
                curve,
            )
            .unwrap();
            let actual: Vec<_> = match kind {
                "position" => {
                    let (p, w, _) = actual.position.unwrap();
                    p.into_iter().chain([w]).collect()
                }
                "rotation" => {
                    let (q, w, _) = actual.rotation.unwrap();
                    q.into_iter().chain([w]).collect()
                }
                _ => vec![actual.deforms[0].1, actual.deforms[0].2],
            };
            for (actual, expected) in actual.into_iter().zip(case["readback"].as_array().unwrap()) {
                let difference = (actual - expected.as_f64().unwrap() as f32).abs();
                maximum_difference = maximum_difference.max(difference);
                assert!(difference < 0.00002,"{kind}, modes {}/{}, frame {}, loop {}, normalized {}: {actual} != {expected}, difference {difference}",interpolator.previous_mode,interpolator.next_mode,case["frame"],case["loop"],case["normalized_rotation"]);
                values_checked += 1;
            }
        }
        println!("{values_checked} native scalar/quaternion readbacks; maximum difference {maximum_difference}");
    }
    #[test]
    #[ignore = "requires the installed Spore packages"]
    fn installed_authored_curves_sample_without_nonfinite_goals() {
        let install = crate::spore_install::discover().unwrap();
        let index = crate::asset_catalog::ResourceIndex::load(&install.packages);
        let catalog = crate::animation_catalog::AnimationCatalog::from_index(&index);
        let mut samples = 0;
        let mut failures = vec![];
        for key in &catalog.clips {
            let clip = crate::animation::parse(&index.read(*key).unwrap()).unwrap();
            for channel in &clip.channels {
                for frame in [
                    0.,
                    (clip.length_frames - 1.).max(0.) * 0.5,
                    (clip.length_frames - 1.).max(0.),
                ] {
                    for curve in [RotationCurve::Spherical, RotationCurve::Normalized] {
                        if let Err(error) = sample(
                            channel,
                            frame,
                            clip.length_frames,
                            clip.flags & 8 != 0,
                            curve,
                        ) {
                            failures.push(format!(
                                "{key:?}: {} {} frame {frame}: {error}",
                                clip.source_path, channel.name
                            ));
                        }
                        samples += 1;
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} failed authored samples; first examples: {:?}",
            failures.len(),
            &failures[..failures.len().min(12)]
        );
        println!(
            "{} installed clips, {samples} finite authored channel samples",
            catalog.clips.len()
        );
    }
}
