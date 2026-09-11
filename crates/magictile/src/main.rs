//! The MagicTile puzzle app (see [`magictile::run_puzzles`]).

fn main() -> eframe::Result {
    env_logger::init();
    magictile::run_puzzles()
}
