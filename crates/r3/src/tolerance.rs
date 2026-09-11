//! Floating point tolerance helpers.

/// Made less strict than 1e-7 to avoid some problems near the Poincaré boundary.
pub const THRESHOLD: f64 = 0.000001;

pub fn equal(d1: f64, d2: f64) -> bool {
    zero(d1 - d2)
}

pub fn zero(d: f64) -> bool {
    zero_t(d, THRESHOLD)
}

pub fn less_than(d1: f64, d2: f64) -> bool {
    less_than_t(d1, d2, THRESHOLD)
}

pub fn greater_than(d1: f64, d2: f64) -> bool {
    greater_than_t(d1, d2, THRESHOLD)
}

pub fn less_than_or_equal(d1: f64, d2: f64) -> bool {
    d1 <= d2 + THRESHOLD
}

pub fn greater_than_or_equal(d1: f64, d2: f64) -> bool {
    d1 >= d2 - THRESHOLD
}

pub fn equal_t(d1: f64, d2: f64, threshold: f64) -> bool {
    zero_t(d1 - d2, threshold)
}

pub fn zero_t(d: f64, threshold: f64) -> bool {
    d > -threshold && d < threshold
}

pub fn less_than_t(d1: f64, d2: f64, threshold: f64) -> bool {
    d1 < d2 - threshold
}

pub fn greater_than_t(d1: f64, d2: f64, threshold: f64) -> bool {
    d1 > d2 + threshold
}

/// The number of decimals the original code rounds to when hashing with a tolerance.
pub fn decimals_for(tolerance: f64) -> i32 {
    (1.0 / tolerance).log10() as i32
}

/// Rounds to a number of decimal digits exactly as .NET's `Math.Round(value, digits)` does
/// (scale, round half to even, unscale).
pub fn round_digits(value: f64, digits: i32) -> f64 {
    const DOUBLE_ROUND_LIMIT: f64 = 1e16;
    if value.abs() < DOUBLE_ROUND_LIMIT {
        let power10 = 10f64.powi(digits);
        (value * power10).round_ties_even() / power10
    } else {
        value
    }
}

pub fn degrees_to_radians(value: f64) -> f64 {
    value / 180.0 * std::f64::consts::PI
}

pub fn radians_to_degrees(value: f64) -> f64 {
    value / std::f64::consts::PI * 180.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals() {
        assert_eq!(decimals_for(THRESHOLD), 6);
        assert_eq!(decimals_for(0.0001), 4);
    }

    #[test]
    fn rounding_matches_dotnet() {
        assert_eq!(round_digits(1.2345675, 6), 1.234568);
        assert_eq!(round_digits(0.5, 0), 0.0);
        assert_eq!(round_digits(1.5, 0), 2.0);
        assert_eq!(round_digits(-2.5, 0), -2.0);
        assert!(round_digits(f64::INFINITY, 6).is_infinite());
    }
}
