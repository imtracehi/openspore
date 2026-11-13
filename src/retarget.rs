//! Capability-based selection and pose specialization in Spore coordinates
//! (+X left, +Y back, +Z up). These are building blocks for the original
//! constraint/IK animation pipeline, not a claim of complete solver parity.
use bevy::math::{Quat, Vec3};

#[derive(Debug, Clone)]
pub struct Body {
    pub capabilities: Vec<String>,
    pub parent: Option<usize>,
    pub position: Vec3,
    pub rotation: Quat,
}

#[derive(Debug, Clone)]
pub struct Rig {
    pub bodies: Vec<Body>,
    pub root: usize,
    pub min: Vec3,
    pub max: Vec3,
}

impl Rig {
    pub fn validate(&self) -> Result<(), String> {
        if self.bodies.is_empty() || self.bodies.len() > 2048 || self.root >= self.bodies.len() {
            return Err("invalid retarget rig body count or root".into());
        }
        if !self.min.is_finite() || !self.max.is_finite() || self.min.cmpgt(self.max).any() {
            return Err("invalid retarget rig bounds".into());
        }
        for (i, body) in self.bodies.iter().enumerate() {
            if !body.position.is_finite()
                || !body.rotation.is_finite()
                || body.rotation.length_squared() < 1e-12
            {
                return Err(format!("body {i} has an invalid rest pose"));
            }
            let mut next = body.parent;
            for depth in 0..=self.bodies.len() {
                let Some(parent) = next else {
                    break;
                };
                if parent >= self.bodies.len() || parent == i || depth == self.bodies.len() {
                    return Err(format!("body {i} has an invalid or cyclic parent"));
                }
                next = self.bodies[parent].parent;
            }
        }
        Ok(())
    }

    /// Intersect capability and spatial queries. Local queries use the bounds
    /// of the capability set before any spatial filter, so axes are independent.
    pub fn select(&self, flags: u32, capability: &str) -> Result<Vec<usize>, String> {
        self.validate()?;
        let mut selected: Vec<_> = match flags & 0x100003 {
            1 => vec![self.root],
            2 => vec![],
            0 | 3 => self
                .bodies
                .iter()
                .enumerate()
                .filter(|(_, b)| b.capabilities.iter().any(|cap| cap == capability))
                .map(|(i, _)| i)
                .collect(),
            kind => return Err(format!("unsupported frame context {kind:08X}")),
        };
        if selected.is_empty() {
            return Ok(selected);
        }
        let (mut local_min, mut local_max) =
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for &i in &selected {
            local_min = local_min.min(self.bodies[i].position);
            local_max = local_max.max(self.bodies[i].position);
        }
        for (axis, mask, local_bit, shift, positive_regular, positive_local) in [
            (0, 0x2000C, 0x20000, 2, 1, 0),
            (1, 0x40030, 0x40000, 4, 2, 1),
            (2, 0x800C0, 0x80000, 6, 1, 0),
        ] {
            let selection = flags & mask;
            if selection == 0 {
                continue;
            }
            let local = selection & local_bit != 0;
            let mode = (selection >> shift) & 3;
            let center = if local {
                (local_min[axis] + local_max[axis]) * 0.5
            } else {
                (self.min[axis] + self.max[axis]) * 0.5
            };
            let center_mode = if local { 2 } else { 3 };
            // Center-plane tolerance avoids floating-point instability. The
            // original game's center-zone width still requires measurement.
            let tolerance = 1e-5;
            selected.retain(|&i| {
                let distance = self.bodies[i].position[axis] - center;
                if mode == center_mode {
                    distance.abs() <= tolerance
                } else if mode
                    == if local {
                        positive_local
                    } else {
                        positive_regular
                    }
                {
                    distance >= -tolerance
                } else {
                    distance <= tolerance
                }
            });
        }
        let extent = flags & 0xE000;
        if extent != 0 && !selected.is_empty() {
            let (axis, positive) = match extent {
                0x2000 => (0, true),
                0x4000 => (0, false),
                0x6000 => (1, false),
                0x8000 => (1, true),
                0xA000 => (2, true),
                0xC000 => (2, false),
                _ => return Err("unsupported extent query".into()),
            };
            selected.sort_by(|&a, &b| {
                let order = self.bodies[a].position[axis].total_cmp(&self.bodies[b].position[axis]);
                (if positive { order.reverse() } else { order }).then(a.cmp(&b))
            });
            selected.truncate(1);
        }
        match flags & 0x801C00 {
            0 => {}
            0x1000 => {
                let mut spines = vec![];
                for &i in &selected {
                    let mut next = self.bodies[i].parent;
                    while let Some(parent) = next {
                        if self.bodies[parent].capabilities.iter().any(|c| c == "spin") {
                            if !spines.contains(&parent) {
                                spines.push(parent);
                            }
                            break;
                        }
                        next = self.bodies[parent].parent;
                    }
                }
                selected = spines;
            }
            modifier => return Err(format!("unsupported limb modifier {modifier:08X}")),
        }
        Ok(selected)
    }

    pub fn limb_length(&self, body: usize) -> Result<f32, String> {
        self.validate()?;
        let mut current = body;
        let mut distance = 0.;
        while let Some(parent) = self.bodies.get(current).ok_or("invalid body index")?.parent {
            distance += self.bodies[current]
                .position
                .distance(self.bodies[parent].position);
            if self.bodies[parent].capabilities.iter().any(|c| c == "spin") {
                break;
            }
            current = parent;
        }
        Ok(distance)
    }
}

/// Rest-relative and absolute position modes, scale modes and ground-relative
/// height specialization. Secondary frames require the caller's target frame;
/// they must not be silently played as character-relative position curves.
pub fn specialize_position(
    rig: &Rig,
    body: usize,
    mut pose: Vec3,
    component_flags: u32,
    movement_flags: u32,
    ground_z: f32,
    mirrored: bool,
) -> Result<Vec3, String> {
    rig.validate()?;
    let rest = rig.bodies.get(body).ok_or("invalid body index")?.position;
    if !pose.is_finite() || !ground_z.is_finite() {
        return Err("nonfinite generalized position".into());
    }
    if movement_flags & 0xB != 0 {
        return Err("secondary/lookat movement requires a target frame".into());
    }
    if mirrored {
        pose.x = -pose.x;
    }
    let original_z = pose.z;
    match component_flags & 0xC0 {
        0 => {}
        0x40 => pose *= rig.max - rig.min,
        0x80 => pose *= rig.limb_length(body)?,
        _ => return Err("unsupported combined position scale mode".into()),
    }
    if component_flags & 0x20 != 0 {
        pose += rest;
        if movement_flags & 0x10 != 0 {
            pose.z = rest.z + original_z * (ground_z - rest.z);
        }
    } else if movement_flags & 0x10 != 0 {
        return Err("ground-relative mode requires rest-relative position".into());
    }
    if !pose.is_finite() {
        return Err("specialized position overflow".into());
    }
    Ok(pose)
}

pub fn mirror_rotation(q: Quat) -> Result<Quat, String> {
    if !q.is_finite() || q.length_squared() < 1e-12 {
        return Err("invalid generalized rotation".into());
    }
    Ok(Quat::from_xyzw(q.x, -q.y, -q.z, q.w).normalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rig() -> Rig {
        Rig {
            bodies: vec![
                Body {
                    capabilities: vec!["spin".into()],
                    parent: None,
                    position: Vec3::new(0., 0., 2.),
                    rotation: Quat::IDENTITY,
                },
                Body {
                    capabilities: vec!["grsp".into()],
                    parent: Some(0),
                    position: Vec3::new(1., -3., 2.),
                    rotation: Quat::IDENTITY,
                },
                Body {
                    capabilities: vec!["grsp".into()],
                    parent: Some(0),
                    position: Vec3::new(-1., -1., 4.),
                    rotation: Quat::IDENTITY,
                },
            ],
            root: 0,
            min: Vec3::new(-2., -4., 0.),
            max: Vec3::new(2., 4., 6.),
        }
    }
    #[test]
    fn spatial_queries_use_original_axes_and_local_set_bounds() {
        let r = rig();
        assert_eq!(r.select(4, "grsp").unwrap(), vec![1]);
        assert_eq!(r.select(8, "grsp").unwrap(), vec![2]);
        assert_eq!(r.select(0x10, "grsp").unwrap(), vec![1, 2]);
        assert_eq!(r.select(0x40000, "grsp").unwrap(), vec![1]);
        assert_eq!(r.select(0x40010, "grsp").unwrap(), vec![2]);
        assert_eq!(r.select(0x20000, "grsp").unwrap(), vec![1]);
        assert_eq!(r.select(0x20004, "grsp").unwrap(), vec![2]);
        assert_eq!(r.select(4 | 0x40, "grsp").unwrap(), Vec::<usize>::new());
    }
    #[test]
    fn extents_parent_spine_and_missing_capability() {
        let r = rig();
        assert_eq!(r.select(0xA000, "grsp").unwrap(), vec![2]);
        assert_eq!(r.select(0x6000, "grsp").unwrap(), vec![1]);
        assert_eq!(r.select(0x1000, "grsp").unwrap(), vec![0]);
        assert_eq!(r.select(1, "unused").unwrap(), vec![0]);
        assert!(r.select(0, "foot").unwrap().is_empty());
        assert!(r.select(0x800000, "grsp").is_err());
    }
    #[test]
    fn ground_relative_hits_ground_for_different_rest_heights() {
        let r = rig();
        for body in [1, 2] {
            let p = specialize_position(&r, body, Vec3::Z, 0x20 | 0x40, 0x10, -1., false).unwrap();
            assert_eq!(p.z, -1.);
            let p =
                specialize_position(&r, body, Vec3::ZERO, 0x20 | 0x80, 0x10, -1., false).unwrap();
            assert_eq!(p, r.bodies[body].position);
        }
        assert!(specialize_position(&r, 1, Vec3::Z, 0x20, 1, 0., false).is_err());
    }
    #[test]
    fn mirroring_twice_restores_rotation_and_bad_rigs_fail() {
        let q = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.2);
        assert!(
            mirror_rotation(mirror_rotation(q).unwrap())
                .unwrap()
                .dot(q)
                .abs()
                > 0.99999
        );
        assert!(mirror_rotation(Quat::from_xyzw(0., 0., 0., 0.)).is_err());
        let mut r = rig();
        r.bodies[0].parent = Some(2);
        assert!(r.select(1, "root").is_err());
    }
}
