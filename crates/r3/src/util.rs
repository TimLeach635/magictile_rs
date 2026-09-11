//! Small helpers.

use std::cmp::Ordering;

/// A stable sort that tolerates comparators which aren't total orders (e.g. tolerance-based
/// comparisons), unlike `slice::sort_by`, which may panic on them.
pub fn stable_sort_by<T>(v: &mut [T], mut compare: impl FnMut(&T, &T) -> Ordering) {
    // Insertion sort: the slices we sort are small.
    for i in 1..v.len() {
        let mut j = i;
        while j > 0 && compare(&v[j - 1], &v[j]) == Ordering::Greater {
            v.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// A stable sort by a floating point key, as C#'s `OrderBy` on doubles does. NaNs sort first,
/// matching .NET's `double.CompareTo`.
pub fn sort_by_f64_key<T>(v: Vec<T>, mut key: impl FnMut(&T) -> f64) -> Vec<T> {
    let mut keyed: Vec<(f64, T)> = v.into_iter().map(|t| (key(&t), t)).collect();
    // A total order (NaNs handled), so the standard stable sort is safe here.
    keyed.sort_by(|a, b| net_compare_f64(a.0, b.0));
    keyed.into_iter().map(|(_, t)| t).collect()
}

/// .NET's `double.CompareTo` (NaN is less than everything, and equal to itself).
pub fn net_compare_f64(a: f64, b: f64) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.partial_cmp(&b).unwrap(),
    }
}

/// Sums in order starting from +0.0, as a C# accumulation loop does.
pub fn sum(values: impl IntoIterator<Item = f64>) -> f64 {
    values.into_iter().fold(0.0, |a, b| a + b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts() {
        let mut v = vec![3, 1, 2, 1];
        stable_sort_by(&mut v, |a, b| a.cmp(b));
        assert_eq!(v, vec![1, 1, 2, 3]);

        let w = vec![("a", 2.0), ("b", f64::NAN), ("c", 1.0), ("d", 1.0)];
        let w = sort_by_f64_key(w, |t| t.1);
        let names: Vec<_> = w.iter().map(|t| t.0).collect();
        assert_eq!(names, vec!["b", "c", "d", "a"]);
    }
}
