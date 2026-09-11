//! A nearest-neighbor search tree, adapted from Larry Andrews' article in the November 2001 issue
//! of C/C++ Users Journal (http://drdobbs.com/184401449).
//!
//! Meant for static data sets: build once (n log n), then do many lookups (log n).

use crate::donhatch;
use crate::geometry2d::Geometry;
use crate::mobius::{Mobius, Transform};
use crate::spherical2d;
use crate::vector3d::Vector3D;

/// The distance metric (anything obeying the triangle inequality works).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    Spherical,
    Euclidean,
    Hyperbolic,
}

impl From<Geometry> for Metric {
    fn from(g: Geometry) -> Metric {
        match g {
            Geometry::Spherical => Metric::Spherical,
            Geometry::Euclidean => Metric::Euclidean,
            Geometry::Hyperbolic => Metric::Hyperbolic,
        }
    }
}

#[derive(Clone, Debug)]
struct Node<T> {
    left: Option<(T, Vector3D)>,
    right: Option<(T, Vector3D)>,
    // Longest distance from the left/right objects to anything below them.
    max_left: f64,
    max_right: f64,
    left_branch: Option<usize>,
    right_branch: Option<usize>,
}

impl<T> Node<T> {
    fn new() -> Self {
        Node { left: None, right: None, max_left: f64::MIN, max_right: f64::MIN, left_branch: None, right_branch: None }
    }
}

/// Stores items of type `T` (typically ids) at locations.
#[derive(Clone, Debug)]
pub struct NearTree<T> {
    metric: Metric,
    nodes: Vec<Node<T>>,
}

impl<T: Copy> NearTree<T> {
    pub fn new(metric: Metric) -> Self {
        NearTree { metric, nodes: vec![Node::new()] }
    }

    pub fn reset(&mut self, metric: Metric) {
        *self = NearTree::new(metric);
    }

    pub fn metric(&self) -> Metric {
        self.metric
    }

    pub fn insert(&mut self, id: T, location: Vector3D) {
        let mut n = 0;
        loop {
            let node = &self.nodes[n];
            let (mut temp_left, mut temp_right) = (0.0, 0.0);
            if let (Some(r), Some(l)) = (&node.right, &node.left) {
                temp_right = self.dist(location, r.1);
                temp_left = self.dist(location, l.1);
            }

            let new_index = self.nodes.len();
            let node = &mut self.nodes[n];
            if node.left.is_none() {
                node.left = Some((id, location));
                return;
            } else if node.right.is_none() {
                node.right = Some((id, location));
                return;
            }

            // Note: this relies on max_left/max_right being negative for new nodes.
            let branch = if temp_left > temp_right {
                if node.max_right < temp_right {
                    node.max_right = temp_right;
                }
                &mut node.right_branch
            } else {
                if node.max_left < temp_left {
                    node.max_left = temp_left;
                }
                &mut node.left_branch
            };

            n = match *branch {
                Some(b) => b,
                None => {
                    *branch = Some(new_index);
                    self.nodes.push(Node::new());
                    new_index
                }
            };
        }
    }

    /// The nearest object within `search_radius` of a location, if any.
    pub fn find_nearest_neighbor(&self, location: Vector3D, mut search_radius: f64) -> Option<T> {
        enum Step {
            Visit(usize),
            CheckRight(usize),
        }

        let mut closest = None;
        let mut stack = vec![Step::Visit(0)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Visit(n) => {
                    let node = &self.nodes[n];

                    // Does either object in this node beat the nearest so far?
                    for obj in [&node.left, &node.right].into_iter().flatten() {
                        let d = self.dist(location, obj.1);
                        if d <= search_radius {
                            search_radius = d;
                            closest = Some(obj.0);
                        }
                    }

                    // The triangle rule tells us whether descending might find something nearer.
                    // The right branch is checked after the left one is fully searched.
                    stack.push(Step::CheckRight(n));
                    if let (Some(b), Some(l)) = (node.left_branch, &node.left)
                        && search_radius + node.max_left >= self.dist(location, l.1)
                    {
                        stack.push(Step::Visit(b));
                    }
                }
                Step::CheckRight(n) => {
                    let node = &self.nodes[n];
                    if let (Some(b), Some(r)) = (node.right_branch, &node.right)
                        && search_radius + node.max_right >= self.dist(location, r.1)
                    {
                        stack.push(Step::Visit(b));
                    }
                }
            }
        }
        closest
    }

    /// All objects within `search_radius` of a location.
    pub fn find_close_objects(&self, location: Vector3D, search_radius: f64) -> Vec<T> {
        let mut result = Vec::new();
        let mut stack = vec![0];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n];
            for obj in [&node.left, &node.right].into_iter().flatten() {
                if self.dist(location, obj.1) <= search_radius {
                    result.push(obj.0);
                }
            }
            // Pushed in reverse so the left branch is searched first, as in the original.
            if let (Some(b), Some(r)) = (node.right_branch, &node.right)
                && search_radius + node.max_right >= self.dist(location, r.1)
            {
                stack.push(b);
            }
            if let (Some(b), Some(l)) = (node.left_branch, &node.left)
                && search_radius + node.max_left >= self.dist(location, l.1)
            {
                stack.push(b);
            }
        }
        result
    }

    fn dist(&self, p1: Vector3D, p2: Vector3D) -> f64 {
        match self.metric {
            Metric::Spherical => {
                let mut m = Mobius::default();
                m.isometry(Geometry::Spherical, 0.0, -p1);
                spherical2d::e2s_norm(m.apply(p2).abs())
            }
            Metric::Euclidean => (p2 - p1).abs(),
            Metric::Hyperbolic => {
                let mut m = Mobius::default();
                m.isometry(Geometry::Hyperbolic, 0.0, -p1);
                donhatch::e2h_norm(m.apply(p2).abs())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nearest_like_brute_force() {
        // A deterministic scatter of points in the disk.
        let points: Vec<Vector3D> = (0..200)
            .map(|i| {
                let t = i as f64 * 2.399963;
                let r = 0.95 * ((i as f64 + 0.5) / 200.0).sqrt();
                Vector3D::new(r * t.cos(), r * t.sin())
            })
            .collect();

        for metric in [Metric::Euclidean, Metric::Hyperbolic, Metric::Spherical] {
            let mut tree = NearTree::new(metric);
            for (i, p) in points.iter().enumerate() {
                tree.insert(i, *p);
            }
            for j in 0..50 {
                let t = j as f64 * 1.3;
                let q = Vector3D::new(0.6 * t.cos() * (j as f64 / 50.0), 0.6 * t.sin());
                let found = tree.find_nearest_neighbor(q, f64::MAX).unwrap();
                let best = (0..points.len())
                    .min_by(|&a, &b| tree.dist(q, points[a]).total_cmp(&tree.dist(q, points[b])))
                    .unwrap();
                assert!((tree.dist(q, points[found]) - tree.dist(q, points[best])).abs() < 1e-12);
            }
            let close = tree.find_close_objects(Vector3D::ORIGIN, 0.2);
            let expected = points.iter().filter(|p| tree.dist(Vector3D::ORIGIN, **p) <= 0.2).count();
            assert_eq!(close.len(), expected);
        }
    }
}
