//! Slicing polygons with (generalized) circles.

use crate::circle::{Circle, CircleNE};
use crate::euclidean2d;
use crate::geometry2d::Geometry;
use crate::mobius::{Mobius, Transform};
use crate::nethash;
use crate::polygon::{Polygon, Segment};
use crate::tolerance;
use crate::vector3d::Vector3D;
use std::f64::consts::PI;

/// Slices a polygon by a circle with some thickness (in the given geometry). The circle may be a
/// line. The input polygon may get reversed.
pub fn slice_polygon_thick(p: &mut Polygon, c: &CircleNE, g: Geometry, thickness: f64) -> Vec<Polygon> {
    // Set up the two slicing circles, offset to either side.
    let (mut c1, mut c2) = (c.clone(), c.clone());
    let point_on_circle = if c.is_line() { c.p1 } else { c.center + Vector3D::new(c.radius, 0.0) };

    let offset = thickness / 2.0;
    let mut m = Mobius::default();
    m.hyperbolic2(g, c1.center_ne, point_on_circle, offset);
    c1.transform(&m);
    m.hyperbolic2(g, c2.center_ne, point_on_circle, -offset);
    c2.transform(&m);

    slice_polygon_helper(p, &c1, &c2)
}

/// Offsets a hyperbolic geodesic (which must be orthogonal to the disk boundary) to either side,
/// giving two equidistant curves.
pub fn offset_hyperbolic_geodesic(c: &Circle, thickness: f64) -> (Circle, Circle) {
    let g = Geometry::Hyperbolic;
    let disk = Circle::default();

    let seg = if c.is_line() {
        let (_, p1, p2) = euclidean2d::intersection_line_circle(c.p1, c.p2, &disk);
        Segment::line(p1, p2)
    } else {
        // The point of the geodesic closest to the origin.
        let mut direction = -c.center;
        direction.normalize();
        direction *= c.radius;
        let closest_to_origin = c.center + direction;

        let (_, p1, p2) = euclidean2d::intersection_circle_circle(c, &disk);
        Segment::arc(p1, closest_to_origin, p2)
    };

    (equidistant_offset(g, &seg, thickness / 2.0), equidistant_offset(g, &seg, -thickness / 2.0))
}

/// The curve at a constant (signed) distance from a geodesic segment.
pub fn equidistant_offset(g: Geometry, seg: &Segment, offset: f64) -> Circle {
    let transform = |v: Vector3D| {
        if tolerance::equal(v.abs(), 1.0) {
            return v; // Not true in general, but the code below goes haywire otherwise.
        }

        let mut m2 = Mobius::default();
        m2.isometry(g, 0.0, -v);
        let p1 = m2.apply(seg.p1);
        let p2 = m2.apply(seg.p2);

        let mut direction2 = p2 - p1;
        direction2.rotate_xy(PI / 2.0);
        direction2.normalize();
        let mut m3 = Mobius::default();
        m3.isometry(g, 0.0, direction2 * offset);

        let final_m = m2.inverse() * m3 * m2;
        final_m.apply(v)
    };

    Circle::from_3_points(transform(seg.p1), transform(seg.midpoint()), transform(seg.p2))
}

/// Slicing used for systolic puzzles. `c` should be a hyperbolic geodesic.
pub fn slice_polygon_with_hyperbolic_geodesic(p: &mut Polygon, c: &CircleNE, thickness: f64) -> Vec<Polygon> {
    let (c1, c2) = offset_hyperbolic_geodesic(c, thickness);

    // Only the center and radius are replaced (the line points and NE center are kept).
    let (mut c1_ne, mut c2_ne) = (c.clone(), c.clone());
    c1_ne.center = c1.center;
    c1_ne.radius = c1.radius;
    c2_ne.center = c2.center;
    c2_ne.radius = c2.radius;
    slice_polygon_helper(p, &c1_ne, &c2_ne)
}

/// Keeps the pieces outside c1 and the pieces inside c2 (in the non-Euclidean sense).
fn slice_polygon_helper(p: &mut Polygon, c1: &CircleNE, c2: &CircleNE) -> Vec<Polygon> {
    let sliced1 = slice_polygon(p, c1);
    let sliced2 = slice_polygon(p, c2);

    let mut output = Vec::new();
    output.extend(sliced1.into_iter().filter(|poly| !c1.is_point_inside_ne(poly.centroid_approx())));
    output.extend(sliced2.into_iter().filter(|poly| c2.is_point_inside_ne(poly.centroid_approx())));
    output
}

#[derive(Clone, Copy, Debug)]
struct IntersectionPoint {
    location: Vector3D,
    /// Index in the diced polygon of the segment starting at this location.
    index: usize,
}

/// Slices up a polygon with a circle (or line). The input polygon may get reversed.
///
/// Returns an empty list if slicing fails (as the original does), and the polygon unchanged if the
/// circle doesn't cut it.
pub fn slice_polygon(p: &mut Polygon, c: &Circle) -> Vec<Polygon> {
    // Our approach:
    // (1) Find the intersection points, and splice them into the polygon.
    // (2) From each intersection point, walk the polygon.
    // (3) At an intersection point, always turn left, which may involve adding a new segment of the
    //     slicing circle.
    // (4) Remove duplicate polygons from the result.

    // We must be a digon at a minimum.
    if p.num_sides() < 2 {
        return Vec::new();
    }

    // The code assumes a well-formed CCW polygon.
    if !p.orientation() {
        p.reverse();
    }

    // Splice in all the intersection points.
    let mut diced = Polygon::default();
    let mut i_points: Vec<IntersectionPoint> = Vec::new();
    for s in &p.segments {
        let Some(intersections) = c.intersection_points(s) else {
            continue;
        };

        match intersections.len() {
            0 => diced.segments.push(*s),
            1 => {
                let seg = split_helper(*s, intersections[0], &mut diced, &mut i_points);
                diced.segments.push(seg);
            }
            2 => {
                // Order the intersection points along the segment.
                let (mut i1, mut i2) = (intersections[0], intersections[1]);
                if !s.ordered(i1, i2) {
                    std::mem::swap(&mut i1, &mut i2);
                }
                let second_to_split = split_helper(*s, i1, &mut diced, &mut i_points);
                let segment_to_add = split_helper(second_to_split, i2, &mut diced, &mut i_points);
                diced.segments.push(segment_to_add);
            }
            _ => return Vec::new(),
        }
    }

    // No intersections, or a single one (a tangency, which we let slip through as unsliced).
    if i_points.len() < 2 {
        return vec![p.clone()];
    }

    // We don't deal with tangencies, and this case could be more problematic.
    if i_points.len() % 2 == 1 {
        return Vec::new();
    }

    if i_points.len() > 2 {
        // We may need to reorder the intersection points by one, so that walking from i1 -> i2
        // along c moves through the interior of the polygon.
        let mut dummy = 0;
        let mut dummy2 = 0;
        let test_arc = smaller_spliced_arc(c, &i_points, &mut dummy, true, &mut dummy2);
        if !p.is_point_inside_paranoid(test_arc.midpoint()) {
            i_points.rotate_left(1);
        }
    }

    // From each pair of intersection points, walk the polygon both ways.
    let mut output = Vec::new();
    for pair in 0..i_points.len() / 2 {
        output.push(walk_polygon(p, &diced, c, pair, &i_points, true));
        output.push(walk_polygon(p, &diced, c, pair, &i_points, false));
    }

    for poly in &mut output {
        poly.center = poly.centroid_approx();
    }

    nethash::distinct(output)
}

/// Splits a segment at a location. The first piece goes into `diced` and the second is returned.
/// If no split happens, the segment is returned as is.
fn split_helper(
    segment_to_split: Segment,
    i_location: Vector3D,
    diced: &mut Polygon,
    i_points: &mut Vec<IntersectionPoint>,
) -> Segment {
    match segment_to_split.split(i_location) {
        Some((first, second)) => {
            diced.segments.push(first);
            i_points.push(IntersectionPoint { location: i_location, index: diced.segments.len() });
            second
        }
        None => {
            // We were presumably at an endpoint. Record it only if it was the starting endpoint,
            // to avoid duplicates.
            if i_location.compare(&segment_to_split.p1) {
                i_points.push(IntersectionPoint { location: i_location, index: diced.segments.len() });
            }
            segment_to_split
        }
    }
}

/// Walks a polygon starting from a pair of intersection points; `increment` sets the direction.
fn walk_polygon(
    parent: &Polygon,
    walking: &Polygon,
    c: &Circle,
    mut pair: usize,
    i_points: &[IntersectionPoint],
    increment: bool,
) -> Polygon {
    let mut new_poly = Polygon::default();

    let (i_point1, _) = get_pair_points(i_points, pair, increment);
    let start_location = i_point1.location;

    let mut i_seg = 0;
    let current = spliced_seg(parent, c, i_points, &mut pair, increment, &mut i_seg);
    new_poly.segments.push(current);

    // The original loops until it gets back to the start; guard against pathological cases.
    let max_steps = 4 * (walking.segments.len() + i_points.len()) + 16;
    for _ in 0..max_steps {
        // Since we don't allow tangent intersections, spliced arcs never come in succession.
        let current = walking.segments[i_seg];
        new_poly.segments.push(current);
        i_seg = (i_seg + 1) % walking.num_sides();
        if current.p2.compare(&start_location) {
            break;
        }

        // Do we need to splice in here?
        let seg_end = current.p2;
        if i_points.iter().any(|ip| ip.location == seg_end) {
            let spliced = spliced_seg(parent, c, i_points, &mut pair, increment, &mut i_seg);
            new_poly.segments.push(spliced);
            if spliced.p2.compare(&start_location) {
                break;
            }
        }
    }

    new_poly
}

fn get_pair_points(
    i_points: &[IntersectionPoint],
    pair: usize,
    increment: bool,
) -> (IntersectionPoint, IntersectionPoint) {
    let (mut idx1, mut idx2) = (pair * 2, pair * 2 + 1);
    if !increment {
        std::mem::swap(&mut idx1, &mut idx2);
    }
    (i_points[idx1], i_points[idx2])
}

/// The smaller spliced arc (or the spliced line), advancing `pair`.
fn smaller_spliced_arc(
    c: &Circle,
    i_points: &[IntersectionPoint],
    pair: &mut usize,
    increment: bool,
    next_seg_index: &mut usize,
) -> Segment {
    let (i_point1, i_point2) = get_pair_points(i_points, *pair, increment);
    let (p1, p2) = (i_point1.location, i_point2.location);
    *next_seg_index = i_point2.index;

    let new_seg = if c.is_line() {
        Segment::line(p1, p2)
    } else {
        let mut s = Segment::arc_with_center(p1, p2, c.center);
        if s.angle() > PI {
            s.clockwise = false;
        }
        s
    };

    *pair += 1;
    if *pair == i_points.len() / 2 {
        *pair = 0;
    }

    new_seg
}

fn spliced_seg(
    parent: &Polygon,
    c: &Circle,
    i_points: &[IntersectionPoint],
    pair: &mut usize,
    increment: bool,
    next_seg_index: &mut usize,
) -> Segment {
    let mut spliced = smaller_spliced_arc(c, i_points, pair, increment, next_seg_index);
    if c.is_line() {
        return spliced;
    }

    // Heuristic, but works quite well.
    if spliced.angle().abs() < PI * 0.75 {
        return spliced;
    }

    // The arc should lie inside the parent polygon, which may not be the case above.
    let mut test_angle = spliced.angle() / 1000.0;
    if spliced.clockwise {
        test_angle *= -1.0;
    }

    let mut t1 = spliced.p1;
    t1.rotate_xy_about(spliced.center, test_angle);
    if !parent.is_point_inside_paranoid(t1) {
        spliced.clockwise = !spliced.clockwise;
    }

    spliced
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Polygon {
        Polygon::from_points(&[
            Vector3D::new(-1.0, -1.0),
            Vector3D::new(1.0, -1.0),
            Vector3D::new(1.0, 1.0),
            Vector3D::new(-1.0, 1.0),
        ])
    }

    fn total_area(polys: &[Polygon]) -> f64 {
        polys.iter().map(|p| p.signed_area()).sum()
    }

    #[test]
    fn line_cuts_square_in_two() {
        let mut sq = square();
        let line = Circle::from_2_points(Vector3D::new(0.0, -5.0), Vector3D::new(0.0, 5.0));
        let pieces = slice_polygon(&mut sq, &line);
        assert_eq!(pieces.len(), 2);
        assert!(tolerance::equal(total_area(&pieces), 4.0));
        for p in &pieces {
            assert!(tolerance::equal(p.signed_area(), 2.0));
        }
    }

    #[test]
    fn circle_cuts_square() {
        let mut sq = square();
        let c = Circle::new(Vector3D::new(1.0, 1.0), 1.0);
        let pieces = slice_polygon(&mut sq, &c);
        assert_eq!(pieces.len(), 2);
        let quarter = PI / 4.0;
        let mut areas: Vec<f64> = pieces.iter().map(|p| p.signed_area()).collect();
        areas.sort_by(f64::total_cmp);
        // Edge points approximate arcs with chords, so allow some slack.
        assert!((areas[0] - quarter).abs() < 1e-2, "{areas:?}");
        assert!((areas[1] - (4.0 - quarter)).abs() < 1e-2, "{areas:?}");
    }

    #[test]
    fn circle_through_middle_makes_a_ring_piece() {
        let mut sq = square();
        // A circle cutting all four sides, twice each.
        let c = Circle::new(Vector3D::ORIGIN, 1.2);
        let pieces = slice_polygon(&mut sq, &c);
        // The inside, plus four corners.
        assert_eq!(pieces.len(), 5);
        assert!((total_area(&pieces) - 4.0).abs() < 1e-2);
    }

    #[test]
    fn untouched_polygon_is_returned() {
        let mut sq = square();
        let c = Circle::new(Vector3D::new(5.0, 5.0), 1.0);
        let pieces = slice_polygon(&mut sq, &c);
        assert_eq!(pieces.len(), 1);
    }

    #[test]
    fn thick_slice_removes_a_band() {
        let mut sq = square();
        // (A circle entirely inside the polygon wouldn't cut it at all.)
        let c = CircleNE::new(Circle::new(Vector3D::new(1.0, 0.0), 0.5), Vector3D::new(1.0, 0.0));
        let pieces = slice_polygon_thick(&mut sq, &c, Geometry::Euclidean, 0.1);
        assert_eq!(pieces.len(), 2);
        let band = PI * (0.55 * 0.55 - 0.45 * 0.45) / 2.0;
        assert!((total_area(&pieces) - (4.0 - band)).abs() < 1e-2);
    }

    #[test]
    fn equidistant_offsets_straddle_geodesic() {
        let geodesic = Circle::from_2_points(Vector3D::new(-1.0, 0.0), Vector3D::new(1.0, 0.0));
        let (c1, c2) = offset_hyperbolic_geodesic(&geodesic, 0.2);
        // Hypercycles about a diameter pass through its ideal endpoints.
        for c in [c1, c2] {
            assert!(!c.is_line());
            assert!(c.is_point_on(Vector3D::new(1.0, 0.0)));
            assert!(c.is_point_on(Vector3D::new(-1.0, 0.0)));
        }
        assert!(c1.center.y * c2.center.y < 0.0);
    }
}
