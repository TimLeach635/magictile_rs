//! MagicTile: Rubik's Cube-like puzzles on colored regular tilings of the sphere, the Euclidean
//! plane and the hyperbolic plane, plus standalone hyperbolic tiling visualisations that share
//! its rendering and navigation.

#[cfg(feature = "puzzle")]
mod app;
mod cayley;
mod draw;
#[cfg(feature = "puzzle")]
mod headless;
mod render;
#[cfg(feature = "puzzle")]
mod scene;
mod selftest;
#[cfg(feature = "puzzle")]
mod settings;
pub mod tiling;
mod view;
#[cfg(target_arch = "wasm32")]
mod web;

/// The puzzle app.
#[cfg(feature = "puzzle")]
///
/// Usage: magictile [puzzle ID or display name]
///        magictile --screenshot out.png [options] [puzzle]   (see `headless.rs`)
pub fn run_puzzles() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--screenshot") {
        if let Err(e) = headless::run(&args) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    eframe::run_native(
        "MagicTile",
        native_options("MagicTile"),
        Box::new(|cc| Ok(Box::new(app::MagicTileApp::new(cc)))),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn native_options(title: &str) -> eframe::NativeOptions {
    let mut viewport = eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 860.0]).with_title(title);
    if selftest::active() {
        // Occluded windows aren't painted, so keep self-test runs visible.
        viewport = viewport.with_window_level(eframe::egui::WindowLevel::AlwaysOnTop);
    }
    eframe::NativeOptions { viewport, ..Default::default() }
}
