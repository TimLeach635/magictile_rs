//! Möbius transformations of the (extended) complex plane.

use crate::complex::Complex;
use crate::donhatch;
use crate::geometry2d::Geometry;
use crate::infinity;
use crate::nethash::NetKey;
use crate::spherical2d;
use crate::tolerance;
use crate::vector3d::Vector3D;
use std::ops::Mul;

/// Anything that can be used to transform points.
pub trait Transform {
    fn apply_c(&self, z: Complex) -> Complex;

    /// Like `apply_c`, but maps infinite inputs sensibly and infinite results to a standard
    /// infinite point.
    fn apply_infinite_safe_c(&self, z: Complex) -> Complex;

    fn apply(&self, v: Vector3D) -> Vector3D {
        Vector3D::from_complex(self.apply_c(v.to_complex()))
    }

    fn apply_infinite_safe(&self, v: Vector3D) -> Vector3D {
        Vector3D::from_complex(self.apply_infinite_safe_c(v.to_complex()))
    }
}

/// z -> (Az + B) / (Cz + D). Methods mutate in place, like the original struct's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mobius {
    pub a: Complex,
    pub b: Complex,
    pub c: Complex,
    pub d: Complex,
}

impl Mobius {
    pub fn new(a: Complex, b: Complex, c: Complex, d: Complex) -> Self {
        Mobius { a, b, c, d }
    }

    pub fn identity() -> Self {
        Mobius::new(Complex::ONE, Complex::ZERO, Complex::ZERO, Complex::ONE)
    }

    pub fn scale(scale: f64) -> Self {
        Mobius::new(scale.into(), Complex::ZERO, Complex::ZERO, Complex::ONE)
    }

    /// Normalize so that ad - bc = 1.
    pub fn normalize(&mut self) {
        // See Visual Complex Analysis, p150.
        let k = (self.a * self.d - self.b * self.c).sqrt().reciprocal();
        self.scale_components(k);
    }

    pub fn scale_components(&mut self, k: Complex) {
        self.a = self.a * k;
        self.b = self.b * k;
        self.c = self.c * k;
        self.d = self.d * k;
    }

    pub fn trace(&self) -> Complex {
        self.a + self.d
    }

    pub fn trace_squared(&self) -> Complex {
        self.trace() * self.trace()
    }

    /// An isometry of the given geometry that rotates CCW by `angle` about the origin, then
    /// translates the origin to `p` (and -p to the origin).
    pub fn isometry(&mut self, g: Geometry, angle: f64, p: impl Into<Complex>) {
        // In the hyperbolic case, any isometry of the Poincaré disk can be written as
        // (T*z + P)/(1 + conj(P)*T*z), with |P| < 1 and |T| = 1. The other geometries are
        // simple variations of the C coefficient.
        let p = p.into();
        let t = Complex::new(angle.cos(), angle.sin());
        self.a = t;
        self.b = p;
        self.d = Complex::ONE;
        self.c = match g {
            Geometry::Spherical => p.conj() * t * -1.0,
            Geometry::Euclidean => Complex::ZERO,
            Geometry::Hyperbolic => p.conj() * t,
        };
    }

    pub fn unity(&mut self) {
        *self = Mobius::identity();
    }

    /// The pure translation taking p1 to p2 (moves the origin straight in some direction).
    /// From Don Hatch's hyperbolic applet.
    pub fn pure_translation(&mut self, g: Geometry, p1: impl Into<Complex>, p2: impl Into<Complex>) {
        let (p1, p2) = (p1.into(), p2.into());
        let a = p2 - p1;
        let b = p2 * p1;
        let denom = 1.0 - (b.re * b.re + b.im * b.im);
        let p = Complex::new((a.re * (1.0 + b.re) + a.im * b.im) / denom, (a.im * (1.0 - b.re) + a.re * b.im) / denom);
        self.isometry(g, 0.0, p);
        self.normalize();
    }

    /// Move from p1 -> p2 along a geodesic. `factor` allows going only part of the way
    /// (only implemented for hyperbolic geometry).
    pub fn geodesic(&mut self, g: Geometry, p1: impl Into<Complex>, p2: impl Into<Complex>, factor: f64) {
        let (p1, p2) = (p1.into(), p2.into());
        let mut t = Mobius::default();
        t.isometry(g, 0.0, p1 * -1.0);
        let mut p2t = t.apply_c(p2);

        if factor != 1.0 && g == Geometry::Hyperbolic {
            let new_mag = donhatch::h2e_norm(donhatch::e2h_norm(p2t.magnitude()) * factor);
            let mut temp = Vector3D::from_complex(p2t);
            temp.normalize();
            temp *= new_mag;
            p2t = temp.to_complex();
        }

        let (mut m1, mut m2) = (Mobius::default(), Mobius::default());
        m1.isometry(g, 0.0, p1 * -1.0);
        m2.isometry(g, 0.0, p2t);
        let m3 = m1.inverse();
        *self = m3 * m2 * m1;
    }

    pub fn hyperbolic(&mut self, g: Geometry, fixed_plus: impl Into<Complex>, scale: f64) {
        let fixed_plus = fixed_plus.into();

        // To the origin.
        let mut m1 = Mobius::default();
        m1.isometry(g, 0.0, fixed_plus * -1.0);

        // Scale.
        let m2 = Mobius::new(scale.into(), Complex::ZERO, Complex::ZERO, Complex::ONE);

        // Back. (m1.inverse() doesn't work well if fixed_plus is on the disk boundary.)
        let mut m3 = Mobius::default();
        m3.isometry(g, 0.0, fixed_plus);

        *self = m3 * m2 * m1;
    }

    /// A hyperbolic transformation using an absolute offset, specified in the geometry.
    pub fn hyperbolic2(&mut self, g: Geometry, fixed_plus: impl Into<Complex>, point: impl Into<Complex>, offset: f64) {
        let fixed_plus = fixed_plus.into();
        let mut m = Mobius::default();
        m.isometry(g, 0.0, fixed_plus * -1.0);
        let e_radius = m.apply_c(point.into()).magnitude();

        let scale = match g {
            Geometry::Spherical => {
                let s_radius = spherical2d::e2s_norm(e_radius) + offset;
                spherical2d::s2e_norm(s_radius) / e_radius
            }
            Geometry::Euclidean => (e_radius + offset) / e_radius,
            Geometry::Hyperbolic => {
                let h_radius = donhatch::e2h_norm(e_radius) + offset;
                donhatch::h2e_norm(h_radius) / e_radius
            }
        };

        self.hyperbolic(g, fixed_plus, scale);
    }

    /// A rotation by `angle` about `fixed_plus`.
    pub fn elliptic(&mut self, g: Geometry, fixed_plus: impl Into<Complex>, angle: f64) {
        let fixed_plus = fixed_plus.into();
        let mut origin = Mobius::default();
        origin.isometry(g, 0.0, fixed_plus * -1.0);

        let mut rotate = Mobius::default();
        rotate.isometry(g, angle, Complex::ZERO);

        *self = origin.inverse() * rotate * origin;
    }

    /// Transforms the unit disk to the upper half plane.
    pub fn upper_half_plane(&mut self) {
        self.map_points3(-Complex::I, Complex::ONE, Complex::I);
    }

    /// Maps z1 to zero, z2 to one, and z3 to infinity.
    pub fn map_points3(&mut self, z1: Complex, z2: Complex, z3: Complex) {
        // If one of the zi is infinite, the formula comes from dividing by zi and taking the limit.
        if infinity::is_infinite_c(z1) {
            self.a = Complex::ZERO;
            self.b = -1.0 * (z2 - z3);
            self.c = Complex::from(-1.0);
            self.d = z3;
        } else if infinity::is_infinite_c(z2) {
            self.a = Complex::ONE;
            self.b = -z1;
            self.c = Complex::ONE;
            self.d = -z3;
        } else if infinity::is_infinite_c(z3) {
            self.a = Complex::from(-1.0);
            self.b = z1;
            self.c = Complex::ZERO;
            self.d = -1.0 * (z2 - z1);
        } else {
            self.a = z2 - z3;
            self.b = -z1 * (z2 - z3);
            self.c = z2 - z1;
            self.d = -z3 * (z2 - z1);
        }
        self.normalize();
    }

    /// Maps the z points to the respective w points.
    pub fn map_points(
        &mut self,
        z1: impl Into<Complex>,
        z2: impl Into<Complex>,
        z3: impl Into<Complex>,
        w1: impl Into<Complex>,
        w2: impl Into<Complex>,
        w3: impl Into<Complex>,
    ) {
        let (mut m1, mut m2) = (Mobius::default(), Mobius::default());
        m1.map_points3(z1.into(), z2.into(), z3.into());
        m2.map_points3(w1.into(), w2.into(), w3.into());
        *self = m2.inverse() * m1;
    }

    /// Applies us to the point at infinity.
    pub fn apply_to_infinite(&self) -> Vector3D {
        if self.c == Complex::ZERO {
            return infinity::INFINITY_VECTOR_2D;
        }
        Vector3D::from_complex(self.a / self.c)
    }

    pub fn inverse(&self) -> Mobius {
        let mut result = Mobius::new(self.d, -self.b, -self.c, self.a);
        result.normalize();
        result
    }

    /// Only here for a numerical accuracy hack.
    pub fn round(&mut self, digits: i32) {
        let r = |c: Complex| Complex::new(tolerance::round_digits(c.re, digits), tolerance::round_digits(c.im, digits));
        self.a = r(self.a);
        self.b = r(self.b);
        self.c = r(self.c);
        self.d = r(self.d);
    }
}

impl Transform for Mobius {
    fn apply_c(&self, z: Complex) -> Complex {
        (self.a * z + self.b) / (self.c * z + self.d)
    }

    fn apply_infinite_safe_c(&self, z: Complex) -> Complex {
        if infinity::is_infinite_c(z) {
            return self.apply_to_infinite().to_complex();
        }
        let result = self.apply_c(z);
        if infinity::is_infinite_c(result) {
            return infinity::INFINITY_VECTOR_2D.to_complex();
        }
        result
    }
}

impl Mul for Mobius {
    type Output = Mobius;
    fn mul(self, m2: Mobius) -> Mobius {
        let m1 = self;
        let mut result = Mobius::new(
            m1.a * m2.a + m1.b * m2.c,
            m1.a * m2.b + m1.b * m2.d,
            m1.c * m2.a + m1.d * m2.c,
            m1.c * m2.b + m1.d * m2.d,
        );
        result.normalize();
        result
    }
}

/// Transforms as hash keys, matching the original `MobiusEqualityComparer`
/// (normalized coefficients compared as vectors).
impl NetKey for Mobius {
    fn net_hash(&self) -> i32 {
        let mut m = *self;
        m.normalize();
        let h = |c: Complex| Vector3D::from_complex(c).net_hash();
        h(m.a) ^ h(m.b) ^ h(m.c) ^ h(m.d)
    }

    fn net_eq(&self, other: &Self) -> bool {
        let (mut m1, mut m2) = (*self, *other);
        m1.normalize();
        m2.normalize();
        let v = Vector3D::from_complex;
        v(m1.a) == v(m2.a) && v(m1.b) == v(m2.b) && v(m1.c) == v(m2.c) && v(m1.d) == v(m2.d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn close(a: Complex, b: Complex) -> bool {
        (a - b).magnitude() < 1e-10
    }

    #[test]
    fn map_points_maps_points() {
        let z = [Complex::new(0.1, 0.2), Complex::new(-0.3, 0.1), Complex::new(0.0, -0.4)];
        let w = [Complex::new(0.5, 0.0), Complex::new(0.2, 0.3), Complex::new(-0.1, -0.1)];
        let mut m = Mobius::default();
        m.map_points(z[0], z[1], z[2], w[0], w[1], w[2]);
        for i in 0..3 {
            assert!(close(m.apply_c(z[i]), w[i]));
        }
        let inv = m.inverse();
        for i in 0..3 {
            assert!(close(inv.apply_c(w[i]), z[i]));
        }
    }

    #[test]
    fn elliptic_rotates_about_fixed_point() {
        for g in [Geometry::Spherical, Geometry::Euclidean, Geometry::Hyperbolic] {
            let fixed = Complex::new(0.2, 0.1);
            let mut m = Mobius::default();
            m.elliptic(g, fixed, PI / 3.0);
            assert!(close(m.apply_c(fixed), fixed));
            // Six sixth-turns bring a point back.
            let p = Complex::new(0.3, -0.2);
            let mut q = p;
            for _ in 0..6 {
                q = m.apply_c(q);
            }
            assert!(close(p, q));
        }
    }

    #[test]
    fn hyperbolic_isometry_preserves_disk() {
        let mut m = Mobius::default();
        m.isometry(Geometry::Hyperbolic, 0.7, Complex::new(0.3, 0.4));
        for i in 0..8 {
            let t = i as f64 * PI / 4.0;
            let z = m.apply_c(Complex::new(t.cos(), t.sin()));
            assert!((z.magnitude() - 1.0).abs() < 1e-12);
        }
        assert!(close(m.apply_c(Complex::ZERO), Complex::new(0.3, 0.4)));
    }

    #[test]
    fn pure_translation_takes_p1_to_p2() {
        let (p1, p2) = (Complex::new(0.1, 0.2), Complex::new(-0.3, 0.4));
        let mut m = Mobius::default();
        m.pure_translation(Geometry::Hyperbolic, p1, p2);
        assert!(close(m.apply_c(p1), p2));
    }

    #[test]
    fn keys_identify_equal_transforms() {
        let mut m1 = Mobius::default();
        m1.elliptic(Geometry::Hyperbolic, Complex::new(0.2, 0.0), 0.5);
        let mut m2 = m1;
        m2.scale_components(Complex::new(3.0, 0.0));
        assert_eq!(m1.net_hash(), m2.net_hash());
        assert!(m1.net_eq(&m2));
    }
}
