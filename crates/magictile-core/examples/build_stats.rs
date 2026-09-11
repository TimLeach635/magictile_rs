//! Builds every puzzle in the library and prints statistics as tab-separated values:
//! id, colors, expected colors, stickers per cell, twists, topology, seconds.
//!
//! Usage: cargo run --release -p magictile-core --example build_stats [filter] > stats.tsv
//!
//! Problems (build failures, unexpected color counts) are summarized on stderr.

use magictile_core::{Library, Puzzle};
use rayon::prelude::*;
use std::time::Instant;

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
    let lib = Library::load_standard();
    let configs: Vec<_> =
        lib.configs.iter().filter(|c| c.display_name.contains(&filter) || c.id.contains(&filter)).collect();
    eprintln!("Building {} puzzles...", configs.len());

    let start = Instant::now();
    let results: Vec<_> = configs
        .par_iter()
        .map(|config| {
            let t = Instant::now();
            let result = Puzzle::build((*config).clone(), &mut ());
            (config, result, t.elapsed().as_secs_f64())
        })
        .collect();

    let mut problems = Vec::new();
    println!("id\tdisplay_name\tcolors\texpected\tstickers\ttwists\ttopology\tseconds");
    for (config, result, secs) in &results {
        match result {
            Ok(p) => {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.2}",
                    config.id,
                    config.display_name,
                    p.masters.len(),
                    config.expected_num_colors,
                    p.stickers_per_cell,
                    p.all_twist_data.len(),
                    p.topology,
                    secs
                );
                if config.expected_num_colors != 0 && p.masters.len() != config.expected_num_colors as usize {
                    problems.push(format!(
                        "{}: {} colors, expected {}",
                        config.display_name,
                        p.masters.len(),
                        config.expected_num_colors
                    ));
                }
                if config.slicing_circles.sliced() && p.stickers_per_cell <= 1 {
                    problems
                        .push(format!("{}: sliced but only {} sticker(s)", config.display_name, p.stickers_per_cell));
                }
            }
            Err(e) => problems.push(format!("{}: {e}", config.display_name)),
        }
    }

    let slowest = results.iter().map(|r| r.2).fold(0.0, f64::max);
    eprintln!(
        "Built {} puzzles in {:.1}s (slowest {:.1}s). {} problems:",
        results.len(),
        start.elapsed().as_secs_f64(),
        slowest,
        problems.len()
    );
    for p in &problems {
        eprintln!("  {p}");
    }
}
