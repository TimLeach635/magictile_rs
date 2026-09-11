//! 2D geometry for MagicTile: the sphere (via stereographic projection), the Euclidean plane and
//! the hyperbolic plane (Poincaré disk), ported from Roice Nelson's R3.Core library.
//!
//! The port is deliberately faithful, down to floating point details and quirks, because puzzle
//! building identifies points through tolerance-based hashing (see [`nethash`]) and saved puzzle
//! files refer to the resulting orderings.

pub mod circle;
pub mod complex;
pub mod donhatch;
pub mod euclidean2d;
pub mod geometry2d;
pub mod h3;
pub mod infinity;
pub mod isometry;
pub mod mobius;
pub mod models;
pub mod near_tree;
pub mod nethash;
pub mod polygon;
pub mod slicer;
pub mod spherical2d;
pub mod texture_helper;
pub mod tile;
pub mod tiling;
pub mod tolerance;
pub mod util;
pub mod vector3d;

pub use circle::{Circle, CircleNE};
pub use complex::Complex;
pub use geometry2d::Geometry;
pub use isometry::Isometry;
pub use mobius::{Mobius, Transform};
pub use near_tree::{Metric, NearTree};
pub use nethash::{NetKey, NetMap, NetSet};
pub use polygon::{Polygon, Segment, SegmentType};
pub use tile::Tile;
pub use tiling::{Tiling, TilingConfig, TilingPositions};
pub use vector3d::Vector3D;
