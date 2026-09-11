//! A single tile of a tiling.

use crate::circle::CircleNE;
use crate::geometry2d::Geometry;
use crate::infinity;
use crate::isometry::Isometry;
use crate::mobius::{Mobius, Transform};
use crate::polygon::{Polygon, Segment};
use crate::vector3d::Vector3D;

#[derive(Clone, Debug)]
pub struct Tile {
    pub boundary: Polygon,
    /// The (possibly shrunken) polygon we draw.
    pub drawn: Polygon,
    pub vertex_circle: CircleNE,
    pub geometry: Geometry,
    /// The isometry taking us back to the home tile. Set once at tiling generation time; not
    /// updated by transformations.
    pub isometry: Isometry,
    /// Indices of edge-adjacent tiles in the tiling.
    pub edge_incidences: Vec<usize>,
    /// Indices of tiles sharing only a vertex with us.
    pub vertex_incidences: Vec<usize>,
}

impl Tile {
    pub fn new(boundary: Polygon, drawn: Polygon, geometry: Geometry) -> Tile {
        let vertex_circle = boundary.circum_circle();
        Tile {
            boundary,
            drawn,
            vertex_circle,
            geometry,
            isometry: Isometry::default(),
            edge_incidences: Vec::new(),
            vertex_incidences: Vec::new(),
        }
    }

    pub fn center(&self) -> Vector3D {
        self.boundary.center
    }

    /// A copy of our geometry (the isometry and incidences are not copied, as in the original).
    pub fn clone_geometry(&self) -> Tile {
        Tile {
            boundary: self.boundary.clone(),
            drawn: self.drawn.clone(),
            vertex_circle: self.vertex_circle.clone(),
            geometry: self.geometry,
            isometry: Isometry::default(),
            edge_incidences: Vec::new(),
            vertex_incidences: Vec::new(),
        }
    }

    /// Reflects the boundary and vertex circle (not the drawn polygon).
    pub fn reflect(&mut self, s: &Segment) {
        self.boundary.reflect(s);
        self.vertex_circle.reflect_segment(s);
    }

    /// Transforms the boundary and vertex circle (not the drawn polygon).
    pub fn transform(&mut self, t: &impl Transform) {
        self.boundary.transform(t);
        self.vertex_circle.transform(t);
    }

    /// Whether any of our points have been projected to infinity (only possible when spherical).
    pub fn has_points_projected_to_infinity(&self) -> bool {
        if self.geometry != Geometry::Spherical {
            return false;
        }
        if infinity::is_infinite(self.boundary.center) {
            return true;
        }
        self.boundary.segments.iter().any(|s| infinity::is_infinite(s.p1) || infinity::is_infinite(s.p2))
    }

    /// Whether we should be included in a tiling after a Möbius transformation is applied.
    /// Spherical and Euclidean tiles are always included (the tile count limits Euclidean tilings);
    /// hyperbolic ones only when near enough to the disk interior.
    pub fn include_after_mobius(&self, _m: &Mobius) -> bool {
        match self.geometry {
            Geometry::Spherical | Geometry::Euclidean => true,
            Geometry::Hyperbolic => self.vertex_circle.center_ne.abs() < 0.99999,
        }
    }

    /// Trims back the drawn polygon, assuming the tile is at the origin. Not correct in
    /// non-Euclidean geometries, but works reasonably for small shrink factors (and puzzles
    /// depend on this exact behaviour).
    pub fn shrink(&mut self, shrink_factor: f64) {
        let mut m = Mobius::default();
        m.hyperbolic(self.geometry, Vector3D::ORIGIN, shrink_factor);
        self.drawn.transform(&m);
    }
}
