//! Segments (lines or arcs) and polygons built from them.

use crate::circle::{Circle, CircleNE};
use crate::euclidean2d;
use crate::geometry2d::{self, Geometry};
use crate::infinity;
use crate::mobius::Transform;
use crate::nethash::{self, NetKey};
use crate::tolerance;
use crate::util;
use crate::vector3d::{Vector3D, compare_lexicographic};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SegmentType {
    #[default]
    Line,
    Arc,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Segment {
    pub kind: SegmentType,
    pub p1: Vector3D,
    pub p2: Vector3D,
    /// Only used for arcs.
    pub center: Vector3D,
    /// Only used for arcs.
    pub clockwise: bool,
}

impl Segment {
    pub fn line(start: Vector3D, end: Vector3D) -> Segment {
        Segment { kind: SegmentType::Line, p1: start, p2: end, ..Default::default() }
    }

    /// An arc with a given center. Note: the original ignores the `clockwise` argument and always
    /// makes the arc clockwise; callers fix up the direction afterwards.
    pub fn arc_with_center(start: Vector3D, end: Vector3D, center: Vector3D) -> Segment {
        Segment { kind: SegmentType::Arc, p1: start, p2: end, center, clockwise: true }
    }

    /// An arc through three points.
    pub fn arc(start: Vector3D, mid: Vector3D, end: Vector3D) -> Segment {
        let c = Circle::from_3_points(start, mid, end);
        let mut seg = Segment { kind: SegmentType::Arc, p1: start, p2: end, center: c.center, clockwise: false };

        // Vectors from the circle center.
        let start_origin = start - c.center;
        let mid_origin = mid - c.center;
        let end_origin = end - c.center;

        let normal_vector = start_origin.cross(end_origin);
        seg.clockwise = normal_vector.z < 0.0;
        let angle_to_traverse = start_origin.angle_to(end_origin);

        // The direction might need reversing, depending on where the midpoint is.
        let compare_angle = start_origin.angle_to(mid_origin) + mid_origin.angle_to(end_origin);
        if !tolerance::equal(angle_to_traverse, compare_angle) {
            seg.clockwise = !seg.clockwise;
        }
        seg
    }

    pub fn radius(&self) -> f64 {
        (self.p1 - self.center).abs()
    }

    pub fn angle(&self) -> f64 {
        if self.kind != SegmentType::Arc {
            return 0.0;
        }
        let v1 = self.p1 - self.center;
        let v2 = self.p2 - self.center;
        if self.clockwise { euclidean2d::angle_to_clock(v1, v2) } else { euclidean2d::angle_to_counter_clock(v1, v2) }
    }

    /// The circle an arc lies on.
    pub fn circle(&self) -> Circle {
        Circle::new(self.center, self.radius())
    }

    pub fn length(&self) -> f64 {
        if self.kind == SegmentType::Arc { self.radius() * self.angle() } else { (self.p2 - self.p1).abs() }
    }

    pub fn midpoint(&self) -> Vector3D {
        if self.kind == SegmentType::Arc {
            let a = self.angle() / 2.0;
            let mut ret = self.p1 - self.center;
            ret.rotate_xy(if self.clockwise { -a } else { a });
            ret + self.center
        } else {
            (self.p1 + self.p2) / 2.0
        }
    }

    pub fn reverse(&mut self) {
        self.swap_points();
        if self.kind == SegmentType::Arc {
            self.clockwise = !self.clockwise;
        }
    }

    /// The vertices from subdividing ourselves (including both endpoints).
    pub fn subdivide(&self, num_segments: i32) -> Vec<Vector3D> {
        let mut ret = Vec::new();
        if num_segments < 1 {
            return ret;
        }

        if self.kind == SegmentType::Arc {
            let mut v = self.p1 - self.center;
            let angle = self.angle() / num_segments as f64;
            for _ in 0..num_segments {
                ret.push(self.center + v);
                v.rotate_xy(if self.clockwise { -angle } else { angle });
            }
        } else {
            let mut v = self.p2 - self.p1;
            v.normalize();
            for i in 0..num_segments {
                ret.push(self.p1 + v * i as f64 * self.length() / num_segments as f64);
            }
        }

        ret.push(self.p2);
        ret
    }

    pub fn swap_points(&mut self) {
        std::mem::swap(&mut self.p1, &mut self.p2);
    }

    pub fn is_point_on(&self, test: Vector3D) -> bool {
        if self.kind == SegmentType::Arc {
            let max_angle = self.angle();
            let v1 = self.p1 - self.center;
            let v2 = test - self.center;
            let angle = if self.clockwise {
                euclidean2d::angle_to_clock(v1, v2)
            } else {
                euclidean2d::angle_to_counter_clock(v1, v2)
            };
            tolerance::less_than_or_equal(angle, max_angle)
        } else {
            // True if the point and the segment ends form a degenerate triangle.
            let d1 = (self.p2 - self.p1).abs();
            let d2 = (test - self.p1).abs();
            let d3 = (self.p2 - test).abs();
            tolerance::equal(d1, d2 + d3)
        }
    }

    pub fn intersects(&self, s: &Segment) -> bool {
        let (num_int, i1, i2) = match (self.kind, s.kind) {
            (SegmentType::Arc, SegmentType::Arc) => {
                euclidean2d::intersection_circle_circle(&self.circle(), &s.circle())
            }
            (SegmentType::Arc, SegmentType::Line) => {
                euclidean2d::intersection_line_circle(self.p1, self.p2, &s.circle())
            }
            (SegmentType::Line, SegmentType::Arc) => euclidean2d::intersection_line_circle(s.p1, s.p2, &self.circle()),
            (SegmentType::Line, SegmentType::Line) => {
                let (n, p) = euclidean2d::intersection_line_line(self.p1, self.p2, s.p1, s.p2);
                (n, p, Vector3D::dne())
            }
        };

        // -1 can denote coincident segments, which we don't include.
        if num_int <= 0 {
            return false;
        }
        if self.is_point_on(i1) && s.is_point_on(i1) {
            return true;
        }
        num_int > 1 && self.is_point_on(i2) && s.is_point_on(i2)
    }

    /// Reflect ourselves in another segment.
    pub fn reflect(&mut self, s: &Segment) {
        self.map_points(|p| s.reflect_point(p), s);
    }

    pub fn transform(&mut self, t: &impl Transform) {
        let this = *self;
        self.map_points(|p| t.apply(p), &this);
    }

    /// Shared implementation of reflect/transform. Arcs can become lines and vice versa, and
    /// arc directions can reverse. `infinity_ref` provides the fallback midpoint when ours is
    /// infinite (the reflecting segment for reflections, ourselves for transforms).
    fn map_points(&mut self, f: impl Fn(Vector3D) -> Vector3D, infinity_ref: &Segment) {
        // We must calculate this before altering the endpoints.
        let mut mid = self.midpoint();
        if infinity::is_infinite(mid) {
            mid = if infinity::is_infinite(infinity_ref.p1) {
                infinity_ref.p2 * infinity::FINITE_SCALE
            } else {
                infinity_ref.p1 * infinity::FINITE_SCALE
            };
        }

        self.p1 = f(self.p1);
        self.p2 = f(self.p2);
        let mid = f(mid);

        let mut temp = Circle::default();
        if !infinity::is_infinite(self.p1)
            && !infinity::is_infinite(self.p2)
            && !infinity::is_infinite(mid)
            && temp.set_from_3_points(self.p1, mid, self.p2)
        {
            self.kind = SegmentType::Arc;
            self.center = temp.center;

            // Work out the orientation of the arc.
            let t1 = self.p1 - self.center;
            let t2 = mid - self.center;
            let t3 = self.p2 - self.center;
            let a1 = euclidean2d::angle_to_counter_clock(t2, t1);
            let a2 = euclidean2d::angle_to_counter_clock(t3, t1);
            self.clockwise = a2 > a1;
        } else {
            // The points are collinear (the arc became a line).
            self.kind = SegmentType::Line;
        }
    }

    /// Euclidean translation.
    pub fn translate(&mut self, v: Vector3D) {
        self.p1 += v;
        self.p2 += v;
        if self.kind == SegmentType::Arc {
            self.center += v;
        }
    }

    /// Euclidean scale relative to a center point.
    pub fn scale(&mut self, center: Vector3D, factor: f64) {
        self.translate(-center);
        if self.kind == SegmentType::Line {
            self.p1 *= factor;
            self.p2 *= factor;
        } else {
            let p1 = self.p1 * factor;
            let p2 = self.p2 * factor;
            let mid = self.midpoint() * factor;
            let temp = Segment::arc(p1, mid, p2);
            self.p1 = p1;
            self.p2 = p2;
            self.center = temp.center;
        }
        self.translate(center);
    }

    pub fn reflect_point(&self, input: Vector3D) -> Vector3D {
        if self.kind == SegmentType::Arc {
            self.circle().reflect_point(input)
        } else {
            euclidean2d::reflect_point_in_line(input, self.p1, self.p2)
        }
    }

    /// Splits us at a point (p1 -> point, point -> p2). Returns `None` if the point is not on us
    /// or is an endpoint.
    pub fn split(&self, point: Vector3D) -> Option<(Segment, Segment)> {
        if !self.is_point_on(point) {
            return None;
        }
        if point.compare(&self.p1) || point.compare(&self.p2) {
            return None;
        }
        let mut s1 = *self;
        let mut s2 = *self;
        s1.p2 = point;
        s2.p1 = point;
        Some((s1, s2))
    }

    /// True if P1 -> test1 -> test2 -> P2 along us. False if the test points are equal, not on
    /// us, or endpoints.
    pub fn ordered(&self, test1: Vector3D, test2: Vector3D) -> bool {
        if test1.compare(&test2) {
            return false;
        }
        if !self.is_point_on(test1) || !self.is_point_on(test2) {
            return false;
        }
        if test1.compare(&self.p1) || test1.compare(&self.p2) || test2.compare(&self.p1) || test2.compare(&self.p2) {
            return false;
        }

        if self.kind == SegmentType::Arc {
            let t1 = self.p1 - self.center;
            let t2 = test1 - self.center;
            let t3 = test2 - self.center;
            let angle = |a, b| {
                if self.clockwise {
                    euclidean2d::angle_to_clock(a, b)
                } else {
                    euclidean2d::angle_to_counter_clock(a, b)
                }
            };
            angle(t1, t2) < angle(t1, t3)
        } else {
            (test1 - self.p1).mag_squared() < (test2 - self.p1).mag_squared()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Polygon {
    pub center: Vector3D,
    pub segments: Vec<Segment>,
}

impl Polygon {
    /// A polygon of line segments through a set of points.
    pub fn from_points(points: &[Vector3D]) -> Polygon {
        let mut result = Polygon::default();
        result.create_euclidean(points);
        result
    }

    /// A regular {p,q} polygon centered at the origin, with a vertex on the positive x axis.
    pub fn create_regular(&mut self, num_sides: i32, q: i32) {
        let p = num_sides;
        self.segments.clear();

        let g = Geometry::from_pq(p, q);
        let circum_radius = geometry2d::normalized_circum_radius(p, q);

        let mut points = Vec::new();
        let mut angle = 0.0f64;
        for _ in 0..p {
            points.push(Vector3D::new(circum_radius * angle.cos(), circum_radius * angle.sin()));
            angle += tolerance::degrees_to_radians(360.0 / p as f64);
        }

        for i in 0..points.len() {
            let mut seg = Segment { p1: points[i], p2: points[(i + 1) % points.len()], ..Default::default() };

            if g != Geometry::Euclidean {
                seg.kind = SegmentType::Arc;
                if p == 2 {
                    // The formula below breaks down for digons.
                    let factor = (PI / 6.0).tan();
                    seg.center = if seg.p1.x > 0.0 {
                        Vector3D::new(0.0, -circum_radius) * factor
                    } else {
                        Vector3D::new(0.0, circum_radius) * factor
                    };
                } else {
                    // Magically, the same formula works for both non-Euclidean geometries.
                    let piq = if q == -1 { 0.0 } else { PI / q as f64 }; // Handle q infinite.
                    let t1 = PI / p as f64;
                    let t2 = PI / 2.0 - piq - t1;
                    let factor = (t1.tan() / t2.tan() + 1.0) / 2.0;
                    seg.center = (seg.p1 + seg.p2) * factor;
                }
                seg.clockwise = g != Geometry::Spherical;
            }

            self.segments.push(seg);
        }
    }

    /// A Euclidean polygon from a set of points (don't repeat the starting point).
    pub fn create_euclidean(&mut self, points: &[Vector3D]) {
        self.segments.clear();
        for i in 0..points.len() {
            self.segments.push(Segment::line(points[i], points[(i + 1) % points.len()]));
        }
        self.center = self.centroid_approx();
    }

    pub fn num_sides(&self) -> usize {
        self.segments.len()
    }

    pub fn length(&self) -> f64 {
        util::sum(self.segments.iter().map(|s| s.length()))
    }

    /// An approximate centroid. For arcs this uses the midpoint rather than the true centroid,
    /// on purpose (it biases towards large arcs, which helps avoid drawing overlaps).
    pub fn centroid_approx(&self) -> Vector3D {
        let mut average = Vector3D::ORIGIN;
        for s in &self.segments {
            average += s.midpoint() * s.length();
        }
        average / self.length()
    }

    /// The first vertex.
    pub fn start(&self) -> Option<Vector3D> {
        self.segments.first().map(|s| s.p1)
    }

    /// The middle point around the polygon (an edge midpoint for odd numbers of sides).
    pub fn mid(&self) -> Option<Vector3D> {
        let count = self.segments.len();
        if count == 0 {
            return None;
        }
        if count.is_multiple_of(2) {
            Some(self.segments[count / 2].p1)
        } else {
            Some(self.segments[count / 2].midpoint())
        }
    }

    pub fn vertices(&self) -> Vec<Vector3D> {
        self.segments.iter().map(|s| s.p1).collect()
    }

    pub fn edge_midpoints(&self) -> Vec<Vector3D> {
        self.segments.iter().map(|s| s.midpoint()).collect()
    }

    /// Points along our boundary, suitable for drawing.
    pub fn edge_points(&self) -> Vec<Vector3D> {
        self.calc_edge_points(tolerance::degrees_to_radians(4.5), 10, true)
    }

    pub fn calc_edge_points(&self, arc_resolution: f64, min_segs: i32, check_for_infinities: bool) -> Vec<Vector3D> {
        let mut points = Vec::new();
        for s in &self.segments {
            let p1 =
                if check_for_infinities && infinity::is_infinite(s.p1) { s.p2 * infinity::FINITE_SCALE } else { s.p1 };
            points.push(p1);

            // For arcs, add in a bunch of extra points.
            if s.kind == SegmentType::Arc {
                let max_angle = s.angle();
                let mut vs = s.p1 - s.center;
                let num_segments = ((max_angle / arc_resolution) as i32).max(min_segs);
                let angle = max_angle / num_segments as f64;
                for _ in 1..num_segments {
                    vs.rotate_xy(if s.clockwise { -angle } else { angle });
                    points.push(vs + s.center);
                }
            }

            let p2 =
                if check_for_infinities && infinity::is_infinite(s.p2) { s.p1 * infinity::FINITE_SCALE } else { s.p2 };
            points.push(p2);
        }
        points
    }

    /// True if CCW, false if CW.
    pub fn orientation(&self) -> bool {
        self.signed_area() > 0.0
    }

    pub fn signed_area(&self) -> f64 {
        // Arcs are handled piecemeal via edge points.
        let edge_points = self.edge_points();
        let n = edge_points.len();
        let mut s_area = 0.0;
        for i in 0..n {
            let v1 = edge_points[i];
            let v2 = edge_points[(i + 1) % n];
            s_area += v1.x * v2.y - v1.y * v2.x;
        }
        s_area / 2.0
    }

    pub fn circum_circle(&self) -> CircleNE {
        let mut result = CircleNE::default();
        if self.segments.len() > 2 {
            result.set_from_3_points(self.segments[0].p1, self.segments[1].p1, self.segments[2].p1);
        }
        result.center_ne = self.center;
        result
    }

    pub fn in_circle(&self) -> CircleNE {
        let mut result = CircleNE::default();
        if self.segments.len() > 2 {
            result.set_from_3_points(
                self.segments[0].midpoint(),
                self.segments[1].midpoint(),
                self.segments[2].midpoint(),
            );
        }
        result.center_ne = self.center;
        result
    }

    /// The bounding box of our vertices, as (min, max).
    pub fn bounding_box(&self) -> (Vector3D, Vector3D) {
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for v in self.vertices() {
            min_x = min_x.min(v.x);
            min_y = min_y.min(v.y);
            max_x = max_x.max(v.x);
            max_y = max_y.max(v.y);
        }
        (Vector3D::new(min_x, min_y), Vector3D::new(max_x, max_y))
    }

    /// Reverses all segments and their order.
    pub fn reverse(&mut self) {
        for s in &mut self.segments {
            s.reverse();
        }
        self.segments.reverse();
    }

    /// Moves the first segment to the end `num` times (like a CW rotation). The center is not
    /// recalculated.
    pub fn cycle(&mut self, num: usize) {
        assert!(num <= self.num_sides(), "Cycle called with invalid input.");
        self.segments.rotate_left(num);
    }

    pub fn reflect(&mut self, s: &Segment) {
        for seg in &mut self.segments {
            seg.reflect(s);
        }
        self.center = s.reflect_point(self.center);
    }

    pub fn transform(&mut self, t: &impl Transform) {
        for s in &mut self.segments {
            s.transform(t);
        }
        self.center = t.apply_infinite_safe(self.center);
    }

    /// Euclidean translation.
    pub fn translate(&mut self, v: Vector3D) {
        for s in &mut self.segments {
            s.translate(v);
        }
        self.center += v;
    }

    /// Euclidean scale about our center.
    pub fn scale(&mut self, factor: f64) {
        let center = self.center;
        for s in &mut self.segments {
            s.scale(center, factor);
        }
    }

    /// Whether we intersect another polygon. Not perfect: only centers are checked for
    /// containment.
    pub fn intersects(&self, p: &Polygon) -> bool {
        if p.is_point_inside_paranoid(self.center) || self.is_point_inside_paranoid(p.center) {
            return true;
        }
        self.segments.iter().any(|s1| p.segments.iter().any(|s2| s1.intersects(s2)))
    }

    /// Intersection points between us and a generalized circle.
    pub fn intersection_points(&self, line: &Circle) -> Vec<Vector3D> {
        self.segments.iter().flat_map(|s| line.intersection_points(s).unwrap_or_default()).collect()
    }

    /// Attempts to return true if our center is not inside us. Used in the spherical case, with
    /// some hardcoded hacks to work better.
    pub fn is_inverted(&self) -> bool {
        // Ignore a little more than one hemisphere of the sphere (and all of the Poincaré disk).
        let factor = 1.5; // Magic tunable number :(
        if self.center.abs() < geometry2d::DISK_RADIUS * factor {
            return false;
        }

        if infinity::is_infinite(self.center) {
            return true;
        }

        if self.is_point_inside(self.center) {
            return false;
        }

        // We think we're inverted, but false positives are common here. Try two more rays and
        // let the majority win.
        let ray = Circle::from_2_points(self.center, self.center + Vector3D::new(103.0, 10007.0));
        if !self.is_point_inside_ray(self.center, &ray) {
            return true;
        }
        // The original reuses its ray object here, and reusing a line normalizes it.
        let mut ray = ray;
        ray.set_from_2_points(self.center, self.center + Vector3D::new(7001.0, 7993.0));
        !self.is_point_inside_ray(self.center, &ray)
    }

    /// A majority vote of three ray casts. Try not to use this; see `is_inverted`.
    pub fn is_point_inside_paranoid(&self, p: Vector3D) -> bool {
        let mut inside_count = 0;
        if self.is_point_inside(p) {
            inside_count += 1;
        }

        let mut ray = Circle::default();
        ray.set_from_2_points(p, p + Vector3D::new(103.0, 10007.0));
        if self.is_point_inside_ray(p, &ray) {
            inside_count += 1;
        }

        ray.set_from_2_points(p, p + Vector3D::new(7001.0, 7993.0));
        if self.is_point_inside_ray(p, &ray) {
            inside_count += 1;
        }

        inside_count >= 2
    }

    /// Ray casting. Suffers from tolerance issues when arcs have very large radii.
    pub fn is_point_inside(&self, p: Vector3D) -> bool {
        let ray = Circle::from_2_points(p, p + Vector3D::new(10007.0, 103.0));
        self.is_point_inside_ray(p, &ray)
    }

    fn is_point_inside_ray(&self, p: Vector3D, ray: &Circle) -> bool {
        // Our "ray" is a line; we throw out intersections with x <= p.x.
        let i_points = self.intersection_points(ray).into_iter().filter(|v| v.x > p.x).map(HighToleranceVector);
        let count = nethash::distinct(i_points).len();
        count % 2 == 1
    }
}

/// A vector compared with a looser tolerance (1e-4), as in the original's ray casting.
/// Making this smaller or bigger both cause issues.
struct HighToleranceVector(Vector3D);

const HIGH_TOLERANCE: f64 = 0.0001;

impl NetKey for HighToleranceVector {
    fn net_hash(&self) -> i32 {
        self.0.net_hash_t(HIGH_TOLERANCE)
    }

    fn net_eq(&self, other: &Self) -> bool {
        self.0.compare_t(&other.0, HIGH_TOLERANCE)
    }
}

/// Polygons as hash keys, matching the original `PolygonEqualityComparer`: equal when they have
/// the same set of vertices (in any order).
impl NetKey for Polygon {
    fn net_hash(&self) -> i32 {
        self.segments.iter().fold(0, |h, s| h ^ s.p1.net_hash())
    }

    fn net_eq(&self, other: &Self) -> bool {
        let sorted = |p: &Polygon| {
            let mut v = p.vertices();
            util::stable_sort_by(&mut v, compare_lexicographic);
            v
        };
        let (v1, v2) = (sorted(self), sorted(other));
        v1.len() == v2.len() && v1.iter().zip(&v2).all(|(a, b)| a == b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobius::Mobius;

    fn square() -> Polygon {
        Polygon::from_points(&[
            Vector3D::new(0.0, 0.0),
            Vector3D::new(1.0, 0.0),
            Vector3D::new(1.0, 1.0),
            Vector3D::new(0.0, 1.0),
        ])
    }

    #[test]
    fn arc_through_three_points() {
        let a = Segment::arc(Vector3D::new(1.0, 0.0), Vector3D::new(0.0, 1.0), Vector3D::new(-1.0, 0.0));
        assert!(!a.clockwise);
        assert!(tolerance::equal(a.angle(), PI));
        assert_eq!(a.midpoint(), Vector3D::new(0.0, 1.0));
        // (Semicircles are ambiguous: the original can't tell their direction, as above.)
        let h = 0.5f64.sqrt();
        let b = Segment::arc(Vector3D::new(1.0, 0.0), Vector3D::new(h, -h), Vector3D::new(0.0, -1.0));
        assert!(b.clockwise);
        assert_eq!(b.midpoint(), Vector3D::new(h, -h));
    }

    #[test]
    fn square_basics() {
        let s = square();
        assert_eq!(s.center, Vector3D::new(0.5, 0.5));
        assert!(s.orientation());
        assert!(tolerance::equal(s.signed_area(), 1.0));
        assert!(s.is_point_inside(Vector3D::new(0.5, 0.5)));
        assert!(!s.is_point_inside(Vector3D::new(1.5, 0.5)));
        assert!(s.is_point_inside_paranoid(Vector3D::new(0.2, 0.7)));
    }

    #[test]
    fn regular_polygons() {
        for (p, q) in [(4, 3), (6, 3), (7, 3), (5, 4)] {
            let mut poly = Polygon::default();
            poly.create_regular(p, q);
            assert_eq!(poly.num_sides(), p as usize);
            let r = geometry2d::normalized_circum_radius(p, q);
            for v in poly.vertices() {
                assert!(tolerance::equal(v.abs(), r));
            }
            // Arcs connect consecutive vertices.
            for s in &poly.segments {
                if s.kind == SegmentType::Arc {
                    assert!(tolerance::equal((s.p2 - s.center).abs(), s.radius()));
                }
            }
            assert!(poly.orientation());
        }
    }

    #[test]
    fn hyperbolic_edges_are_orthogonal_to_the_disk() {
        let mut poly = Polygon::default();
        poly.create_regular(7, 3);
        for s in &poly.segments {
            // Orthogonal circles satisfy |c|^2 = r^2 + 1.
            assert!(tolerance::equal(s.center.mag_squared(), s.radius() * s.radius() + 1.0));
        }
    }

    #[test]
    fn transform_and_reflect() {
        let mut poly = Polygon::default();
        poly.create_regular(7, 3);
        let mut m = Mobius::default();
        m.isometry(Geometry::Hyperbolic, 0.3, Vector3D::new(0.2, 0.1));
        let expected: Vec<_> = poly.vertices().iter().map(|v| m.apply(*v)).collect();
        let mut moved = poly.clone();
        moved.transform(&m);
        for (a, b) in moved.vertices().iter().zip(&expected) {
            assert_eq!(*a, *b);
        }
        // Edges stay geodesics.
        for s in &moved.segments {
            assert!(tolerance::equal(s.center.mag_squared(), s.radius() * s.radius() + 1.0));
        }

        let mut reflected = poly.clone();
        reflected.reflect(&poly.segments[0]);
        assert!(!reflected.orientation());
        assert_eq!(reflected.segments[0].p1, poly.segments[0].p1);
    }

    #[test]
    fn polygon_equality_ignores_vertex_order() {
        let mut a = square();
        let b = square();
        a.cycle(2);
        assert!(a.net_eq(&b));
        assert_eq!(a.net_hash(), b.net_hash());
    }
}
