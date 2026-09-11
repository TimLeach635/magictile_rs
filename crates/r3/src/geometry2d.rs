//! The three 2D geometries and {p,q} triangle helpers.

use crate::donhatch;
use crate::spherical2d;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Geometry {
    Spherical,
    Euclidean,
    Hyperbolic,
}

impl Geometry {
    /// The geometry induced by {p,q}: p-gons, q meeting at each vertex.
    pub fn from_pq(p: i32, q: i32) -> Geometry {
        let test = 1.0 / p as f64 + 1.0 / q as f64;
        if test > 0.5 {
            Geometry::Spherical
        } else if test == 0.5 {
            Geometry::Euclidean
        } else {
            Geometry::Hyperbolic
        }
    }
}

// The original's Euclidean tile size choice.
const EUCLIDEAN_HYPOTENUSE: f64 = 1.0 / 3.0;
pub const DISK_RADIUS: f64 = 1.0;

pub fn num_platonic_facets(p: i32, q: i32) -> Option<i32> {
    match (p, q) {
        (3, 3) => Some(4),
        (4, 3) => Some(6),
        (5, 3) => Some(12),
        (3, 4) => Some(8),
        (3, 5) => Some(20),
        _ => None,
    }
}

pub fn normalized_circum_radius(p: i32, q: i32) -> f64 {
    let hypot = triangle_hypotenuse(p, q);
    match Geometry::from_pq(p, q) {
        Geometry::Spherical => spherical2d::s2e_norm(hypot) * DISK_RADIUS,
        Geometry::Euclidean => EUCLIDEAN_HYPOTENUSE,
        Geometry::Hyperbolic => donhatch::h2e_norm(hypot) * DISK_RADIUS,
    }
}

/// The hypotenuse of the (2,p,q) triangle, in the induced geometry.
pub fn triangle_hypotenuse(p: i32, q: i32) -> f64 {
    let g = Geometry::from_pq(p, q);
    if g == Geometry::Euclidean {
        return EUCLIDEAN_HYPOTENUSE;
    }
    // The right angle alpha is opposite the hypotenuse.
    let alpha = PI / 2.0;
    let beta = PI / q as f64;
    let gamma = PI / p as f64;
    triangle_side(g, alpha, beta, gamma)
}

/// The side opposite the angle PI/p, in the induced geometry.
pub fn triangle_p_side(p: i32, q: i32) -> f64 {
    let g = Geometry::from_pq(p, q);
    let alpha = PI / 2.0;
    let beta = PI / q as f64;
    let gamma = PI / p as f64;
    if g == Geometry::Euclidean {
        return EUCLIDEAN_HYPOTENUSE * gamma.sin();
    }
    triangle_side(g, gamma, beta, alpha)
}

/// The side opposite the angle PI/q, in the induced geometry.
pub fn triangle_q_side(p: i32, q: i32) -> f64 {
    let g = Geometry::from_pq(p, q);
    let alpha = PI / 2.0;
    let beta = PI / q as f64;
    let gamma = PI / p as f64;
    if g == Geometry::Euclidean {
        return EUCLIDEAN_HYPOTENUSE * beta.sin();
    }
    triangle_side(g, beta, gamma, alpha)
}

/// The side opposite alpha, given all three angles of a triangle.
/// Not determined (returns 0) in Euclidean geometry.
pub fn triangle_side(g: Geometry, alpha: f64, beta: f64, gamma: f64) -> f64 {
    let ratio = (alpha.cos() + beta.cos() * gamma.cos()) / (beta.sin() * gamma.sin());
    match g {
        Geometry::Spherical => ratio.acos(),
        Geometry::Euclidean => 0.0,
        Geometry::Hyperbolic => donhatch::acosh(ratio),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometries() {
        assert_eq!(Geometry::from_pq(4, 3), Geometry::Spherical);
        assert_eq!(Geometry::from_pq(6, 3), Geometry::Euclidean);
        assert_eq!(Geometry::from_pq(4, 4), Geometry::Euclidean);
        assert_eq!(Geometry::from_pq(7, 3), Geometry::Hyperbolic);
    }

    #[test]
    fn cube_circumradius() {
        // A cube face's circumradius on the unit sphere is atan(sqrt(2)); stereographically tan(r/2).
        let r = normalized_circum_radius(4, 3);
        assert!((r - (2f64.sqrt().atan() / 2.0).tan()).abs() < 1e-12);
    }
}
