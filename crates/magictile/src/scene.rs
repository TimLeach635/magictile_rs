//! Builds the geometry to draw each frame (on the CPU, as the original's immediate-mode OpenGL
//! did): stencil-filled polygons for directly drawn (spherical) puzzles, textured triangles for
//! the cells of Euclidean and hyperbolic puzzles, and lines for twisting circles.

use crate::settings::Settings;
use crate::view::{Model, View};
use bytemuck::{Pod, Zeroable};
use eframe::egui::Color32;
use magictile_core::cell::{CellId, Sticker};
use magictile_core::twist_data::TwistDataId;
use magictile_core::{Puzzle, TwistController};
use r3::infinity;
use r3::models::HyperbolicModel;
use r3::{Circle, CircleNE, Complex, Geometry, Isometry, Mobius, Polygon, Segment, Transform, Vector3D};
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

    fn solid_triangles(&mut self, points: impl IntoIterator<Item = Vector3D>, color: Color32) -> Range<u32> {
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
    fn fill(&mut self, fan_origin: Vector3D, points: &[Vector3D], inverted: bool, color: Color32, clipped: bool) {
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
    fn fill_polygon(&mut self, p: &Polygon, color: Color32, f: impl Fn(Vector3D) -> Vector3D, clipped: bool) {
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
        let half = width_px * self.pixel / 2.0;
        let mut tris = Vec::new();
        for w in points.windows(2) {
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
            let (a, b) = (a - along, b + along);
            tris.extend([a + n, a - n, b + n, b + n, a - n, b - n]);
        }
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

fn pt(v: Vector3D) -> [f32; 2] {
    [v.x.clamp(-1e5, 1e5) as f32, v.y.clamp(-1e5, 1e5) as f32]
}

pub fn rgba(c: Color32) -> [f32; 4] {
    [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, c.a() as f32 / 255.0]
}

/// Per-puzzle data prepared once after building, for textured rendering.
#[derive(Debug, Default)]
pub struct RenderData {
    /// Texture lattice vertices for every cell (empty for spherical puzzles).
    pub cell_vertices: Vec<Vec<[f64; 2]>>,
    /// Texture coordinates of the lattice points.
    pub uvs: Vec<[f32; 2]>,
    /// For each level of detail: the lattice points used, and triangles indexing into those.
    pub lods: Vec<(Vec<u32>, Vec<u32>)>,
    /// The texture covers [-half, half]^2 around the template cell.
    pub texture_half_width: f64,
}

impl RenderData {
    pub fn new(puzzle: &Puzzle) -> RenderData {
        if puzzle.is_spherical() || puzzle.masters.is_empty() {
            return RenderData::default();
        }

        let template = &puzzle.cells[puzzle.masters[0]];
        let half = template.vertex_circle.radius;
        let factor = 1.0 / half;

        // Everybody uses the texture coordinates of the template.
        let template_coords =
            Isometry::transform_vertices(&puzzle.template_texture_coords, &template.isometry_inverse());
        let uvs = template_coords
            .iter()
            .map(|v| [((v.x * factor + 1.0) / 2.0) as f32, ((1.0 - v.y * factor) / 2.0) as f32])
            .collect();

        let mut cell_vertices = vec![Vec::new(); puzzle.cells.len()];
        for cell in puzzle.all_cells() {
            let inverse = puzzle.cells[cell].isometry_inverse();
            cell_vertices[cell] =
                puzzle.template_texture_coords.iter().map(|v| inverse.apply(*v)).map(|v| [v.x, v.y]).collect();
        }

        let lods = puzzle
            .texture_helper
            .element_indices
            .iter()
            .map(|elements| {
                let mut unique = Vec::new();
                let mut remap = std::collections::HashMap::new();
                let local = elements
                    .iter()
                    .map(|&e| {
                        *remap.entry(e).or_insert_with(|| {
                            unique.push(e);
                            (unique.len() - 1) as u32
                        })
                    })
                    .collect();
                (unique, local)
            })
            .collect();

        RenderData { cell_vertices, uvs, lods, texture_half_width: half }
    }
}

/// Everything needed to draw a frame.
pub struct SceneContext<'a> {
    pub puzzle: &'a Puzzle,
    pub data: &'a RenderData,
    pub controller: &'a TwistController,
    pub settings: &'a Settings,
    pub model: Model,
    pub slice_mask: i32,
    pub closest_twist: Option<TwistDataId>,
    pub closest_geodesic_seg: i32,
}

impl SceneContext<'_> {
    fn geometry(&self) -> Geometry {
        self.puzzle.config.geometry()
    }

    fn sticker_color(&self, sticker: &Sticker) -> Color32 {
        let p = self.puzzle;
        if p.config.coxeter_complex {
            // A simple light-dark scheme.
            let mut parity = sticker.sticker_index.is_multiple_of(2);
            if let Some(&m) = usize::try_from(sticker.cell_index).ok().and_then(|i| p.masters.get(i))
                && p.cells[m].reflected()
            {
                parity = !parity;
            }
            return if parity { Color32::WHITE } else { Color32::GRAY };
        }
        self.raw_color(sticker)
    }

    fn raw_color(&self, sticker: &Sticker) -> Color32 {
        match usize::try_from(sticker.cell_index) {
            Ok(c) if c < self.puzzle.state.num_cells() => {
                self.settings.sticker_color(self.puzzle.state.sticker_color_index(c, sticker.sticker_index))
            }
            _ => Color32::GRAY,
        }
    }

    /// The signed, eased rotation of the animating twist.
    fn rotation(&self) -> f64 {
        let r = self.controller.smoothed_rotation(self.puzzle);
        match self.controller.current_twist() {
            Some(t) if !t.left_click => -r,
            _ => r,
        }
    }
}

/// The level of detail for texturing a cell (higher is more accurate), or `None` to skip it.
fn lod(circle: &CircleNE, view: &Isometry, state_calc_cell: bool, geometry: Geometry) -> Option<usize> {
    let mut c = circle.clone();
    c.transform(view);
    if !state_calc_cell && c.radius < 0.005 {
        return None;
    }
    if geometry == Geometry::Euclidean {
        return Some(0);
    }
    if c.radius == f64::INFINITY {
        return Some(3);
    }
    Some(((c.radius.powf(0.4) * 8.0) as i32).clamp(0, 3) as usize)
}

/// Builds the main view. Also returns the copy of the first master closest to the center (for
/// recentering).
pub fn build_view(ctx: &SceneContext, view: &View, pixels_per_point: f32) -> (DrawList, Option<CellId>) {
    let disks = ctx.model.is_hemisphere_disks();
    let scale = if disks { view.view_scale * 2.0 } else { view.view_scale };
    let camera = Camera::view(view.width, view.height, scale, view.rotation);
    let pixel = 2.0 * scale / (view.height as f64 * pixels_per_point as f64);

    if ctx.puzzle.is_spherical() {
        let clear = if disks { ctx.settings.color_bg } else { ctx.settings.color_tile_edges };
        let mut list = DrawList::new(clear, camera, pixel);
        if disks {
            build_hemisphere_disks(ctx, view, &mut list);
        } else {
            build_direct(ctx, &view.isometry, &mut list, &|v| ctx.model.apply(v), false);
            if ctx.model == Model::Spherical(r3::models::SphericalModel::Fisheye) {
                fill_background_except_disk(ctx, &mut list);
            }
            twisting_circles(ctx, view, &view.isometry, &mut list, &|v| ctx.model.apply(v), false);
        }
        (list, None)
    } else {
        let mut list = DrawList::new(ctx.settings.color_bg, camera, pixel);
        let closest = build_textured(ctx, view, &mut list);
        twisting_circles(ctx, view, &view.isometry, &mut list, &|v| ctx.model.apply(v), false);
        (list, closest)
    }
}

/// Draws spherical puzzles sticker by sticker.
fn build_direct(
    ctx: &SceneContext,
    view: &Isometry,
    list: &mut DrawList,
    f: &dyn Fn(Vector3D) -> Vector3D,
    clipped: bool,
) {
    let p = ctx.puzzle;
    let s = ctx.settings;
    for cell in p.all_cells() {
        let c = &p.cells[cell];
        if lod(&c.vertex_circle, view, p.is_state_calc_cell(cell), ctx.geometry()).is_none() {
            continue;
        }
        if s.show_only_fundamental && !c.is_master() {
            continue;
        }
        for &sticker in &c.stickers {
            let st = &p.stickers[sticker];
            if st.twisting {
                continue;
            }
            let mut poly = st.poly.clone();
            poly.transform(view);
            if clipped && poly.center.abs() > 2.0 {
                continue;
            }
            list.fill_polygon(&poly, ctx.sticker_color(st), f, clipped);
        }
    }
    moving_stickers_direct(ctx, view, list, f, clipped);
}

fn moving_stickers_direct(
    ctx: &SceneContext,
    view: &Isometry,
    list: &mut DrawList,
    f: &dyn Fn(Vector3D) -> Vector3D,
    clipped: bool,
) {
    let Some(twist) = ctx.controller.current_twist() else {
        return;
    };
    let p = ctx.puzzle;
    let rotation = ctx.rotation();
    for &td_id in &p.all_twist_data[twist.identified].twist_data_for_drawing {
        let td = &p.twist_data[td_id];
        let mut mobius = Mobius::default();
        mobius.elliptic(ctx.geometry(), td.center, if td.reverse { -rotation } else { rotation });
        let isometry = Isometry::new(view.mobius * mobius, view.reflection().copied());

        for list_stickers in td.affected_stickers_for_slice_mask(twist.slice_mask) {
            for &sticker in list_stickers {
                let st = &p.stickers[sticker];
                let mut poly = st.poly.clone();
                poly.transform(&isometry);
                if clipped && poly.center.abs() > 2.0 {
                    continue;
                }
                list.fill_polygon(&poly, ctx.sticker_color(st), f, clipped);
            }
        }
    }
}

/// The fisheye model only covers a disk; fill the rest with the background.
fn fill_background_except_disk(ctx: &SceneContext, list: &mut DrawList) {
    let num = 200;
    let points: Vec<Vector3D> =
        (0..=num).map(|i| 2.0 * PI * i as f64 / num as f64).map(|a| Vector3D::new(a.cos(), a.sin())).collect();
    // The disk's outside is filled (this polygon "contains" infinity).
    list.fill(Vector3D::new(10.1, 0.0), &points, true, ctx.settings.color_bg, false);
}

/// Two side-by-side disks, one per hemisphere.
fn build_hemisphere_disks(ctx: &SceneContext, view: &View, list: &mut DrawList) {
    let mut half_turn = Mobius::default();
    half_turn.elliptic(Geometry::Spherical, Complex::I, PI);

    for upper in [false, true] {
        let offset = Vector3D::new(if upper { 1.0 } else { -1.0 }, 0.0);
        let ring: Vec<Vector3D> =
            (0..=100).map(|i| 2.0 * PI * i as f64 / 100.0).map(|a| Vector3D::new(a.cos(), a.sin()) + offset).collect();

        let clip = list.convex_fan(offset, &ring, Color32::BLACK);
        list.cmds.push(Cmd::SetClip(clip));
        let edges = list.convex_fan(offset, &ring, ctx.settings.color_tile_edges);
        list.push_solid(edges, true);

        // The upper hemisphere is rotated onto the unit disk.
        let hemisphere_view =
            if upper { &Isometry::new(half_turn, None) * &view.isometry } else { view.isometry.clone() };
        let shifted = |v: Vector3D| v + offset;
        build_direct(ctx, &hemisphere_view, list, &shifted, true);
        twisting_circles(ctx, view, &hemisphere_view, list, &shifted, true);
        list.cmds.push(Cmd::ClearClip);
    }
}

/// Draws Euclidean and hyperbolic puzzles with cell textures.
/// Fills the whole hyperbolic plane as it appears in a model.
pub(crate) fn fill_hyperbolic_plane(list: &mut DrawList, model: Model, color: Color32) {
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
            let r = list.solid_triangles(quad, color);
            list.push_solid(r, false);
        }
        _ => {
            let ring: Vec<Vector3D> =
                (0..=250).map(|i| 2.0 * PI * i as f64 / 250.0).map(|a| Vector3D::new(a.cos(), a.sin())).collect();
            let r = list.convex_fan(Vector3D::ORIGIN, &ring, color);
            list.push_solid(r, false);
        }
    }
}

fn build_textured(ctx: &SceneContext, view: &View, list: &mut DrawList) -> Option<CellId> {
    let p = ctx.puzzle;
    let s = ctx.settings;
    let data = ctx.data;

    // The disk, where the background and edge colors differ.
    if p.config.geometry() == Geometry::Hyperbolic && s.color_bg != s.color_tile_edges {
        fill_hyperbolic_plane(list, ctx.model, s.color_tile_edges);
    }

    let start = list.cell_indices.len() as u32;
    let mut closest: Option<(f64, CellId)> = None;
    for (layer, &master) in p.masters.iter().enumerate() {
        for cell in std::iter::once(master).chain(p.slave_cells(master).iter().copied()) {
            let c = &p.cells[cell];

            // Track what is closest to the center (only for the first master's copies).
            if layer == 0 {
                let d = view.isometry.apply_c(c.center().to_complex()).magnitude();
                if closest.is_none_or(|(best, _)| d < best) {
                    closest = Some((d, cell));
                }
            }

            let state_calc = p.is_state_calc_cell(cell);
            let Some(lod) = lod(&c.vertex_circle, &view.isometry, state_calc, ctx.geometry()) else {
                continue;
            };
            if (s.show_state_calc_cells && !state_calc) || (s.show_only_fundamental && !c.is_master()) {
                continue;
            }
            let verts = &data.cell_vertices[cell];
            let Some((unique, local)) = data.lods.get(lod) else {
                continue;
            };

            let base = list.cell_vertices.len() as u32;
            for &u in unique {
                let [x, y] = verts[u as usize];
                let v = ctx.model.apply(view.isometry.apply(Vector3D::new(x, y)));
                list.cell_vertices.push(TexVertex { pos: pt(v), uv: data.uvs[u as usize], layer: layer as u32 });
            }
            list.cell_indices.extend(local.iter().map(|&l| base + l));
        }
    }
    let end = list.cell_indices.len() as u32;
    if end > start {
        list.cmds.push(Cmd::Cells(start..end));
    }
    closest.map(|(_, c)| c)
}

/// Draws the twisting circles closest to the mouse.
fn twisting_circles(
    ctx: &SceneContext,
    view: &View,
    isometry: &Isometry,
    list: &mut DrawList,
    f: &dyn Fn(Vector3D) -> Vector3D,
    clipped: bool,
) {
    let Some(closest) = ctx.closest_twist else {
        return;
    };
    if !ctx.settings.highlight_twisting_circles {
        return;
    }
    let p = ctx.puzzle;
    let Some(identified) = p.twist_data[closest].identified else {
        return;
    };
    let color = ctx.settings.color_twisting_circles;
    let width = 2.0;
    let mask = if p.config.systolic() {
        magictile_core::slice_mask::dir_seg_to_mask(ctx.closest_geodesic_seg)
    } else {
        ctx.slice_mask.max(1)
    };
    let _ = view;

    for &td_id in &p.all_twist_data[identified].twist_data_for_drawing {
        let td = &p.twist_data[td_id];
        for circle in td.circles_for_slice_mask(mask) {
            let mut c = circle.clone();
            c.transform(isometry);
            if c.radius < 0.005 {
                continue;
            }

            if p.is_spherical() {
                let points = circle_safe_points(&c);
                list.polyline(&points.into_iter().map(f).collect::<Vec<_>>(), width, color, clipped);
            } else if p.config.systolic() {
                let seg = hypercycle_segment(&c);
                let points: Vec<Vector3D> = seg.subdivide(75).into_iter().map(f).collect();
                list.polyline(&points, width, color, clipped);

                if let Some(pants) = &td.pants {
                    if ctx.settings.show_systolic_pants {
                        let mut hex = pants.hexagon.clone();
                        hex.transform(isometry);
                        let colors = [
                            Color32::from_gray(153),
                            Color32::from_rgb(255, 255, 153),
                            Color32::from_gray(153),
                            Color32::from_rgb(102, 255, 102),
                            Color32::from_gray(153),
                            Color32::from_rgb(153, 153, 255),
                        ];
                        for (seg, color) in hex.segments.iter().zip(colors) {
                            let points: Vec<Vector3D> = seg.subdivide(10).into_iter().map(f).collect();
                            list.polyline(&points, width, color, clipped);
                        }
                    }

                    // Earthquakes have the pants chopped off, which we need to show.
                    if p.config.earthquake() && ctx.closest_geodesic_seg != -1 {
                        let chopped = magictile_core::pants::Pants::chopped_pants_seg(ctx.closest_geodesic_seg);
                        let mut hex = pants.hexagon.clone();
                        hex.transform(isometry);
                        if let Some(seg) = hex.segments.get(chopped as usize) {
                            let points: Vec<Vector3D> = seg.subdivide(15).into_iter().map(f).collect();
                            list.polyline(&points, width, color, clipped);
                        }
                    }
                }
            } else {
                let points: Vec<Vector3D> = circle_points(&c.circle, 100).into_iter().map(f).collect();
                list.polyline(&points, width, color, clipped);
            }
        }
    }
}

fn circle_points(c: &Circle, divisions: usize) -> Vec<Vector3D> {
    let mut radius = Vector3D::new(0.0, c.radius);
    (0..=divisions)
        .map(|_| {
            radius.rotate_xy(2.0 * PI / divisions as f64);
            c.center + radius
        })
        .collect()
}

/// Points on a generalized circle, handling large radii (the original's `DrawCircleSafe`).
fn circle_safe_points(c: &CircleNE) -> Vec<Vector3D> {
    if c.is_line() {
        let start = r3::euclidean2d::project_onto_line(Vector3D::ORIGIN, c.p1, c.p2);
        let mut d = c.p2 - c.p1;
        d.normalize();
        d *= 50.0;
        let (begin, end) = (start + d, start - d);
        let divisions = 500;
        let inc = (end - begin) / divisions as f64;
        return (0..divisions).map(|i| begin + inc * i as f64).collect();
    }

    match segment_through_box(c) {
        Some(seg) => seg.subdivide(1000),
        None => circle_points(c, 500),
    }
}

/// The part of a circle crossing a large box, if it crosses it.
fn segment_through_box(c: &CircleNE) -> Option<Segment> {
    let b = 25.0;
    let corners = [Vector3D::new(-b, -b), Vector3D::new(b, -b), Vector3D::new(b, b), Vector3D::new(-b, b)];
    let mut boxed = Polygon::default();
    for i in 0..4 {
        boxed.segments.push(Segment::line(corners[i], corners[(i + 1) % 4]));
    }
    let i_points = boxed.intersection_points(c);
    if i_points.len() != 2 {
        return None;
    }

    // The midpoint closer to the origin.
    let t1 = i_points[0] - c.center;
    let t2 = i_points[1] - c.center;
    let (mut mid1, mut mid2) = (t1, t1);
    mid1.rotate_xy(r3::euclidean2d::angle_to_counter_clock(t1, t2) / 2.0);
    mid2.rotate_xy(-r3::euclidean2d::angle_to_clock(t1, t2) / 2.0);
    mid1 += c.center;
    mid2 += c.center;
    let mid = if mid2.abs() < mid1.abs() { mid2 } else { mid1 };
    Some(Segment::arc(i_points[0], mid, i_points[1]))
}

/// The segment of a hypercycle (or geodesic) inside the disk.
fn hypercycle_segment(c: &CircleNE) -> Segment {
    if c.is_line() {
        // It goes through the origin.
        let p = c.p1.normalized();
        return Segment::line(p, -p);
    }
    let (_, p1, p2) = r3::euclidean2d::intersection_circle_circle(c, &Circle::default());
    let mut direction = -c.center;
    direction.normalize();
    let closest_to_origin = c.center + direction * c.radius;
    Segment::arc(p1, closest_to_origin, p2)
}

/// Builds a master cell's texture: its stickers at the template position, plus any stickers
/// moving through it in the current twist.
pub fn build_cell_texture(ctx: &SceneContext, master_index: usize) -> DrawList {
    let p = ctx.puzzle;
    let half = ctx.data.texture_half_width;
    let pixel = 2.0 * half / 512.0;
    let mut list = DrawList::new(ctx.settings.color_tile_edges, Camera::square(half), pixel);
    let master = p.masters[master_index];
    let template = &p.cells[p.masters[0]];

    // Unmoving stickers.
    for (i, &template_sticker) in template.stickers.iter().enumerate() {
        let Some(&own) = p.cells[master].stickers.get(i) else {
            continue;
        };
        if p.stickers[own].twisting {
            continue;
        }
        let poly = &p.stickers[template_sticker].poly;
        list.fill_polygon(poly, ctx.sticker_color(&p.stickers[own]), |v| v, false);
    }

    // Moving stickers.
    let Some(twist) = ctx.controller.current_twist() else {
        return list;
    };
    let rotation = ctx.rotation();
    let systolic = p.config.systolic();
    let num_primary = p.all_twist_data[twist.identified].twist_data_for_state_calcs.len();
    for (count, td_id) in p.state_calc_twist_data(twist).into_iter().enumerate() {
        let td = &p.twist_data[td_id];
        if !td.affected_master_cells.as_ref().is_some_and(|m| m.contains(&master)) {
            continue;
        }
        let mobius = td.mobius_for_twist(&p.config, twist, rotation, count + 1 > num_primary);
        let master_iso = &p.cells[master].isometry;
        let isometry = Isometry::new(master_iso.mobius * mobius, master_iso.reflection().copied());

        for stickers in td.affected_stickers_for_slice_mask(twist.slice_mask) {
            for &sticker in stickers {
                let st = &p.stickers[sticker];
                // A performance boost for systolic puzzles.
                if systolic && isometry.apply(st.poly.center).abs() > 0.5 {
                    continue;
                }
                let mut poly = st.poly.clone();
                poly.transform(&isometry);
                list.fill_polygon(&poly, ctx.raw_color(st), |v| v, false);
            }
        }
    }
    list
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
