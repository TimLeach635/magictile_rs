//! Complex numbers with the exact arithmetic semantics of .NET's `System.Numerics.Complex`.
//!
//! We don't use `num-complex` because puzzle building hashes rounded coordinates, so small
//! numerical differences (division algorithm, reciprocal of zero, signed zeros from
//! scalar multiplication) can change which points are identified.

use std::ops::{Add, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    pub const ZERO: Complex = Complex { re: 0.0, im: 0.0 };
    pub const ONE: Complex = Complex { re: 1.0, im: 0.0 };
    pub const I: Complex = Complex { re: 0.0, im: 1.0 };

    pub const fn new(re: f64, im: f64) -> Self {
        Complex { re, im }
    }

    /// Scaled hypot, as .NET computes `Complex.Abs`.
    pub fn magnitude(self) -> f64 {
        if self.re.is_infinite() || self.im.is_infinite() {
            return f64::INFINITY;
        }
        let c = self.re.abs();
        let d = self.im.abs();
        if c > d {
            let r = d / c;
            c * (1.0 + r * r).sqrt()
        } else if d == 0.0 {
            c // c is either 0.0 or NaN
        } else {
            let r = c / d;
            d * (1.0 + r * r).sqrt()
        }
    }

    pub fn phase(self) -> f64 {
        self.im.atan2(self.re)
    }

    pub fn from_polar(magnitude: f64, phase: f64) -> Self {
        Complex::new(magnitude * phase.cos(), magnitude * phase.sin())
    }

    pub fn conj(self) -> Self {
        Complex::new(self.re, -self.im)
    }

    pub fn sqrt(self) -> Self {
        Complex::from_polar(self.magnitude().sqrt(), self.phase() / 2.0)
    }

    /// Note: the reciprocal of zero is zero, as in .NET.
    pub fn reciprocal(self) -> Self {
        if self.re == 0.0 && self.im == 0.0 {
            return Complex::ZERO;
        }
        Complex::ONE / self
    }

    pub fn is_nan(self) -> bool {
        self.re.is_nan() || self.im.is_nan()
    }
}

impl From<f64> for Complex {
    fn from(re: f64) -> Self {
        Complex::new(re, 0.0)
    }
}

impl Add for Complex {
    type Output = Complex;
    fn add(self, o: Complex) -> Complex {
        Complex::new(self.re + o.re, self.im + o.im)
    }
}

impl Sub for Complex {
    type Output = Complex;
    fn sub(self, o: Complex) -> Complex {
        Complex::new(self.re - o.re, self.im - o.im)
    }
}

impl Neg for Complex {
    type Output = Complex;
    fn neg(self) -> Complex {
        Complex::new(-self.re, -self.im)
    }
}

impl Mul for Complex {
    type Output = Complex;
    fn mul(self, o: Complex) -> Complex {
        Complex::new(self.re * o.re - self.im * o.im, self.im * o.re + self.re * o.im)
    }
}

/// Smith's algorithm, as .NET uses.
impl Div for Complex {
    type Output = Complex;
    fn div(self, o: Complex) -> Complex {
        let (a, b, c, d) = (self.re, self.im, o.re, o.im);
        if d.abs() < c.abs() {
            let doc = d / c;
            Complex::new((a + b * doc) / (c + d * doc), (b - a * doc) / (c + d * doc))
        } else {
            let cod = c / d;
            Complex::new((b + a * cod) / (d + c * cod), (-a + b * cod) / (d + c * cod))
        }
    }
}

// Scalar operations promote the scalar to a complex number first (as C# implicit conversions do),
// which preserves .NET's signed-zero and NaN behaviour.

impl Add<f64> for Complex {
    type Output = Complex;
    fn add(self, s: f64) -> Complex {
        self + Complex::from(s)
    }
}

impl Sub<f64> for Complex {
    type Output = Complex;
    fn sub(self, s: f64) -> Complex {
        self - Complex::from(s)
    }
}

impl Mul<f64> for Complex {
    type Output = Complex;
    fn mul(self, s: f64) -> Complex {
        self * Complex::from(s)
    }
}

impl Mul<Complex> for f64 {
    type Output = Complex;
    fn mul(self, c: Complex) -> Complex {
        Complex::from(self) * c
    }
}

impl Div<f64> for Complex {
    type Output = Complex;
    fn div(self, s: f64) -> Complex {
        self / Complex::from(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_arithmetic() {
        let a = Complex::new(1.0, 2.0);
        let b = Complex::new(3.0, -1.0);
        assert_eq!(a * b, Complex::new(5.0, 5.0));
        let q = a / b;
        assert!((q.re - 0.1).abs() < 1e-15 && (q.im - 0.7).abs() < 1e-15);
        assert_eq!(Complex::ZERO.reciprocal(), Complex::ZERO);
        let s = Complex::new(-4.0, 0.0).sqrt();
        assert!(s.re.abs() < 1e-15 && (s.im - 2.0).abs() < 1e-15);
    }
}
