//! Renders a gallery of tilings, sliced tiles and display models to an HTML page of SVGs, as a
//! visual check of the geometry port.
//!
//! Usage: cargo run -p r3 --example gallery [output.html]
//!
//! Tiles are colored by tile index. Real puzzle coloring (identifying tiles into colors) needs the
//! puzzle model from phase 2.

use r3::models;
use r3::nethash::NetSet;
use r3::slicer;
use r3::{CircleNE, Geometry, Isometry, Mobius, Polygon, Tiling, TilingConfig, Vector3D};
use std::fmt::Write as _;

const PALETTE: [&str; 12] = [
    "#e6194b", "#3cb44b", "#ffe119", "#4363d8", "#f58231", "#911eb4", "#46f0f0", "#f032e6", "#bcf60c", "#fabebe",
    "#008080", "#e6beff",
];

/// A viewport in model coordinates: (xmin, xmax, ymin, ymax).
type View = (f64, f64, f64, f64);

struct Panel {
    title: String,
    note: String,
    svg: String,
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "target/r3-gallery.html".to_string());
    let mut panels = Vec::new();

    // Plain tilings in the three geometries.
    let cube = Tiling::generate(TilingConfig::spherical(4, 3).unwrap());
    panels.push(tiling_panel(
        "{4,3} cube",
        "Spherical, stereographic projection. The sixth face is outside, at infinity.",
        &cube,
        (-2.6, 2.6, -2.6, 2.6),
        |v| v,
    ));
    let ico = Tiling::generate(TilingConfig::spherical(3, 5).unwrap());
    panels.push(tiling_panel(
        "{3,5} icosahedron",
        "Spherical, stereographic projection.",
        &ico,
        (-3.0, 3.0, -3.0, 3.0),
        |v| v,
    ));
    let hex = Tiling::generate(TilingConfig::new(6, 3, 300));
    panels.push(tiling_panel("{6,3} hexagons", "Euclidean.", &hex, (-1.6, 1.6, -1.6, 1.6), |v| v));
    let t73 = Tiling::generate(TilingConfig::new(7, 3, 1500));
    panels.push(tiling_panel("{7,3}", "Hyperbolic, Poincaré disk.", &t73, (-1.02, 1.02, -1.02, 1.02), |v| v));
    let t54 = Tiling::generate(TilingConfig::new(5, 4, 1500));
    panels.push(tiling_panel("{5,4}", "Hyperbolic, Poincaré disk.", &t54, (-1.02, 1.02, -1.02, 1.02), |v| v));

    // Display models of the hyperbolic plane.
    let t73_small = Tiling::generate(TilingConfig::new(7, 3, 700));
    panels.push(tiling_panel(
        "{7,3} Klein model",
        "Geodesics become straight lines.",
        &t73_small,
        (-1.02, 1.02, -1.02, 1.02),
        models::poincare_to_klein,
    ));
    panels.push(tiling_panel(
        "{7,3} upper half plane",
        "The disk maps to y > -1.",
        &t73_small,
        (-3.0, 3.0, -1.2, 3.6),
        models::poincare_to_upper,
    ));
    panels.push(tiling_panel(
        "{7,3} orthographic",
        "Projected from the hyperboloid.",
        &t73_small,
        (-3.0, 3.0, -3.0, 3.0),
        models::poincare_to_ortho,
    ));

    // Sliced template tiles, as puzzle building does, then copied onto every tile.
    let (rubik_stickers, rubik) = sliced_puzzle(4, 3, 6, (0.67, 0.0, 1.0), 0.025);
    println!("Rubik's Cube: {} stickers per face", rubik_stickers.len());
    panels.push(stickers_panel(
        "Rubik's Cube face stickers",
        &format!("{{4,3}}, slicing distance .67:0:1, thickness 0.025: {} stickers.", rubik_stickers.len()),
        &rubik_stickers,
        (-0.5, 0.5, -0.5, 0.5),
    ));
    panels.push(puzzle_panel(
        "Rubik's Cube",
        "Stickers copied onto all six faces (the sixth is outside, at infinity).",
        &rubik,
        &rubik_stickers,
        (-2.6, 2.6, -2.6, 2.6),
    ));

    let (h_stickers, h_tiling) = sliced_puzzle(7, 3, 400, (0.67, 0.0, 1.0), 0.01);
    println!("{{7,3}} Classic: {} stickers per tile", h_stickers.len());
    panels.push(stickers_panel(
        "{7,3} Classic tile stickers",
        &format!("Slicing distance 0.67:0:1, thickness 0.01: {} stickers.", h_stickers.len()),
        &h_stickers,
        (-0.32, 0.32, -0.32, 0.32),
    ));
    panels.push(puzzle_panel(
        "{7,3} Classic",
        "Stickers copied onto 400 tiles.",
        &h_tiling,
        &h_stickers,
        (-1.02, 1.02, -1.02, 1.02),
    ));

    std::fs::write(&out, html(&panels)).expect("failed to write gallery");
    println!("Wrote {out}");
}

/// Mirrors `Puzzle.SliceUpTemplate` for face-centered puzzles with one slicing distance.
fn sliced_puzzle(
    p: i32,
    q: i32,
    num_tiles: usize,
    (dp, dq, dr): (f64, f64, f64),
    thickness: f64,
) -> (Vec<Polygon>, Tiling) {
    let mut config = TilingConfig::new(p, q, num_tiles);
    config.shrink = 0.94;
    let tiling = Tiling::generate(config);
    let g = config.geometry();
    let template = &tiling.tiles[0];

    // The slicing circle about the template center, with its radius measured in the geometry.
    let dist = dp * r3::geometry2d::triangle_p_side(p, q)
        + dq * r3::geometry2d::triangle_q_side(p, q)
        + dr * r3::geometry2d::triangle_hypotenuse(p, q);
    let radius = match g {
        Geometry::Spherical => r3::spherical2d::s2e_norm(dist),
        Geometry::Euclidean => dist,
        Geometry::Hyperbolic => r3::donhatch::h2e_norm(dist),
    };
    let mut base = CircleNE::new(r3::Circle::new(template.center(), radius), template.center());
    let mut to_center = Mobius::default();
    to_center.isometry(g, 0.0, template.center());
    base.transform(&to_center);

    // Copies of the circle around the template: every tile for spherical puzzles, otherwise
    // the neighbors plus any tile within the circle.
    let mut isometries: Vec<Isometry> = if g == Geometry::Spherical {
        tiling.tiles.iter().map(|t| t.isometry.clone()).collect()
    } else {
        std::iter::once(0)
            .chain(template.edge_incidences.iter().copied())
            .chain(template.vertex_incidences.iter().copied())
            .map(|i| tiling.tiles[i].isometry.clone())
            .collect()
    };
    if g != Geometry::Spherical {
        for t in &tiling.tiles {
            if t.center().abs() <= base.radius || t.boundary.vertices().iter().any(|v| v.abs() <= base.radius) {
                let mut i = Isometry::default();
                i.calculate_from_two_polygons(template, &t.boundary, g);
                isometries.push(i.inverse());
            }
        }
    }
    let mut slicers = NetSet::new();
    for i in &isometries {
        let mut c = base.clone();
        c.transform(i);
        slicers.insert(c);
    }
    let mut slicers = slicers.into_vec();

    // Slice with each circle in turn (last first, as the original does).
    let mut pieces = vec![template.drawn.clone()];
    while let Some(slicer_circle) = slicers.pop() {
        let mut next = Vec::new();
        for mut piece in pieces {
            next.extend(slicer::slice_polygon_thick(&mut piece, &slicer_circle, g, thickness));
        }
        pieces = next;
    }
    pieces.retain(|p| !r3::tolerance::zero(p.signed_area()));
    (pieces, tiling)
}

fn tiling_panel(title: &str, note: &str, tiling: &Tiling, view: View, model: impl Fn(Vector3D) -> Vector3D) -> Panel {
    let mut body = String::new();
    for (i, t) in tiling.tiles.iter().enumerate() {
        if t.vertex_circle.radius < 0.004 && !t.has_points_projected_to_infinity() {
            continue; // Too small to see.
        }
        body += &polygon_path(&t.boundary, PALETTE[i % PALETTE.len()], &model);
    }
    Panel { title: title.into(), note: note.into(), svg: svg(view, &body) }
}

fn stickers_panel(title: &str, note: &str, stickers: &[Polygon], view: View) -> Panel {
    let body: String =
        stickers.iter().enumerate().map(|(i, s)| polygon_path(s, PALETTE[i % PALETTE.len()], &|v| v)).collect();
    Panel { title: title.into(), note: note.into(), svg: svg(view, &body) }
}

fn puzzle_panel(title: &str, note: &str, tiling: &Tiling, stickers: &[Polygon], view: View) -> Panel {
    let mut body = String::new();
    for (i, t) in tiling.tiles.iter().enumerate() {
        if t.vertex_circle.radius < 0.004 && !t.has_points_projected_to_infinity() {
            continue;
        }
        let to_tile = t.isometry.inverse();
        for s in stickers {
            let mut s = s.clone();
            s.transform(&to_tile);
            body += &polygon_path(&s, PALETTE[i % PALETTE.len()], &|v| v);
        }
    }
    Panel { title: title.into(), note: note.into(), svg: svg(view, &body) }
}

/// An SVG path for a polygon. Polygons containing infinity (inverted) are filled outside their
/// boundary, the SVG equivalent of the original's stencil trick.
fn polygon_path(poly: &Polygon, color: &str, model: &impl Fn(Vector3D) -> Vector3D) -> String {
    let points: Vec<Vector3D> = poly.edge_points().into_iter().map(model).collect();
    if points.iter().any(|p| p.is_dne()) {
        return String::new();
    }
    let mut d = String::new();
    if poly.is_inverted() {
        d += "M-1e4,-1e4 L1e4,-1e4 L1e4,1e4 L-1e4,1e4 Z ";
    }
    for (i, p) in points.iter().enumerate() {
        let (x, y) = (p.x.clamp(-1e4, 1e4), (-p.y).clamp(-1e4, 1e4));
        let _ = write!(d, "{}{:.4},{:.4} ", if i == 0 { "M" } else { "L" }, x, y);
    }
    d += "Z";
    format!("<path d=\"{d}\" fill=\"{color}\" fill-rule=\"evenodd\"/>\n")
}

fn svg((x0, x1, y0, y1): View, body: &str) -> String {
    let (w, h) = (x1 - x0, y1 - y0);
    let stroke = w / 700.0;
    format!(
        "<svg viewBox=\"{x0} {} {w} {h}\" xmlns=\"http://www.w3.org/2000/svg\">\
         <rect x=\"{x0}\" y=\"{}\" width=\"{w}\" height=\"{h}\" fill=\"#15151c\"/>\
         <g stroke=\"#15151c\" stroke-width=\"{stroke}\" stroke-linejoin=\"round\">{body}</g></svg>",
        -y1, -y1
    )
}

fn html(panels: &[Panel]) -> String {
    let mut s = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>r3 geometry gallery</title><style>\
         body{font-family:system-ui,sans-serif;background:#f4f4f6;color:#222;margin:24px}\
         h1{font-size:20px;margin:0 0 4px}p.sub{margin:0 0 20px;color:#555}\
         .grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(340px,1fr));gap:18px}\
         figure{margin:0;background:#fff;border-radius:8px;padding:10px;box-shadow:0 1px 3px #0002}\
         figure svg{width:100%;height:auto;display:block;border-radius:4px}\
         figcaption b{display:block;margin-top:8px}figcaption span{color:#666;font-size:13px}\
         </style></head><body><h1>r3 geometry gallery</h1>\
         <p class=\"sub\">Rendered by the Rust port of R3.Core. Colors are by tile index, not puzzle coloring.</p>\
         <div class=\"grid\">",
    );
    for p in panels {
        let _ =
            write!(s, "<figure>{}<figcaption><b>{}</b><span>{}</span></figcaption></figure>", p.svg, p.title, p.note);
    }
    s += "</div></body></html>";
    s
}
