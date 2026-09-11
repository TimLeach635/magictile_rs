//! Reads presentations of regular maps, from
//! https://www.math.auckland.ac.nz/~conder/OrientableRegularMaps101.txt
//! (see also http://dfgm.math.msu.su/files/papers-sym/conder.pdf).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationError(pub String);

impl fmt::Display for PresentationError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid group presentation: {}", self.0)
    }
}

impl std::error::Error for PresentationError {}

const START: char = '(';
const END: char = ')';
const MULT: char = '*';
const POWER: char = '^';

/// Reads a presentation such as `[R^4, S^7, (R*S)^2, (R*S^-1)^6]` as a list of words in the
/// reflections a, b, c (returned as mirror indices 0, 1, 2).
pub fn read_relations(presentation: &str) -> Result<Vec<Vec<usize>>, PresentationError> {
    let presentation = presentation.trim_matches(|c| c == '[' || c == ']');
    let presentation: String = presentation.chars().filter(|c| !c.is_whitespace()).collect();

    presentation
        .split(',')
        .map(|condensed| {
            // Full formula parsing is overkill, since these relations are limited in structure:
            // expand powers of single items, then powers of parenthesized items, then convert
            // R, S, T into reflections.
            let relation = expand_simple_powers(condensed)?;
            let relation = expand_paren_powers(&relation)?;
            let relation = rotary_to_reflections(&relation);
            Ok(word_as_reflections(&relation))
        })
        .collect()
}

fn err(msg: impl Into<String>) -> PresentationError {
    PresentationError(msg.into())
}

/// Expands carets not preceded by a closing paren, e.g. `S^3` -> `S*S*S`.
fn expand_simple_powers(condensed: &str) -> Result<String, PresentationError> {
    let chars: Vec<char> = condensed.chars().collect();
    let carets: Vec<usize> =
        (0..chars.len()).filter(|&i| chars[i] == POWER && (i == 0 || chars[i - 1] != END)).collect();

    // Go backwards so edits don't disturb earlier indices.
    let mut result = chars.clone();
    for &caret in carets.iter().rev() {
        if caret == 0 {
            return Err(err(condensed));
        }
        let power = read_power(&chars, caret)?;
        let expanded = expand_power(power, &chars[caret - 1].to_string())?;
        let length = if power < 0 { 4 } else { 3 };
        let end = (caret - 1 + length).min(result.len());
        result.splice(caret - 1..end, expanded.chars());
    }
    Ok(result.into_iter().collect())
}

/// Reads a single digit power (possibly negative) after a caret.
fn read_power(word: &[char], caret: usize) -> Result<i32, PresentationError> {
    let start = caret + 1;
    let length = if word.get(start) == Some(&'-') { 2 } else { 1 };
    let s: String = word.get(start..start + length).ok_or_else(|| err("missing power"))?.iter().collect();
    s.parse().map_err(|_| err(format!("bad power {s}")))
}

fn expand_power(mut power: i32, val: &str) -> Result<String, PresentationError> {
    let mut val = val.to_string();
    if power < 0 {
        power = -power;
        val = reverse_word(&val)?;
    }
    Ok(vec![val; power as usize].join(&MULT.to_string()))
}

fn expand_paren_powers(condensed: &str) -> Result<String, PresentationError> {
    let chars: Vec<char> = condensed.chars().collect();
    let Some(start) = chars.iter().position(|&c| c == START) else {
        return Ok(condensed.to_string());
    };

    let mut current = start;
    let mut count = 1;
    while count > 0 {
        current += 1;
        match chars.get(current) {
            Some(&START) => count += 1,
            Some(&END) => count -= 1,
            Some(_) => {}
            None => return Err(err("unbalanced parentheses")),
        }
    }
    let end = current;
    if chars.get(end + 1) != Some(&POWER) {
        return Err(err("expected a power after parentheses"));
    }

    let power = read_power(&chars, end + 1)?;
    let sub: String = chars[start + 1..end].iter().collect();
    let mut result: String = chars[..start].iter().collect();
    result += &expand_power(power, &sub)?;
    let rest_start = if power < 0 { end + 4 } else { end + 3 };
    result.extend(chars.get(rest_start..).unwrap_or_default());
    let result = result.trim_end_matches([' ', MULT]).to_string();
    expand_paren_powers(&result)
}

fn reverse_word(word: &str) -> Result<String, PresentationError> {
    if word.contains(POWER) {
        return Err(err("can't reverse a word containing powers"));
    }
    let reversed: Result<Vec<&str>, _> = word
        .split(MULT)
        .rev()
        .map(|s| match s {
            "R" => Ok("r"),
            "r" => Ok("R"),
            "S" => Ok("s"),
            "s" => Ok("S"),
            "T" => Ok("T"),
            other => Err(err(format!("unknown generator {other}"))),
        })
        .collect();
    Ok(reversed?.join("*"))
}

/// Turn a rotary word (R, S, T) into a reflection one (a, b, c).
fn rotary_to_reflections(rotary: &str) -> String {
    rotary.replace('R', "a*b").replace('r', "b*a").replace('S', "b*c").replace('s', "c*b").replace('T', "b")
}

fn word_as_reflections(word: &str) -> Vec<usize> {
    word.trim()
        .split(MULT)
        .filter_map(|s| match s {
            "a" => Some(0),
            "b" => Some(1),
            "c" => Some(2),
            "d" => Some(3),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_relations() {
        let r = read_relations("[ R^4, S^3, (R * S)^2 ]").unwrap();
        // R^4 = (ab)^4
        assert_eq!(r[0], vec![0, 1, 0, 1, 0, 1, 0, 1]);
        // S^3 = (bc)^3
        assert_eq!(r[1], vec![1, 2, 1, 2, 1, 2]);
        // (RS)^2 = (ab bc)^2
        assert_eq!(r[2], vec![0, 1, 1, 2, 0, 1, 1, 2]);
    }

    #[test]
    fn negative_powers_reverse() {
        let r = read_relations("[(R*S^-1)^2]").unwrap();
        // S^-1 = s = cb, so (R s)^2 = (ab cb)^2
        assert_eq!(r[0], vec![0, 1, 2, 1, 0, 1, 2, 1]);
    }
}
