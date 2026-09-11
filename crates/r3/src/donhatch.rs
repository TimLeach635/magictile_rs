//! Numerically careful hyperbolic functions, from Don Hatch.

pub fn expm1(x: f64) -> f64 {
    let u = x.exp();
    if u == 1.0 {
        return x;
    }
    if u - 1.0 == -1.0 {
        return -1.0;
    }
    (u - 1.0) * x / u.ln()
}

pub fn log1p(x: f64) -> f64 {
    let u = 1.0 + x;
    u.ln() - ((u - 1.0) - x) / u
}

pub fn tanh(x: f64) -> f64 {
    let u = expm1(x);
    u / (u * (u + 2.0) + 2.0) * (u + 2.0)
}

pub fn atanh(x: f64) -> f64 {
    0.5 * log1p(2.0 * x / (1.0 - x))
}

pub fn sinh(x: f64) -> f64 {
    let u = expm1(x);
    0.5 * u / (u + 1.0) * (u + 2.0)
}

pub fn asinh(x: f64) -> f64 {
    log1p(x * (1.0 + x / ((x * x + 1.0).sqrt() + 1.0)))
}

pub fn cosh(x: f64) -> f64 {
    let e_x = x.exp();
    (e_x + 1.0 / e_x) * 0.5
}

pub fn acosh(x: f64) -> f64 {
    2.0 * (((x + 1.0) * 0.5).sqrt() + ((x - 1.0) * 0.5).sqrt()).ln()
}

/// Hyperbolic to Euclidean norm (distance from the origin) in the Poincaré disk.
pub fn h2e_norm(h_norm: f64) -> f64 {
    if h_norm.is_nan() {
        return 1.0;
    }
    tanh(0.5 * h_norm)
}

pub fn e2h_norm(e_norm: f64) -> f64 {
    2.0 * atanh(e_norm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for x in [0.1, 0.5, 1.0, 3.0] {
            assert!((tanh(x) - x.tanh()).abs() < 1e-14);
            assert!((atanh(tanh(x)) - x).abs() < 1e-12);
            assert!((acosh(cosh(x)) - x).abs() < 1e-12);
            assert!((e2h_norm(h2e_norm(x)) - x).abs() < 1e-12);
        }
    }
}
