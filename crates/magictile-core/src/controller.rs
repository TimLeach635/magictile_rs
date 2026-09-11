//! Twisting logic: animated twists, undo/redo/solve, scrambling, macros and setup moves.
//! (The original's `TwistHandler`, without its timers: callers drive animation via
//! [`TwistController::advance`].)

use crate::cell::CellId;
use crate::macros::{Macro, SetupMoves};
use crate::puzzle::Puzzle;
use crate::slice_mask;
use crate::twist::SingleTwist;
use rand::{Rng, RngExt};

/// Animation step per tick (in degrees) for a rotation rate setting in [0, 1].
pub fn rotation_step(rotation_rate: f64, twist_order: i32) -> f64 {
    // The odd .123 avoids rotations that project lines through infinity at "round" angles.
    if rotation_rate == 1.0 {
        // Extremely large, for "disco ball" mode.
        return 250.0;
    }
    (rotation_rate * 150.0 + 1.123456789) / twist_order as f64
}

/// What happened when a twist (or toggle) completed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Completed {
    /// A scrambled puzzle was solved by the user (worth celebrating).
    pub solved: bool,
}

#[derive(Debug, Default)]
pub struct TwistController {
    current: Option<SingleTwist>,
    rotation: f64,
    /// Whether we're undoing everything, one animated twist at a time.
    pub solving: bool,
    pub setup_moves: SetupMoves,
    /// The macro being recorded.
    pub working_macro: Macro,
}

impl TwistController {
    /// Call when a new puzzle is loaded.
    pub fn reset(&mut self) {
        *self = TwistController::default();
    }

    /// Whether a twist is animating.
    pub fn twisting(&self) -> bool {
        self.current.is_some()
    }

    pub fn current_twist(&self) -> Option<&SingleTwist> {
        self.current.as_ref()
    }

    /// The current eased rotation of the animating twist (0 to 1 for systolic puzzles).
    pub fn smoothed_rotation(&self, puzzle: &Puzzle) -> f64 {
        let Some(twist) = &self.current else {
            return 0.0;
        };
        let max = puzzle.twist_magnitude(twist);
        let mut result = (max / 2.0) * (-(std::f64::consts::PI * self.rotation / max).cos() + 1.0);
        if puzzle.config.systolic() {
            result /= max;
        }
        result
    }

    /// The order used for animation speed.
    pub fn current_twist_order(&self, puzzle: &Puzzle) -> i32 {
        match &self.current {
            Some(t) if t.identified_systolic.is_none() => puzzle.twist_order(t.identified),
            Some(_) => 10,
            None => 1,
        }
    }

    /// Starts animating a twist (ignored if one is already animating).
    pub fn start_rotate(&mut self, puzzle: &mut Puzzle, twist: SingleTwist) {
        if self.twisting() {
            return;
        }
        self.rotation = 0.0;
        puzzle.set_twisting(&twist, true);
        self.current = Some(twist);
    }

    /// Advances the animation by some degrees. Returns `Some` when the twist completes.
    pub fn advance(&mut self, puzzle: &mut Puzzle, degrees: f64) -> Option<Completed> {
        let twist = self.current.as_ref()?;
        let magnitude = puzzle.twist_magnitude(twist);
        self.rotation += degrees.to_radians();
        if self.rotation <= magnitude {
            return None;
        }

        let completed = self.finish_rotate(puzzle);
        if self.solving && !self.undo_internal(puzzle) {
            self.solving = false;
            // So auto-solves don't count as solving a scramble.
            puzzle.history.scrambles = 0;
        }
        Some(completed)
    }

    fn finish_rotate(&mut self, puzzle: &mut Puzzle) -> Completed {
        self.rotation = 0.0;
        let Some(twist) = self.current.take() else {
            return Completed::default();
        };

        puzzle.update_state(&twist);
        // This must happen before the history update (which clears undo mode).
        let completed =
            Completed { solved: !puzzle.history.undoing() && puzzle.history.scrambled() && puzzle.is_solved() };

        puzzle.history.update(&twist);
        self.setup_moves.update(&twist);
        self.working_macro.update(&twist);
        puzzle.set_twisting(&twist, false);
        completed
    }

    /// Completes a twist instantly.
    fn apply_instantly(&mut self, puzzle: &mut Puzzle, twist: SingleTwist) -> Completed {
        self.current = Some(twist);
        self.finish_rotate(puzzle)
    }

    pub fn undo(&mut self, puzzle: &mut Puzzle) {
        if !self.twisting() {
            self.undo_internal(puzzle);
        }
    }

    fn undo_internal(&mut self, puzzle: &mut Puzzle) -> bool {
        let Some(undo) = puzzle.history.get_undo_twist() else {
            return false;
        };
        if undo.macro_end {
            self.apply_undo_block_instantly(puzzle, undo);
        } else {
            self.start_rotate(puzzle, undo);
        }
        true
    }

    pub fn redo(&mut self, puzzle: &mut Puzzle) {
        if self.twisting() {
            return;
        }
        if let Some(redo) = puzzle.history.get_redo_twist() {
            if redo.macro_start {
                self.apply_redo_block_instantly(puzzle, redo);
            } else {
                self.start_rotate(puzzle, redo);
            }
        }
    }

    /// Undoes everything, one twist at a time.
    pub fn solve(&mut self, puzzle: &mut Puzzle) {
        if !self.twisting() {
            self.solving = self.undo_internal(puzzle);
        }
    }

    /// Undoes a whole macro at once.
    fn apply_undo_block_instantly(&mut self, puzzle: &mut Puzzle, start: SingleTwist) {
        let macro_start = start.macro_start;
        self.apply_instantly(puzzle, start);
        if !macro_start {
            while let Some(undo) = puzzle.history.get_undo_twist() {
                let done = undo.macro_start;
                self.apply_instantly(puzzle, undo);
                if done {
                    break;
                }
            }
        }

        // Keep solving.
        if self.solving {
            self.solve(puzzle);
        }
    }

    fn apply_redo_block_instantly(&mut self, puzzle: &mut Puzzle, start: SingleTwist) {
        let macro_end = start.macro_end;
        self.apply_instantly(puzzle, start);
        if !macro_end {
            while let Some(redo) = puzzle.history.get_redo_twist() {
                let done = redo.macro_end;
                self.apply_instantly(puzzle, redo);
                if done {
                    break;
                }
            }
        }
    }

    /// Applies a macro (reversed for right clicks) instantly.
    pub fn apply_macro(&mut self, puzzle: &mut Puzzle, m: &Macro, reverse: bool) {
        let twists = if reverse { m.reverse_twists() } else { m.twists().to_vec() };
        self.apply_macro_twists(puzzle, twists);
    }

    /// Undoes the setup moves.
    pub fn unwind(&mut self, puzzle: &mut Puzzle) {
        let twists = self.setup_moves.take_unwind_twists();
        self.apply_macro_twists(puzzle, twists);
    }

    /// Completes a commutator.
    pub fn commutator(&mut self, puzzle: &mut Puzzle) {
        let twists = self.setup_moves.take_commutator_twists();
        self.apply_macro_twists(puzzle, twists);
    }

    /// Applies twists instantly, marked as a macro block (not meant for undo/redo).
    fn apply_macro_twists(&mut self, puzzle: &mut Puzzle, twists: Vec<SingleTwist>) {
        let n = twists.len();
        for (i, mut twist) in twists.into_iter().enumerate() {
            // Only mark as a macro if there's more than one twist.
            if n > 1 {
                if i == 0 {
                    twist.macro_start = true;
                }
                if i == n - 1 {
                    twist.macro_end = true;
                }
            }
            self.apply_instantly(puzzle, twist);
        }
    }

    /// A toggling ("lights on") move.
    pub fn toggle(&mut self, puzzle: &mut Puzzle, cell: CellId) -> Completed {
        puzzle.toggle(cell);
        Completed { solved: !puzzle.history.undoing() && puzzle.history.scrambled() && puzzle.is_solved() }
    }

    /// Applies random twists (or toggles).
    pub fn scramble<R: Rng + ?Sized>(&mut self, puzzle: &mut Puzzle, num_twists: usize, rng: &mut R) {
        if puzzle.config.is_toggling() {
            let n = puzzle.masters.len();
            for _ in 0..num_twists {
                let cell = puzzle.masters[rng.random_range(0..n)];
                puzzle.toggle(cell);
            }
            puzzle.history.scrambles += num_twists;
            return;
        }

        let count = puzzle.all_twist_data.len();
        if count == 0 {
            return;
        }

        for _ in 0..num_twists {
            let mut twist = SingleTwist { left_click: rng.random_range(0..2) == 1, ..Default::default() };

            // Try to avoid repeating the last twist (suggested by Melinda Green).
            let last = puzzle.history.all_twists().last().map(|t| t.identified);
            twist.identified = rng.random_range(0..count);
            if last.is_some() && count > 2 {
                while Some(twist.identified) == last {
                    twist.identified = rng.random_range(0..count);
                }
            }

            let Some(&td_id) = puzzle.all_twist_data[twist.identified].twist_data_for_state_calcs.first() else {
                continue;
            };
            let td = &puzzle.twist_data[td_id];
            let num_slices = td.num_slices().max(1);
            twist.slice_mask = slice_mask::slice_to_mask(rng.random_range(0..num_slices) + 1);

            // Earthquake scrambling takes some more care.
            if puzzle.config.earthquake()
                && let Some((identified, mask)) =
                    puzzle.earthquake_companion(td_id, slice_mask::mask_to_dir_seg(twist.slice_mask))
            {
                twist.identified_systolic = Some(identified);
                twist.slice_mask_systolic = mask;
            }

            self.apply_instantly(puzzle, twist);
        }
        puzzle.history.scrambles += num_twists;
    }

    /// Back to the solved state, clearing history.
    pub fn reset_state(&mut self, puzzle: &mut Puzzle) {
        puzzle.state.reset();
        puzzle.history.clear();
        self.setup_moves.reset();
        self.working_macro.reset();
    }
}
