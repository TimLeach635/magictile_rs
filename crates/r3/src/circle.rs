//! Generalized circles (lines are a limiting case), and circles from non-Euclidean geometries.

use crate::euclidean2d;
use crate::infinity;
use crate::mobius::Transform;
use crate::nethash::{self, NetKey};
use crate::polygon::{Polygon, Segment, SegmentType};
use crate::tolerance::{self, THRESHOLD};
use crate::vector3d::Vector3D;
use std::f64::consts::PI;
use std::ops::{Deref, DerefMut};

/// A generalized circle. When `radius` is infinite we are a line through `p1` and `p2`.
///
/// Note: some operations in the original left stale values in the unused fields (`center` for
/// lines, `p1`/`p2` for circles); we preserve that behaviour.
#[derive(Clone, Copy, Debug)]
pub struct Circle {
    pub center: Vector3D,
    pub radius: f64,
    pub p1: Vector3D,
    pub p2: Vector3D,
}

impl Default for Circle {
    fn default() -> Self {
        Circle { center: Vector3D::ORIGIN, radius: 1.0, p1: Vector3D::ORIGIN, p2: Vector3D::ORIGIN }
    }
}

impl Circle {
    pub fn new(center: Vector3D, radius: f64) -> Self {
        Circle { center, radius, ..Default::default() }
    }

    /// A circle through 3 points (a line if they are collinear or one is infinite).
    pub fn from_3_points(p1: Vector3D, p2: Vector3D, p3: Vector3D) -> Self {
        let mut c = Circle::default();
        c.set_from_3_points(p1, p2, p3);
        c
    }

    /// A line through 2 points.
    pub fn from_2_points(p1: Vector3D, p2: Vector3D) -> Self {
        let mut c = Circle::default();
        c.set_from_2_points(p1, p2);
        c
    }

    pub fn is_line(&self) -> bool {
        self.radius.is_infinite()
    }

    /// Returns false if the construction resulted in a line.
    pub fn set_from_3_points(&mut self, p1: Vector3D, p2: Vector3D, p3: Vector3D) -> bool {
        *self = Circle::default();

        // Any infinite points make us a line. (The Big Chop puzzle needs this.)
        if infinity::is_infinite(p1) {
            self.set_from_2_points(p2, p3);
            return false;
        } else if infinity::is_infinite(p2) {
            self.set_from_2_points(p1, p3);
            return false;
        } else if infinity::is_infinite(p3) {
            self.set_from_2_points(p1, p2);
            return false;
        }

        // Intersect the perpendicular bisectors.
        let m1 = (p1 + p2) / 2.0;
        let m2 = (p1 + p3) / 2.0;
        let mut b1 = (p2 - p1) / 2.0;
        let mut b2 = (p3 - p1) / 2.0;
        b1.normalize();
        b2.normalize();
        b1.rotate90();
        b2.rotate90();

        let (found, new_center) = euclidean2d::intersection_line_line(m1, m1 + b1, m2, m2 + b2);
        self.center = new_center;
        if found == 0 {
            // The points are collinear, so we are a line.
            self.set_from_2_points(p1, p2);
            return false;
        }

        self.radius = (p1 - self.center).abs();
        true
    }

    /// Makes us a line through 2 points. (`center` is left untouched, as in the original.)
    pub fn set_from_2_points(&mut self, p1: Vector3D, p2: Vector3D) {
        self.p1 = p1;
        self.p2 = p2;
        // The original normalizes here "so that line comparisons work", but it does so before
        // setting the radius, so this only has an effect if we were already a line.
        self.normalize_line();
        self.radius = f64::INFINITY;
    }

    /// Normalize so p1 is the closest point to the origin, and the direction is of unit length.
    pub fn normalize_line(&mut self) {
        if !self.is_line() {
            return;
        }

        let mut d = self.p2 - self.p1;
        d.normalize();

        self.p1 = euclidean2d::project_onto_line(Vector3D::ORIGIN, self.p1, self.p2);

        if tolerance::greater_than_or_equal(euclidean2d::angle_to_clock(d, Vector3D::new(1.0, 0.0)), PI) {
            d *= -1.0;
        }

        self.p2 = self.p1 + d;
    }

    /// Strictly inside.
    pub fn is_point_inside(&self, test: Vector3D) -> bool {
        tolerance::less_than((test - self.center).abs(), self.radius)
    }

    pub fn is_point_on(&self, test: Vector3D) -> bool {
        tolerance::equal((test - self.center).abs(), self.radius)
    }

    /// Reflect ourselves in another circle.
    pub fn reflect(&mut self, c: &Circle) {
        if self.is_point_on(c.center) {
            // We reflect to a line. Use the 2 points 120 degrees away from c.center.
            let mut v = c.center - self.center;
            v.rotate_xy(2.0 * PI / 3.0);
            self.p1 = c.reflect_point(self.center + v);
            v.rotate_xy(2.0 * PI / 3.0);
            self.p2 = c.reflect_point(self.center + v);
            self.radius = f64::INFINITY;
        } else {
            // We can't just reflect the center. See http://mathworld.wolfram.com/Inversion.html
            let a = self.radius;
            let k = c.radius;
            let v = self.center - c.center;
            let s = k * k / (v.mag_squared() - a * a);
            self.center = c.center + v * s;
            self.radius = s.abs() * a;
        }
    }

    /// Reflect ourselves in a segment.
    pub fn reflect_segment(&mut self, s: &Segment) {
        if s.kind == SegmentType::Arc {
            self.reflect(&s.circle());
        } else {
            self.center = s.reflect_point(self.center);
        }
    }

    /// Reflect a point in us.
    pub fn reflect_point(&self, p: Vector3D) -> Vector3D {
        if self.is_line() {
            return euclidean2d::reflect_point_in_line(p, self.p1, self.p2);
        }

        if p.compare(&self.center) {
            return infinity::INFINITY_VECTOR;
        }
        if p == infinity::INFINITY_VECTOR {
            return self.center;
        }

        let mut v = p - self.center;
        let d = v.abs();
        v.normalize();
        self.center + v * (self.radius * self.radius / d)
    }

    pub fn transform(&mut self, t: &impl Transform) {
        // Transform 3 points on the circle.
        let (p1, p2, p3) = if self.is_line() {
            (self.p1, (self.p1 + self.p2) / 2.0, self.p2)
        } else {
            (
                self.center + Vector3D::new(self.radius, 0.0),
                self.center + Vector3D::new(-self.radius, 0.0),
                self.center + Vector3D::new(0.0, self.radius),
            )
        };
        self.set_from_3_points(t.apply(p1), t.apply(p2), t.apply(p3));
    }

    /// Intersection points with a segment. `None` means infinitely many (coincident arc).
    pub fn intersection_points(&self, segment: &Segment) -> Option<Vec<Vector3D>> {
        let (result, p1, p2) = if self.is_line() {
            if segment.kind == SegmentType::Arc {
                euclidean2d::intersection_line_circle(self.p1, self.p2, &segment.circle())
            } else {
                let (n, p) = euclidean2d::intersection_line_line(self.p1, self.p2, segment.p1, segment.p2);
                (n, p, Vector3D::dne())
            }
        } else if segment.kind == SegmentType::Arc {
            euclidean2d::intersection_circle_circle(&segment.circle(), self)
        } else {
            euclidean2d::intersection_line_circle(segment.p1, segment.p2, self)
        };

        if result == -1 {
            return None;
        }

        let mut ret = Vec::new();
        if result >= 1 && segment.is_point_on(p1) {
            ret.push(p1);
        }
        if result >= 2 && segment.is_point_on(p2) {
            ret.push(p2);
        }
        Some(ret)
    }

    pub fn intersects(&self, poly: &Polygon) -> bool {
        poly.segments.iter().any(|seg| self.intersection_points(seg).is_some_and(|i| !i.is_empty()))
    }

    pub fn has_vertex_inside(&self, poly: &Polygon) -> bool {
        poly.segments.iter().any(|seg| self.is_point_inside(seg.p1))
    }
}

/// A projected circle from a non-Euclidean geometry. It also stores the true (non-Euclidean)
/// center, which in general does not coincide with the Euclidean circle center.
#[derive(Clone, Debug, Default)]
pub struct CircleNE {
    pub circle: Circle,
    pub center_ne: Vector3D,
}

impl Deref for CircleNE {
    type Target = Circle;
    fn deref(&self) -> &Circle {
        &self.circle
    }
}

impl DerefMut for CircleNE {
    fn deref_mut(&mut self) -> &mut Circle {
        &mut self.circle
    }
}

impl CircleNE {
    pub fn new(circle: Circle, center_ne: Vector3D) -> Self {
        CircleNE { circle, center_ne }
    }

    pub fn reflect(&mut self, c: &Circle) {
        self.circle.reflect(c);
        self.center_ne = c.reflect_point(self.center_ne);
    }

    pub fn reflect_segment(&mut self, s: &Segment) {
        self.circle.reflect_segment(s);
        self.center_ne = s.reflect_point(self.center_ne);
    }

    pub fn transform(&mut self, t: &impl Transform) {
        self.circle.transform(t);
        self.center_ne = t.apply(self.center_ne);
    }

    /// True if our non-Euclidean center is infinite or lies outside us.
    pub fn inverted(&self) -> bool {
        infinity::is_infinite(self.center_ne) || !self.is_point_inside(self.center_ne)
    }

    /// Whether a point is inside us in the non-Euclidean sense. Works when we are inverted,
    /// and even when we are a line (half the plane is then "inside").
    pub fn is_point_inside_ne(&self, test_point: Vector3D) -> bool {
        if self.is_line() {
            // Inside if on the same side as the non-Euclidean center.
            return euclidean2d::same_side_of_line(self.p1, self.p2, test_point, self.center_ne);
        }

        let point_inside = !infinity::is_infinite(test_point) && self.is_point_inside(test_point);
        let inverted = self.inverted();
        (!inverted && point_inside) || (inverted && !point_inside)
    }

    /// A fast inside check for when we know we are not inverted (non-spherical geometries).
    /// Assumes we are most likely not in the circle. http://stackoverflow.com/a/7227057/5700835
    pub fn is_point_inside_fast(&self, test_point: Vector3D) -> bool {
        let r = self.radius;
        let dx = (test_point.x - self.center.x).abs();
        if dx > r {
            return false;
        }
        let dy = (test_point.y - self.center.y).abs();
        if dy > r {
            return false;
        }
        if dx + dy <= r {
            return true;
        }
        dx * dx + dy * dy <= r * r
    }

    /// For hypercycles the fast check doesn't work (they can be inverted). The sense of "inside"
    /// on systolic puzzles is based on the NE center of a systolic hexagon vertex.
    pub fn is_point_inside_hypercycle(&self, test_point: Vector3D) -> bool {
        self.is_point_inside_ne(test_point)
    }

    /// Whether a point is outside c1 and inside c2 (in the non-Euclidean sense). (The original
    /// cached `inverted` on the circles here; it's a pure function of the circle, so we don't.)
    pub fn is_between_hypercycles_fast(c1: &CircleNE, c2: &CircleNE, test_point: Vector3D) -> bool {
        let mut inside_c1 = c1.is_point_inside_fast(test_point);
        if c1.inverted() {
            inside_c1 = !inside_c1;
        }
        if inside_c1 {
            return false;
        }

        let mut inside_c2 = c2.is_point_inside_fast(test_point);
        if c2.inverted() {
            inside_c2 = !inside_c2;
        }
        inside_c2
    }
}

/// Circles as hash keys, matching the original `CircleNE_EqualityComparer`.
impl NetKey for CircleNE {
    fn net_hash(&self) -> i32 {
        if self.is_line() {
            self.p1.net_hash() ^ self.p2.net_hash()
        } else {
            let decimals = tolerance::decimals_for(THRESHOLD);
            self.center.net_hash()
                ^ self.center_ne.net_hash()
                ^ nethash::double_hash(tolerance::round_digits(self.radius, decimals))
        }
    }

    fn net_eq(&self, other: &Self) -> bool {
        let radius_equal = tolerance::equal(self.radius, other.radius)
            || (infinity::is_infinite_f(self.radius) && infinity::is_infinite_f(other.radius));
        if self.is_line() {
            self.p1 == other.p1 && self.p2 == other.p2 && radius_equal
        } else {
            self.center == other.center && self.center_ne == other.center_ne && radius_equal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobius::Mobius;

    #[test]
    fn three_points() {
        let c = Circle::from_3_points(Vector3D::new(1.0, 0.0), Vector3D::new(0.0, 1.0), Vector3D::new(-1.0, 0.0));
        assert!(!c.is_line());
        assert_eq!(c.center, Vector3D::ORIGIN);
        assert!(tolerance::equal(c.radius, 1.0));

        let l = Circle::from_3_points(Vector3D::new(0.0, 1.0), Vector3D::new(1.0, 1.0), Vector3D::new(2.0, 1.0));
        assert!(l.is_line());
        assert_eq!(l.p1, Vector3D::new(0.0, 1.0));
    }

    #[test]
    fn reflect_point_is_inversion() {
        let c = Circle::new(Vector3D::ORIGIN, 2.0);
        assert_eq!(c.reflect_point(Vector3D::new(1.0, 0.0)), Vector3D::new(4.0, 0.0));
        let l = Circle::from_2_points(Vector3D::new(0.0, 0.0), Vector3D::new(1.0, 0.0));
        assert_eq!(l.reflect_point(Vector3D::new(0.3, 0.5)), Vector3D::new(0.3, -0.5));
    }

    #[test]
    fn reflecting_a_circle_matches_reflecting_its_points() {
        let mirror = Circle::new(Vector3D::new(0.2, 0.1), 0.7);
        let mut c = Circle::new(Vector3D::new(1.0, 0.5), 0.4);
        let pts: Vec<_> = (0..3)
            .map(|i| {
                let t = i as f64 * 2.0;
                mirror.reflect_point(c.center + Vector3D::new(t.cos(), t.sin()) * c.radius)
            })
            .collect();
        c.reflect(&mirror);
        for p in pts {
            assert!(c.is_point_on(p));
        }
    }

    #[test]
    fn transform_moves_ne_center() {
        let mut c = CircleNE::new(Circle::new(Vector3D::ORIGIN, 0.5), Vector3D::ORIGIN);
        let mut m = Mobius::default();
        m.isometry(crate::Geometry::Hyperbolic, 0.0, Vector3D::new(0.3, 0.0));
        c.transform(&m);
        assert_eq!(c.center_ne, Vector3D::new(0.3, 0.0));
        assert!(c.is_point_inside_ne(Vector3D::new(0.3, 0.0)));
        assert!(!c.inverted());
    }
}
