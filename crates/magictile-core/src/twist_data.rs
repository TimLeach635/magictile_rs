//! Cached data for twisting: the slicing circles about a twist center and what they affect.

use crate::cell::{Cell, CellId, Sticker, StickerId};
use crate::config::PuzzleConfig;
use crate::pants::Pants;
use crate::slice_mask;
use crate::twist::SingleTwist;
use r3::{CircleNE, Geometry, Isometry, Mobius, Transform, Vector3D};

/// Index into [`crate::puzzle::Puzzle::twist_data`].
pub type TwistDataId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementType {
    Face,
    Edge,
    Vertex,
}

/// All the copies of one logical twist (twist data identified with each other). Saved twists
/// refer to these by index.
#[derive(Clone, Debug, Default)]
pub struct IdentifiedTwistData {
    /// Our index in the puzzle's list of all identified twist data.
    pub index: usize,
    pub twist_data_for_drawing: Vec<TwistDataId>,
    pub twist_data_for_state_calcs: Vec<TwistDataId>,
}

/// One twist center and its concentric slicing circles.
#[derive(Clone, Debug)]
pub struct TwistData {
    pub twist_type: ElementType,
    /// The (non-Euclidean) center of the twist.
    pub center: Vector3D,
    pub order: i32,
    /// Whether to reverse the twist (for non-orientable puzzles).
    pub reverse: bool,
    /// Concentric slicing circles, ordered by depth.
    pub circles: Vec<CircleNE>,
    /// For systolic puzzles, twisting is based on a pair of pants.
    pub pants: Option<Pants>,
    num_slices: Option<i32>,
    /// The number of slices excluding antipodal ones (for GAP output).
    pub num_slices_no_opp: i32,
    /// Affected stickers, one list per slice. `None` until computed.
    pub affected_stickers: Option<Vec<Vec<StickerId>>>,
    /// Master cells affected by this twist (whose textures need updating). `None` until computed.
    pub affected_master_cells: Option<Vec<CellId>>,
    /// Index of our [`IdentifiedTwistData`].
    pub identified: Option<usize>,
}

impl TwistData {
    pub fn new(twist_type: ElementType, center: Vector3D, order: i32) -> Self {
        TwistData {
            twist_type,
            center,
            order,
            reverse: false,
            circles: Vec::new(),
            pants: None,
            num_slices: None,
            num_slices_no_opp: 0,
            affected_stickers: None,
            affected_master_cells: None,
            identified: None,
        }
    }

    pub fn systolic(&self) -> bool {
        self.pants.is_some()
    }

    pub fn earthquake(&self) -> bool {
        self.pants.is_some() && self.circles.len() == 3
    }

    /// The number of slices, which may differ from the number of circles (spherical puzzles have
    /// a slice beyond the last circle).
    pub fn num_slices(&self) -> i32 {
        self.num_slices.unwrap_or(self.circles.len() as i32)
    }

    pub fn set_num_slices(&mut self, n: i32) {
        self.num_slices = Some(n);
    }

    /// Slice -> circles, for systolic puzzles (where slices mark the three directions).
    pub fn circles_for_systolic_slice(&self, slice: i32) -> &[CircleNE] {
        let range = match slice {
            1 => 2..4,
            2 => 4..6,
            3 => 0..2,
            _ => return &[],
        };
        self.circles.get(range).unwrap_or(&[])
    }

    /// The circles bounding the slices of a slice mask.
    pub fn circles_for_slice_mask(&self, mask: i32) -> Vec<&CircleNE> {
        // All 3 circles for earthquakes, for now.
        if self.earthquake() {
            return self.circles.iter().collect();
        }

        if self.systolic() {
            return self.circles_for_systolic_slice(slice_mask::mask_to_slice(mask)).iter().collect();
        }

        let count = self.circles.len() as i32;
        let mut indexes = Vec::new();
        for slice in slice_mask::mask_to_slices(mask) {
            if slice > self.num_slices() {
                continue;
            }
            for index in [slice - 2, slice - 1] {
                if (0..count).contains(&index) && !indexes.contains(&index) {
                    indexes.push(index);
                }
            }
        }
        indexes.sort();
        indexes.into_iter().map(|i| &self.circles[i as usize]).collect()
    }

    /// The affected stickers for a slice mask, one list per slice.
    pub fn affected_stickers_for_slice_mask(&self, mask: i32) -> Vec<&[StickerId]> {
        let Some(affected) = &self.affected_stickers else {
            return Vec::new();
        };

        // Earthquakes need the full list.
        if self.earthquake() {
            return affected.iter().map(|v| v.as_slice()).collect();
        }

        // For systolic puzzles the "slices" are really the three directions.
        if self.systolic() {
            let direction = slice_mask::mask_to_slice(mask);
            return affected.get((direction - 1) as usize).map(|v| vec![v.as_slice()]).unwrap_or_default();
        }

        slice_mask::mask_to_slices(mask)
            .into_iter()
            .filter(|&slice| slice >= 1 && slice as usize <= affected.len())
            .map(|slice| affected[(slice - 1) as usize].as_slice())
            .collect()
    }

    /// Whether this twist will affect a cell.
    pub fn will_affect_cell(&self, cell: &Cell, spherical_puzzle: bool) -> bool {
        if self.earthquake() {
            let pants = self.pants.as_ref().unwrap();
            if pants.test_circle.has_vertex_inside(&cell.boundary) {
                return true;
            }
            return (0..3).any(|i| {
                let mut c = pants.test_circle.clone();
                c.reflect_segment(&pants.hexagon.segments[i * 2]);
                c.has_vertex_inside(&cell.boundary)
            });
        }

        if self.systolic() {
            return self.circles.iter().any(|c| c.intersects(&cell.boundary));
        }

        self.circles.iter().any(|c| {
            let inside = if spherical_puzzle {
                c.is_point_inside_ne(cell.center())
            } else {
                c.is_point_inside_fast(cell.center())
            };
            inside || c.intersects(&cell.boundary)
        })
    }

    /// Which of these sticker(s) we affect, and in which slice. Computes what the original's
    /// `WillAffectSticker` adds for a sticker; returns the slice index (0-based) if any.
    pub fn affected_slices_for_sticker(&self, sticker: &Sticker, spherical_puzzle: bool) -> Vec<usize> {
        let cen = sticker.poly.center;

        if self.earthquake() {
            return if self.pants.as_ref().unwrap().is_point_inside_optimized(cen) { vec![0] } else { vec![] };
        }

        if self.systolic() {
            // Only one ribbon is supported (6 circles).
            return (1..=3)
                .filter(|&slice| {
                    let circles = self.circles_for_systolic_slice(slice);
                    circles.len() == 2 && CircleNE::is_between_hypercycles_fast(&circles[0], &circles[1], cen)
                })
                .map(|slice| (slice - 1) as usize)
                .collect();
        }

        let is_inside =
            |c: &CircleNE| if spherical_puzzle { c.is_point_inside_ne(cen) } else { c.is_point_inside_fast(cen) };

        // Slices are ordered by depth, so cycle from the inner slice outward.
        if let Some(slice) = self.circles.iter().position(is_inside) {
            return vec![slice];
        }

        // For spherical puzzles we're in the last slice. (The second check was needed for {3,5} 8C.)
        if spherical_puzzle && self.num_slices() != self.circles.len() as i32 {
            return vec![(self.num_slices() - 1) as usize];
        }
        vec![]
    }

    /// The transformation for (part of) a twist.
    pub fn mobius_for_twist(
        &self,
        config: &PuzzleConfig,
        twist: &SingleTwist,
        mut rotation: f64,
        use_systolic_twist_data: bool,
    ) -> Mobius {
        let mut mobius = Mobius::default();
        if config.systolic() {
            let mask = if use_systolic_twist_data { twist.slice_mask_systolic } else { twist.slice_mask };
            let hex_seg = slice_mask::mask_to_dir_seg(mask) as usize;
            let pants = self.pants.as_ref().expect("systolic twist data has pants");
            let mut p1 = pants.hexagon.segments[hex_seg].p2;
            let mut p2 = pants.hexagon.segments[hex_seg].p1;
            if pants.isometry.reflected() {
                std::mem::swap(&mut p1, &mut p2);
            }

            // Earthquake puzzles twist twice as far.
            if !config.earthquake() {
                rotation /= 2.0;
            }
            mobius.geodesic(Geometry::Hyperbolic, p1, p2, rotation);
        } else {
            mobius.elliptic(config.geometry(), self.center, if self.reverse { -rotation } else { rotation });
        }
        mobius
    }

    /// A copy of this (template) twist data moved by an isometry.
    pub fn transformed(&self, isometry: &Isometry, reverse: bool) -> TwistData {
        let mut t = TwistData::new(self.twist_type, isometry.apply(self.center), self.order);
        // (The center can't be made infinity-safe here: it needs to stay accurate.)
        t.reverse = reverse;
        t.num_slices = Some(self.num_slices());
        t.pants = self.pants.as_ref().map(|p| {
            let mut p = p.clone();
            p.transform(isometry);
            p
        });
        t.circles = self
            .circles
            .iter()
            .map(|c| {
                let mut c = c.clone();
                c.transform(isometry);
                c
            })
            .collect();
        t
    }
}
