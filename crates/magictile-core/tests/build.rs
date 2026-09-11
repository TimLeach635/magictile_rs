//! Building known puzzles and checking their structure and twisting behavior.

use magictile_core::controller::TwistController;
use magictile_core::{Library, Puzzle, PuzzleConfig, SingleTwist};
use rand::SeedableRng;

fn build(config: PuzzleConfig) -> Puzzle {
    Puzzle::build(config, &mut ()).expect("puzzle builds")
}

fn twist(puzzle: &mut Puzzle, identified: usize, left: bool, slice_mask: i32) {
    let t = SingleTwist { identified, left_click: left, slice_mask, ..Default::default() };
    puzzle.update_state(&t);
}

#[test]
fn default_klein_quartic() {
    let p = build(PuzzleConfig::default());
    assert_eq!(p.masters.len(), 24);
    assert_eq!(p.stickers_per_cell, 15);
    assert_eq!(p.topology, "F=24, E=84, V=56, χ=-4");
    assert_eq!(p.all_twist_data.len(), 24, "one face twist per color");
    assert!(p.is_solved());
}

#[test]
fn rubiks_cube() {
    let lib = Library::load_standard();
    let mut p = build(lib.config_by_id("RubikCube").unwrap().clone());
    assert_eq!(p.masters.len(), 6);
    assert_eq!(p.stickers_per_cell, 9);
    assert_eq!(p.topology, "F=6, E=12, V=8, χ=2");
    assert_eq!(p.all_twist_data.len(), 6);

    // A quarter turn unsolves; four solve again.
    twist(&mut p, 0, true, 1);
    assert!(!p.is_solved());
    // A quarter turn moves 3 stickers on each of the 4 adjacent faces, plus the face's own 8.
    let moved: usize = (0..6).map(|c| (0..9).filter(|&s| p.state.sticker_color_index(c, s) != c as i32).count()).sum();
    assert_eq!(moved, 12);
    for _ in 0..3 {
        twist(&mut p, 0, true, 1);
    }
    assert!(p.is_solved());

    // A twist and its inverse.
    twist(&mut p, 3, true, 1);
    twist(&mut p, 3, false, 1);
    assert!(p.is_solved());

    // Sexy move (R U R' U') six times is the identity, for any two adjacent faces.
    let (a, b) = (0, 1);
    for _ in 0..6 {
        twist(&mut p, a, true, 1);
        twist(&mut p, b, true, 1);
        twist(&mut p, a, false, 1);
        twist(&mut p, b, false, 1);
    }
    assert!(p.is_solved());
}

#[test]
fn scramble_and_solve() {
    let lib = Library::load_standard();
    let mut p = build(lib.config_by_id("RubikCube").unwrap().clone());
    let mut c = TwistController::default();
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    c.scramble(&mut p, 30, &mut rng);
    assert!(!p.is_solved());
    assert_eq!(p.history.scrambles, 30);

    c.solve(&mut p);
    let mut steps = 0;
    while c.twisting() {
        c.advance(&mut p, 45.0);
        steps += 1;
        assert!(steps < 1000);
    }
    assert!(p.is_solved());
    assert!(p.history.all_twists().is_empty());
}

#[test]
fn hyperbolic_twists_are_permutations() {
    let mut p = build(PuzzleConfig::default());
    for id in 0..p.all_twist_data.len() {
        for _ in 0..7 {
            twist(&mut p, id, true, 1);
        }
        assert!(p.is_solved(), "7 twists of face {id} should be the identity");
    }
    // Twist, twist another, undo both.
    twist(&mut p, 0, true, 1);
    twist(&mut p, 5, false, 1);
    assert!(!p.is_solved());
    twist(&mut p, 5, true, 1);
    twist(&mut p, 0, false, 1);
    assert!(p.is_solved());
}

#[test]
fn save_and_load_log() {
    use magictile_core::loader;
    use magictile_core::macros::{Macro, MacroList};

    let lib = Library::load_standard();
    let mut p = build(lib.config_by_id("RubikCube").unwrap().clone());
    let mut c = TwistController::default();
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    c.scramble(&mut p, 12, &mut rng);
    twist(&mut p, 2, true, 1);
    p.history.update(&SingleTwist { identified: 2, left_click: true, slice_mask: 1, ..Default::default() });

    // A macro recorded from a click on cell 0.
    let mut m = Macro::default();
    m.display_name = "Test & <macro>".into();
    let cell = p.masters[0];
    let point = p.cells[cell].center();
    m.setup_mobius(&p, cell, point, false);
    m.start_recording();
    m.update(&SingleTwist { identified: 1, left_click: false, slice_mask: 1, ..Default::default() });
    m.stop_recording();
    let macros = MacroList { macros: vec![m] };

    let text = loader::write_log(&p, &macros);
    assert!(text.starts_with("<?xml"));

    let saved = loader::read_log(&text).unwrap();
    assert_eq!(saved.config.id, "RubikCube");
    let mut q = build(saved.config.clone());
    let loaded_macros = saved.apply(&mut q).unwrap();

    for cell in 0..6 {
        for s in 0..9 {
            assert_eq!(q.state.sticker_color_index(cell, s), p.state.sticker_color_index(cell, s));
        }
    }
    assert_eq!(q.history.all_twists(), p.history.all_twists());
    assert_eq!(q.history.scrambles, 12);
    assert_eq!(loaded_macros.macros.len(), 1);
    assert_eq!(loaded_macros.macros[0].display_name, "Test & <macro>");
    assert_eq!(loaded_macros.macros[0].twists(), macros.macros[0].twists());
    assert_eq!(loaded_macros.macros[0].mobius, macros.macros[0].mobius);

    // Macros can also be read from a log for the same puzzle.
    assert_eq!(loader::read_macros(&text, &q).unwrap().macros.len(), 1);
    let other = build(PuzzleConfig::default());
    assert!(loader::read_macros(&text, &other).is_err());
}

#[test]
fn log_without_version_uses_preview_build() {
    let log = r#"<MagicTileLog><PuzzleConfig><P>7</P><Q>3</Q></PuzzleConfig></MagicTileLog>"#;
    let saved = magictile_core::loader::read_log(log).unwrap();
    assert_eq!(saved.config.version, "2.0");
}

#[test]
fn macros_transform_to_other_cells() {
    use magictile_core::macros::Macro;
    let mut p = build(PuzzleConfig::default());

    // Record a twist of the central face, clicking near its first vertex.
    let center_cell = p.masters[0];
    let v0 = p.cells[center_cell].boundary.segments[0].p1 * 0.9;
    let mut m = Macro::default();
    m.setup_mobius(&p, center_cell, v0, false);
    m.start_recording();
    let center_twist = p.closest_twisting_circles(p.cells[center_cell].center()).unwrap();
    let identified = p.twist_data[center_twist].identified.unwrap();
    m.update(&SingleTwist { identified, left_click: true, slice_mask: 1, ..Default::default() });
    m.stop_recording();

    // Applied on another cell, it twists that cell's face instead.
    let other = p.masters[3];
    let other_v0 = p.cells[other].center() * 0.1 + p.cells[other].boundary.segments[0].p1 * 0.9;
    let moved = m.transform(&p, other, other_v0, false);
    let other_twist = p.closest_twisting_circles(p.cells[other].center()).unwrap();
    assert_eq!(moved.twists()[0].identified, p.twist_data[other_twist].identified.unwrap());

    let mut c = TwistController::default();
    c.apply_macro(&mut p, &moved, false);
    assert!(!p.is_solved());
    c.apply_macro(&mut p, &moved, true);
    assert!(p.is_solved());
}

/// Builds every puzzle in the library (slow; run with `cargo test --release -- --ignored`).
#[test]
#[ignore]
fn all_library_puzzles_build() {
    let lib = Library::load_standard();
    // Classes whose ExpectedNumColors doesn't match what they build, in the original too.
    let known = ["{4,4} 9C (shift)", "{6,3} 9C (3x3C)"];
    for config in &lib.configs {
        let p = Puzzle::build(config.clone(), &mut ()).unwrap_or_else(|e| panic!("{}: {e}", config.display_name));
        if config.expected_num_colors != 0 && !known.iter().any(|k| config.display_name.starts_with(k)) {
            assert_eq!(p.masters.len(), config.expected_num_colors as usize, "{}", config.display_name);
        }
    }
}
