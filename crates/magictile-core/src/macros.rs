//! Macros (recorded twist sequences that can be applied anywhere) and setup moves (for
//! conjugates and commutators).

use crate::cell::CellId;
use crate::netfmt::{format_xml_double, parse_bool, parse_double};
use crate::puzzle::Puzzle;
use crate::twist::{self, SingleTwist};
use crate::xml::{self, XElement};
use r3::euclidean2d;
use r3::util;
use r3::{Complex, Isometry, Mobius, Transform, Vector3D};
use roxmltree::Node;
use std::f64::consts::PI;

const NUMERICS_NS: &str = "http://schemas.datacontract.org/2004/07/System.Numerics";

#[derive(Clone, Debug, Default)]
pub struct Macro {
    pub display_name: String,
    /// Reorients the macro to a standard position on the home tile (from the point clicked when
    /// it was defined).
    pub mobius: Mobius,
    /// Whether the view was reflected when we were defined (possible with non-orientable
    /// puzzles), so we can reflect properly when applying.
    pub view_reflected: bool,
    pub recording: bool,
    twists: Vec<SingleTwist>,
}

impl Macro {
    pub fn clone_all_but_twists(&self) -> Macro {
        Macro {
            display_name: self.display_name.clone(),
            mobius: self.mobius,
            view_reflected: self.view_reflected,
            recording: false,
            twists: Vec::new(),
        }
    }

    /// Calculates our reference transform from a click (in coordinates untransformed by view
    /// motion).
    pub fn setup_mobius(
        &mut self,
        puzzle: &Puzzle,
        clicked_cell: CellId,
        clicked_point: Vector3D,
        view_reflected: bool,
    ) {
        self.mobius = setup_isometry(puzzle, clicked_cell, clicked_point).mobius;
        self.view_reflected = view_reflected;
    }

    /// This macro moved to a different click location.
    pub fn transform(
        &self,
        puzzle: &Puzzle,
        clicked_cell: CellId,
        clicked_point: Vector3D,
        view_reflected: bool,
    ) -> Macro {
        let mut m = self.clone_all_but_twists();
        m.setup_mobius(puzzle, clicked_cell, clicked_point, view_reflected);

        // Did we have an odd number of view reflections?
        let view_reflected = self.view_reflected ^ m.view_reflected;
        let mut iso1 = Isometry::new(m.mobius, None);
        let iso2 = Isometry::new(self.mobius, None);
        if view_reflected {
            iso1 = &Isometry::reflect_x() * &iso1;
        }
        let combined = &iso1.inverse() * &iso2;

        for t in &self.twists {
            // Use the twist data closest to the origin after transforming. (Using the first one
            // sometimes transformed near the disk boundary, where we'd run out of cells.)
            let candidates = puzzle.all_twist_data[t.identified].twist_data_for_state_calcs.clone();
            let sorted =
                util::sort_by_f64_key(candidates, |&td| combined.apply(puzzle.twist_data[td].center).mag_squared());
            let Some(&td_original) = sorted.first() else {
                continue;
            };
            let new_center = combined.apply(puzzle.twist_data[td_original].center);
            let Some(td_new) = puzzle.closest_twisting_circles(new_center) else {
                continue;
            };
            let Some(identified) = puzzle.twist_data[td_new].identified else {
                continue;
            };

            let mut clone = t.clone();
            clone.identified = identified;

            // If the transformed twist's reverse state changed, we may need to reverse it.
            let reverse = puzzle.twist_data[td_original].reverse ^ puzzle.twist_data[td_new].reverse;
            if reverse ^ view_reflected {
                clone.reverse_twist();
            }
            m.twists.push(clone);
        }
        m
    }

    pub fn reset(&mut self) {
        self.twists.clear();
        self.recording = false;
    }

    pub fn start_recording(&mut self) {
        self.twists.clear();
        self.recording = true;
    }

    pub fn stop_recording(&mut self) {
        self.recording = false;
    }

    /// Records a twist (if recording), dropping it and the previous twist if it undoes it.
    pub fn update(&mut self, twist: &SingleTwist) {
        if !self.recording {
            return;
        }
        if self.twists.last().is_some_and(|last| twist.is_undo(last)) {
            self.twists.pop();
            return;
        }
        self.twists.push(twist.clone());
    }

    /// Clears macro markings on our twists (to avoid nesting issues when using macros while
    /// creating others).
    pub fn clear_start_end_markings(&mut self) {
        for t in &mut self.twists {
            t.macro_start = false;
            t.macro_end = false;
        }
    }

    pub fn twists(&self) -> &[SingleTwist] {
        &self.twists
    }

    /// The twists undoing this macro.
    pub fn reverse_twists(&self) -> Vec<SingleTwist> {
        self.twists
            .iter()
            .rev()
            .map(|t| {
                let mut t = t.clone();
                t.reverse_twist();
                t
            })
            .collect()
    }

    pub fn save(&self) -> XElement {
        let mut e = XElement::new("Macro")
            .child(XElement::with_text("DisplayName", &self.display_name))
            .child(save_mobius(&self.mobius))
            .child(XElement::with_text("ViewReflected", if self.view_reflected { "true" } else { "false" }));
        twist::save_twists(&mut e, &self.twists);
        e
    }

    pub fn load(node: Node, num_twist_data: usize) -> Result<Macro, String> {
        let mut m = Macro {
            display_name: xml::child(node, "DisplayName").map(xml::text).unwrap_or_default(),
            ..Default::default()
        };
        let mobius = xml::child(node, "Mobius").ok_or("macro is missing its Mobius")?;
        m.mobius = load_mobius(mobius)?;
        if let Some(v) = xml::child(node, "ViewReflected") {
            m.view_reflected = parse_bool(&xml::text(v)).unwrap_or(false);
        }
        m.twists = twist::load_twists(node, num_twist_data)?;
        Ok(m)
    }
}

/// Rounds a clicked point to the nearest vertex of its cell, giving the transform to a canonical
/// position.
fn setup_isometry(puzzle: &Puzzle, clicked_cell: CellId, clicked_point: Vector3D) -> Isometry {
    let p = puzzle.config.p;
    let g = puzzle.config.geometry();
    let mut cell_isometry = puzzle.cells[clicked_cell].isometry.clone();

    // Take out reflections.
    if cell_isometry.reflected() {
        cell_isometry = &Isometry::reflect_x() * &cell_isometry;
    }

    // Round to the nearest vertex.
    let centered = cell_isometry.apply(clicked_point);
    let angle = euclidean2d::angle_to_counter_clock(centered, Vector3D::new(1.0, 0.0));
    let mut angle_from_zero_to_p = (p as f64 * angle / (2.0 * PI)).round_ties_even();
    if p == angle_from_zero_to_p as i32 {
        angle_from_zero_to_p = 0.0;
    }
    let angle = 2.0 * PI * angle_from_zero_to_p / p as f64;

    // Takes the vertex to its canonical position.
    let mut rotation = Mobius::default();
    rotation.isometry(g, angle, Complex::ZERO);
    &Isometry::new(rotation, None) * &cell_isometry
}

/// The DataContract form of a Möbius transformation (its coefficients are
/// `System.Numerics.Complex`, serialized by field).
fn save_mobius(m: &Mobius) -> XElement {
    let complex = |name: &str, c: Complex| {
        XElement::new(name)
            .attr("xmlns:d2p1", NUMERICS_NS)
            .child(XElement::with_text("d2p1:m_imaginary", format_xml_double(c.im)))
            .child(XElement::with_text("d2p1:m_real", format_xml_double(c.re)))
    };
    XElement::new("Mobius")
        .attr("xmlns:i", xml::XSI_NS)
        .child(complex("A", m.a))
        .child(complex("B", m.b))
        .child(complex("C", m.c))
        .child(complex("D", m.d))
}

fn load_mobius(node: Node) -> Result<Mobius, String> {
    let complex = |name: &str| -> Result<Complex, String> {
        let n = xml::child(node, name).ok_or_else(|| format!("Mobius is missing {name}"))?;
        let part = |field: &str| xml::child(n, field).and_then(|f| parse_double(&xml::text(f))).unwrap_or(0.0);
        Ok(Complex::new(part("m_real"), part("m_imaginary")))
    };
    Ok(Mobius::new(complex("A")?, complex("B")?, complex("C")?, complex("D")?))
}

/// The macros for a puzzle.
#[derive(Clone, Debug, Default)]
pub struct MacroList {
    pub macros: Vec<Macro>,
}

impl MacroList {
    /// Saves with the puzzle's ID and name (macros only make sense for the puzzle they were made
    /// on).
    pub fn save(&self, name: &str, puzzle: &Puzzle) -> XElement {
        let mut e = XElement::new(name)
            .child(XElement::with_text("PuzzleID", &puzzle.config.id))
            .child(XElement::with_text("PuzzleName", &puzzle.config.display_name));
        for m in &self.macros {
            e.push(m.save());
        }
        e
    }

    pub fn load(node: Node, num_twist_data: usize) -> Result<MacroList, String> {
        let macros = xml::children(node, "Macro").map(|m| Macro::load(m, num_twist_data)).collect::<Result<_, _>>()?;
        Ok(MacroList { macros })
    }
}

/// Setup moves, for conjugates (setup, twist, unwind) and commutators.
#[derive(Clone, Debug, Default)]
pub struct SetupMoves {
    setup_moves: Macro,
    commutator_moves: Macro,
    recording_setup: bool,
    recording_commutator: bool,
}

impl SetupMoves {
    pub fn reset(&mut self) {
        self.setup_moves.reset();
        self.commutator_moves.reset();
        self.recording_setup = false;
        self.recording_commutator = false;
    }

    pub fn start_recording(&mut self) {
        self.setup_moves.start_recording();
        self.recording_setup = true;
    }

    pub fn update(&mut self, twist: &SingleTwist) {
        if self.recording_setup {
            self.setup_moves.update(twist);
        }
        if self.recording_commutator {
            self.commutator_moves.update(twist);
        }
    }

    /// Ends the setup moves, and starts recording the moves for a commutator.
    pub fn stop_recording(&mut self) {
        self.setup_moves.stop_recording();
        self.recording_setup = false;
        self.commutator_moves.start_recording();
        self.recording_commutator = true;
    }

    pub fn recording_setup(&self) -> bool {
        self.recording_setup
    }

    pub fn recording_commutator(&self) -> bool {
        self.recording_commutator
    }

    /// The twists undoing the setup moves (this stops recording).
    pub fn take_unwind_twists(&mut self) -> Vec<SingleTwist> {
        let result = self.setup_moves.reverse_twists();
        self.reset();
        result
    }

    /// The second half of a commutator ABA'B' (AB having been done already). This stops recording.
    pub fn take_commutator_twists(&mut self) -> Vec<SingleTwist> {
        let mut result = self.setup_moves.reverse_twists();
        result.extend(self.commutator_moves.reverse_twists());
        self.reset();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_drops_undos() {
        let t = |i, left| SingleTwist { identified: i, left_click: left, slice_mask: 1, ..Default::default() };
        let mut m = Macro::default();
        m.update(&t(1, true));
        assert!(m.twists().is_empty(), "not recording");
        m.start_recording();
        m.update(&t(1, true));
        m.update(&t(2, true));
        m.update(&t(2, false));
        assert_eq!(m.twists(), &[t(1, true)]);
        assert_eq!(m.reverse_twists(), vec![t(1, false)]);
    }

    #[test]
    fn mobius_round_trip() {
        let mut m = Mobius::default();
        m.isometry(r3::Geometry::Hyperbolic, 0.3, Complex::new(0.1, -0.2));
        let text = save_mobius(&m).to_pretty_string();
        let doc = roxmltree::Document::parse(&text).unwrap();
        assert_eq!(load_mobius(doc.root_element()).unwrap(), m);
    }
}
