//! Editable vertebral body. A smooth sweep through spine points gives parts a
//! stable body-relative attachment surface; the imported sea-monster rig is
//! reserved for the original-asset preview.
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::mesh::{Indices, PrimitiveTopology},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpinePoint {
    pub position: [f32; 3],
    pub radius: f32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BodyProfile {
    pub spine: Vec<SpinePoint>,
}
/// Why a spine cannot be stored as authored. `Coincident` is recoverable by
/// *rejecting* the edit that caused it, which keeps the user's shape; the other
/// cases only arise in corrupt records or impossible data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpineProblem {
    TooFew,
    TooMany,
    NonFinite,
    Coincident,
}
impl Default for BodyProfile {
    fn default() -> Self {
        Self {
            spine: vec![
                SpinePoint {
                    position: [0., 0.05, 0.85],
                    radius: 0.25,
                },
                SpinePoint {
                    position: [0., 0.2, 0.45],
                    radius: 0.6,
                },
                SpinePoint {
                    position: [0., 0.3, 0.],
                    radius: 0.72,
                },
                SpinePoint {
                    position: [0., 0.5, -0.55],
                    radius: 0.5,
                },
                SpinePoint {
                    position: [0., 0.65, -1.0],
                    radius: 0.3,
                },
            ],
        }
    }
}
impl BodyProfile {
    /// The first reason this profile is unusable as authored, if any.
    pub fn problem(&self) -> Option<SpineProblem> {
        if self.spine.len() < 2 {
            Some(SpineProblem::TooFew)
        } else if self.spine.len() > 32 {
            Some(SpineProblem::TooMany)
        } else if self
            .spine
            .iter()
            .any(|p| !p.radius.is_finite() || p.position.iter().any(|n| !n.is_finite()))
        {
            Some(SpineProblem::NonFinite)
        } else if self.spine.windows(2).any(|p| {
            Vec3::from_array(p[0].position).distance_squared(Vec3::from_array(p[1].position))
                < 0.0001
        }) {
            Some(SpineProblem::Coincident)
        } else {
            None
        }
    }

    /// The same profile with every value inside the ranges the editor and the
    /// mesh builder assume.
    fn clamped(&self) -> Self {
        let mut out = self.clone();
        for p in &mut out.spine {
            p.radius = p.radius.clamp(0.08, 2.);
            p.position = p.position.map(|v| v.clamp(-3., 3.));
        }
        out
    }

    pub fn validated(&self) -> Self {
        let out = self.clamped();
        if out.problem().is_some() {
            return Self::default();
        }
        out
    }

    /// Apply a spine edit, committing it only when the result still holds a
    /// usable shape. `mutate` returns whether it changed anything; a rejection
    /// leaves `self` untouched and returns false.
    ///
    /// Rejecting — rather than silently falling back to the default profile — is
    /// what keeps a hand-shaped body when one edit would drop a spine point onto
    /// its neighbour. A profile that already arrived malformed stays editable so
    /// the player can still repair it.
    pub fn edit_spine(&mut self, mutate: impl FnOnce(&mut Vec<SpinePoint>) -> bool) -> bool {
        let mut next = self.clone();
        if !mutate(&mut next.spine) {
            return false;
        }
        let mut next = next.clamped();
        if self.clamped().problem().is_none() && next.problem().is_some() {
            return false;
        }
        self.spine = core::mem::take(&mut next.spine);
        true
    }
    /// Attachment slots: head, eyes, feet, hands, dorsal details and tail.
    pub fn slot(&self, bone: u32) -> Vec3 {
        let body = self.validated();
        let front = body.spine.last().unwrap();
        let back = &body.spine[0];
        let center = &body.spine[body.spine.len() / 2];
        let head = Vec3::from_array(front.position);
        let mid = Vec3::from_array(center.position);
        match bone % 8 {
            0 => head + Vec3::NEG_Z * front.radius,
            1 => head + Vec3::new(-front.radius * 0.75, front.radius * 0.6, 0.),
            2 => head + Vec3::new(front.radius * 0.75, front.radius * 0.6, 0.),
            3 => mid + Vec3::new(-center.radius * 0.85, -center.radius * 0.5, 0.),
            4 => mid + Vec3::new(center.radius * 0.85, -center.radius * 0.5, 0.),
            5 => mid + Vec3::new(-center.radius, 0.2, -0.25),
            6 => mid + Vec3::new(center.radius, 0.2, -0.25),
            _ => Vec3::from_array(back.position) + Vec3::Z * back.radius,
        }
    }
}
fn catmull(a: Vec3, b: Vec3, c: Vec3, d: Vec3, t: f32) -> Vec3 {
    0.5 * ((2. * b)
        + (-a + c) * t
        + (2. * a - 5. * b + 4. * c - d) * t * t
        + (-a + 3. * b - 3. * c + d) * t * t * t)
}
pub struct Surface {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}
pub fn surface(body: &BodyProfile) -> Surface {
    let body = body.validated();
    let subdivisions = 8;
    let sides = 20;
    let rings = (body.spine.len() - 1) * subdivisions + 1;
    let mut out = Surface {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::new(),
    };
    let point = |i: usize| Vec3::from_array(body.spine[i].position);
    let mut centers = Vec::new();
    let mut radii = Vec::new();
    for i in 0..rings {
        let seg = (i / subdivisions).min(body.spine.len() - 2);
        let t = (i - seg * subdivisions) as f32 / subdivisions as f32;
        centers.push(catmull(
            point(seg.saturating_sub(1)),
            point(seg),
            point(seg + 1),
            point((seg + 2).min(body.spine.len() - 1)),
            t,
        ));
        radii.push(body.spine[seg].radius * (1. - t) + body.spine[seg + 1].radius * t);
    }
    for i in 0..rings {
        let tangent = (centers[(i + 1).min(rings - 1)] - centers[i.saturating_sub(1)])
            .normalize_or(Vec3::NEG_Z);
        let right = tangent.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(tangent).normalize();
        for j in 0..sides {
            let theta = j as f32 / sides as f32 * std::f32::consts::TAU;
            let normal = right * theta.cos() + up * theta.sin();
            out.positions
                .push((centers[i] + normal * radii[i]).to_array());
            out.normals.push(normal.to_array());
            out.uvs
                .push([j as f32 / sides as f32, i as f32 / (rings - 1) as f32]);
            if i > 0 {
                let a = ((i - 1) * sides + j) as u32;
                let b = ((i - 1) * sides + (j + 1) % sides) as u32;
                let c = (i * sides + j) as u32;
                let d = (i * sides + (j + 1) % sides) as u32;
                out.indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
    }
    for (ring, center, normal) in [
        (0, centers[0], (centers[0] - centers[1]).normalize()),
        (
            rings - 1,
            centers[rings - 1],
            (centers[rings - 1] - centers[rings - 2]).normalize(),
        ),
    ] {
        let cap = out.positions.len() as u32;
        out.positions.push(center.to_array());
        out.normals.push(normal.to_array());
        out.uvs.push([0.5, if ring == 0 { 0. } else { 1. }]);
        for j in 0..sides {
            let a = (ring * sides + j) as u32;
            let b = (ring * sides + (j + 1) % sides) as u32;
            if ring == 0 {
                out.indices.extend_from_slice(&[cap, a, b]);
            } else {
                out.indices.extend_from_slice(&[cap, b, a]);
            }
        }
    }
    out
}
pub fn mesh(body: &BodyProfile) -> Mesh {
    let s = surface(body);
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, s.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, s.normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, s.uvs);
    mesh.insert_indices(Indices::U32(s.indices));
    mesh
}

/// Project a placement onto the same triangles the body renderer uses.
/// Checking face interiors as well as edges avoids snapping between vertices.
pub fn nearest_surface(body: &BodyProfile, point: Vec3) -> Vec3 {
    let s = surface(body);
    s.indices
        .chunks_exact(3)
        .map(|tri| {
            closest_triangle(
                point,
                Vec3::from_array(s.positions[tri[0] as usize]),
                Vec3::from_array(s.positions[tri[1] as usize]),
                Vec3::from_array(s.positions[tri[2] as usize]),
            )
        })
        .min_by(|a, b| {
            a.distance_squared(point)
                .total_cmp(&b.distance_squared(point))
        })
        .unwrap_or(point)
}
fn closest_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0. && d2 <= 0. {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0. && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0. && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && d4 - d3 >= 0. && d5 - d6 >= 0. {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denominator = 1. / (va + vb + vc);
    a + ab * (vb * denominator) + ac * (vc * denominator)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_projects_to_triangle_interiors_edges_and_changed_body() {
        let (a, b, c) = (Vec3::ZERO, Vec3::X, Vec3::Y);
        assert!(
            closest_triangle(Vec3::new(0.2, 0.3, 5.), a, b, c).distance(Vec3::new(0.2, 0.3, 0.))
                < 1e-6
        );
        assert_eq!(closest_triangle(Vec3::new(-1., -1., 3.), a, b, c), a);
        assert!(
            closest_triangle(Vec3::new(1., 1., 3.), a, b, c).distance(Vec3::new(0.5, 0.5, 0.))
                < 1e-6
        );
        let mut body = BodyProfile::default();
        let p = nearest_surface(&body, Vec3::new(2., 0.3, 0.));
        assert!(nearest_surface(&body, p).distance(p) < 1e-5);
        body.spine[2].radius = 1.4;
        let wider = nearest_surface(&body, Vec3::new(2., 0.3, 0.));
        assert!(wider.x > p.x + 0.3);
    }
    #[test]
    fn body_is_closed_finite_and_has_consistent_winding() {
        let s = surface(&BodyProfile::default());
        let mut edges = std::collections::HashMap::new();
        for tri in s.indices.chunks_exact(3) {
            let a = Vec3::from_array(s.positions[tri[0] as usize]);
            let b = Vec3::from_array(s.positions[tri[1] as usize]);
            let c = Vec3::from_array(s.positions[tri[2] as usize]);
            assert!((b - a).cross(c - a).length_squared() > 1e-10);
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let e = edges.entry((a.min(b), a.max(b))).or_insert((0, 0));
                e.0 += 1;
                e.1 += if a < b { 1 } else { -1 };
            }
        }
        assert!(edges.values().all(|e| *e == (2, 0)));
        assert!(s.positions.iter().flatten().all(|v| v.is_finite()));
        assert!(s
            .normals
            .iter()
            .all(|n| (Vec3::from_array(*n).length() - 1.).abs() < 1e-5));
    }
    #[test]
    fn malformed_profiles_recover_and_shapes_roundtrip() {
        let b = BodyProfile {
            spine: vec![SpinePoint {
                position: [f32::NAN, 0., 0.],
                radius: -1.,
            }],
        };
        assert_eq!(b.validated(), BodyProfile::default());
        let mut b = BodyProfile::default();
        b.spine[2].radius = 1.1;
        b.spine[3].position[1] = 1.4;
        let saved: BodyProfile = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_eq!(b, saved);
        assert_ne!(
            surface(&b).positions,
            surface(&BodyProfile::default()).positions
        );
    }
    #[test]
    fn a_collapsing_edit_is_rejected_and_keeps_the_whole_body() {
        let mut body = BodyProfile::default();
        // Shape the body first: the edits must not be undone by validation.
        assert!(body.edit_spine(|spine| {
            spine[1].radius = 1.3;
            spine[2].position[1] = 1.2;
            true
        }));
        let shaped = body.clone();
        // Dragging a point onto its neighbour is refused, not repaired by
        // resetting every point back to the default profile.
        let target = shaped.spine[3].position;
        assert!(!body.edit_spine(|spine| {
            spine[2].position = target;
            true
        }));
        assert_eq!(body, shaped, "a rejected edit must not touch the profile");
        // A legal edit on the same index still commits.
        assert!(body.edit_spine(|spine| {
            spine[2].position[1] += 0.2;
            true
        }));
        assert_ne!(body, shaped);
        // Removing the point that blocked the move makes it legal again.
        assert!(body.edit_spine(|spine| {
            spine.remove(3);
            true
        }));
        assert!(body.edit_spine(|spine| {
            spine[2].position = target;
            true
        }));
        // Mutations that report "nothing changed" never commit or save.
        let before = body.clone();
        assert!(!body.edit_spine(|spine| { spine.get_mut(99).is_some() }));
        assert_eq!(body, before);
    }
    #[test]
    fn problem_matches_validated_and_clamps_then_checks() {
        let body = BodyProfile::default();
        assert_eq!(body.problem(), None);
        // Two points that only coincide *after* clamping must still be seen.
        let mut bad = BodyProfile::default();
        bad.spine[2].position = [3.4, 0.3, 0.];
        bad.spine[3].position = [3.2, 0.3, 0.];
        assert_eq!(bad.clamped().problem(), Some(SpineProblem::Coincident));
        assert_eq!(bad.validated(), BodyProfile::default());
        // Already-malformed input still reports the reason and still validates
        // to the default for readers.
        let mut few = BodyProfile::default();
        few.spine.truncate(1);
        assert_eq!(few.problem(), Some(SpineProblem::TooFew));
        let mut many = BodyProfile::default();
        many.spine.resize(
            33,
            SpinePoint {
                position: [0., 1., 0.],
                radius: 0.2,
            },
        );
        assert_eq!(many.problem(), Some(SpineProblem::TooMany));
        assert_eq!(body.problem(), None);
    }
}
