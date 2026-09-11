//! Euclidean plane helpers: angles, projections and intersections.

use crate::circle::Circle;
use crate::tolerance;
use crate::vector3d::Vector3D;
use std::f64::consts::PI;

/// The counterclockwise angle from v1 to v2, between 0 and 2*pi. Intended for 2D inputs.
pub fn angle_to_counter_clock(v1: Vector3D, v2: Vector3D) -> f64 {
    let angle = v2.y.atan2(v2.x) - v1.y.atan2(v1.x);
    if angle < 0.0 { angle + 2.0 * PI } else { angle }
}

/// The clockwise angle from v1 to v2, between 0 and 2*pi. Intended for 2D inputs.
pub fn angle_to_clock(v1: Vector3D, v2: Vector3D) -> f64 {
    2.0 * PI - angle_to_counter_clock(v1, v2)
}

pub fn distance_point_line(p: Vector3D, line_p1: Vector3D, line_p2: Vector3D) -> f64 {
    let v1 = line_p2 - line_p1;
    let line_mag = v1.abs();
    if tolerance::zero(line_mag) {
        // Line definition points are the same.
        return f64::NAN;
    }
    let v2 = p - line_p1;
    v1.cross(v2).abs() / line_mag
}

pub fn project_onto_line(p: Vector3D, line_p1: Vector3D, line_p2: Vector3D) -> Vector3D {
    let mut v1 = line_p2 - line_p1;
    if tolerance::zero(v1.abs()) {
        return Vector3D::ORIGIN;
    }
    v1.normalize();
    let v2 = p - line_p1;
    let distance_along_line = v2.dot(v1);
    line_p1 + v1 * distance_along_line
}

/// Returns the number of intersection points (0 or 1) and the intersection.
pub fn intersection_line_line(p1: Vector3D, p2: Vector3D, p3: Vector3D, p4: Vector3D) -> (i32, Vector3D) {
    let n1 = p2 - p1;
    let n2 = p4 - p3;

    // Parallel (or coincident, which we don't handle separately).
    if tolerance::zero(n1.cross(n2).abs()) {
        return (0, Vector3D::ORIGIN);
    }

    let d3 = distance_point_line(p3, p1, p2);
    let d4 = distance_point_line(p4, p1, p2);

    // Distances on the same side?
    let a3 = angle_to_clock(p3 - p1, n1);
    let a4 = angle_to_clock(p4 - p1, n1);
    let same_side = if a3 > PI { a4 > PI } else { a4 <= PI };

    let factor = if same_side { d3 / (d3 - d4) } else { d3 / (d3 + d4) };
    (1, p3 + n2 * factor)
}

/// Returns the number of intersection points (-1 if the circles are coincident).
pub fn intersection_circle_circle(c1: &Circle, c2: &Circle) -> (i32, Vector3D, Vector3D) {
    // http://paulbourke.net/geometry/circlesphere/
    let mut v = c2.center - c1.center;
    let d = v.abs();
    let r1 = c1.radius;
    let r2 = c2.radius;
    let none = (0, Vector3D::ORIGIN, Vector3D::ORIGIN);

    // Circle centers coincident.
    if tolerance::zero(d) {
        return if tolerance::equal(r1, r2) { (-1, Vector3D::ORIGIN, Vector3D::ORIGIN) } else { none };
    }

    if !v.normalize() {
        return none;
    }

    // Disjoint circles, or one containing the other.
    if tolerance::greater_than(d, r1 + r2) || tolerance::less_than(d, (r1 - r2).abs()) {
        return none;
    }

    // One intersection point.
    if tolerance::equal(d, r1 + r2) || tolerance::equal(d, (r1 - r2).abs()) {
        return (1, c1.center + v * r1, Vector3D::ORIGIN);
    }

    // There must be two intersection points.
    let mut p1 = v * r1;
    let mut p2 = p1;
    let temp = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let angle = (temp / r1).acos();
    p1.rotate_xy(angle);
    p2.rotate_xy(-angle);
    (2, p1 + c1.center, p2 + c1.center)
}

pub fn intersection_line_circle(line_p1: Vector3D, line_p2: Vector3D, circle: &Circle) -> (i32, Vector3D, Vector3D) {
    // Distance from the circle center to the closest point on the line.
    let d = distance_point_line(circle.center, line_p1, line_p2);

    let r = circle.radius;
    if d > r {
        return (0, Vector3D::ORIGIN, Vector3D::ORIGIN);
    }

    let mut p1 = project_onto_line(circle.center, line_p1, line_p2);
    if tolerance::equal(d, r) {
        return (1, p1, Vector3D::ORIGIN);
    }

    // Special case when the line goes through the circle center, to avoid numerical issues.
    let p2;
    if tolerance::zero(d) {
        let mut line = line_p2 - line_p1;
        line.normalize();
        line *= r;
        p1 = circle.center + line;
        p2 = circle.center - line;
    } else {
        p1 -= circle.center;
        p1.normalize();
        p1 *= r;
        let mut q2 = p1;
        let angle = (d / r).acos();
        p1.rotate_xy(angle);
        q2.rotate_xy(-angle);
        p1 += circle.center;
        p2 = q2 + circle.center;
    }
    (2, p1, p2)
}

/// Reflects a point in a line defined by two points.
pub fn reflect_point_in_line(input: Vector3D, p1: Vector3D, p2: Vector3D) -> Vector3D {
    let p = project_onto_line(input, p1, p2);
    input + (p - input) * 2.0
}

pub fn same_side_of_line(line_p1: Vector3D, line_p2: Vector3D, test1: Vector3D, test2: Vector3D) -> bool {
    let d = line_p2 - line_p1;
    let t1 = (test1 - line_p1).cross(d);
    let t2 = (test2 - line_p1).cross(d);
    let pos1 = t1.z > 0.0;
    let pos2 = t2.z > 0.0;
    !(pos1 ^ pos2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_line() {
        let (n, p) = intersection_line_line(
            Vector3D::new(0.0, 0.0),
            Vector3D::new(1.0, 1.0),
            Vector3D::new(0.0, 1.0),
            Vector3D::new(1.0, 0.0),
        );
        assert_eq!(n, 1);
        assert_eq!(p, Vector3D::new(0.5, 0.5));
    }

    #[test]
    fn circle_circle() {
        let c1 = Circle::new(Vector3D::ORIGIN, 1.0);
        let c2 = Circle::new(Vector3D::new(1.0, 0.0), 1.0);
        let (n, p1, p2) = intersection_circle_circle(&c1, &c2);
        assert_eq!(n, 2);
        let y = 0.75f64.sqrt();
        assert_eq!(p1, Vector3D::new(0.5, y));
        assert_eq!(p2, Vector3D::new(0.5, -y));
    }

    #[test]
    fn line_circle() {
        let c = Circle::new(Vector3D::ORIGIN, 1.0);
        let (n, p1, p2) = intersection_line_circle(Vector3D::new(-2.0, 0.5), Vector3D::new(2.0, 0.5), &c);
        assert_eq!(n, 2);
        assert!(tolerance::equal(p1.abs(), 1.0) && tolerance::equal(p2.abs(), 1.0));
        assert!(tolerance::equal(p1.y, 0.5) && tolerance::equal(p2.y, 0.5));
    }
}
