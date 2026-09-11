//! MagicTile: Rubik's Cube-like puzzles on colored regular tilings of the sphere, the Euclidean
//! plane and the hyperbolic plane.
//!
//! Usage: magictile [puzzle ID or display name]
//!        magictile --screenshot out.png [options] [puzzle]   (see `headless.rs`)

mod app;
mod headless;
mod render;
mod scene;
mod selftest;
mod settings;
mod view;

fn main() -> eframe::Result {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--screenshot") {
        if let Err(e) = headless::run(&args) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    let mut viewport =
        eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 860.0]).with_title("MagicTile");
    if std::env::var("MAGICTILE_SELFTEST").is_ok() {
        // Occluded windows aren't painted, so keep self-test runs visible.
        viewport = viewport.with_window_level(eframe::egui::WindowLevel::AlwaysOnTop);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("MagicTile", options, Box::new(|cc| Ok(Box::new(app::MagicTileApp::new(cc)))))
}
