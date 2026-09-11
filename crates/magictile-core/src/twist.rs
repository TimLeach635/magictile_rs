//! Single twists, lists of them, and the undo/redo history.

use crate::cell::CellId;
use crate::xml::XElement;
use roxmltree::Node;

/// One twist of the puzzle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SingleTwist {
    /// Index of the identified twist data (the logical twist center).
    pub identified: usize,
    /// Systolic (earthquake) twists involve a second set of identified twist data.
    pub identified_systolic: Option<usize>,
    /// Left click twists CCW, right click CW.
    pub left_click: bool,
    pub slice_mask: i32,
    /// The extra slice mask for systolic twists.
    pub slice_mask_systolic: i32,
    pub macro_start: bool,
    pub macro_end: bool,
}

impl SingleTwist {
    /// Compares twists, ignoring the macro markers.
    pub fn same_twist(&self, other: &SingleTwist) -> bool {
        self.identified == other.identified
            && self.left_click == other.left_click
            && self.slice_mask == other.slice_mask
            && self.slice_mask_systolic == other.slice_mask_systolic
    }

    /// Whether we undo another twist.
    pub fn is_undo(&self, other: &SingleTwist) -> bool {
        let mut reversed = self.clone();
        reversed.reverse_twist();
        reversed.same_twist(other)
    }

    pub fn reverse_twist(&mut self) {
        self.left_click = !self.left_click;
    }

    /// E.g. "12:L:1", wrapped in brackets at macro boundaries.
    pub fn save_to_string(&self) -> String {
        let mut ret = String::new();
        if self.macro_start {
            ret.push('[');
        }
        ret += &format!("{}:{}:{}", self.identified, if self.left_click { "L" } else { "R" }, self.slice_mask);
        if self.macro_end {
            ret.push(']');
        }
        ret
    }

    /// Parses a saved twist. Returns `None` if it's malformed or refers to twist data that
    /// doesn't exist (`num_twist_data`).
    pub fn load_from_string(saved: &str, num_twist_data: usize) -> Option<SingleTwist> {
        let mut t =
            SingleTwist { macro_start: saved.starts_with('['), macro_end: saved.ends_with(']'), ..Default::default() };
        if t.macro_start && t.macro_end {
            // A macro of one twist: clear the markings.
            t.macro_start = false;
            t.macro_end = false;
        }

        let split: Vec<&str> = saved.trim_matches(|c| c == '[' || c == ']').split(':').collect();
        if split.len() != 3 {
            return None;
        }
        let index: usize = split[0].trim().parse().ok()?;
        if index >= num_twist_data {
            return None;
        }
        t.identified = index;
        t.left_click = split[1] == "L";
        t.slice_mask = split[2].trim().parse().ok()?;
        if t.slice_mask == 0 {
            t.slice_mask = 1;
        }
        Some(t)
    }
}

/// Saves twists as `Block` elements of up to 10 tab-separated twists.
pub fn save_twists(parent: &mut XElement, twists: &[SingleTwist]) {
    for chunk in twists.chunks(10) {
        let mut line: String = chunk.iter().map(|t| t.save_to_string() + "\t").collect();
        // The original only trimmed its trailing tab from full blocks (a `TrimEnd` whose result it
        // discarded on the last one); we match its output.
        if chunk.len() == 10 {
            line.pop();
        }
        parent.push(XElement::with_text("Block", line));
    }
}

/// Loads twists saved by [`save_twists`]. Returns an error message for invalid twists.
pub fn load_twists(node: Node, num_twist_data: usize) -> Result<Vec<SingleTwist>, String> {
    let mut twists = Vec::new();
    for block in crate::xml::children(node, "Block") {
        for item in crate::xml::text(block).split('\t').filter(|s| !s.is_empty()) {
            let t = SingleTwist::load_from_string(item, num_twist_data)
                .ok_or_else(|| format!("invalid twist '{item}' (the puzzle has {num_twist_data} twists)"))?;
            twists.push(t);
        }
    }
    Ok(twists)
}

/// The history of twists, for undo, redo and "solving" (undoing everything).
#[derive(Clone, Debug, Default)]
pub struct TwistHistory {
    twists: Vec<SingleTwist>,
    redo_twists: Vec<SingleTwist>,
    toggles: Vec<CellId>,
    undo_mode: bool,
    redo_mode: bool,
    /// The number of scramble twists applied.
    pub scrambles: usize,
}

impl TwistHistory {
    pub fn clear(&mut self) {
        self.twists.clear();
        self.redo_twists.clear();
        self.toggles.clear();
        self.scrambles = 0;
    }

    pub fn scrambled(&self) -> bool {
        self.scrambles != 0
    }

    /// Whether the twist in progress is an undo.
    pub fn undoing(&self) -> bool {
        self.undo_mode
    }

    /// Records a completed twist.
    pub fn update(&mut self, twist: &SingleTwist) {
        if self.undo_mode {
            self.twists.pop();
            let mut redo = twist.clone();
            redo.reverse_twist();
            self.redo_twists.push(redo);
            self.undo_mode = false;
            return;
        }

        if self.redo_mode {
            self.redo_twists.pop();
            self.redo_mode = false;
        } else {
            self.redo_twists.clear();
        }

        // Normal and redo twists.
        self.twists.push(twist.clone());
    }

    /// Records a toggling ("lights on") move.
    pub fn update_toggle(&mut self, cell: CellId) {
        self.toggles.push(cell);
    }

    /// The twist that undoes the last one, putting us in undo mode for it.
    pub fn get_undo_twist(&mut self) -> Option<SingleTwist> {
        let mut twist = self.twists.last()?.clone();
        twist.reverse_twist();
        self.undo_mode = true;
        Some(twist)
    }

    /// The twist to redo, putting us in redo mode for it.
    pub fn get_redo_twist(&mut self) -> Option<SingleTwist> {
        let twist = self.redo_twists.last()?.clone();
        self.redo_mode = true;
        Some(twist)
    }

    pub fn all_twists(&self) -> &[SingleTwist] {
        &self.twists
    }

    pub fn all_toggles(&self) -> &[CellId] {
        &self.toggles
    }

    pub fn all_moves_count(&self) -> usize {
        self.twists.len() + self.toggles.len()
    }

    pub fn save(&self) -> XElement {
        let mut e = XElement::new("History").child(XElement::with_text("Scrambles", self.scrambles.to_string()));
        save_twists(&mut e, &self.twists);
        e
    }

    pub fn load(&mut self, node: Node, num_twist_data: usize) -> Result<(), String> {
        self.clear();
        self.scrambles =
            crate::xml::child(node, "Scrambles").and_then(|n| crate::xml::text(n).trim().parse().ok()).unwrap_or(0);
        self.twists = load_twists(node, num_twist_data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn twist(i: usize, left: bool) -> SingleTwist {
        SingleTwist { identified: i, left_click: left, slice_mask: 1, ..Default::default() }
    }

    #[test]
    fn string_round_trip() {
        let mut t = twist(12, true);
        t.macro_start = true;
        assert_eq!(t.save_to_string(), "[12:L:1");
        assert_eq!(SingleTwist::load_from_string("[12:L:1", 20), Some(t));
        let single = SingleTwist::load_from_string("[3:R:0]", 20).unwrap();
        assert!(!single.macro_start && !single.macro_end && single.slice_mask == 1);
        assert_eq!(SingleTwist::load_from_string("30:R:1", 20), None);
    }

    #[test]
    fn blocks_match_original_format() {
        let twists: Vec<_> = (0..12).map(|i| twist(i, i % 2 == 0)).collect();
        let mut e = XElement::new("History");
        save_twists(&mut e, &twists);
        assert_eq!(e.children.len(), 2);
        assert!(!e.children[0].text.as_ref().unwrap().ends_with('\t'));
        assert_eq!(e.children[1].text.as_deref(), Some("10:L:1\t11:R:1\t"));

        let s = e.to_pretty_string();
        let doc = roxmltree::Document::parse(&s).unwrap();
        assert_eq!(load_twists(doc.root_element(), 20).unwrap(), twists);
    }

    #[test]
    fn undo_redo() {
        let mut h = TwistHistory::default();
        h.update(&twist(1, true));
        h.update(&twist(2, true));
        let undo = h.get_undo_twist().unwrap();
        assert_eq!(undo, twist(2, false));
        h.update(&undo);
        assert_eq!(h.all_twists(), &[twist(1, true)]);
        let redo = h.get_redo_twist().unwrap();
        assert_eq!(redo, twist(2, true));
        h.update(&redo);
        assert_eq!(h.all_twists().len(), 2);
        assert!(h.get_redo_twist().is_none());
    }
}
