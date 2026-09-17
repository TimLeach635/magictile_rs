//! The drawing layer: the commands a frame is built from, and the geometry that goes with them.
//!
//! This knows nothing about puzzles, so the tiling viewer (and a browser build) can use it
//! without pulling in the puzzle model.
#![cfg_attr(not(feature = "puzzle"), allow(dead_code))]

use crate::view::Model;
use bytemuck::{Pod, Zeroable};
use eframe::egui::Color32;
use r3::models::HyperbolicModel;
use r3::{Polygon, Vector3D, infinity};
use std::f64::consts::PI;
use std::ops::Range;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ColorVertex {
    pub pos: [f32; 2],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct TexVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub layer: u32,
}

/// Drawing commands, executed in order.
#[derive(Clone, Debug)]
pub enum Cmd {
    /// Solid triangles (indices into `solid`), optionally restricted to the clip region.
    Solid {
        range: Range<u32>,
        clipped: bool,
    },
    /// Textured cell triangles (indices into `cell_indices`).
    Cells(Range<u32>),
    /// A concave (or inverted) polygon filled with the stencil buffer: a fan into the stencil
    /// (`fan`), then a covering quad (`solid`) drawn where the stencil is set.
    Fill {
        fan: Range<u32>,
        cover: Range<u32>,
        inverted: bool,
        clipped: bool,
    },
    /// Marks the clip region with triangles from `solid` (stencil bit 1).
    SetClip(Range<u32>),
    ClearClip,
}

/// Maps world coordinates to normalized device coordinates: `ndc = matrix * world`.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub matrix: [[f32; 2]; 2],
}

impl Camera {
    /// The view: rotated, with half-height `scale`.
    pub fn view(width: f32, height: f32, scale: f64, rotation: f64) -> Camera {
        let aspect = width as f64 / height as f64;
        let (sx, sy) = (1.0 / (aspect * scale), 1.0 / scale);
        let (c, s) = (rotation.cos(), rotation.sin());
        Camera { matrix: [[(sx * c) as f32, (sx * -s) as f32], [(sy * s) as f32, (sy * c) as f32]] }
    }

    /// A square region of half-width `half` (for cell textures).
    pub fn square(half: f64) -> Camera {
        let f = (1.0 / half) as f32;
        Camera { matrix: [[f, 0.0], [0.0, f]] }
    }
}

#[derive(Clone, Debug)]
pub struct DrawList {
    pub clear: [f32; 4],
    pub camera: Camera,
    pub solid: Vec<ColorVertex>,
    pub fan: Vec<[f32; 2]>,
    pub cell_vertices: Vec<TexVertex>,
    pub cell_indices: Vec<u32>,
    pub cmds: Vec<Cmd>,
    /// World units per pixel, for line widths.
    pixel: f64,
}

impl DrawList {
    pub fn new(clear: Color32, camera: Camera, pixel: f64) -> Self {
        DrawList {
            clear: rgba(clear),
            camera,
            solid: Vec::new(),
            fan: Vec::new(),
            cell_vertices: Vec::new(),
            cell_indices: Vec::new(),
            cmds: Vec::new(),
            pixel,
        }
    }

    pub(crate) fn solid_triangles(&mut self, points: impl IntoIterator<Item = Vector3D>, color: Color32) -> Range<u32> {
        let start = self.solid.len() as u32;
        let c = rgba(color);
        self.solid.extend(points.into_iter().map(|p| ColorVertex { pos: pt(p), color: c }));
        start..self.solid.len() as u32
    }

    /// Draws solid triangles, merging with the previous command when it draws the triangles just
    /// before these (so runs of simple polygons and lines become a single draw).
    pub(crate) fn push_solid(&mut self, range: Range<u32>, clipped: bool) {
        if range.is_empty() {
            return;
        }
        if let Some(Cmd::Solid { range: last, clipped: last_clipped }) = self.cmds.last_mut()
            && *last_clipped == clipped
            && last.end == range.start
        {
            last.end = range.end;
            return;
        }
        self.cmds.push(Cmd::Solid { range, clipped });
    }

    /// A filled fan (convex region) around a center.
    pub(crate) fn convex_fan(&mut self, center: Vector3D, ring: &[Vector3D], color: Color32) -> Range<u32> {
        let tris = ring.windows(2).flat_map(|w| [center, w[0], w[1]]);
        self.solid_triangles(tris, color)
    }

    /// Fills a polygon (possibly concave, or containing infinity when `inverted`). `fan_origin`
    /// can be any point; `points` are the (transformed) closed edge points.
    ///
    /// Polygons that a fan from `fan_origin` covers exactly (most stickers) are drawn as plain
    /// triangles, which batch together; the rest use the stencil technique.
    pub(crate) fn fill(
        &mut self,
        fan_origin: Vector3D,
        points: &[Vector3D],
        inverted: bool,
        color: Color32,
        clipped: bool,
    ) {
        if points.len() < 3 || points.iter().any(|p| p.is_dne()) {
            return;
        }
        if !inverted && !infinity::is_infinite(fan_origin) && fan_covers(fan_origin, points) {
            let range = self.convex_fan(fan_origin, points, color);
            self.push_solid(range, clipped);
            return;
        }
        let cen = if infinity::is_infinite(fan_origin) { infinity::LARGE_FINITE_VECTOR } else { fan_origin };

        let fan_start = self.fan.len() as u32;
        let (mut min, mut max) = (cen, cen);
        for w in points.windows(2) {
            self.fan.extend([pt(cen), pt(w[0]), pt(w[1])]);
        }
        for p in points {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        let fan = fan_start..self.fan.len() as u32;

        if inverted {
            const LARGE: f64 = 1000.0;
            min = Vector3D::new(-LARGE, -LARGE);
            max = Vector3D::new(LARGE, LARGE);
        }
        let quad = [
            Vector3D::new(min.x, min.y),
            Vector3D::new(min.x, max.y),
            Vector3D::new(max.x, max.y),
            Vector3D::new(min.x, min.y),
            Vector3D::new(max.x, max.y),
            Vector3D::new(max.x, min.y),
        ];
        let cover = self.solid_triangles(quad, color);
        self.cmds.push(Cmd::Fill { fan, cover, inverted, clipped });
    }

    /// Fills a polygon (in world coordinates), applying a point transform to its edges.
    pub(crate) fn fill_polygon(
        &mut self,
        p: &Polygon,
        color: Color32,
        f: impl Fn(Vector3D) -> Vector3D,
        clipped: bool,
    ) {
        let points: Vec<Vector3D> = p.edge_points().into_iter().map(&f).collect();
        self.fill(f(p.center), &points, p.is_inverted(), color, clipped);
    }

    /// A polyline with a width in pixels.
    pub(crate) fn polyline(&mut self, points: &[Vector3D], width_px: f64, color: Color32, clipped: bool) {
        let range = self.polyline_triangles(points, width_px, color);
        self.push_solid(range, clipped);
    }

    /// A polyline's triangles, without a command to draw them (for batching).
    pub(crate) fn polyline_triangles(&mut self, points: &[Vector3D], width_px: f64, color: Color32) -> Range<u32> {
        self.stroke(points, width_px, color, false)
    }

    /// A polyline's triangles, cut square at its first and last points, for lines that meet
    /// others there.
    pub(crate) fn open_polyline_triangles(&mut self, points: &[Vector3D], width_px: f64, color: Color32) -> Range<u32> {
        self.stroke(points, width_px, color, true)
    }

    fn stroke(&mut self, points: &[Vector3D], width_px: f64, color: Color32, square_ends: bool) -> Range<u32> {
        let half = width_px * self.pixel / 2.0;
        let segments = points.len().saturating_sub(1);
        let mut tris = Vec::new();
        for (i, w) in points.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            if a.is_dne() || b.is_dne() || infinity::is_infinite(a) || infinity::is_infinite(b) {
                continue;
            }
            let mut d = b - a;
            if !d.normalize() {
                continue;
            }
            // Extend a little along the segment so joints don't show gaps.
            let along = d * half;
            let n = Vector3D::new(-d.y, d.x) * half;
            let a = if square_ends && i == 0 { a } else { a - along };
            let b = if square_ends && i + 1 == segments { b } else { b + along };
            tris.extend([a + n, a - n, b + n, b + n, a - n, b - n]);
        }
        self.solid_triangles(tris, color)
    }

    /// The outline of a closed polygon, with a width in pixels, mitred so its corners meet
    /// cleanly. `ring` repeats its first point at the end.
    pub(crate) fn outline_triangles(&mut self, ring: &[Vector3D], width_px: f64, color: Color32) -> Range<u32> {
        let half = width_px * self.pixel / 2.0;
        let mut points: Vec<Vector3D> = Vec::with_capacity(ring.len());
        for &p in ring {
            if points.last().is_none_or(|last| last.dist(p) > 1e-12) {
                points.push(p);
            }
        }
        if points.len() > 1 && points[0].dist(points[points.len() - 1]) <= 1e-12 {
            points.pop();
        }
        let n = points.len();
        if n < 3 || points.iter().any(|p| p.is_dne() || infinity::is_infinite(*p)) {
            return self.solid_triangles(std::iter::empty(), color);
        }
        let normal = |a: Vector3D, b: Vector3D| {
            let mut d = b - a;
            d.normalize();
            Vector3D::new(-d.y, d.x)
        };
        let sides: Vec<Vector3D> = (0..n).map(|i| normal(points[i], points[(i + 1) % n])).collect();
        let corners: Vec<(Vector3D, Vector3D)> = (0..n)
            .map(|i| {
                let (before, after) = (sides[(i + n - 1) % n], sides[i]);
                let mut miter = before + after;
                // The miter is longer by one over the cosine of half the turn; sharp turns are
                // capped so they don't spike.
                let cos = if miter.normalize() {
                    (miter.x * after.x + miter.y * after.y).max(0.5)
                } else {
                    miter = after;
                    1.0
                };
                let offset = miter * (half / cos);
                (points[i] + offset, points[i] - offset)
            })
            .collect();
        let tris: Vec<Vector3D> = (0..n)
            .flat_map(|i| {
                let (a, b) = (corners[i], corners[(i + 1) % n]);
                [a.0, a.1, b.0, b.0, a.1, b.1]
            })
            .collect();
        self.solid_triangles(tris, color)
    }
}

/// Whether a fan of triangles from `center` to a closed boundary covers exactly the region the
/// stencil fill would: every triangle turns the same way, and the boundary goes around once.
fn fan_covers(center: Vector3D, points: &[Vector3D]) -> bool {
    let scale = points.iter().map(|p| (*p - center).abs()).fold(0.0, f64::max);
    let tiny = 1e-12 * scale * scale;
    let (mut sign, mut turned) = (0.0, 0.0);
    for w in points.windows(2) {
        let (a, b) = (w[0] - center, w[1] - center);
        let cross = a.x * b.y - a.y * b.x;
        let dot = a.x * b.x + a.y * b.y;
        if cross.abs() <= tiny {
            // Repeated points are fine; doubling back through the center isn't.
            if dot < 0.0 {
                return false;
            }
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
        turned += cross.atan2(dot);
    }
    (turned.abs() - 2.0 * PI).abs() < 1e-6
}

pub(crate) fn pt(v: Vector3D) -> [f32; 2] {
    [v.x.clamp(-1e5, 1e5) as f32, v.y.clamp(-1e5, 1e5) as f32]
}

pub fn rgba(c: Color32) -> [f32; 4] {
    [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0]
}

/// Fills the whole hyperbolic plane as it appears in a model.
pub(crate) fn fill_hyperbolic_plane(list: &mut DrawList, model: Model, color: Color32) {
    let range = plane_triangles(list, model, color);
    list.push_solid(range, false);
}

/// Triangles covering the whole hyperbolic plane as it appears in a model, without a command to
/// draw them.
pub(crate) fn plane_triangles(list: &mut DrawList, model: Model, color: Color32) -> Range<u32> {
    match model {
        Model::Hyperbolic(HyperbolicModel::UpperHalfPlane) | Model::Hyperbolic(HyperbolicModel::Orthographic) => {
            let big = 10000.0;
            let bottom = if model == Model::Hyperbolic(HyperbolicModel::UpperHalfPlane) { -1.0 } else { -big };
            let quad = [
                Vector3D::new(big, bottom),
                Vector3D::new(big, big),
                Vector3D::new(-big, big),
                Vector3D::new(big, bottom),
                Vector3D::new(-big, big),
                Vector3D::new(-big, bottom),
            ];
            list.solid_triangles(quad, color)
        }
        _ => {
            let ring: Vec<Vector3D> =
                (0..=250).map(|i| 2.0 * PI * i as f64 / 250.0).map(|a| Vector3D::new(a.cos(), a.sin())).collect();
            list.convex_fan(Vector3D::ORIGIN, &ring, color)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(points: &[(f64, f64)]) -> Vec<Vector3D> {
        let mut r: Vec<Vector3D> = points.iter().map(|&(x, y)| Vector3D::new(x, y)).collect();
        r.push(r[0]);
        r
    }

    #[test]
    fn fans_cover_star_shaped_polygons_only() {
        let o = Vector3D::ORIGIN;
        // Convex, either way round, with a repeated point.
        assert!(fan_covers(o, &ring(&[(1.0, 0.0), (0.0, 1.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)])));
        assert!(fan_covers(o, &ring(&[(0.0, -1.0), (-1.0, 0.0), (0.0, 1.0), (1.0, 0.0)])));
        // An L shape seen from a point in the corner it doesn't contain.
        let l = ring(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)]);
        assert!(!fan_covers(Vector3D::new(1.8, 1.8), &l));
        // ...but star-shaped from a point near its elbow.
        assert!(fan_covers(Vector3D::new(0.5, 0.5), &l));
        // Going around twice.
        let twice =
            ring(&[(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]);
        assert!(!fan_covers(o, &twice));
        // The center outside the polygon.
        assert!(!fan_covers(Vector3D::new(5.0, 0.0), &ring(&[(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)])));
    }

    #[test]
    fn consecutive_solid_draws_merge() {
        let mut list = DrawList::new(Color32::WHITE, Camera::square(1.0), 0.01);
        let square = ring(&[(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]);
        list.fill(Vector3D::ORIGIN, &square, false, Color32::RED, false);
        list.fill(Vector3D::ORIGIN, &square, false, Color32::BLUE, false);
        assert_eq!(list.cmds.len(), 1);
        // A clipped draw can't join an unclipped one, and a concave polygon needs the stencil.
        list.fill(Vector3D::ORIGIN, &square, false, Color32::RED, true);
        let l = ring(&[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)]);
        list.fill(Vector3D::new(1.8, 1.8), &l, false, Color32::RED, false);
        assert_eq!(list.cmds.len(), 3);
        assert!(matches!(list.cmds[2], Cmd::Fill { .. }));
    }
}
