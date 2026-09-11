//! Texture coordinates and triangle lattices for texture-mapping polygons.
//!
//! Each polygon is split into one big triangle per edge (edge + center), each subdivided into a
//! lattice of smaller triangles, evenly spaced in the relevant geometry.

use crate::donhatch;
use crate::geometry2d::Geometry;
use crate::mobius::{Mobius, Transform};
use crate::nethash::NetMap;
use crate::polygon::{Polygon, Segment, SegmentType};
use crate::spherical2d;
use crate::vector3d::Vector3D;

#[derive(Clone, Debug, Default)]
pub struct TextureHelper {
    /// Triangle element indices into the texture coordinates, for each level of detail
    /// (the first entry has the least detail).
    pub element_indices: Vec<Vec<u32>>,
}

impl TextureHelper {
    pub fn setup_element_indices(&mut self, poly: &Polygon) {
        self.element_indices = calc_element_indices(poly, 3);
    }
}

fn num_div(levels: u32) -> usize {
    1 << levels
}

pub fn calc_element_indices(poly: &Polygon, levels: u32) -> Vec<Vec<u32>> {
    let num_base_triangles = poly.segments.len();
    let num_div = num_div(levels);
    (0..=levels).map(|lod| texture_elements(num_base_triangles, lod, num_div)).collect()
}

/// Texture coordinates for a polygon: a triangle lattice for each segment (the segment and the
/// polygon center form one big triangle, which is subdivided).
pub fn texture_coords(poly: &Polygon, g: Geometry, max_div: usize) -> Vec<Vector3D> {
    let divisions = max_div;
    let mut points = Vec::new();
    for s in &poly.segments {
        let s1 = subdivide_segment_in_geometry(s.p1, poly.center, divisions, g);
        let s2 = subdivide_segment_in_geometry(s.p2, poly.center, divisions, g);
        for i in 0..divisions {
            points.extend(subdivide_segment_in_geometry(s1[i], s2[i], divisions - i, g));
        }
        points.push(poly.center);
    }
    points
}

/// Merges duplicate vertices. The number of indices stays the same (with new values).
pub fn merge_verts(coords: &[Vector3D], indices: &[u32]) -> (Vec<Vector3D>, Vec<u32>) {
    let mut new_coords = Vec::new();
    let mut new_indices = Vec::with_capacity(indices.len());
    let mut vector_map: NetMap<Vector3D, u32> = NetMap::new();
    for &idx in indices {
        let v = coords[idx as usize];
        let new_idx = *vector_map.get_or_insert_with(v, || {
            new_coords.push(v);
            (new_coords.len() - 1) as u32
        });
        new_indices.push(new_idx);
    }
    (new_coords, new_indices)
}

/// Subdivides p1 -> p2 evenly in the given geometry (endpoints included).
fn subdivide_segment_in_geometry(p1: Vector3D, p2: Vector3D, divisions: usize, g: Geometry) -> Vec<Vector3D> {
    if g == Geometry::Euclidean {
        return Segment::line(p1, p2).subdivide(divisions as i32);
    }

    let mut p1_to_origin = Mobius::default();
    p1_to_origin.isometry(g, 0.0, -p1);
    let inverse = p1_to_origin.inverse();

    let new_p2 = p1_to_origin.apply(p2);
    let radial = Segment::line(Vector3D::ORIGIN, new_p2);
    subdivide_radial_in_geometry(&radial, divisions, g).into_iter().map(|v| inverse.apply(v)).collect()
}

/// Evenly subdivides a segment starting at the origin, in the given geometry.
fn subdivide_radial_in_geometry(radial: &Segment, divisions: usize, g: Geometry) -> Vec<Vector3D> {
    if radial.kind != SegmentType::Line {
        return Vec::new();
    }

    let e_length = radial.length();
    type NormFn = fn(f64) -> f64;
    let (to_geometry, to_euclidean): (NormFn, NormFn) = match g {
        Geometry::Euclidean => return radial.subdivide(divisions as i32),
        Geometry::Spherical => (spherical2d::e2s_norm, spherical2d::s2e_norm),
        Geometry::Hyperbolic => (donhatch::e2h_norm, donhatch::h2e_norm),
    };

    let div_length = to_geometry(e_length) / divisions as f64;
    (0..=divisions).map(|i| radial.p2 * to_euclidean(div_length * i as f64) / e_length).collect()
}

fn triangular_number(n: usize) -> usize {
    n * (n + 1) / 2
}

/// Indices into `texture_coords` output forming triangles (each 3 indices is one triangle),
/// at a level of detail (0 is coarsest).
pub fn texture_elements(num_base_triangles: usize, lod: u32, max_div: usize) -> Vec<u32> {
    let divisions = max_div;
    let stride = divisions / (1 << lod);

    let num_verts_per_segment = triangular_number(divisions + 1);

    let mut result = Vec::new();
    let mut offset = 0;
    for _ in 0..num_base_triangles {
        let mut start2 = offset;
        let mut i = 0;
        while i < divisions {
            let start1 = start2;

            let mut temp = divisions - i + 1;
            for _ in 0..stride {
                start2 += temp;
                temp -= 1;
            }

            let mut j = 0;
            while j < divisions - i {
                result.extend([start1 + j, start1 + j + stride, start2 + j].map(|x| x as u32));
                j += stride;
            }

            let mut j = 0;
            while j + stride < divisions - i {
                result.extend([start2 + j, start1 + j + stride, start2 + j + stride].map(|x| x as u32));
                j += stride;
            }

            i += stride;
        }

        offset += num_verts_per_segment;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lattice_sizes() {
        let mut poly = Polygon::default();
        poly.create_regular(7, 3);
        let coords = texture_coords(&poly, Geometry::Hyperbolic, 8);
        assert_eq!(coords.len(), 7 * triangular_number(9));

        let elements = calc_element_indices(&poly, 3);
        assert_eq!(elements.len(), 4);
        for (lod, e) in elements.iter().enumerate() {
            let per_side = 1usize << (2 * lod); // 4^lod triangles per base triangle
            assert_eq!(e.len(), 7 * per_side * 3, "lod {lod}");
            assert!(e.iter().all(|&i| (i as usize) < coords.len()));
        }

        // Lattice points stay within the polygon's circumcircle.
        let r = poly.segments[0].p1.abs();
        assert!(coords.iter().all(|c| c.abs() <= r + 1e-9));
    }

    #[test]
    fn merging() {
        let mut poly = Polygon::default();
        poly.create_regular(4, 4);
        let coords = texture_coords(&poly, Geometry::Euclidean, 2);
        let elements = texture_elements(4, 1, 2);
        let (merged, indices) = merge_verts(&coords, &elements);
        assert_eq!(indices.len(), elements.len());
        // A 4-gon split into 4 triangles of 4 each: center, 4 corners, 4 edge midpoints, 4 inner.
        assert_eq!(merged.len(), 13);
        for (i, &idx) in indices.iter().enumerate() {
            assert_eq!(merged[idx as usize], coords[elements[i] as usize]);
        }
    }
}
