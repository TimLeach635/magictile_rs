//! Somewhat hackish helpers for dealing with points projected to infinity.

use crate::complex::Complex;
use crate::vector3d::Vector3D;

pub const FINITE_SCALE: f64 = 10000.0;
pub const INFINITE_SCALE: f64 = 500000.0;

pub const INFINITY_VECTOR: Vector3D = Vector3D::new3(f64::INFINITY, f64::INFINITY, f64::INFINITY);
pub const INFINITY_VECTOR_2D: Vector3D = Vector3D::new(f64::INFINITY, f64::INFINITY);
pub const LARGE_FINITE_VECTOR: Vector3D = Vector3D::new3(FINITE_SCALE, FINITE_SCALE, FINITE_SCALE);

pub fn is_infinite_f(input: f64) -> bool {
    input.is_nan() || input.is_infinite() || input.abs() >= INFINITE_SCALE
}

pub fn is_infinite(input: Vector3D) -> bool {
    is_infinite_f(input.x)
        || is_infinite_f(input.y)
        || is_infinite_f(input.z)
        || is_infinite_f(input.w)
        || input.abs() > INFINITE_SCALE
}

pub fn is_infinite_c(input: Complex) -> bool {
    is_infinite_f(input.re) || is_infinite_f(input.im)
}

/// Replaces infinite vectors with a large finite one.
pub fn infinity_safe(input: Vector3D) -> Vector3D {
    if is_infinite(input) { LARGE_FINITE_VECTOR } else { input }
}
