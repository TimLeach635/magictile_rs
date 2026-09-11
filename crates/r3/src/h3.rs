//! The few helpers from R3's `H3Models.Ball` (hyperbolic 3-space in the Poincaré ball) that 2D
//! systolic puzzles use, restricted to the plane.

use crate::infinity;
use crate::tolerance;
use crate::vector3d::Vector3D;
use std::f64::consts::PI;

/// The circle orthogonal to the unit circle through two points on it, as (center, radius).
pub fn orthogonal_circle(v1: Vector3D, v2: Vector3D) -> (Vector3D, f64) {
    let sector_angle = v1.angle_to(v2);
    if tolerance::equal(sector_angle, PI) {
        return (infinity::INFINITY_VECTOR, f64::INFINITY);
    }

    let dist_to_center = 1.0 / (sector_angle / 2.0).cos();
    let mut center = v1 + v2;
    center.normalize();
    center *= dist_to_center;
    (center, dist_to_center * (sector_angle / 2.0).sin())
}

/// The circle orthogonal to the unit circle through two points, at least one of them interior,
/// as (center, radius).
pub fn orthogonal_circle_interior(v1: Vector3D, v2: Vector3D) -> (Vector3D, f64) {
    if tolerance::equal(v1.abs(), 1.0) && tolerance::equal(v2.abs(), 1.0) {
        return orthogonal_circle(v1, v2);
    }

    // The circle passes through the inversion of an interior point in the unit circle.
    // http://www.math.washington.edu/~king/coursedir/m445w06/ortho/01-07-ortho-to3.html
    let interior = if tolerance::equal(v1.abs(), 1.0) { v2 } else { v1 };
    let reflected = reflect_in_unit_sphere(interior);
    circle_from_3_points_3d(reflected, v1, v2)
}

/// Inversion in the unit sphere (R3's `Sphere.ReflectPoint`).
fn reflect_in_unit_sphere(p: Vector3D) -> Vector3D {
    if p == Vector3D::ORIGIN {
        return infinity::INFINITY_VECTOR;
    }
    if infinity::is_infinite(p) {
        return Vector3D::ORIGIN;
    }
    let mut v = p;
    let d = v.abs();
    v.normalize();
    v * (1.0 / d)
}

/// The circumcircle of three points via barycentric coordinates (R3's `Circle3D.From3Points`),
/// as (center, radius). http://mathworld.wolfram.com/Circumcenter.html
pub fn circle_from_3_points_3d(v1: Vector3D, v2: Vector3D, v3: Vector3D) -> (Vector3D, f64) {
    let a = (v3 - v2).abs(); // Opposite v1
    let b = (v1 - v3).abs(); // Opposite v2
    let c = (v2 - v1).abs(); // Opposite v3
    let (a2, b2, c2) = (a * a, b * b, c * c);

    let mut bary = Vector3D::new3(a2 * (b2 + c2 - a2), b2 * (c2 + a2 - b2), c2 * (a2 + b2 - c2));
    bary /= bary.x + bary.y + bary.z;
    let center = v1 * bary.x + v2 * bary.y + v3 * bary.z;

    let s = (a + b + c) / 2.0; // Semiperimeter
    let radius = a * b * c / (4.0 * (s * (a + b - s) * (a + c - s) * (b + c - s)).sqrt());
    (center, radius)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orthogonal_circles() {
        let (p1, p2) = (Vector3D::new(0.3, 0.1), Vector3D::new(-0.2, 0.4));
        let (center, radius) = orthogonal_circle_interior(p1, p2);
        assert!(tolerance::equal((p1 - center).abs(), radius));
        assert!(tolerance::equal((p2 - center).abs(), radius));
        // Orthogonal to the unit circle.
        assert!(tolerance::equal(center.mag_squared(), radius * radius + 1.0));
    }

    #[test]
    fn circumcircle() {
        let (c, r) =
            circle_from_3_points_3d(Vector3D::new(1.0, 0.0), Vector3D::new(0.0, 1.0), Vector3D::new(-1.0, 0.0));
        assert_eq!(c, Vector3D::ORIGIN);
        assert!(tolerance::equal(r, 1.0));
    }
}
