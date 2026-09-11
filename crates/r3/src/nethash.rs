//! Hashed collections with the exact lookup semantics of .NET's `Dictionary`, `HashSet` and
//! `Enumerable.Distinct`.
//!
//! The original code uses geometric objects as hash keys, with tolerance-based `Equals` and hash
//! codes built from rounded coordinates. Those two are not consistent with each other (values
//! within tolerance can round differently, and XORed hash codes collide in systematic ways), so
//! which keys are identified depends on the details of .NET's implementation:
//!
//! * a stored key matches a probe when their hash codes (masked to 31 bits) are equal and
//!   `stored.Equals(probe)` holds;
//! * bucket chains are searched most-recently-inserted first;
//! * enumeration is in insertion order (nothing is ever removed).
//!
//! Puzzle building identifies cells and twists this way, and save files refer to the resulting
//! indices, so we reproduce it exactly.

use std::collections::HashMap;

/// A key with .NET-style hash code and equality.
pub trait NetKey {
    fn net_hash(&self) -> i32;
    /// `self` plays the role of the stored key, `other` the probe.
    fn net_eq(&self, other: &Self) -> bool;
}

/// .NET's hash code for a NaN produced by `0.0 / 0.0` (which is what `double.NaN` is).
const NET_NAN_BITS: u64 = 0xFFF8_0000_0000_0000;

/// .NET Framework's `double.GetHashCode()`.
pub fn double_hash(d: f64) -> i32 {
    if d == 0.0 {
        // 0 and -0 hash alike.
        return 0;
    }
    let bits = if d.is_nan() { NET_NAN_BITS } else { d.to_bits() };
    (bits as i32) ^ ((bits >> 32) as i32)
}

fn masked(h: i32) -> i32 {
    h & 0x7FFF_FFFF
}

/// An insertion-ordered map with .NET `Dictionary` lookup semantics.
#[derive(Clone, Debug)]
pub struct NetMap<K, V> {
    entries: Vec<(K, V)>,
    buckets: HashMap<i32, Vec<usize>>,
}

impl<K, V> Default for NetMap<K, V> {
    fn default() -> Self {
        NetMap { entries: Vec::new(), buckets: HashMap::new() }
    }
}

impl<K: NetKey, V> NetMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.buckets.clear();
    }

    /// The insertion index of the entry matching `k`.
    pub fn index_of(&self, k: &K) -> Option<usize> {
        self.buckets.get(&masked(k.net_hash()))?.iter().rev().copied().find(|&i| self.entries[i].0.net_eq(k))
    }

    pub fn contains_key(&self, k: &K) -> bool {
        self.index_of(k).is_some()
    }

    pub fn get(&self, k: &K) -> Option<&V> {
        self.index_of(k).map(|i| &self.entries[i].1)
    }

    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        self.index_of(k).map(|i| &mut self.entries[i].1)
    }

    /// Like C#'s `dict[key] = value`: an existing entry keeps its original key and position.
    /// Returns the entry's index.
    pub fn insert(&mut self, k: K, value: V) -> usize {
        match self.index_of(&k) {
            Some(i) => {
                self.entries[i].1 = value;
                i
            }
            None => self.push(k, value),
        }
    }

    /// Like C#'s `Dictionary.Add`/`HashSet.Add`: returns false (leaving the map unchanged) if the
    /// key is already present.
    pub fn try_add(&mut self, k: K, value: V) -> bool {
        if self.contains_key(&k) {
            return false;
        }
        self.push(k, value);
        true
    }

    pub fn get_or_insert_with(&mut self, k: K, f: impl FnOnce() -> V) -> &mut V {
        let i = match self.index_of(&k) {
            Some(i) => i,
            None => self.push(k, f()),
        };
        &mut self.entries[i].1
    }

    fn push(&mut self, k: K, value: V) -> usize {
        let i = self.entries.len();
        self.buckets.entry(masked(k.net_hash())).or_default().push(i);
        self.entries.push((k, value));
        i
    }

    pub fn get_index(&self, i: usize) -> Option<(&K, &V)> {
        self.entries.get(i).map(|(k, v)| (k, v))
    }

    /// Entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.iter_mut().map(|(_, v)| v)
    }

    pub fn into_entries(self) -> Vec<(K, V)> {
        self.entries
    }
}

/// An insertion-ordered set with .NET `HashSet` semantics.
#[derive(Clone, Debug)]
pub struct NetSet<K> {
    map: NetMap<K, ()>,
}

impl<K> Default for NetSet<K> {
    fn default() -> Self {
        NetSet { map: NetMap::default() }
    }
}

impl<K: NetKey> NetSet<K> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the item was not already present.
    pub fn insert(&mut self, k: K) -> bool {
        self.map.try_add(k, ())
    }

    pub fn contains(&self, k: &K) -> bool {
        self.map.contains_key(k)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &K> {
        self.map.keys()
    }

    pub fn into_vec(self) -> Vec<K> {
        self.map.into_entries().into_iter().map(|(k, _)| k).collect()
    }
}

impl<K: NetKey> FromIterator<K> for NetSet<K> {
    fn from_iter<I: IntoIterator<Item = K>>(iter: I) -> Self {
        let mut set = NetSet::new();
        for k in iter {
            set.insert(k);
        }
        set
    }
}

/// `Enumerable.Distinct`: keeps the first of each group of equal items, in order.
pub fn distinct<K: NetKey>(items: impl IntoIterator<Item = K>) -> Vec<K> {
    items.into_iter().collect::<NetSet<K>>().into_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vector3D;

    #[test]
    fn double_hashes() {
        assert_eq!(double_hash(0.0), 0);
        assert_eq!(double_hash(-0.0), 0);
        // 1.0 = 0x3FF0000000000000
        assert_eq!(double_hash(1.0), 0x3FF0_0000);
        assert_eq!(double_hash(f64::NAN), double_hash(-f64::NAN));
    }

    #[test]
    fn map_keeps_insertion_order_and_first_key() {
        let mut m = NetMap::new();
        m.insert(Vector3D::new(3.0, 0.0), 'a');
        m.insert(Vector3D::new(1.0, 0.0), 'b');
        m.insert(Vector3D::new(3.0 + 1e-9, 0.0), 'c');
        let entries: Vec<_> = m.iter().map(|(k, v)| (k.x, *v)).collect();
        assert_eq!(entries, vec![(3.0, 'c'), (1.0, 'b')]);
    }

    #[test]
    fn diagonal_points_match_by_tolerance_even_across_rounding_boundaries() {
        // Both round differently (0.250001 vs 0.25), but hash alike because x == y.
        let mut s = NetSet::new();
        assert!(s.insert(Vector3D::new(0.2500008, 0.2500008)));
        assert!(!s.insert(Vector3D::new(0.2499999, 0.2499999)));
        // Off the diagonal, the same straddle makes the points distinct.
        assert!(s.insert(Vector3D::new(0.2500008, 0.1)));
        assert!(s.insert(Vector3D::new(0.2499999, 0.1)));
    }

    #[test]
    fn distinct_keeps_first() {
        let v = distinct(vec![Vector3D::new(1.0, 0.0), Vector3D::new(2.0, 0.0), Vector3D::new(1.0, 1e-9)]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].y, 0.0);
    }
}
