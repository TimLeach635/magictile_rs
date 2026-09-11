//! Stereographic helpers for the sphere (mimicking Don Hatch's hyperbolic norms).

use crate::vector3d::Vector3D;

pub fn s2e_norm(s_norm: f64) -> f64 {
    (0.5 * s_norm).tan()
}

pub fn e2s_norm(e_norm: f64) -> f64 {
    2.0 * e_norm.atan()
}

/// Unit sphere centered at the origin.
pub fn plane_to_sphere(mut plane_point: Vector3D) -> Vector3D {
    plane_point.z = 0.0;
    let mag_squared = plane_point.mag_squared();
    Vector3D::new3(
        2.0 * plane_point.x / (1.0 + mag_squared),
        2.0 * plane_point.y / (1.0 + mag_squared),
        (mag_squared - 1.0) / (mag_squared + 1.0),
    )
}
