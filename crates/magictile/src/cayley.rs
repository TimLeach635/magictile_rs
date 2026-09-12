//! Reading a truncated tiling t{p,q} as the Cayley graph of the symmetric group S_q.
//!
//! Every vertex of t{p,q} has three edges: two around its q-gon, and one to the next q-gon over.
//! Label the vertices with permutations so that a step around a q-gon multiplies by the cycle
//! `c = (1 2 ... q)` and a step along the third edge multiplies by the swap `t = (1 2)`, always on
//! the right. Going around a q-gon gives `c^q = 1`; going around a 2p-gon alternates swap and
//! cycle p times each, which needs `(c t)^p = 1`. Since `c t` is a (q-1)-cycle, that holds exactly
//! when `q - 1` divides `p` — for t{4,5}, the order-5 square tiling, `(c t)^4 = 1` and the group is
//! all of S_5.
//!
//! Because the tiling is simply connected, those two face relations are the only constraints, so
//! the labelling is consistent. Writing permutations in one-line form (the word of images) makes
//! the edges legible: a q-gon step rotates the word, and a swap step exchanges its first two
//! letters.
//!
//! Panning far from the origin loses precision and runs off the end of the generated patch, so
//! the viewer recenters by moving the home vertex onto whichever vertex has come closest to the
//! middle. That symmetry carries the labelling with it: moving the home vertex onto a vertex
//! labelled `g` renames every vertex `x` to `g x`. Keeping track of the accumulated `g` and
//! multiplying by it when drawing means the permutations stay exactly where they are on screen.
//!
//! The symmetries that preserve the labelling are the deck transformations of the quotient
//! surface, and their fundamental domain — the repeating unit — is the Dirichlet domain around a
//! q-gon's center. For t{4,5} it is a 20-gon holding exactly one vertex of each of the 120
//! permutations.

use r3::{Complex, Isometry, Mobius, Transform, Vector3D};
use std::collections::HashMap;

/// The largest q we label (the labels have to fit on screen).
pub const MAX_Q: usize = 9;

/// A permutation in one-line form: `p[i]` is the image of `i + 1`. Entries past `q` are 0.
pub type Perm = [u8; MAX_Q];

fn identity(q: usize) -> Perm {
    let mut p = [0; MAX_Q];
    for (i, slot) in p.iter_mut().enumerate().take(q) {
        *slot = i as u8 + 1;
    }
    p
}

/// `g * s`, i.e. `s` followed by `g` as functions: `(g s)(i) = g(s(i))`.
fn mul(g: &Perm, s: &Perm, q: usize) -> Perm {
    let mut r = [0; MAX_Q];
    for i in 0..q {
        r[i] = g[s[i] as usize - 1];
    }
    r
}

fn inverse(g: &Perm, q: usize) -> Perm {
    let mut r = [0; MAX_Q];
    for i in 0..q {
        r[g[i] as usize - 1] = i as u8 + 1;
    }
    r
}

/// The cycle `(1 2 ... q)`, one step around a q-gon.
fn cycle(q: usize) -> Perm {
    let mut p = [0; MAX_Q];
    for (i, slot) in p.iter_mut().enumerate().take(q) {
        *slot = (i as u8 + 1) % q as u8 + 1;
    }
    p
}

/// The swap `(1 2)`, the edge to the next q-gon.
fn swap(q: usize) -> Perm {
    let mut p = identity(q);
    p.swap(0, 1);
    p
}

/// One-line form, e.g. `31452`.
pub fn word(g: &Perm, q: usize) -> String {
    g[..q].iter().map(|d| char::from(b'0' + d)).collect()
}

/// Cycle notation, e.g. `(1 3 4)(2 5)`, or `identity`.
pub fn cycles(g: &Perm, q: usize) -> String {
    let mut seen = [false; MAX_Q];
    let mut out = String::new();
    for start in 0..q {
        if seen[start] || g[start] as usize == start + 1 {
            continue;
        }
        let mut items = Vec::new();
        let mut i = start;
        while !seen[i] {
            seen[i] = true;
            items.push((i + 1).to_string());
            i = g[i] as usize - 1;
        }
        out.push('(');
        out.push_str(&items.join(" "));
        out.push(')');
    }
    if out.is_empty() { "identity".into() } else { out }
}

/// A vertex of the truncated tiling: one element of the group, in the Poincaré disk.
pub struct Vertex {
    pub pos: Vector3D,
    /// `None` out at the edge of the generated patch, where a q-gon is missing.
    pub label: Option<Perm>,
    /// The next vertex counterclockwise around our q-gon (a multiplication by the cycle).
    next: Option<usize>,
    /// Across the edge to the neighboring q-gon (a multiplication by the swap).
    across: Option<usize>,
}

impl Vertex {
    /// A neighbor, for judging how much room the vertex has on screen.
    pub fn neighbor(&self) -> Option<usize> {
        self.across.or(self.next)
    }
}

pub struct Cayley {
    pub q: usize,
    pub vertices: Vec<Vertex>,
    /// The vertex labelled with the identity, which recentering moves around.
    pub home_vertex: usize,
    /// The center of the repeating unit (the home q-gon's center).
    pub center: Vector3D,
    /// The corners of the repeating unit, around the home q-gon's center.
    pub unit: Vec<Vector3D>,
    /// Symmetries carrying the home unit onto each copy (the identity first).
    pub deck: Vec<Isometry>,
    by_label: HashMap<Perm, Vec<usize>>,
}

impl Cayley {
    /// Labels the vertices of a truncated tiling. `q_gons` are the q-gons' centers with their
    /// corners in counterclockwise order, and `swaps` the edges between 2p-gons. Returns `None`
    /// when the labelling can't be consistent (see the module comment).
    pub fn build(
        p: i32,
        q: i32,
        q_gons: &[(Vector3D, Vec<Vector3D>)],
        swaps: &[(Vector3D, Vector3D)],
        start: &Isometry,
    ) -> Option<Cayley> {
        let q = q as usize;
        if !(3..=MAX_Q).contains(&q) || p % (q as i32 - 1) != 0 {
            return None;
        }

        // Gather the vertices, merging the copies that the faces and edges each computed.
        let mut index = PointIndex::default();
        let mut rings: Vec<Vec<usize>> = Vec::with_capacity(q_gons.len());
        for (_, corners) in q_gons {
            rings.push(corners.iter().map(|&c| index.insert(c)).collect());
        }
        let swap_pairs: Vec<(usize, usize)> = swaps.iter().map(|&(a, b)| (index.insert(a), index.insert(b))).collect();

        let mut vertices: Vec<Vertex> =
            index.points.iter().map(|&pos| Vertex { pos, label: None, next: None, across: None }).collect();
        for ring in &rings {
            for (i, &v) in ring.iter().enumerate() {
                vertices[v].next = Some(ring[(i + 1) % ring.len()]);
            }
        }
        for &(a, b) in &swap_pairs {
            vertices[a].across = Some(b);
            vertices[b].across = Some(a);
        }

        // Start from the vertex the opening view puts to the right of the central q-gon.
        let home = q_gons.iter().position(|(center, _)| start.apply(*center).abs() < 1e-9)?;
        let first = *rings[home]
            .iter()
            .min_by(|&&a, &&b| {
                let angle = |v: usize| start.apply(vertices[v].pos).to_complex().phase().abs();
                angle(a).total_cmp(&angle(b))
            })
            .unwrap();

        let (c, t, c_inverse) = (cycle(q), swap(q), inverse(&cycle(q), q));
        vertices[first].label = Some(identity(q));
        let mut queue = std::collections::VecDeque::from([first]);
        while let Some(v) = queue.pop_front() {
            let label = vertices[v].label.expect("queued vertices are labelled");
            let steps = [(vertices[v].next, c), (vertices[v].across, t), (back_link(&vertices, v), c_inverse)];
            for (neighbor, generator) in steps {
                let Some(n) = neighbor else { continue };
                let found = mul(&label, &generator, q);
                match vertices[n].label {
                    // Every closed walk is built from face boundaries, so the relations checked in
                    // the module comment make this hold; it is cheap to be sure.
                    Some(existing) => assert_eq!(existing, found, "inconsistent labelling"),
                    None => {
                        vertices[n].label = Some(found);
                        queue.push_back(n);
                    }
                }
            }
        }

        let mut by_label: HashMap<Perm, Vec<usize>> = HashMap::new();
        for (i, v) in vertices.iter().enumerate() {
            if let Some(label) = v.label {
                by_label.entry(label).or_default().push(i);
            }
        }

        // The deck transformations are the label-preserving symmetries: each carries the home
        // q-gon onto another q-gon holding the same labels, matching them up.
        let center = q_gons[home].0;
        let anchor = vertices[first].pos;
        let mut deck = vec![Isometry::identity()];
        let mut copies = vec![center];
        for (i, (other_center, _)) in q_gons.iter().enumerate() {
            if i == home {
                continue;
            }
            let Some(&matching) = rings[i].iter().find(|&&v| vertices[v].label == vertices[first].label) else {
                continue;
            };
            deck.push(matching_isometry(center, anchor, *other_center, vertices[matching].pos));
            copies.push(*other_center);
        }

        let unit = dirichlet(center, &copies[1..]);
        Some(Cayley { q, vertices, home_vertex: first, center, unit, deck, by_label })
    }

    /// Every vertex carrying the same permutation as this one (itself included).
    pub fn copies_of(&self, vertex: usize) -> &[usize] {
        self.vertices[vertex]
            .label
            .as_ref()
            .and_then(|label| self.by_label.get(label))
            .map_or(&[][..], |copies| copies.as_slice())
    }

    /// The permutation a vertex was labelled with, from the home vertex.
    pub fn label(&self, vertex: usize) -> Option<Perm> {
        self.vertices[vertex].label
    }

    /// The permutation to show at a vertex in a view recentered by `frame` (see
    /// [`Cayley::frame_isometry`]).
    pub fn displayed(&self, vertex: usize, frame: &Perm) -> Option<Perm> {
        Some(mul(frame, &self.vertices[vertex].label?, self.q))
    }

    pub fn label_word(&self, vertex: usize, frame: &Perm) -> Option<String> {
        Some(word(&self.displayed(vertex, frame)?, self.q))
    }

    pub fn label_cycles(&self, vertex: usize, frame: &Perm) -> Option<String> {
        Some(cycles(&self.displayed(vertex, frame)?, self.q))
    }

    pub fn identity_perm(&self) -> Perm {
        identity(self.q)
    }

    pub fn compose(&self, a: &Perm, b: &Perm) -> Perm {
        mul(a, b, self.q)
    }

    pub fn inverse_of(&self, a: &Perm) -> Perm {
        inverse(a, self.q)
    }

    /// Every vertex labelled `g`.
    pub fn vertices_with_label(&self, g: &Perm) -> &[usize] {
        self.by_label.get(g).map_or(&[][..], |v| v.as_slice())
    }

    /// The symmetry taking the home vertex onto `vertex`, keeping the tiling and the way the
    /// q-gons run. It renames every label `x` to `label(vertex) x`, so a view recentered by it
    /// shows the same permutations in the same places as long as the drawing multiplies by that
    /// label (see [`Cayley::displayed`]).
    pub fn frame_isometry(&self, vertex: usize) -> Option<Isometry> {
        let anchored = |v: usize| Some((self.vertices[v].pos, self.vertices[self.vertices[v].next?].pos));
        let (home, home_next) = anchored(self.home_vertex)?;
        let (there, there_next) = anchored(vertex)?;
        Some(matching_isometry(home, home_next, there, there_next))
    }

    /// The vertex at a point, if there is one (within rounding).
    pub fn nearest(&self, p: Vector3D) -> Option<usize> {
        (0..self.vertices.len()).min_by(|&a, &b| self.vertices[a].pos.dist(p).total_cmp(&self.vertices[b].pos.dist(p)))
    }

    /// How many distinct permutations the labelling used.
    pub fn distinct_labels(&self) -> usize {
        self.by_label.len()
    }

    /// The vertices of the repeating unit: those strictly inside it, and those sitting on its
    /// boundary, which it shares with the unit next door.
    pub fn unit_vertices(&self) -> (Vec<usize>, Vec<usize>) {
        let center = self.center;
        let copies: Vec<Vector3D> = self.deck[1..].iter().map(|g| g.apply(center)).collect();
        let (mut inside, mut boundary) = (Vec::new(), Vec::new());
        for v in 0..self.vertices.len() {
            let p = self.vertices[v].pos;
            let ours = hyperbolic_distance(p, center);
            let theirs = copies.iter().map(|&c| hyperbolic_distance(p, c)).fold(f64::MAX, f64::min);
            if theirs > ours + 1e-9 {
                inside.push(v);
            } else if theirs > ours - 1e-9 {
                boundary.push(v);
            }
        }
        (inside, boundary)
    }
}

/// The vertex before `v` around its q-gon (the one whose `next` is `v`).
fn back_link(vertices: &[Vertex], v: usize) -> Option<usize> {
    // A q-gon's ring is short, so search the neighbors of our neighbors.
    let next = vertices[v].next?;
    let mut at = next;
    loop {
        let following = vertices[at].next?;
        if following == v {
            return Some(at);
        }
        at = following;
        if at == next {
            return None;
        }
    }
}

/// The orientation-preserving isometry taking `from` to `to` and `from_anchor` to `to_anchor`
/// (both anchors the same distance from their centers).
fn matching_isometry(from: Vector3D, from_anchor: Vector3D, to: Vector3D, to_anchor: Vector3D) -> Isometry {
    let (f, t) = (from.to_complex(), to.to_complex());
    let turn = Complex::from_polar(
        1.0,
        to_origin(t, to_anchor.to_complex()).phase() - to_origin(f, from_anchor.to_complex()).phase(),
    );
    // Move `from` to the origin, turn, then move the origin to `to`.
    let recenter = Mobius::new(Complex::ONE, f * -1.0, f.conj() * -1.0, Complex::ONE);
    let rotate = Mobius::new(turn, Complex::ZERO, Complex::ZERO, Complex::ONE);
    let place = Mobius::new(Complex::ONE, t, t.conj(), Complex::ONE);
    Isometry::new(place * rotate * recenter, None)
}

/// The Dirichlet domain around `center`: the points closer to it than to any of `others`.
///
/// In the Klein model, geodesics are straight and "closer to p than to q" is a half-plane, so this
/// is ordinary polygon clipping. The corners come back as Poincaré disk points.
fn dirichlet(center: Vector3D, others: &[Vector3D]) -> Vec<Vector3D> {
    // On the hyperboloid (signature +,-,-) cosh of the distance is the inner product, so x is
    // closer to p than to q exactly when <x, p - q> <= 0, which in Klein coordinates k (with
    // x = (1, k) up to scale) reads k . dx - dt >= 0.
    let lift = |v: Vector3D| {
        let r2 = v.x * v.x + v.y * v.y;
        let s = 1.0 - r2;
        (((1.0 + r2) / s), Vector3D::new(2.0 * v.x / s, 2.0 * v.y / s))
    };
    let (pt, px) = lift(center);
    let mut poly =
        vec![Vector3D::new(-1.0, -1.0), Vector3D::new(1.0, -1.0), Vector3D::new(1.0, 1.0), Vector3D::new(-1.0, 1.0)];
    for &other in others {
        let (qt, qx) = lift(other);
        let (dt, dx) = (pt - qt, px - qx);
        let inside = |k: &Vector3D| (k.x * dx.x + k.y * dx.y) - dt;
        let mut clipped = Vec::with_capacity(poly.len() + 2);
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (fa, fb) = (inside(&a), inside(&b));
            if fa >= 0.0 {
                clipped.push(a);
            }
            if (fa < 0.0) != (fb < 0.0) && (fa - fb).abs() > f64::EPSILON {
                let s = fa / (fa - fb);
                clipped.push(a + (b - a) * s);
            }
        }
        poly = clipped;
        if poly.is_empty() {
            break;
        }
    }
    // Klein to Poincaré, dropping the duplicates clipping leaves behind.
    let mut corners: Vec<Vector3D> = Vec::with_capacity(poly.len());
    for k in poly {
        let p = k / (1.0 + (1.0 - k.x * k.x - k.y * k.y).max(0.0).sqrt());
        if corners.last().is_none_or(|last: &Vector3D| last.dist(p) > 1e-9) {
            corners.push(p);
        }
    }
    if corners.len() > 1 && corners[0].dist(*corners.last().unwrap()) < 1e-9 {
        corners.pop();
    }
    corners
}

fn to_origin(a: Complex, z: Complex) -> Complex {
    (z - a) / (Complex::ONE - a.conj() * z)
}

fn hyperbolic_distance(a: Vector3D, b: Vector3D) -> f64 {
    2.0 * to_origin(a.to_complex(), b.to_complex()).magnitude().atanh()
}

/// Merges points that the tiling computed more than once (to within rounding).
#[derive(Default)]
struct PointIndex {
    points: Vec<Vector3D>,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl PointIndex {
    const CELL: f64 = 1e-5;

    fn insert(&mut self, v: Vector3D) -> usize {
        let key = ((v.x / Self::CELL).round() as i64, (v.y / Self::CELL).round() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(nearby) = self.cells.get(&(key.0 + dx, key.1 + dy)) {
                    for &i in nearby {
                        if self.points[i].dist(v) < 1e-9 {
                            return i;
                        }
                    }
                }
            }
        }
        let i = self.points.len();
        self.points.push(v);
        self.cells.entry(key).or_default().push(i);
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiling::TruncatedTiling;

    fn angle_at(corner: Vector3D, a: Vector3D, b: Vector3D) -> f64 {
        // Move the corner to the origin, where geodesics through it are straight lines.
        let c = corner.to_complex();
        let (pa, pb) = (to_origin(c, a.to_complex()).phase(), to_origin(c, b.to_complex()).phase());
        let d = (pa - pb).abs();
        if d > std::f64::consts::PI { 2.0 * std::f64::consts::PI - d } else { d }
    }

    #[test]
    fn labels_the_symmetric_group() {
        let t = TruncatedTiling::new(4, 5).unwrap();
        let c = t.cayley.as_ref().expect("t{4,5} is a Cayley graph of S5");
        assert_eq!(c.distinct_labels(), 120, "S5 has 120 elements");

        // The edges do what they should to the one-line words.
        for (i, v) in c.vertices.iter().enumerate() {
            let Some(label) = v.label else { continue };
            let word = word(&label, c.q);
            let frame = c.identity_perm();
            if let Some(next) = v.next.filter(|&n| c.vertices[n].label.is_some()) {
                let rotated = format!("{}{}", &word[1..], &word[..1]);
                assert_eq!(c.label_word(next, &frame).unwrap(), rotated, "a q-gon step should rotate the word");
            }
            if let Some(across) = v.across.filter(|&n| c.vertices[n].label.is_some()) {
                let mut swapped: Vec<char> = word.chars().collect();
                swapped.swap(0, 1);
                let swapped: String = swapped.into_iter().collect();
                assert_eq!(c.label_word(across, &frame).unwrap(), swapped, "a swap step should swap the first two");
            }
            let _ = i;
        }
    }

    /// The unit should hold each of the 120 permutations once. Vertices sitting exactly on its
    /// boundary belong half to it and half to the unit next door, so they come in pairs.
    #[test]
    fn the_repeating_unit_holds_each_permutation_once() {
        let t = TruncatedTiling::new(4, 5).unwrap();
        let c = t.cayley.as_ref().unwrap();
        let (inside, boundary) = c.unit_vertices();
        assert_eq!(inside.len() + boundary.len() / 2, 120, "inside {}, boundary {}", inside.len(), boundary.len());

        let label = |v: &usize| c.vertices[*v].label.expect("unit vertices are labelled");
        let mut counts: HashMap<Perm, usize> = HashMap::new();
        for v in inside.iter().chain(boundary.iter()) {
            *counts.entry(label(v)).or_default() += 1;
        }
        assert_eq!(counts.len(), 120, "each permutation should appear");
        for v in &inside {
            assert_eq!(counts[&label(v)], 1, "a permutation inside the unit appears twice");
        }
        for v in &boundary {
            assert_eq!(counts[&label(v)], 2, "a permutation on the boundary should be shared by two units");
        }
    }

    /// The unit should come out as the 20-gon found independently for this surface: equilateral,
    /// with angles alternating 36 and 72 degrees.
    #[test]
    fn the_repeating_unit_is_the_expected_20_gon() {
        let t = TruncatedTiling::new(4, 5).unwrap();
        let c = t.cayley.as_ref().unwrap();
        let n = c.unit.len();
        assert_eq!(n, 20);

        let sides: Vec<f64> = (0..n).map(|i| hyperbolic_distance(c.unit[i], c.unit[(i + 1) % n])).collect();
        let (min, max) = sides.iter().fold((f64::MAX, 0.0f64), |(a, b), &s| (a.min(s), b.max(s)));
        assert!(max - min < 1e-6, "sides range from {min} to {max}");
        assert!((min - 2.9387).abs() < 1e-3, "side length {min}");

        let angles: Vec<f64> =
            (0..n).map(|i| angle_at(c.unit[i], c.unit[(i + n - 1) % n], c.unit[(i + 1) % n]).to_degrees()).collect();
        for (i, a) in angles.iter().enumerate() {
            let expected = if i % 2 == 0 { angles[0] } else { angles[1] };
            assert!((a - expected).abs() < 1e-6, "angle {i} is {a}");
        }
        let mut two = [angles[0], angles[1]];
        two.sort_by(f64::total_cmp);
        assert!((two[0] - 36.0).abs() < 1e-4 && (two[1] - 72.0).abs() < 1e-4, "angles {two:?}");
    }

    /// Recentering moves the home vertex onto another vertex, and that symmetry renames every
    /// label by multiplying on the left by the label of the vertex moved onto. This is what lets
    /// the viewer recenter while the permutations stay put on screen.
    #[test]
    fn recentering_multiplies_the_labels_on_the_left() {
        let t = TruncatedTiling::new(4, 5).unwrap();
        let c = t.cayley.as_ref().unwrap();
        // Vertices near the middle, so that most images stay inside the generated patch.
        let near = |limit: f64| {
            (0..c.vertices.len())
                .filter(|&v| c.vertices[v].pos.abs() < limit && c.label(v).is_some())
                .collect::<Vec<_>>()
        };
        let (centers, others) = (near(0.55), near(0.85));
        let mut checked = 0;
        for &v in &centers {
            let (Some(onto), Some(sigma)) = (c.label(v), c.frame_isometry(v)) else { continue };
            for &u in &others {
                let Some(label) = c.label(u) else { continue };
                let moved = sigma.apply(c.vertices[u].pos);
                if moved.abs() > 0.97 {
                    continue;
                }
                let Some(landed) = c.nearest(moved) else { continue };
                if c.vertices[landed].pos.dist(moved) > 1e-9 {
                    continue;
                }
                assert_eq!(
                    c.label(landed),
                    Some(c.compose(&onto, &label)),
                    "moving the home vertex onto {v} should rename {u} by left multiplication"
                );
                checked += 1;
            }
        }
        assert!(checked > 300, "only checked {checked} vertices");
    }

    #[test]
    fn deck_symmetries_preserve_the_labels() {
        let t = TruncatedTiling::new(4, 5).unwrap();
        let c = t.cayley.as_ref().unwrap();
        assert!(c.deck.len() > 10);
        let mut checked = 0;
        for g in c.deck.iter().take(12) {
            for (i, v) in c.vertices.iter().enumerate().step_by(3) {
                let Some(label) = v.label else { continue };
                let moved = g.apply(v.pos);
                // Only points that stay well inside the generated patch are expected to land on
                // another labelled vertex.
                if moved.abs() > 0.95 {
                    continue;
                }
                let Some(landed) = c.nearest(moved) else { continue };
                if c.vertices[landed].pos.dist(moved) > 1e-9 {
                    continue;
                }
                assert_eq!(
                    c.vertices[landed].label,
                    Some(label),
                    "deck symmetry moved vertex {i} to a different label"
                );
                checked += 1;
            }
        }
        assert!(checked > 500, "only checked {checked} vertices");
    }
}
