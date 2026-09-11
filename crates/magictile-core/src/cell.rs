//! Cells (tiles of the puzzle) and the stickers on them.

use r3::{CircleNE, Isometry, Polygon, Vector3D};

/// Index into [`crate::puzzle::Puzzle::cells`].
pub type CellId = usize;
/// Index into [`crate::puzzle::Puzzle::stickers`].
pub type StickerId = usize;

#[derive(Clone, Debug)]
pub struct Cell {
    pub boundary: Polygon,
    pub vertex_circle: CircleNE,
    /// Only cells involved in state calculations (or all cells, for spherical puzzles) have
    /// stickers.
    pub stickers: Vec<StickerId>,
    /// Takes us back to the cell at the origin.
    pub isometry: Isometry,
    /// The index of our master cell (ourselves if we're a master), used for state calculations.
    /// -1 for cells beyond the expected number of colors.
    pub index_of_master: i32,
    /// For slave cells, the master cell.
    pub master: Option<CellId>,
    /// For toggling ("lights on") puzzles: the master cells sharing an edge with us (masters
    /// are their own neighbors).
    pub neighbors: Vec<CellId>,
}

impl Cell {
    pub fn new(boundary: Polygon, vertex_circle: CircleNE) -> Self {
        Cell {
            boundary,
            vertex_circle,
            stickers: Vec::new(),
            isometry: Isometry::default(),
            index_of_master: -1,
            master: None,
            neighbors: Vec::new(),
        }
    }

    pub fn center(&self) -> Vector3D {
        self.boundary.center
    }

    /// Takes the cell at the origin to us.
    pub fn isometry_inverse(&self) -> Isometry {
        self.isometry.inverse()
    }

    pub fn is_master(&self) -> bool {
        self.master.is_none()
    }

    /// Whether we have gone through an odd number of reflections.
    pub fn reflected(&self) -> bool {
        self.isometry.reflected()
    }
}

#[derive(Clone, Debug)]
pub struct Sticker {
    /// The index of the master cell this sticker belongs to (-1 for dropped cells).
    pub cell_index: i32,
    /// The index of this sticker within its cell.
    pub sticker_index: usize,
    pub poly: Polygon,
    /// Set while this sticker is moving in a twist animation.
    pub twisting: bool,
}
