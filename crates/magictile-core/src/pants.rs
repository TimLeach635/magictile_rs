//! Hyperbolic pairs of pants, the basis of systolic (and "earthquake") twists on the Klein quartic.
//! https://en.wikipedia.org/wiki/Pair_of_pants_(mathematics)

use r3::euclidean2d;
use r3::h3;
use r3::{Circle, CircleNE, Geometry, Isometry, Mobius, Polygon, Segment, SegmentType, Transform, Vector3D};
use std::f64::consts::PI;

/// A pair of pants is two hexagons stitched together at every other side. We store one hexagon
/// and reflect it across its sides to get the other.
#[derive(Debug)]
pub struct Pants {
    pub hexagon: Polygon,
    /// The net isometry applied during puzzle building.
    pub isometry: Isometry,
    /// Helps find affected cells/stickers (a non-Euclidean circle).
    pub test_circle: CircleNE,
    pub circum_circle: CircleNE,
}

impl Clone for Pants {
    /// Note: like the original, cloning doesn't copy the isometry.
    fn clone(&self) -> Self {
        Pants {
            hexagon: self.hexagon.clone(),
            isometry: Isometry::default(),
            test_circle: self.test_circle.clone(),
            circum_circle: self.circum_circle.clone(),
        }
    }
}

impl Pants {
    pub fn transform(&mut self, i: &Isometry) {
        self.hexagon.transform(i);
        self.test_circle.transform(i);
        self.circum_circle.transform(i);
        self.isometry = &self.isometry * i;
    }

    pub fn transform_mobius(&mut self, m: &Mobius) {
        self.hexagon.transform(m);
        self.test_circle.transform(m);
        self.circum_circle.transform(m);
        self.isometry = &self.isometry * &Isometry::new(*m, None);
    }

    /// Whether a point is inside our hexagon, using what we know to be much faster than the
    /// generic polygon check (this is used during puzzle building).
    pub fn is_point_inside_optimized(&self, p: Vector3D) -> bool {
        if !self.circum_circle.is_point_inside_fast(p) {
            return false;
        }

        // We must be on the same side of every segment as the center.
        let cen = self.hexagon.center;
        self.hexagon.segments.iter().all(|s| {
            if s.kind == SegmentType::Line {
                euclidean2d::same_side_of_line(s.p1, s.p2, cen, p)
            } else {
                let c = s.circle();
                c.is_point_inside(cen) == c.is_point_inside(p)
            }
        })
    }

    /// The segment chopped in an earthquake, for a closest geodesic segment.
    pub fn chopped_pants_seg(closest_geodesic_seg: i32) -> i32 {
        match closest_geodesic_seg {
            1 => 4,
            3 => 0,
            5 => 2,
            _ => -1,
        }
    }

    /// The index (of the hexagon segments) of the pants geodesic closest to a point.
    pub fn closest_geodesic_seg(&self, p: Vector3D) -> i32 {
        // This needs to be a non-Euclidean calculation, so move the hexagon to the center first.
        let m = self.mobius_to_center();
        let mut poly = self.hexagon.clone();
        poly.transform(&m);
        let p = m.apply(p);
        let d1 = poly.segments[1].midpoint().dist(p);
        let d2 = poly.segments[3].midpoint().dist(p);
        let d3 = poly.segments[5].midpoint().dist(p);
        let min = d1.min(d2.min(d3));
        if min == d1 {
            1
        } else if min == d2 {
            3
        } else if min == d3 {
            5
        } else {
            -1
        }
    }

    fn mobius_to_center(&self) -> Mobius {
        let mut m = Mobius::default();
        m.isometry(Geometry::Hyperbolic, 0.0, -self.hexagon.center);
        m
    }

    pub fn tiny_offset(&self, toward_seg: i32) -> Vector3D {
        let away_from_seg = Pants::chopped_pants_seg(toward_seg);

        let m = self.mobius_to_center();
        let mut poly = self.hexagon.clone();
        poly.transform(&m);
        let mut p = m.apply(self.hexagon.center);
        p -= poly.segments[away_from_seg as usize].midpoint() / 10.0;
        m.inverse().apply(p)
    }

    /// The pants for the Klein quartic's systoles.
    pub fn for_klein_quartic() -> Pants {
        let mut central_tile = Polygon::default();
        central_tile.create_regular(7, 3);
        let vertex0 = central_tile.segments[0].p1;
        let other_three_sides = other_three_sides();
        let systoles = systoles_for_kq();

        // The vertices.
        let (_, t1, t2) = euclidean2d::intersection_circle_circle(&other_three_sides[0], &systoles[0]);
        let mut intersection = if t1.abs() < 1.0 { t1 } else { t2 };
        let mut verts = vec![intersection];
        intersection.y *= -1.0;
        verts.push(intersection);
        let m = rot_mobius(vertex0);
        for i in 0..4 {
            verts.push(m.apply(verts[i]));
        }

        let arc = |i: usize, j: usize, center: Vector3D| Segment::arc_with_center(verts[i], verts[j], center);
        let hexagon = Polygon {
            center: vertex0,
            segments: vec![
                arc(0, 1, other_three_sides[0].center),
                arc(1, 2, systoles[1].center),
                arc(2, 3, other_three_sides[1].center),
                arc(3, 4, systoles[2].center),
                arc(4, 5, other_three_sides[2].center),
                arc(5, 0, systoles[0].center),
            ],
        };

        // The test circles, computed with the hexagon at the origin.
        let mut m = Mobius::default();
        m.isometry(Geometry::Hyperbolic, 0.0, -vertex0);
        let mut clone = hexagon.clone();
        clone.transform(&m);
        let circle_ne = |p1, p2, p3| {
            let mut c = CircleNE::new(Circle::from_3_points(p1, p2, p3), Vector3D::ORIGIN);
            c.transform(&m.inverse());
            c
        };
        let test_circle =
            circle_ne(clone.segments[0].midpoint(), clone.segments[2].midpoint(), clone.segments[4].midpoint());
        let circum_circle = circle_ne(clone.segments[0].p1, clone.segments[1].p1, clone.segments[2].p1);

        Pants { hexagon, isometry: Isometry::default(), test_circle, circum_circle }
    }
}

/// The three systoles of the Klein quartic around the central heptagon's vertex 0.
/// (Arnaud Chéritat's applet helps with visualizing this:
/// http://www.math.univ-toulouse.fr/~cheritat/AppletsDivers/Klein/)
pub fn systoles_for_kq() -> Vec<CircleNE> {
    let mut central_tile = Polygon::default();
    central_tile.create_regular(7, 3);
    let vertex0 = central_tile.segments[0].p1;
    let mid1 = central_tile.segments[1].midpoint();
    let mid2 = central_tile.segments[2].midpoint();
    cycle_circles(orthogonal_circle_ne(mid1, mid2, vertex0), vertex0)
}

fn other_three_sides() -> Vec<CircleNE> {
    let mut central_tile = Polygon::default();
    central_tile.create_regular(7, 3);
    let vertex0 = central_tile.segments[0].p1;
    let seg = central_tile.segments[3];
    cycle_circles(orthogonal_circle_ne(seg.p1, seg.p2, vertex0), vertex0)
}

/// The geodesic through two points, with the hexagon center as its non-Euclidean center.
fn orthogonal_circle_ne(p1: Vector3D, p2: Vector3D, center_ne: Vector3D) -> CircleNE {
    let (center, radius) = h3::orthogonal_circle_interior(p1, p2);
    CircleNE::new(Circle::new(center, radius), center_ne)
}

/// A third of a turn about a vertex.
fn rot_mobius(vertex0: Vector3D) -> Mobius {
    let mut m = Mobius::default();
    m.elliptic(Geometry::Hyperbolic, vertex0, 2.0 * PI / 3.0);
    m
}

fn cycle_circles(template: CircleNE, vertex0: Vector3D) -> Vec<CircleNE> {
    let m = rot_mobius(vertex0);
    let mut result = vec![template];
    for _ in 0..2 {
        let mut next = result.last().unwrap().clone();
        next.transform(&m);
        result.push(next);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn klein_quartic_pants() {
        let pants = Pants::for_klein_quartic();
        assert_eq!(pants.hexagon.num_sides(), 6);
        assert!(pants.is_point_inside_optimized(pants.hexagon.center));
        assert!(!pants.is_point_inside_optimized(Vector3D::new(0.9, 0.0)));
        // Each systole side is closest to its own midpoint.
        for seg in [1, 3, 5] {
            let mid = pants.hexagon.segments[seg as usize].midpoint();
            assert_eq!(pants.closest_geodesic_seg(mid), seg);
        }
    }
}
