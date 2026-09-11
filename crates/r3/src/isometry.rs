//! Isometries: a Möbius transformation followed by an optional reflection in a generalized circle
//! (reflections can't be represented by Möbius transformations).

use crate::circle::{Circle, CircleNE};
use crate::complex::Complex;
use crate::geometry2d::Geometry;
use crate::infinity;
use crate::mobius::{Mobius, Transform};
use crate::polygon::Polygon;
use crate::tile::Tile;
use crate::vector3d::Vector3D;
use std::ops::Mul;

#[derive(Clone, Debug)]
pub struct Isometry {
    pub mobius: Mobius,
    reflection: Option<Circle>,
    // Applying reflections was slow, so we cache the Möbius transforms that implement them.
    cache1: Mobius,
    cache2: Mobius,
}

impl Default for Isometry {
    fn default() -> Self {
        Isometry { mobius: Mobius::identity(), reflection: None, cache1: Mobius::default(), cache2: Mobius::default() }
    }
}

impl Isometry {
    pub fn new(mobius: Mobius, reflection: Option<Circle>) -> Self {
        let mut i = Isometry { mobius, ..Default::default() };
        i.set_reflection(reflection);
        i
    }

    pub fn identity() -> Self {
        Isometry::default()
    }

    /// The circle (or line) we reflect in after applying the Möbius part, if any.
    pub fn reflection(&self) -> Option<&Circle> {
        self.reflection.as_ref()
    }

    pub fn set_reflection(&mut self, reflection: Option<Circle>) {
        self.reflection = reflection;
        if let Some(c) = reflection {
            self.cache_circle_inversion(&c);
        }
    }

    pub fn reflected(&self) -> bool {
        self.reflection.is_some()
    }

    fn cache_circle_inversion(&mut self, inversion_circle: &Circle) {
        let (p1, p2, p3): (Complex, Complex, Complex) = if inversion_circle.is_line() {
            let p1 = inversion_circle.p1.to_complex();
            let p2 = inversion_circle.p2.to_complex();
            (p1, p2, (p1 + p2) / 2.0)
        } else {
            let c = inversion_circle.center;
            let r = inversion_circle.radius;
            (
                (c + Vector3D::new(r, 0.0)).to_complex(),
                (c + Vector3D::new(-r, 0.0)).to_complex(),
                (c + Vector3D::new(0.0, r)).to_complex(),
            )
        };

        let mut to_unit_circle = Mobius::default();
        to_unit_circle.map_points(p1, p2, p3, Complex::new(1.0, 0.0), Complex::new(-1.0, 0.0), Complex::new(0.0, 1.0));
        self.cache1 = to_unit_circle;
        self.cache2 = to_unit_circle.inverse();
    }

    fn apply_cached_circle_inversion(&self, input: Complex) -> Complex {
        let mut result = self.cache1.apply_c(input);
        // Reflect in the unit circle.
        result = if result.is_nan() { Complex::ZERO } else { Complex::ONE / result.conj() };
        self.cache2.apply_c(result)
    }

    pub fn inverse(&self) -> Isometry {
        let inverse = self.mobius.inverse();
        let reflection = self.reflection.map(|mut r| {
            r.transform(&inverse);
            r
        });
        Isometry::new(inverse, reflection)
    }

    /// A reflection across the x axis.
    pub fn reflect_x() -> Isometry {
        let mut i = Isometry::default();
        i.set_reflection(Some(Circle::from_2_points(Vector3D::ORIGIN, Vector3D::new(1.0, 0.0))));
        i
    }

    /// Calculates the isometry taking a tile boundary polygon to a home tile.
    pub fn calculate_from_two_polygons(&mut self, home: &Tile, boundary: &Polygon, g: Geometry) {
        self.calculate_from_two_polygons_internal(&home.boundary, boundary, &home.vertex_circle, g);
    }

    fn calculate_from_two_polygons_internal(
        &mut self,
        home: &Polygon,
        boundary: &Polygon,
        home_vertex_circle: &CircleNE,
        g: Geometry,
    ) {
        // We have to use the boundary, even though it can be projected to infinity for some
        // spherical tilings.
        let poly1 = boundary;
        let poly2 = home;

        // Poor poor digons.
        if poly1.segments.len() < 3 || poly2.segments.len() < 3 {
            return;
        }

        let (p1, p2, p3) = (poly1.segments[0].p1, poly1.segments[1].p1, poly1.segments[2].p1);
        let (w1, w2, w3) = (poly2.segments[0].p1, poly2.segments[1].p1, poly2.segments[2].p1);
        if p1 == w1 && p2 == w2 && p3 == w3 {
            self.mobius = Mobius::identity();
            return;
        }

        let mut m = Mobius::default();
        m.map_points(p1, p2, p3, w1, w2, w3);
        self.mobius = m;

        // Worry about reflections as well.
        let needs_reflection = if g == Geometry::Spherical {
            // If inverted matches the orientation, we need a reflection.
            !(poly1.is_inverted() ^ poly1.orientation())
        } else {
            !poly1.orientation()
        };
        if needs_reflection {
            self.set_reflection(Some(home_vertex_circle.circle));
        }

        // NOTE: The Möbius part can project a point to infinity before the reflection brings it
        // back, so `self.apply(boundary.center)` doesn't always equal `home.center`. This hasn't
        // been much of a problem in practice.
    }

    /// Transforms an array of vertices (allocating a new one).
    pub fn transform_vertices(vertices: &[Vector3D], isometry: &Isometry) -> Vec<Vector3D> {
        vertices.iter().map(|v| isometry.apply(*v)).collect()
    }
}

impl Transform for Isometry {
    fn apply_c(&self, z: Complex) -> Complex {
        let z = self.mobius.apply_c(z);
        if self.reflection.is_some() { self.apply_cached_circle_inversion(z) } else { z }
    }

    fn apply_infinite_safe_c(&self, z: Complex) -> Complex {
        let mut z = self.mobius.apply_infinite_safe_c(z);
        if self.reflection.is_some() {
            z = self.apply_cached_circle_inversion(z);
        }
        if infinity::is_infinite_c(z) {
            z = infinity::INFINITY_VECTOR_2D.to_complex();
        }
        z
    }
}

/// Composition: `i1 * i2` applies i2 first.
impl Mul for &Isometry {
    type Output = Isometry;
    fn mul(self, i2: &Isometry) -> Isometry {
        let i1 = self;

        // Apply both isometries to a canonical set of points, then find the isometry doing that.
        let p1 = Complex::new(1.0, 0.0);
        let p2 = Complex::new(-1.0, 0.0);
        let p3 = Complex::new(0.0, 1.0);
        let w1 = i1.apply_c(i2.apply_c(p1));
        let w2 = i1.apply_c(i2.apply_c(p2));
        let w3 = i1.apply_c(i2.apply_c(p3));

        let mut m = Mobius::default();
        m.map_points(p1, p2, p3, w1, w2, w3);

        let mut result = Isometry { mobius: m, ..Default::default() };

        // Exactly one reflection means we need to reflect at the end.
        if i1.reflected() ^ i2.reflected() {
            result.set_reflection(Some(Circle::from_3_points(w1.into(), w2.into(), w3.into())));
        }
        result
    }
}

impl Mul for Isometry {
    type Output = Isometry;
    fn mul(self, i2: Isometry) -> Isometry {
        &self * &i2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pts() -> Vec<Vector3D> {
        vec![Vector3D::new(0.1, 0.2), Vector3D::new(-0.3, 0.05), Vector3D::new(0.0, -0.4)]
    }

    #[test]
    fn reflection_and_inverse() {
        let r = Isometry::reflect_x();
        assert_eq!(r.apply(Vector3D::new(0.3, 0.2)), Vector3D::new(0.3, -0.2));

        let mut m = Mobius::default();
        m.isometry(Geometry::Hyperbolic, 0.4, Complex::new(0.2, -0.1));
        let i = Isometry::new(m, Some(Circle::new(Vector3D::new(2.0, 0.0), 1.9)));
        let inv = i.inverse();
        for p in pts() {
            assert_eq!(inv.apply(i.apply(p)), p);
        }
    }

    #[test]
    fn composition() {
        let mut m1 = Mobius::default();
        m1.isometry(Geometry::Hyperbolic, 0.4, Complex::new(0.2, -0.1));
        let mut m2 = Mobius::default();
        m2.isometry(Geometry::Hyperbolic, -1.1, Complex::new(-0.3, 0.3));
        let i1 = Isometry::new(m1, None);
        let i2 = Isometry::new(m2, Some(Circle::from_2_points(Vector3D::ORIGIN, Vector3D::new(1.0, 1.0))));
        let composed = &i1 * &i2;
        assert!(composed.reflected());
        for p in pts() {
            assert_eq!(composed.apply(p), i1.apply(i2.apply(p)));
        }
    }
}
