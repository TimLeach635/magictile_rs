//! Models (projections) of the hyperbolic plane and the sphere, and stereographic helpers.
//!
//! Puzzles are built in the Poincaré disk (hyperbolic) or stereographic projection (spherical);
//! these maps convert to the other models for display.

use crate::complex::Complex;
use crate::geometry2d::Geometry;
use crate::infinity;
use crate::mobius::{Mobius, Transform};
use crate::vector3d::Vector3D;
use std::f64::consts::PI;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HyperbolicModel {
    #[default]
    Poincare,
    Klein,
    UpperHalfPlane,
    Orthographic,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SphericalModel {
    #[default]
    Sterographic,
    Gnomonic,
    Fisheye,
    HemisphereDisks,
}

pub fn poincare_to_klein(p: Vector3D) -> Vector3D {
    let mag = 2.0 / (1.0 + p.dot(p));
    p * mag
}

pub fn klein_to_poincare(k: Vector3D) -> Vector3D {
    // Clamping avoids some NaN problems. (The original also divided 0/0 at the origin; this is
    // only used for mouse input, so fixing that doesn't affect puzzle building.)
    let dot = k.dot(k).min(1.0);
    if dot == 0.0 {
        return k;
    }
    let mag = (1.0 - (1.0 - dot).sqrt()) / dot;
    k * mag
}

/// Disk -> upper half plane, and its inverse. Cached, since this is used per vertex.
static UPPER: LazyLock<(Mobius, Mobius)> = LazyLock::new(|| {
    let (mut m1, mut m2) = (Mobius::default(), Mobius::default());
    m2.isometry(Geometry::Euclidean, 0.0, Complex::new(0.0, -1.0));
    m1.upper_half_plane();
    let upper = m2 * m1;
    (upper, upper.inverse())
});

pub fn poincare_to_upper(v: Vector3D) -> Vector3D {
    UPPER.0.apply(v)
}

pub fn upper_to_poincare(v: Vector3D) -> Vector3D {
    UPPER.1.apply(v)
}

pub fn poincare_to_ortho(v: Vector3D) -> Vector3D {
    stereo_to_gnomonic(v)
}

pub fn ortho_to_poincare(v: Vector3D) -> Vector3D {
    gnomonic_to_stereo(v)
}

const G_SCALE: f64 = 0.5;

pub fn stereo_to_gnomonic(p: Vector3D) -> Vector3D {
    let mut sphere = plane_to_sphere(p);

    // We can only represent the lower hemisphere.
    if sphere.z >= 0.0 {
        sphere.z = 0.0;
        sphere.normalize();
        return sphere * infinity::FINITE_SCALE;
    }

    let z = sphere.z;
    sphere.z = 0.0;
    -sphere * G_SCALE / z
}

pub fn gnomonic_to_stereo(g: Vector3D) -> Vector3D {
    let g = g / G_SCALE;
    let dot = g.dot(g);
    let z = -1.0 / (dot + 1.0).sqrt();
    g * z / (z - 1.0)
}

fn half_turn_about_i() -> Mobius {
    let mut m = Mobius::default();
    m.elliptic(Geometry::Spherical, Complex::I, PI);
    m
}

/// Stereographic projection -> two side-by-side hemisphere disks.
pub fn to_disks(mut p: Vector3D) -> Vector3D {
    if p.abs() <= 1.0 {
        p.x -= 1.0;
        return p;
    }
    let mut p = half_turn_about_i().apply(p);
    p.x += 1.0;
    p
}

/// Two side-by-side hemisphere disks -> stereographic projection.
pub fn from_disks(mut p: Vector3D, normalize: bool) -> Vector3D {
    if p.x <= 0.0 {
        p.x += 1.0;
        if normalize || p.abs() > 1.0 {
            p.normalize();
        }
        return p;
    }

    p.x -= 1.0;
    let mut p = half_turn_about_i().apply(p);
    if normalize || p.abs() < 1.0 {
        p.normalize();
    }
    p
}

/// Stereographic projection from the plane to the unit sphere.
pub fn plane_to_sphere(mut plane_point: Vector3D) -> Vector3D {
    plane_point.z = 0.0;
    let dot = plane_point.dot(plane_point);
    Vector3D::new3(2.0 * plane_point.x / (dot + 1.0), 2.0 * plane_point.y / (dot + 1.0), (dot - 1.0) / (dot + 1.0))
}

pub fn plane_to_sphere_safe(plane_point: Vector3D) -> Vector3D {
    if infinity::is_infinite(plane_point) {
        return Vector3D::new3(0.0, 0.0, 1.0);
    }
    plane_to_sphere(plane_point)
}

pub fn sphere_to_plane(sphere_point: Vector3D) -> Vector3D {
    let z = sphere_point.z;
    Vector3D::new(sphere_point.x / (1.0 - z), sphere_point.y / (1.0 - z))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts() -> Vec<Vector3D> {
        vec![Vector3D::new(0.1, 0.2), Vector3D::new(-0.5, 0.3), Vector3D::new(0.0, -0.8), Vector3D::ORIGIN]
    }

    #[test]
    fn hyperbolic_round_trips() {
        for p in pts() {
            assert_eq!(klein_to_poincare(poincare_to_klein(p)), p);
            assert_eq!(upper_to_poincare(poincare_to_upper(p)), p);
            assert_eq!(ortho_to_poincare(poincare_to_ortho(p)), p);
            // The disk maps to the upper half plane shifted down by one.
            assert!(poincare_to_upper(p).y > -1.0);
        }
    }

    #[test]
    fn spherical_round_trips() {
        for p in pts() {
            assert_eq!(sphere_to_plane(plane_to_sphere(p)), p);
            assert_eq!(gnomonic_to_stereo(stereo_to_gnomonic(p)), p);
            let outer = Vector3D::new(1.5, -2.0);
            for q in [p, outer] {
                assert_eq!(from_disks(to_disks(q), false), q);
            }
        }
    }
}
