//! Counts the faces, edges and vertices of a puzzle's surface, grouping the physical copies of each
//! logical element. Easy for regular colorings, but the stranger colorings need a traversal of
//! all the identified cells. The groupings also index the puzzle's twists.

use crate::cell::{Cell, CellId};
use crate::twist_data::ElementType;
use r3::{NetSet, Tile, Transform, Vector3D};
use std::fmt;

/// For each element type, one set per logical element holding all its copies.
#[derive(Clone, Debug, Default)]
pub struct Topology {
    faces: Vec<NetSet<Vector3D>>,
    edges: Vec<NetSet<Vector3D>>,
    vertices: Vec<NetSet<Vector3D>>,
}

impl fmt::Display for Topology {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "F={}, E={}, V={}, χ={}", self.f(), self.e(), self.v(), self.euler_characteristic())
    }
}

impl Topology {
    pub fn f(&self) -> usize {
        self.faces.len()
    }
    pub fn e(&self) -> usize {
        self.edges.len()
    }
    pub fn v(&self) -> usize {
        self.vertices.len()
    }

    pub fn euler_characteristic(&self) -> i64 {
        self.v() as i64 - self.e() as i64 + self.f() as i64
    }

    /// Analyzes a puzzle's cells. `slaves[i]` are the slaves of `masters[i]`, and `infinity_safe`
    /// maps points at infinity to a finite stand-in (for spherical puzzles).
    pub fn analyze(
        cells: &[Cell],
        masters: &[CellId],
        slaves: &[Vec<CellId>],
        template: &Tile,
        infinity_safe: impl Fn(Vector3D) -> Vector3D,
    ) -> Topology {
        let mut t = Topology::default();

        // Faces are easy, since their canonical points are only in one place.
        for (m, &master) in masters.iter().enumerate() {
            let mut identified = NetSet::new();
            identified.insert(infinity_safe(cells[master].center()));
            for &slave in &slaves[m] {
                identified.insert(infinity_safe(cells[slave].center()));
            }
            t.faces.push(identified);
        }

        for element in [ElementType::Edge, ElementType::Vertex] {
            let point = |cell: &Cell, seg: usize| match element {
                // We must transform, not take the segment midpoint: the Euclidean midpoint of a
                // transformed segment isn't the transformed midpoint.
                ElementType::Edge => {
                    infinity_safe(cell.isometry_inverse().apply(template.boundary.segments[seg].midpoint()))
                }
                _ => infinity_safe(cell.boundary.segments[seg].p1),
            };
            let list = t.list_mut(element);
            analyze_element(list, cells, masters, slaves, point);
        }
        t
    }

    fn list(&self, element: ElementType) -> &Vec<NetSet<Vector3D>> {
        match element {
            ElementType::Face => &self.faces,
            ElementType::Edge => &self.edges,
            ElementType::Vertex => &self.vertices,
        }
    }

    fn list_mut(&mut self, element: ElementType) -> &mut Vec<NetSet<Vector3D>> {
        match element {
            ElementType::Face => &mut self.faces,
            ElementType::Edge => &mut self.edges,
            ElementType::Vertex => &mut self.vertices,
        }
    }

    /// The index of the logical element at a point, or `None`. Indices are unique across element
    /// types (faces, then edges, then vertices).
    pub fn logical_element_index(&self, element: ElementType, point: Vector3D) -> Option<usize> {
        let index = find_identified(self.list(element), point)?;
        Some(match element {
            ElementType::Face => index,
            ElementType::Edge => index + self.f(),
            ElementType::Vertex => index + self.f() + self.e(),
        })
    }
}

/// Starting from each master point, collect all the identified slave points into a set. These
/// "trees" of points can intersect, in which case they are merged, since everything in both is
/// identified. (The {8,4} 10C puzzle drove this: a logical point may physically be in several
/// places on the edge of the fundamental region, each covering only some of its copies.)
fn analyze_element(
    list: &mut Vec<NetSet<Vector3D>>,
    cells: &[Cell],
    masters: &[CellId],
    slaves: &[Vec<CellId>],
    point: impl Fn(&Cell, usize) -> Vector3D,
) {
    let mut complete = NetSet::new();
    for (m, &master) in masters.iter().enumerate() {
        for i in 0..cells[master].boundary.segments.len() {
            let master_point = point(&cells[master], i);

            // Start a new set for this point, unless it's in a previous one.
            let mut identified = if complete.contains(&master_point) {
                match find_identified(list, master_point) {
                    Some(index) => index,
                    None => continue,
                }
            } else {
                let mut set = NetSet::new();
                set.insert(master_point);
                list.push(set);
                complete.insert(master_point);
                list.len() - 1
            };

            for &slave in &slaves[m] {
                let slave_point = point(&cells[slave], i);
                if complete.contains(&slave_point) {
                    // Already in a set, perhaps another one, in which case we merge with it.
                    if let Some(other) = find_identified(list, slave_point)
                        && other != identified
                    {
                        let merged: Vec<Vector3D> = list[other].iter().copied().collect();
                        for v in merged {
                            list[identified].insert(v);
                        }
                        list.remove(other);
                        if other < identified {
                            identified -= 1;
                        }
                    }
                    continue;
                }

                list[identified].insert(slave_point);
                complete.insert(slave_point);
            }
        }
    }
}

fn find_identified(list: &[NetSet<Vector3D>], point: Vector3D) -> Option<usize> {
    list.iter().position(|set| set.contains(&point))
}
