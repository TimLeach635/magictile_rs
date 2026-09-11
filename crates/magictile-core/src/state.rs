//! Puzzle state: the color (index) of every sticker on every master cell.

use crate::xml::{self, XElement};
use roxmltree::Node;

/// The color index used for "off" stickers in toggling (lights on) puzzles.
pub const OFF_COLOR: i32 = -1;

#[derive(Clone, Debug)]
pub struct State {
    n_cells: usize,
    n_stickers: usize,
    // Indexed by cell, then sticker. Changes go into `copy` and are then committed.
    state: Vec<Vec<i32>>,
    copy: Vec<Vec<i32>>,
    original: Vec<Vec<i32>>,
}

impl State {
    pub fn new(n_cells: usize, n_stickers: usize) -> Self {
        let solved: Vec<Vec<i32>> = (0..n_cells).map(|c| vec![c as i32; n_stickers]).collect();
        State { n_cells, n_stickers, state: solved.clone(), copy: solved.clone(), original: solved }
    }

    pub fn reset(&mut self) {
        *self = State::new(self.n_cells, self.n_stickers);
    }

    pub fn num_cells(&self) -> usize {
        self.n_cells
    }

    pub fn num_stickers(&self) -> usize {
        self.n_stickers
    }

    pub fn sticker_color_index(&self, cell: usize, sticker: usize) -> i32 {
        self.state[cell][sticker]
    }

    /// Stages a change (visible after [`State::commit_changes`]).
    pub fn set_sticker_color_index(&mut self, cell: usize, sticker: usize, color: i32) {
        self.copy[cell][sticker] = color;
    }

    /// Stages toggling a sticker between its original color and off.
    pub fn toggle_sticker_color_index(&mut self, cell: usize, sticker: usize) {
        self.copy[cell][sticker] =
            if self.copy[cell][sticker] == OFF_COLOR { self.original[cell][sticker] } else { OFF_COLOR };
    }

    pub fn commit_changes(&mut self) {
        self.state.clone_from(&self.copy);
    }

    pub fn commit_cell(&mut self, cell: usize) {
        self.state[cell].clone_from(&self.copy[cell]);
    }

    /// Every cell a single color.
    pub fn is_solved(&self) -> bool {
        self.state.iter().all(|cell| cell.iter().all(|&c| c == cell[0]))
    }

    /// For toggling puzzles: everything back on.
    pub fn is_all_on(&self) -> bool {
        self.state == self.original
    }

    /// One `Cell` element per cell, with two hex digits per sticker.
    pub fn save(&self) -> XElement {
        let mut e = XElement::new("State");
        for cell in &self.state {
            let s: String = cell.iter().map(|&c| save_color(c)).collect();
            e.push(XElement::with_text("Cell", s));
        }
        e
    }

    pub fn load(&mut self, node: Node) -> Result<(), String> {
        for (c, cell) in xml::children(node, "Cell").enumerate() {
            if c >= self.n_cells {
                return Err(format!("saved state has more than {} cells", self.n_cells));
            }
            let text = xml::text(cell);
            let colors = load_cell(&text, self.n_stickers)
                .ok_or_else(|| format!("invalid saved state for cell {c} (expected {} stickers)", self.n_stickers))?;
            for (s, color) in colors.into_iter().enumerate() {
                self.set_sticker_color_index(c, s, color);
            }
            self.commit_cell(c);
        }
        Ok(())
    }
}

/// Two lowercase hex digits. The original wrote the off color (-1) as "0ffffffff", which it
/// couldn't read back; we write it as "ff" and read both.
fn save_color(c: i32) -> String {
    if c == OFF_COLOR { "ff".into() } else { format!("{c:02x}") }
}

fn load_cell(text: &str, n_stickers: usize) -> Option<Vec<i32>> {
    let text = text.trim();
    let text = text.replace("0ffffffff", "ff");
    if text.len() != 2 * n_stickers || !text.is_ascii() {
        return None;
    }
    (0..n_stickers)
        .map(|s| {
            let v = i32::from_str_radix(&text[2 * s..2 * s + 2], 16).ok()?;
            Some(if v == 0xff { OFF_COLOR } else { v })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solved_and_toggling() {
        let mut s = State::new(3, 4);
        assert!(s.is_solved() && s.is_all_on());
        s.set_sticker_color_index(0, 1, 2);
        assert!(s.is_solved(), "not committed yet");
        s.commit_changes();
        assert!(!s.is_solved());

        let mut t = State::new(2, 1);
        t.toggle_sticker_color_index(1, 0);
        t.commit_changes();
        assert_eq!(t.sticker_color_index(1, 0), OFF_COLOR);
        assert!(!t.is_all_on());
        t.toggle_sticker_color_index(1, 0);
        t.commit_changes();
        assert!(t.is_all_on());
    }

    #[test]
    fn save_load() {
        let mut s = State::new(20, 3);
        s.set_sticker_color_index(19, 2, 17);
        s.set_sticker_color_index(0, 0, OFF_COLOR);
        s.commit_changes();
        let e = s.save();
        assert_eq!(e.children[19].text.as_deref(), Some("131311"));
        let text = e.to_pretty_string();
        let doc = roxmltree::Document::parse(&text).unwrap();
        let mut t = State::new(20, 3);
        t.load(doc.root_element()).unwrap();
        assert_eq!(t.state, s.state);
        // The original's broken off-color output reads back too.
        assert_eq!(load_cell("0ffffffff0101", 3), Some(vec![OFF_COLOR, 1, 1]));
    }
}
