//! A mouse-navigable truncated hyperbolic tiling (see [`magictile::tiling`]).

fn main() -> eframe::Result {
    env_logger::init();
    magictile::tiling::run()
}
