//! Number formatting and parsing compatible with the original (.NET Framework) program.
//!
//! Puzzle IDs are built with .NET's default `double.ToString()` (the "G" format, 15 significant
//! digits), and `menu.xml` and saved macros refer to those IDs, so we must reproduce it exactly.

/// .NET Framework's `double.ToString()` with the invariant culture ("G": up to 15 significant
/// digits, scientific notation for exponents below -4 or from 15 up).
pub fn format_g(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if value == 0.0 {
        return "0".into();
    }

    // 15 significant digits, correctly rounded.
    let sci = format!("{:.14e}", value);
    let (mantissa, exponent) = sci.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };

    let mut out = String::new();
    if negative {
        out.push('-');
    }

    if (-4..15).contains(&exponent) {
        if exponent < 0 {
            out.push_str("0.");
            for _ in 0..(-exponent - 1) {
                out.push('0');
            }
            out.push_str(digits);
        } else {
            let int_len = exponent as usize + 1;
            if digits.len() <= int_len {
                out.push_str(digits);
                for _ in digits.len()..int_len {
                    out.push('0');
                }
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('E');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exponent.abs()));
    }
    out
}

/// Formats a double for XML (as `XmlConvert.ToString` does, up to digit choice: we write the
/// shortest round-tripping representation, which .NET parses back to the same value).
pub fn format_xml_double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "INF".into() } else { "-INF".into() };
    }
    if value == 0.0 {
        return "0".into();
    }
    format!("{value}")
}

/// Parses a double the way .NET's invariant `double.Parse` / `XmlConvert.ToDouble` accept the
/// values found in MagicTile files.
pub fn parse_double(s: &str) -> Option<f64> {
    let s = s.trim();
    match s {
        "INF" | "Infinity" | "∞" => return Some(f64::INFINITY),
        "-INF" | "-Infinity" | "-∞" => return Some(f64::NEG_INFINITY),
        "NaN" => return Some(f64::NAN),
        _ => {}
    }
    s.parse::<f64>().ok()
}

pub fn parse_int(s: &str) -> Option<i32> {
    s.trim().parse::<i32>().ok()
}

pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim() {
        "true" | "True" | "1" => Some(true),
        "false" | "False" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g_format_matches_dotnet() {
        assert_eq!(format_g(0.67), "0.67");
        assert_eq!(format_g(1.0), "1");
        assert_eq!(format_g(0.025), "0.025");
        assert_eq!(format_g(2.0 / 3.0), "0.666666666666667");
        assert_eq!(format_g(0.1 + 0.2), "0.3");
        assert_eq!(format_g(1e-5), "1E-05");
        assert_eq!(format_g(0.0001), "0.0001");
        assert_eq!(format_g(123456789012345.0), "123456789012345");
        assert_eq!(format_g(1e15), "1E+15");
        assert_eq!(format_g(-2.5), "-2.5");
        assert_eq!(format_g(-0.0), "0");
        assert_eq!(format_g(100.0), "100");
    }

    #[test]
    fn parsing() {
        assert_eq!(parse_double(".67"), Some(0.67));
        assert_eq!(parse_double(" 1E-05 "), Some(1e-5));
        assert_eq!(parse_double("INF"), Some(f64::INFINITY));
        assert_eq!(parse_bool("true"), Some(true));
    }
}
