//! A 4-component vector (mostly used as a 2D point), mirroring R3's `Vector3D`.

use crate::complex::Complex;
use crate::nethash::{self, NetKey};
use crate::tolerance::{self, THRESHOLD};
use std::cmp::Ordering;
use std::ops::{Add, AddAssign, Div, DivAssign, Index, IndexMut, Mul, MulAssign, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default)]
pub struct Vector3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Vector3D {
    pub const ORIGIN: Vector3D = Vector3D { x: 0.0, y: 0.0, z: 0.0, w: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Vector3D { x, y, z: 0.0, w: 0.0 }
    }

    pub const fn new3(x: f64, y: f64, z: f64) -> Self {
        Vector3D { x, y, z, w: 0.0 }
    }

    pub const fn new4(x: f64, y: f64, z: f64, w: f64) -> Self {
        Vector3D { x, y, z, w }
    }

    /// A vector that "does not exist" (all NaN).
    pub const fn dne() -> Self {
        Vector3D::new4(f64::NAN, f64::NAN, f64::NAN, f64::NAN)
    }

    /// True if any component is NaN.
    pub fn is_dne(&self) -> bool {
        self.x.is_nan() || self.y.is_nan() || self.z.is_nan() || self.w.is_nan()
    }

    pub fn valid(&self) -> bool {
        !self.is_dne()
    }

    /// Tolerance-based comparison. Exactly-equal vectors (including infinite ones) compare equal,
    /// as do any two DNE vectors.
    pub fn compare_t(&self, other: &Vector3D, threshold: f64) -> bool {
        if self.x == other.x && self.y == other.y && self.z == other.z && self.w == other.w {
            return true;
        }
        if self.is_dne() && other.is_dne() {
            return true;
        }
        if self.is_dne() || other.is_dne() {
            return false;
        }
        tolerance::equal_t(self.x, other.x, threshold)
            && tolerance::equal_t(self.y, other.y, threshold)
            && tolerance::equal_t(self.z, other.z, threshold)
            && tolerance::equal_t(self.w, other.w, threshold)
    }

    pub fn compare(&self, other: &Vector3D) -> bool {
        self.compare_t(other, THRESHOLD)
    }

    /// The original's `GetHashCode`: components rounded to the tolerance's decimals, then the
    /// .NET double hash codes XORed together.
    pub fn net_hash_t(&self, tolerance: f64) -> i32 {
        // DNE vectors all compare equal, so they share a hash.
        if self.is_dne() {
            return nethash::double_hash(f64::NAN);
        }
        let decimals = tolerance::decimals_for(tolerance);
        let h = |d: f64| nethash::double_hash(tolerance::round_digits(d, decimals));
        h(self.x) ^ h(self.y) ^ h(self.z) ^ h(self.w)
    }

    pub fn empty(&mut self) {
        *self = Vector3D::ORIGIN;
    }

    pub fn mag_squared(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    pub fn abs(&self) -> f64 {
        self.mag_squared().sqrt()
    }

    /// Normalizes in place. Returns false (and leaves us unchanged) if we are too small.
    pub fn normalize(&mut self) -> bool {
        let magnitude = self.abs();
        if tolerance::zero(magnitude) {
            return false;
        }
        *self /= magnitude;
        true
    }

    /// Normalize and scale.
    pub fn normalize_scaled(&mut self, scale: f64) -> bool {
        if !self.normalize() {
            return false;
        }
        *self *= scale;
        true
    }

    pub fn normalized(mut self) -> Self {
        self.normalize();
        self
    }

    pub fn is_origin(&self) -> bool {
        *self == Vector3D::ORIGIN
    }

    pub fn dist(&self, v: Vector3D) -> f64 {
        (*self - v).abs()
    }

    pub fn dot(&self, v: Vector3D) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z + self.w * v.w
    }

    /// 3D cross product (the 4th component does not enter into calculations).
    pub fn cross(&self, v: Vector3D) -> Vector3D {
        Vector3D::new3(self.y * v.z - self.z * v.y, self.z * v.x - self.x * v.z, self.x * v.y - self.y * v.x)
    }

    /// Rotate CCW in the XY plane by an angle in radians.
    pub fn rotate_xy(&mut self, angle: f64) {
        let (c1, c2) = (self.x, self.y);
        self.x = angle.cos() * c1 - angle.sin() * c2;
        self.y = angle.sin() * c1 + angle.cos() * c2;
    }

    pub fn rotate90(&mut self) {
        let (c1, c2) = (self.x, self.y);
        self.x = -c2;
        self.y = c1;
    }

    /// Rotate CCW in the XY plane about a center.
    pub fn rotate_xy_about(&mut self, center: Vector3D, angle: f64) {
        *self -= center;
        self.rotate_xy(angle);
        *self += center;
    }

    pub fn rotate_about_axis(&mut self, mut axis: Vector3D, angle: f64) {
        axis.normalize();
        let (ax, ay, az) = (axis.x, axis.y, axis.z);
        let c = angle.cos();
        let s = -angle.sin();
        let t = 1.0 - c;
        let m = [
            [t * ax * ax + c, t * ax * ay - s * az, t * ax * az + s * ay],
            [t * ax * ay + s * az, t * ay * ay + c, t * ay * az - s * ax],
            [t * ax * az - s * ay, t * ay * az + s * ax, t * az * az + c],
        ];
        let (x, y, z) = (self.x, self.y, self.z);
        *self = Vector3D::new3(
            m[0][0] * x + m[1][0] * y + m[2][0] * z,
            m[0][1] * x + m[1][1] * y + m[2][1] * z,
            m[0][2] * x + m[1][2] * y + m[2][2] * z,
        );
    }

    /// Unsigned angle between 0 and pi.
    pub fn angle_to(&self, p2: Vector3D) -> f64 {
        let magmult = self.abs() * p2.abs();
        if tolerance::zero(magmult) {
            return 0.0;
        }
        // Floating point errors can push us slightly out of acos' domain.
        let val = (self.dot(p2) / magmult).clamp(-1.0, 1.0);
        val.acos()
    }

    /// Rounds the first three components (as the original does).
    pub fn round(&mut self, digits: i32) {
        for i in 0..3 {
            self[i] = tolerance::round_digits(self[i], digits);
        }
    }

    pub fn to_complex(&self) -> Complex {
        Complex::new(self.x, self.y)
    }

    pub fn from_complex(c: Complex) -> Self {
        Vector3D::new(c.re, c.im)
    }
}

impl From<Vector3D> for Complex {
    fn from(v: Vector3D) -> Complex {
        v.to_complex()
    }
}

impl From<Complex> for Vector3D {
    fn from(c: Complex) -> Vector3D {
        Vector3D::from_complex(c)
    }
}

/// Tolerance-based equality, matching the original's `==` operator.
/// Note this is not transitive; hashed collections use [`NetKey`].
impl PartialEq for Vector3D {
    fn eq(&self, other: &Self) -> bool {
        self.compare(other)
    }
}

impl Index<usize> for Vector3D {
    type Output = f64;
    fn index(&self, i: usize) -> &f64 {
        match i {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            3 => &self.w,
            _ => panic!("Vector3D index out of range: {i}"),
        }
    }
}

impl IndexMut<usize> for Vector3D {
    fn index_mut(&mut self, i: usize) -> &mut f64 {
        match i {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.z,
            3 => &mut self.w,
            _ => panic!("Vector3D index out of range: {i}"),
        }
    }
}

impl Add for Vector3D {
    type Output = Vector3D;
    fn add(self, v: Vector3D) -> Vector3D {
        Vector3D::new4(self.x + v.x, self.y + v.y, self.z + v.z, self.w + v.w)
    }
}

impl Neg for Vector3D {
    type Output = Vector3D;
    fn neg(self) -> Vector3D {
        Vector3D::new4(-self.x, -self.y, -self.z, -self.w)
    }
}

impl Sub for Vector3D {
    type Output = Vector3D;
    fn sub(self, v: Vector3D) -> Vector3D {
        self + (-v)
    }
}

impl Mul<f64> for Vector3D {
    type Output = Vector3D;
    fn mul(self, s: f64) -> Vector3D {
        Vector3D::new4(self.x * s, self.y * s, self.z * s, self.w * s)
    }
}

impl Mul<Vector3D> for f64 {
    type Output = Vector3D;
    fn mul(self, v: Vector3D) -> Vector3D {
        v * self
    }
}

impl Div<f64> for Vector3D {
    type Output = Vector3D;
    fn div(self, s: f64) -> Vector3D {
        Vector3D::new4(self.x / s, self.y / s, self.z / s, self.w / s)
    }
}

impl AddAssign for Vector3D {
    fn add_assign(&mut self, v: Vector3D) {
        *self = *self + v;
    }
}

impl SubAssign for Vector3D {
    fn sub_assign(&mut self, v: Vector3D) {
        *self = *self - v;
    }
}

impl MulAssign<f64> for Vector3D {
    fn mul_assign(&mut self, s: f64) {
        *self = *self * s;
    }
}

impl DivAssign<f64> for Vector3D {
    fn div_assign(&mut self, s: f64) {
        *self = *self / s;
    }
}

/// Tolerance-safe lexicographic ordering (the original's `Vector3DComparer`).
pub fn compare_lexicographic(v1: &Vector3D, v2: &Vector3D) -> Ordering {
    for i in 0..4 {
        if tolerance::less_than(v1[i], v2[i]) {
            return Ordering::Less;
        }
        if tolerance::greater_than(v1[i], v2[i]) {
            return Ordering::Greater;
        }
    }
    Ordering::Equal
}

/// Vectors as hash keys use the original's tolerance-based equality and rounded hash.
impl NetKey for Vector3D {
    fn net_hash(&self) -> i32 {
        self.net_hash_t(THRESHOLD)
    }

    fn net_eq(&self, other: &Self) -> bool {
        self.compare(other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerance_equality() {
        let a = Vector3D::new(1.0, 2.0);
        assert_eq!(a, Vector3D::new(1.0 + 1e-8, 2.0));
        assert_ne!(a, Vector3D::new(1.0 + 1e-5, 2.0));
        assert_eq!(Vector3D::dne(), Vector3D::new(f64::NAN, 0.0));
        let inf = Vector3D::new(f64::INFINITY, f64::INFINITY);
        assert_eq!(inf, inf);
    }

    #[test]
    fn hashes_round_like_the_original() {
        assert_eq!(Vector3D::new(0.1234564, 0.0).net_hash(), Vector3D::new(0.1234561, -0.0).net_hash());
        assert_ne!(Vector3D::new(0.1234564, 0.0).net_hash(), Vector3D::new(0.1234566, 0.0).net_hash());
        assert_eq!(Vector3D::dne().net_hash(), Vector3D::new(0.0, f64::NAN).net_hash());
        // XOR of equal component hashes cancels, so diagonal points all hash alike.
        assert_eq!(Vector3D::new(0.25, 0.25).net_hash(), Vector3D::new(0.5, 0.5).net_hash());
    }

    #[test]
    fn rotation() {
        let mut v = Vector3D::new(1.0, 0.0);
        v.rotate_xy(std::f64::consts::FRAC_PI_2);
        assert_eq!(v, Vector3D::new(0.0, 1.0));
        let mut v = Vector3D::new3(1.0, 0.0, 0.0);
        v.rotate_about_axis(Vector3D::new3(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_2);
        assert_eq!(v, Vector3D::new3(0.0, 1.0, 0.0));
    }
}
