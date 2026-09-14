//! Truncated hyperbolic tilings t{p,q}, navigable with the mouse like the puzzles.
//!
//! Truncating the regular {p,q} tiling cuts each vertex off: every p-gon becomes a 2p-gon and
//! every vertex a q-gon. For t{4,5} (from the order-5 square tiling) that's red pentagons among
//! yellow octagons, with blue lines where two octagons meet.
//!
//! Usage: tiling [p q]   (default 4 5)

use crate::cayley::{Cayley, MAX_Q, Perm};
use crate::draw::{self, Camera, DrawList};
use crate::render::{FrameJob, PuzzleCallback, Renderer};
use crate::selftest::SelfTest;
use crate::view::{Model, View};
use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, Stroke};
use eframe::egui_wgpu;
use r3::models::{HyperbolicModel, SphericalModel};
use r3::{Complex, Geometry, Isometry, Mobius, Tiling, TilingConfig, Transform, Vector3D};
use std::f64::consts::PI;
use std::sync::Arc;
use web_time::Instant;

const BACKGROUND: Color32 = Color32::WHITE;
const BIG_COLOR: Color32 = Color32::from_rgb(255, 255, 0);
const SMALL_COLOR: Color32 = Color32::from_rgb(255, 0, 0);
const LINE_COLOR: Color32 = Color32::from_rgb(0, 0, 255);
/// Width of the lines between 2p-gons at the center of the Poincaré disk, in disk units.
const LINE_WIDTH: f64 = 0.0125;
/// Faces smaller than this many pixels across aren't drawn.
const MIN_FACE_PX: f64 = 0.6;
/// Lines thinner than this many pixels aren't drawn.
const MIN_LINE_PX: f64 = 0.3;
/// Chords approximating curved edges stay within about this many pixels of the true edge.
const MAX_SAG_PX: f64 = 0.15;
/// Vertices with at least this much room between neighbors get their permutation written on.
const LABEL_MIN_PX: f32 = 46.0;
/// The mouse picks up a vertex within this many pixels, and never from further than half way to
/// the vertex's neighbors (so crowded ones out near the rim don't grab the pointer).
const HOVER_PX: f32 = 45.0;
const LABEL_COLOR: Color32 = Color32::from_gray(15);
const UNIT_COLOR: Color32 = Color32::from_gray(80);
const HIGHLIGHT_COLOR: Color32 = Color32::from_rgb(130, 0, 200);
/// The straight hyperbolic line between two picked vertices.
const GEODESIC_COLOR: Color32 = Color32::from_rgb(0, 140, 70);
/// The route through the graph, hop by hop.
const PATH_COLOR: Color32 = Color32::from_rgb(255, 110, 0);

/// A face of the truncated tiling, in the Poincaré disk.
struct Face {
    center: Vector3D,
    vertices: Vec<Vector3D>,
    /// The hyperbolic midpoint of the edge from each vertex to the next.
    midpoints: Vec<Vector3D>,
    color: Color32,
    /// Hyperbolic distance from the center to the vertices.
    radius: f64,
}

/// A line between two 2p-gons, in the Poincaré disk.
struct Line {
    a: Vector3D,
    b: Vector3D,
    mid: Vector3D,
}

/// The truncated tiling t{p,q}, generated around the origin.
pub struct TruncatedTiling {
    pub p: i32,
    pub q: i32,
    faces: Vec<Face>,
    /// Edges between two 2p-gons (along the original tiling's edges).
    lines: Vec<Line>,
    /// For each tile of the original tiling: its center, and the symmetry taking it home.
    homes: Vec<(Vector3D, Isometry)>,
    /// The starting view: centered on a q-gon, with a corner pointing right.
    pub start: Isometry,
    /// Edges shorter than this many pixels can't bend by more than [`MAX_SAG_PX`], so are drawn
    /// as single chords.
    short_edge_px: f64,
    /// The tiling read as a Cayley graph, when t{p,q} can be one.
    pub cayley: Option<Cayley>,
}

/// How the tiling maps to the screen this frame.
struct Screen {
    model: Model,
    /// World units per pixel.
    pixel: f64,
    short_edge_px: f64,
}

impl TruncatedTiling {
    /// Builds t{p,q} for a hyperbolic {p,q}.
    pub fn new(p: i32, q: i32) -> Result<TruncatedTiling, String> {
        if p < 3 || q < 3 || (p - 2) * (q - 2) <= 4 {
            return Err(format!("{{{p},{q}}} isn't a hyperbolic tiling (need (p-2)(q-2) > 4)"));
        }
        let tiling = Tiling::generate(TilingConfig::new(p, q, max_tiles(p, q)));

        // Cut each edge a distance d from both ends, so all edges of the result are equal: the
        // middle of an original edge (L - 2d) matches the q-gon's side, which spans the angle
        // 2π/q between two edges at a vertex (hyperbolic law of cosines).
        let base = tiling.tiles[0].boundary.vertices();
        let edge = distance(base[0], base[1]);
        let q_side = |d: f64| (d.cosh().powi(2) - d.sinh().powi(2) * (2.0 * PI / q as f64).cos()).acosh();
        let (mut lo, mut hi) = (0.0, edge / 2.0);
        for _ in 0..100 {
            let d = (lo + hi) / 2.0;
            if q_side(d) < edge - 2.0 * d {
                lo = d;
            } else {
                hi = d;
            }
        }
        let d = (lo + hi) / 2.0;

        let mut faces = Vec::new();
        // The 2p-gons.
        for tile in &tiling.tiles {
            let vs = tile.boundary.vertices();
            let n = vs.len();
            let vertices =
                (0..n).flat_map(|i| [towards(vs[i], vs[(i + 1) % n], d), towards(vs[(i + 1) % n], vs[i], d)]).collect();
            faces.push(Face::new(tile.center(), vertices, BIG_COLOR));
        }
        // The q-gons, one per vertex with all its tiles present.
        for (&v, tiles) in tiling.vertex_incidences.iter() {
            if tiles.len() != q as usize {
                continue;
            }
            let mut corners: Vec<Vector3D> = Vec::new();
            for &t in tiles {
                let vs = tiling.tiles[t].boundary.vertices();
                let n = vs.len();
                let Some(i) = (0..n).min_by(|&a, &b| vs[a].dist(v).total_cmp(&vs[b].dist(v))) else {
                    continue;
                };
                for neighbor in [vs[(i + 1) % n], vs[(i + n - 1) % n]] {
                    let c = towards(v, neighbor, d);
                    if !corners.iter().any(|x| x.dist(c) < 1e-9) {
                        corners.push(c);
                    }
                }
            }
            if corners.len() != q as usize {
                continue;
            }
            // Order them around the vertex.
            let angle = |c: &Vector3D| to_origin(v.to_complex(), c.to_complex()).phase();
            corners.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
            faces.push(Face::new(v, corners, SMALL_COLOR));
        }

        // The lines between 2p-gons: the middle parts of the original edges.
        let mut lines = Vec::new();
        for (&mid, tiles) in tiling.edge_incidences.iter() {
            let Some(seg) = tiles
                .first()
                .and_then(|&t| tiling.tiles[t].boundary.segments.iter().find(|s| s.midpoint().dist(mid) < 1e-9))
            else {
                continue;
            };
            let (a, b) = (towards(seg.p1, seg.p2, d), towards(seg.p2, seg.p1, d));
            lines.push(Line { a, b, mid: midpoint(a, b) });
        }

        let homes = tiling.tiles.iter().map(|t| (t.center(), t.isometry.clone())).collect();

        // z -> e^(iφ) (z - v) / (1 - conj(v) z) moves a vertex v of the home tile to the center,
        // turned so the corner of its q-gon towards the next vertex points right.
        let v = base[0].to_complex();
        let phi = -to_origin(v, towards(base[0], base[1], d).to_complex()).phase();
        let turn = Complex::from_polar(1.0, phi);
        let start = Isometry::new(Mobius::new(turn, turn * v * -1.0, v.conj() * -1.0, Complex::ONE), None);
        // A geodesic segment bends most relative to its length at the top of a semicircle in the
        // upper half-plane (which the disk looks like near its edge). There, a segment of length
        // e spans ±θ with sin θ = tanh(e/2), and sags by tan(θ/2)/2 of its chord.
        let theta = ((edge - 2.0 * d) / 2.0).tanh().asin();
        let sag_ratio = (theta / 2.0).tan() / 2.0;
        let short_edge_px = MAX_SAG_PX / (1.25 * sag_ratio);

        // The q-gons' corners (counterclockwise) and the edges between 2p-gons are the two kinds
        // of step in the Cayley graph.
        let q_gons: Vec<(Vector3D, Vec<Vector3D>)> =
            faces.iter().filter(|f| f.color == SMALL_COLOR).map(|f| (f.center, f.vertices.clone())).collect();
        let swaps: Vec<(Vector3D, Vector3D)> = lines.iter().map(|l| (l.a, l.b)).collect();
        // How far apart two vertices of one face can be. Every point lies in some face, and so
        // within this of each of that face's vertices, which is what lets the Cayley graph confirm
        // that no nearer copy lies beyond the generated patch. Faces of a kind are congruent, so
        // measure those nearest the origin, where rounding is least.
        let diameter = |f: &Face| {
            f.vertices.iter().flat_map(|&a| f.vertices.iter().map(move |&b| distance(a, b))).fold(0.0, f64::max)
        };
        let central = |color: Color32| {
            faces.iter().filter(|f| f.color == color).min_by(|a, b| a.center.abs().total_cmp(&b.center.abs()))
        };
        let face_diameter =
            [central(BIG_COLOR), central(SMALL_COLOR)].into_iter().flatten().map(diameter).fold(0.0, f64::max);
        let cayley = Cayley::build(p, q, &q_gons, &swaps, &start, face_diameter);

        Ok(TruncatedTiling { p, q, faces, lines, homes, start, short_edge_px, cayley })
    }

    /// Builds the frame's draw list. Also returns a symmetry recentering the view (see
    /// [`View::recenter`]), if the home tile has drifted away from the center.
    ///
    /// Everything goes into two batched draws (faces, then lines), so the GPU sees a handful of
    /// commands however many faces are visible.
    fn draw(&self, view: &View, model: Model, pixels_per_point: f32) -> (DrawList, Option<Isometry>) {
        let camera = Camera::view(view.width, view.height, view.view_scale, view.rotation);
        let pixel = 2.0 * view.view_scale / (view.height as f64 * pixels_per_point as f64);
        let mut list = DrawList::new(BACKGROUND, camera, pixel);
        // The far reaches of the tiling (beyond what we generate) are mostly 2p-gon.
        draw::fill_hyperbolic_plane(&mut list, model, BIG_COLOR);

        let screen = Screen { model, pixel, short_edge_px: self.short_edge_px };
        let start = list.solid.len() as u32;
        let mut scratch = Scratch::default();
        for face in &self.faces {
            if let Some(center) = face.ring(&view.isometry, &screen, &mut scratch) {
                list.convex_fan(center, &scratch.ring, face.color);
            }
        }
        list.push_solid(start..list.solid.len() as u32, false);

        let start = list.solid.len() as u32;
        for line in &self.lines {
            // Lines shrink with everything else towards the edge of the disk.
            let mid = view.isometry.apply(line.mid);
            let width = LINE_WIDTH * (1.0 - mid.abs().powi(2)) / pixel;
            if width < MIN_LINE_PX {
                continue;
            }
            let (a, b) = (view.isometry.apply(line.a), view.isometry.apply(line.b));
            scratch.ring.clear();
            edge_points(a, b, || mid, &screen, &mut scratch.ring);
            scratch.ring.push(model.apply(b));
            list.polyline_triangles(&scratch.ring, width, LINE_COLOR);
        }
        list.push_solid(start..list.solid.len() as u32, false);

        // Without a labelling, any symmetry of the tiling will do to recenter; with one, the
        // viewer recenters itself with a symmetry that carries the labels (see `TilingApp`).
        let recenter = self
            .cayley
            .is_none()
            .then(|| {
                let closest = (0..self.homes.len()).min_by(|&a, &b| {
                    view.isometry.apply(self.homes[a].0).abs().total_cmp(&view.isometry.apply(self.homes[b].0).abs())
                });
                closest.filter(|&i| i != 0).map(|i| self.homes[i].1.inverse())
            })
            .flatten();
        (list, recenter)
    }
}

/// Buffers reused from face to face.
#[derive(Default)]
struct Scratch {
    disk: Vec<Vector3D>,
    projected: Vec<Vector3D>,
    ring: Vec<Vector3D>,
}

impl Face {
    fn new(center: Vector3D, vertices: Vec<Vector3D>, color: Color32) -> Face {
        let radius = vertices.iter().map(|&v| distance(center, v)).fold(0.0, f64::max);
        let n = vertices.len();
        let midpoints = (0..n).map(|i| midpoint(vertices[i], vertices[(i + 1) % n])).collect();
        Face { center, vertices, midpoints, color, radius }
    }

    /// The face as it appears on screen: returns its center and leaves its closed boundary
    /// (curved edges as chords) in `scratch.ring`, or returns `None` if it's too small to see.
    ///
    /// Faces are convex hyperbolic polygons, so a fan of flat triangles from the center covers
    /// them exactly: neighboring triangles share their straight inner edges, and the boundary is
    /// sampled finely enough that the chords are within a fraction of a pixel of the true edges.
    fn ring(&self, view: &Isometry, screen: &Screen, scratch: &mut Scratch) -> Option<Vector3D> {
        let (model, pixel) = (screen.model, screen.pixel);
        let center = view.apply(self.center);

        // Cheap cull first: the disk model's size of a hyperbolic disk of our radius around the
        // center, generously scaled for models that can magnify (up to 2x for Klein).
        let magnification = match model {
            Model::Plain => Some(1.0),
            Model::Hyperbolic(HyperbolicModel::Klein) => Some(2.0),
            _ => None,
        };
        if let Some(m) = magnification {
            let (r2, t) = (center.abs().powi(2), (self.radius / 2.0).tanh());
            let diameter = 2.0 * m * t * (1.0 - r2) / (1.0 - t * t * r2);
            if diameter / pixel < MIN_FACE_PX {
                return None;
            }
        }

        let Scratch { disk, projected, ring } = scratch;
        disk.clear();
        disk.extend(self.vertices.iter().map(|&v| view.apply(v)));
        projected.clear();
        projected.extend(disk.iter().map(|&v| model.apply(v)));
        let size = projected.iter().map(|v| v.dist(projected[0])).fold(0.0, f64::max) / pixel;
        if size < MIN_FACE_PX || projected.iter().any(|v| v.is_dne()) {
            return None;
        }
        ring.clear();
        for i in 0..disk.len() {
            let j = (i + 1) % disk.len();
            edge_points(disk[i], disk[j], || view.apply(self.midpoints[i]), screen, ring);
        }
        ring.push(projected[0]);
        Some(model.apply(center))
    }
}

/// Appends the on-screen points of the edge from `a` to `b` (in the disk, with hyperbolic
/// midpoint `mid`), leaving off `b`. Uses as few chords as keep within [`MAX_SAG_PX`] of the curve,
/// and samples the same points whichever way round the edge is given, so neighboring faces meet
/// exactly.
fn edge_points(a: Vector3D, b: Vector3D, mid: impl FnOnce() -> Vector3D, screen: &Screen, out: &mut Vec<Vector3D>) {
    let (model, pixel) = (screen.model, screen.pixel);
    let (fa, fb) = (model.apply(a), model.apply(b));
    out.push(fa);
    // Edges this short can't bend visibly, so skip finding the midpoint.
    if fa.dist(fb) / pixel < screen.short_edge_px {
        return;
    }
    // A chord's distance from the arc drops with the square of the number of chords.
    let sag = model.apply(mid()).dist((fa + fb) * 0.5) / pixel;
    let n = ((sag / MAX_SAG_PX).sqrt().ceil() as usize).clamp(1, 32);
    if n == 1 {
        return;
    }
    let forwards = (a.x, a.y) < (b.x, b.y);
    let (from, to) = if forwards { (a, b) } else { (b, a) };
    let inner = geodesic(from, to, n);
    let inner = inner[1..n].iter().map(|&v| model.apply(v));
    if forwards { out.extend(inner) } else { out.extend(inner.rev()) }
}

fn midpoint(a: Vector3D, b: Vector3D) -> Vector3D {
    // Computed from a canonical end, so both faces sharing an edge agree exactly.
    let (from, to) = if (a.x, a.y) < (b.x, b.y) { (a, b) } else { (b, a) };
    towards(from, to, distance(from, to) / 2.0)
}

/// Enough tiles to fill the disk down to pixel size, wherever the view is centered in the home
/// tile: roughly those within a hyperbolic distance of 7.5 of the origin.
fn max_tiles(p: i32, q: i32) -> usize {
    let (p, q) = (p as f64, q as f64);
    let tile_area = (p - 2.0) * PI - p * 2.0 * PI / q;
    let disk_area = 4.0 * PI * (7.5f64 / 2.0).sinh().powi(2);
    ((disk_area / tile_area) as usize).clamp(500, 40000)
}

/// The Möbius translation taking `a` to the origin.
fn to_origin(a: Complex, z: Complex) -> Complex {
    (z - a) / (Complex::ONE - a.conj() * z)
}

fn from_origin(a: Complex, z: Complex) -> Complex {
    (z + a) / (Complex::ONE + a.conj() * z)
}

/// Hyperbolic distance between two points of the Poincaré disk.
fn distance(a: Vector3D, b: Vector3D) -> f64 {
    2.0 * to_origin(a.to_complex(), b.to_complex()).magnitude().atanh()
}

/// The point a hyperbolic distance `d` from `a` towards `b`.
fn towards(a: Vector3D, b: Vector3D, d: f64) -> Vector3D {
    let (a, b) = (a.to_complex(), b.to_complex());
    let w = to_origin(a, b);
    Vector3D::from_complex(from_origin(a, w * ((d / 2.0).tanh() / w.magnitude())))
}

/// `n + 1` evenly spaced points along the geodesic from `a` to `b`.
fn geodesic(a: Vector3D, b: Vector3D, n: usize) -> Vec<Vector3D> {
    let (ac, w) = (a.to_complex(), to_origin(a.to_complex(), b.to_complex()));
    let (r, half) = (w.magnitude(), w.magnitude().atanh());
    if r < 1e-12 {
        return vec![a, b];
    }
    (0..=n).map(|k| Vector3D::from_complex(from_origin(ac, w * ((half * k as f64 / n as f64).tanh() / r)))).collect()
}

/// A measurement between two picked vertices.
struct Measurement {
    from: usize,
    to: usize,
    /// Where the copy of `to` nearest `from` is: at `to`, unless the short way wraps around. It
    /// may lie beyond the generated patch.
    nearest: Vector3D,
    wraps: bool,
    /// Hyperbolic distance to that nearest copy, and to the vertex actually picked.
    shortest: f64,
    direct: f64,
    /// Whether `shortest` is certainly the shortest. If not, a nearer copy may lie beyond the
    /// generated patch, and it is only an upper bound.
    confirmed: bool,
    /// A shortest route through the graph as far as it could be traced, and its length in steps
    /// (exact either way).
    path: Vec<Vector3D>,
    hops: usize,
}

pub struct TilingApp {
    tiling: TruncatedTiling,
    view: View,
    /// The vertices picked for measuring, and what they measure.
    picked: Vec<usize>,
    measurement: Option<Measurement>,
    /// The vertex under the mouse, if any.
    hovered: Option<usize>,
    /// Recentering renames the labels; this is the accumulated renaming (see
    /// [`Cayley::frame_isometry`]).
    frame: Perm,
    hyperbolic_model: HyperbolicModel,
    gliding: f64,
    last_frame: Instant,
    selftest: Option<SelfTest>,
}

impl TilingApp {
    pub fn new(cc: &eframe::CreationContext, tiling: TruncatedTiling) -> TilingApp {
        if let Some(render_state) = &cc.wgpu_render_state {
            Renderer::install(render_state);
        }
        let frame = tiling.cayley.as_ref().map_or([0; MAX_Q], |c| c.identity_perm());
        let mut view = View::default();
        view.reset(Geometry::Hyperbolic);
        view.isometry = tiling.start.clone();
        TilingApp {
            tiling,
            view,
            picked: Vec::new(),
            measurement: None,
            hovered: None,
            frame,
            hyperbolic_model: HyperbolicModel::Poincare,
            gliding: 0.5,
            last_frame: Instant::now(),
            selftest: SelfTest::from_env(),
        }
    }

    fn model(&self) -> Model {
        Model::for_puzzle(Geometry::Hyperbolic, self.hyperbolic_model, SphericalModel::Sterographic)
    }

    /// Picks a vertex to measure from, then one to measure to; a third pick starts again.
    fn pick(&mut self, vertex: Option<usize>) {
        let (Some(vertex), true) = (vertex, self.tiling.cayley.is_some()) else { return };
        if self.picked.len() >= 2 {
            self.picked.clear();
        }
        self.picked.push(vertex);
        self.measurement = self.measure(None);
    }

    /// The distance between the two picked vertices, both across the surface (allowing the short
    /// way to wrap around to a copy) and directly between the two as drawn.
    /// Where copies tie for nearest, the one at `marked` is kept if it is one of them.
    fn measure(&self, marked: Option<Vector3D>) -> Option<Measurement> {
        let cayley = self.tiling.cayley.as_ref()?;
        let (&from, &to) = (self.picked.first()?, self.picked.get(1)?);
        let distance = cayley.measure(from, to, marked)?;
        Some(Measurement {
            from,
            to,
            nearest: distance.nearest,
            wraps: distance.nearest.dist(cayley.vertices[to].pos) > 1e-6,
            shortest: distance.length,
            direct: cayley.distance(from, to),
            confirmed: distance.confirmed,
            path: distance.path,
            hops: distance.hops,
        })
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (f7, reset) = ctx.input(|i| (i.key_pressed(Key::F7), i.key_pressed(Key::R)));
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.picked.clear();
            self.measurement = None;
        }
        if f7 {
            self.hyperbolic_model = match self.hyperbolic_model {
                HyperbolicModel::Poincare => HyperbolicModel::Klein,
                HyperbolicModel::Klein => HyperbolicModel::UpperHalfPlane,
                HyperbolicModel::UpperHalfPlane => HyperbolicModel::Orthographic,
                HyperbolicModel::Orthographic => HyperbolicModel::Poincare,
            };
        }
        if reset {
            self.view.reset(Geometry::Hyperbolic);
            self.view.isometry = self.tiling.start.clone();
            self.picked.clear();
            self.measurement = None;
            if let Some(cayley) = &self.tiling.cayley {
                self.frame = cayley.identity_perm();
            }
        }
    }

    fn tiling_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        self.view.set_size(rect.width(), rect.height());
        let model = self.model();
        self.view.navigate(ctx, &response, rect, model, self.gliding);

        let ppp = ctx.pixels_per_point();
        let (list, recenter) = self.tiling.draw(&self.view, model, ppp);
        self.view.recenter = recenter;
        let size_px = [(rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32];
        let job = FrameJob {
            puzzle_generation: 0,
            num_layers: 1,
            cell_jobs: Vec::new(),
            mipmaps: false,
            view: list,
            size_px,
        };
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, PuzzleCallback { job: Arc::new(job) }));

        let painter = ui.painter().with_clip_rect(rect);
        let found = self.overlay(&painter, rect, model, response.hover_pos());
        self.hovered = found.hovered;
        if response.clicked() {
            self.pick(found.hovered);
            ctx.request_repaint();
        }
        self.recenter_onto(found.center);

        let help = format!(
            "t{{{},{}}}   drag: pan   right drag / scroll: zoom   middle drag: rotate   F7: model ({:?})   R: reset\
             {}",
            self.tiling.p,
            self.tiling.q,
            self.hyperbolic_model,
            if self.tiling.cayley.is_some() {
                "   hover a vertex to find its copies   click two to measure (Esc clears)"
            } else {
                ""
            }
        );
        ui.painter().text(
            rect.left_top() + egui::vec2(8.0, 6.0),
            egui::Align2::LEFT_TOP,
            help,
            egui::FontId::proportional(13.0),
            Color32::from_gray(90),
        );
    }
}

/// What the overlay picked out this frame.
#[derive(Default, Clone, Copy)]
struct Found {
    hovered: Option<usize>,
    /// The labelled vertex nearest the middle of the view, which recentering moves onto.
    center: Option<usize>,
}

impl TilingApp {
    /// Keeps the home vertex near the middle of the view: panning far otherwise loses precision
    /// and runs off the end of the generated patch. The symmetry used carries the labelling with
    /// it, so the permutations stay where they are on screen.
    fn recenter_onto(&mut self, center: Option<usize>) {
        let Some(cayley) = &self.tiling.cayley else { return };
        let Some(vertex) = center.filter(|&v| v != cayley.home_vertex) else { return };
        let (Some(symmetry), Some(label)) = (cayley.frame_isometry(vertex), cayley.label(vertex)) else { return };
        let frame = cayley.compose(&self.frame, &label);

        // Vertex indices are in tiling coordinates, which this symmetry moves, so anything held
        // that way has to come along: otherwise a measurement would jump to the vertices the
        // picked ones land on. Dropped if a picked vertex lands outside the patch.
        let inverse = symmetry.inverse();
        let carry = |v: usize| cayley.transport(&inverse, v);
        let carried: Vec<usize> = self.picked.iter().filter_map(|&v| carry(v)).collect();
        let keep = carried.len() == self.picked.len();

        // The measurement is worked out again. Copies often tie exactly for nearest, with nothing
        // between them but vertex index, which this symmetry permutes; keeping the copy already
        // marked stops the marker hopping between equally good answers.
        let marked = self.measurement.as_ref().map(|m| inverse.apply(m.nearest));

        self.view.isometry = &self.view.isometry * &symmetry;
        self.frame = frame;
        self.picked = if keep { carried } else { Vec::new() };
        self.measurement = self.measure(marked);
    }

    /// Draws the Cayley graph overlay over the tiling: the repeating unit as a dotted outline,
    /// the permutation at each vertex with room for it, and the hovered vertex together with its
    /// copies in the other units. Returns the hovered vertex.
    fn overlay(&self, painter: &egui::Painter, rect: Rect, model: Model, pointer: Option<Pos2>) -> Found {
        let Some(cayley) = self.tiling.cayley.as_ref() else { return Found::default() };
        let view = &self.view;
        let project = |p: Vector3D| -> Option<Pos2> {
            let disk = view.isometry.apply(p);
            if disk.abs() > 0.9995 {
                return None;
            }
            let projected = model.apply(disk);
            if projected.is_dne() {
                return None;
            }
            let (x, y) = view.model_to_screen(projected);
            let pos = rect.min + egui::vec2(x, y);
            (pos.x.is_finite() && pos.y.is_finite()).then_some(pos)
        };

        // Where every vertex sits on screen (just off screen too, so labels don't pop in).
        let margin = rect.expand(40.0);
        let screen: Vec<Option<Pos2>> =
            cayley.vertices.iter().map(|v| project(v.pos).filter(|p| margin.contains(*p))).collect();
        // How much room a vertex has, from the distance to a neighbor.
        let room = |i: usize| -> f32 {
            let neighbor = cayley.vertices[i].neighbor();
            match (screen[i], neighbor.and_then(|n| screen[n])) {
                (Some(a), Some(b)) => a.distance(b),
                _ => 0.0,
            }
        };

        // The vertices showing the identity permutation mark the copies of the repeating unit;
        // outline the one nearest the middle of the view.
        let middle = rect.center();
        let showing_identity = cayley.inverse_of(&self.frame);
        let nearest_unit = cayley
            .vertices_with_label(&showing_identity)
            .iter()
            .copied()
            .min_by(|&a, &b| {
                let from_middle = |v: usize| view.isometry.apply(cayley.vertices[v].pos).abs();
                from_middle(a).total_cmp(&from_middle(b))
            })
            .and_then(|v| cayley.frame_isometry(v));
        let mut unit: Vec<Vec<Pos2>> = Vec::new();
        if let Some(g) = nearest_unit {
            let corners: Vec<Vector3D> = cayley.unit.iter().map(|&c| g.apply(c)).collect();
            for i in 0..corners.len() {
                let side = geodesic(corners[i], corners[(i + 1) % corners.len()], 16);
                let points: Vec<Pos2> = side.into_iter().filter_map(project).collect();
                if points.len() > 1 {
                    unit.push(points);
                }
            }
        }
        for side in &unit {
            painter.extend(egui::Shape::dashed_line(side, Stroke::new(1.5, UNIT_COLOR), 5.0, 4.0));
        }

        // The measurement: the straight hyperbolic line to the nearest copy, and the route
        // through the graph that the hop count counts.
        let mut measured: Vec<(Pos2, Color32, bool)> = Vec::new();
        let (mut hops, mut straight): (Vec<Pos2>, Vec<Pos2>) = (Vec::new(), Vec::new());
        if let Some(m) = &self.measurement {
            let at = |v: usize| cayley.vertices[v].pos;
            for step in m.path.windows(2) {
                hops.extend(geodesic(step[0], step[1], 6).into_iter().filter_map(project));
            }
            if hops.len() > 1 {
                painter.add(egui::Shape::line(hops.clone(), Stroke::new(3.5, PATH_COLOR)));
            }
            straight.extend(geodesic(at(m.from), m.nearest, 64).into_iter().filter_map(project));
            if straight.len() > 1 {
                painter.add(egui::Shape::line(straight.clone(), Stroke::new(2.5, GEODESIC_COLOR)));
            }
            // Mark where the measurement runs from and to, and the copy it reaches if it wrapped.
            measured.push((project(at(m.from)).unwrap_or(middle), GEODESIC_COLOR, true));
            measured.push((project(at(m.to)).unwrap_or(middle), GEODESIC_COLOR, !m.wraps));
            if m.wraps {
                measured.push((project(m.nearest).unwrap_or(middle), PATH_COLOR, true));
            }
        } else {
            for &v in &self.picked {
                measured.push((project(cayley.vertices[v].pos).unwrap_or(middle), GEODESIC_COLOR, true));
            }
        }
        // The vertex under the mouse, and the one nearest the middle for recentering.
        let mut center = None;
        let mut nearest_middle = f32::MAX;
        for (i, pos) in screen.iter().enumerate() {
            if let Some(pos) = pos
                && cayley.vertices[i].label.is_some()
                && pos.distance(middle) < nearest_middle
            {
                nearest_middle = pos.distance(middle);
                center = Some(i);
            }
        }

        let mut hovered = None;
        if let Some(pointer) = pointer {
            let mut closest = HOVER_PX;
            for (i, pos) in screen.iter().enumerate() {
                if let Some(pos) = pos
                    && cayley.vertices[i].label.is_some()
                    && pos.distance(pointer) < closest.min(room(i) * 0.5)
                {
                    closest = pos.distance(pointer);
                    hovered = Some(i);
                }
            }
        }
        let mut highlights: Vec<(Pos2, f32, bool)> = Vec::new();
        if let Some(h) = hovered {
            for &copy in cayley.copies_of(h) {
                let Some(pos) = screen[copy] else { continue };
                highlights.push((pos, (room(copy) * 0.16).clamp(4.0, 13.0), copy == h));
            }
        }
        for &(pos, radius, is_hovered) in &highlights {
            painter.circle(pos, radius, HIGHLIGHT_COLOR, Stroke::new(1.5, Color32::WHITE));
            if is_hovered {
                painter.circle_stroke(pos, radius + 4.0, Stroke::new(2.0, HIGHLIGHT_COLOR));
            }
        }

        // The permutations themselves, wherever there is room.
        let mut labels: Vec<(usize, Pos2, f32, String)> = Vec::new();
        for (i, pos) in screen.iter().enumerate() {
            let (Some(pos), Some(word)) = (pos, cayley.label_word(i, &self.frame)) else { continue };
            let room = room(i);
            if room < LABEL_MIN_PX {
                continue;
            }
            labels.push((i, *pos, (room * 0.24).clamp(11.0, 26.0), word));
        }
        for (i, pos, size, word) in &labels {
            // The vertices sit where a q-gon, two 2p-gons and an edge meet, so back the text to
            // keep it readable whatever is behind it.
            let galley = painter.layout_no_wrap(word.clone(), egui::FontId::monospace(*size), LABEL_COLOR);
            let backing = egui::Rect::from_center_size(*pos, galley.size()).expand(*size * 0.12);
            let hovered_here = Some(*i) == hovered;
            let (fill, text) = if hovered_here {
                (HIGHLIGHT_COLOR, Color32::WHITE)
            } else {
                (Color32::from_white_alpha(205), LABEL_COLOR)
            };
            painter.rect_filled(backing, *size * 0.2, fill);
            painter.galley(*pos - galley.size() / 2.0, galley, text);
        }

        // Markers last: a label's backing would otherwise hide which vertices were picked, and
        // when the short way wraps, the hollow marker is the only sign of where it was headed.
        for &(pos, color, filled) in &measured {
            if filled {
                painter.circle(pos, 8.0, color, Stroke::new(2.0, Color32::WHITE));
            } else {
                painter.circle_stroke(pos, 9.0, Stroke::new(3.0, color));
                painter.circle_stroke(pos, 12.0, Stroke::new(1.5, Color32::WHITE));
            }
        }

        // Readouts along the bottom: the measurement, then the hovered vertex.
        let mut readout: Vec<String> = Vec::new();
        if let Some(m) = &self.measurement
            && let (Some(from), Some(to)) =
                (cayley.label_word(m.from, &self.frame), cayley.label_word(m.to, &self.frame))
        {
            let edges = m.shortest / cayley.edge_length();
            let at_most = if m.confirmed { "" } else { "at most " };
            readout.push(format!(
                "{from} to {to}:  {at_most}{:.3} ({edges:.1} edges), {} hops{}{}",
                m.shortest,
                m.hops,
                if m.wraps { format!("   wraps around; {:.3} the way you picked", m.direct) } else { String::new() },
                if m.confirmed { "" } else { "   (too near the edge of the tiling to rule out a shorter way)" }
            ));
        }
        if let Some(h) = hovered
            && let (Some(word), Some(cycles)) = (cayley.label_word(h, &self.frame), cayley.label_cycles(h, &self.frame))
        {
            readout.push(format!("{word}   {cycles}"));
        }
        let mut bottom = rect.left_bottom() + egui::vec2(10.0, -10.0);
        for line in &readout {
            let galley = painter.layout_no_wrap(line.clone(), egui::FontId::monospace(20.0), LABEL_COLOR);
            let size = galley.size();
            let box_rect = egui::Rect::from_min_size(bottom - egui::vec2(4.0, size.y), size).expand(6.0);
            painter.rect_filled(box_rect, 4.0, Color32::from_white_alpha(220));
            painter.galley(bottom - egui::vec2(0.0, size.y), galley, LABEL_COLOR);
            bottom.y -= size.y + 10.0;
        }

        // Self-test screenshots only capture the tiling itself, so write down what was drawn over
        // it (in image pixels) for checking afterwards.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = crate::selftest::peek_shot_request() {
            let ppp = painter.ctx().pixels_per_point();
            let at = |p: Pos2| ((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);
            let points = |ps: &Vec<Pos2>| {
                ps.iter()
                    .map(|&p| {
                        let (x, y) = at(p);
                        format!("[{x:.1},{y:.1}]")
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            };
            let json = format!(
                "{{\"pointer\":{},\"labels\":[{}],\"unit\":[{}],\"highlights\":[{}],\"readout\":{},\"measure\":{}}}",
                pointer.map_or("null".into(), |p| {
                    let (x, y) = at(p);
                    format!("[{x:.1},{y:.1}]")
                }),
                labels
                    .iter()
                    .map(|(_, p, size, word)| {
                        let (x, y) = at(*p);
                        format!("{{\"x\":{x:.1},\"y\":{y:.1},\"size\":{:.1},\"word\":\"{word}\"}}", size * ppp)
                    })
                    .collect::<Vec<_>>()
                    .join(","),
                unit.iter().map(|side| format!("[{}]", points(side))).collect::<Vec<_>>().join(","),
                highlights
                    .iter()
                    .map(|(p, r, hovered)| {
                        let (x, y) = at(*p);
                        format!("{{\"x\":{x:.1},\"y\":{y:.1},\"r\":{:.1},\"hovered\":{hovered}}}", r * ppp)
                    })
                    .collect::<Vec<_>>()
                    .join(","),
                readout.first().map_or("null".into(), |t| format!("\"{t}\"")),
                self.measurement.as_ref().map_or("null".into(), |m| {
                    format!(
                        "{{\"hops\":{},\"shortest\":{:.6},\"direct\":{:.6},\"confirmed\":{},\"wraps\":{},\"steps\":{},\"geodesic\":[{}],\"path\":[{}],\"marks\":[{}]}}",
                        m.hops,
                        m.shortest,
                        m.direct,
                        m.confirmed,
                        m.wraps,
                        m.path.len(),
                        points(&straight),
                        points(&hops),
                        measured
                            .iter()
                            .map(|(p, _, filled)| {
                                let (x, y) = at(*p);
                                format!("{{\"x\":{x:.1},\"y\":{y:.1},\"filled\":{filled}}}")
                            })
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                })
            );
            let _ = std::fs::write(format!("{path}.json"), json);
        }

        Found { hovered, center }
    }
}

impl eframe::App for TilingApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(selftest) = &mut self.selftest {
            selftest.hook(ctx, raw_input, false);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().min(0.25);
        self.last_frame = now;
        if let Some(selftest) = &mut self.selftest {
            selftest.frame(&ctx);
        }
        self.handle_keys(&ctx);
        if self.view.spinning() {
            self.view.step_spin(self.model(), dt, self.gliding);
            ctx.request_repaint();
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(ui, |ui| self.tiling_view(&ctx, ui));
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Runs the tiling viewer: `tiling [p q]`.
pub fn run() -> eframe::Result {
    let args: Vec<i32> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let (p, q) = match args[..] {
        [p, q, ..] => (p, q),
        _ => (4, 5),
    };
    let start = Instant::now();
    let tiling = match TruncatedTiling::new(p, q) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "t{{{p},{q}}}: {} faces, {} lines, built in {:.2}s",
        tiling.faces.len(),
        tiling.lines.len(),
        start.elapsed().as_secs_f64()
    );
    let title = format!("Truncated {{{p},{q}}} tiling");
    eframe::run_native(
        &title.clone(),
        crate::native_options(&title),
        Box::new(|cc| Ok(Box::new(TilingApp::new(cc, tiling)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The faces and lines near the center, with all their edge lengths.
    fn edge_lengths(t: &TruncatedTiling) -> Vec<f64> {
        let near = |v: &Vector3D| v.abs() < 0.9;
        let mut lengths: Vec<f64> = t
            .faces
            .iter()
            .filter(|f| near(&f.center))
            .flat_map(|f| {
                (0..f.vertices.len()).map(|i| distance(f.vertices[i], f.vertices[(i + 1) % f.vertices.len()]))
            })
            .collect();
        lengths.extend(t.lines.iter().filter(|l| near(&l.a) && near(&l.b)).map(|l| distance(l.a, l.b)));
        lengths
    }

    #[test]
    fn truncated_tilings_are_uniform() {
        for (p, q) in [(4, 5), (3, 7), (7, 3), (5, 4), (6, 6)] {
            let t = TruncatedTiling::new(p, q).unwrap();
            let lengths = edge_lengths(&t);
            assert!(lengths.len() > 20, "t{{{p},{q}}}: too few edges near the center");
            let (min, max) = lengths.iter().fold((f64::MAX, 0.0f64), |(a, b), &l| (a.min(l), b.max(l)));
            assert!(max - min < 1e-9, "t{{{p},{q}}}: edge lengths range from {min} to {max}");

            // Faces near the center are 2p-gons and q-gons in the right colors.
            for f in t.faces.iter().filter(|f| f.center.abs() < 0.6) {
                let expected = if f.color == BIG_COLOR { 2 * p } else { q };
                assert_eq!(f.vertices.len(), expected as usize);
            }
        }
    }

    /// Fans of flat triangles are only right if no triangle folds over: all must turn the same
    /// way. Check every drawn face, across many views and all the models.
    #[test]
    fn faces_can_be_drawn_as_fans() {
        let t = TruncatedTiling::new(4, 5).unwrap();
        let models = [
            HyperbolicModel::Poincare,
            HyperbolicModel::Klein,
            HyperbolicModel::UpperHalfPlane,
            HyperbolicModel::Orthographic,
        ];
        let pixel = 2.0 * 1.1 / 1600.0;
        let mut checked = 0;
        for k in 0..40 {
            // Views moved a range of distances in a range of directions.
            let v = Complex::from_polar(0.95 * (k as f64 / 40.0).sqrt(), k as f64 * 2.4);
            let turn = Complex::from_polar(1.0, k as f64 * 0.7);
            let view = Isometry::new(Mobius::new(turn, turn * v * -1.0, v.conj() * -1.0, Complex::ONE), None);
            for m in models {
                let model = Model::for_puzzle(Geometry::Hyperbolic, m, SphericalModel::Sterographic);
                let mut scratch = Scratch::default();
                let screen = Screen { model, pixel, short_edge_px: t.short_edge_px };
                for face in &t.faces {
                    let Some(c) = face.ring(&view, &screen, &mut scratch) else {
                        continue;
                    };
                    let areas: Vec<f64> = scratch
                        .ring
                        .windows(2)
                        .map(|w| (w[0].x - c.x) * (w[1].y - c.y) - (w[0].y - c.y) * (w[1].x - c.x))
                        .collect();
                    let largest = areas.iter().fold(0.0f64, |a, &b| a.max(b.abs()));
                    let sign = areas.iter().sum::<f64>().signum();
                    for a in &areas {
                        assert!(a * sign > -1e-9 * largest, "{m:?}: a fan triangle folds over (view {k})");
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 10000);
    }

    #[test]
    fn rejects_non_hyperbolic_tilings() {
        assert!(TruncatedTiling::new(4, 4).is_err());
        assert!(TruncatedTiling::new(3, 5).is_err());
    }

    #[test]
    fn starts_centered_on_a_q_gon() {
        let t = TruncatedTiling::new(4, 5).unwrap();
        let center = t.faces.iter().filter(|f| f.color == SMALL_COLOR).map(|f| t.start.apply(f.center).abs());
        assert!(center.fold(f64::MAX, f64::min) < 1e-9);
    }
}
