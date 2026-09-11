//! Saving and loading puzzle logs (config, state, twist history and macros) and macro files, in
//! the original program's XML formats.

use crate::config::{self, PuzzleConfig};
use crate::macros::MacroList;
use crate::puzzle::Puzzle;
use crate::xml::{self, XElement};

const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n";

/// A parsed log file. Build a puzzle from `config`, then call [`SavedLog::apply`].
#[derive(Clone, Debug)]
pub struct SavedLog {
    pub config: PuzzleConfig,
    text: String,
}

/// Reads a saved puzzle log.
pub fn read_log(text: &str) -> Result<SavedLog, String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    let config_node = xml::child(root, "PuzzleConfig").ok_or("not a MagicTile log (no PuzzleConfig)")?;
    let mut config = PuzzleConfig::read(config_node);

    // The puzzle building changed between versions 2.0 and 2.1 (for edge and vertex turning), so
    // the saved version controls how we build. (The original crashed on files with no version.)
    config.version = root.attribute("Version").unwrap_or(config::VERSION_PREVIEW).to_string();

    Ok(SavedLog { config, text: text.to_string() })
}

impl SavedLog {
    /// Loads the saved state, history and macros into a puzzle built from our config.
    pub fn apply(&self, puzzle: &mut Puzzle) -> Result<MacroList, String> {
        let doc = roxmltree::Document::parse(&self.text).map_err(|e| e.to_string())?;
        let root = doc.root_element();
        let num_twist_data = puzzle.all_twist_data.len();

        if let Some(state) = xml::child(root, "State") {
            puzzle.state.load(state)?;
        }
        if let Some(history) = xml::child(root, "History") {
            puzzle.history.load(history, num_twist_data)?;
        }

        // Older files have no macros.
        match xml::child(root, "Macros") {
            Some(macros) => MacroList::load(macros, num_twist_data),
            None => Ok(MacroList::default()),
        }
    }
}

/// Writes a puzzle log.
pub fn write_log(puzzle: &Puzzle, macros: &MacroList) -> String {
    let root = XElement::new("MagicTileLog")
        .attr("Version", &puzzle.config.version)
        .child(puzzle.config.write())
        .child(puzzle.state.save())
        .child(puzzle.history.save())
        .child(macros.save("Macros", puzzle));
    format!("{XML_DECLARATION}{}", root.to_pretty_string())
}

/// Writes a macro file.
pub fn write_macros(puzzle: &Puzzle, macros: &MacroList) -> String {
    let root = macros.save("MagicTileMacros", puzzle).attr("Version", &puzzle.config.version);
    format!("{XML_DECLARATION}{}", root.to_pretty_string())
}

/// Reads macros for a puzzle, from a macro file or a saved log. Only macros saved for the same
/// puzzle (and, for edge/vertex turning puzzles, the same version) are accepted.
pub fn read_macros(text: &str, puzzle: &Puzzle) -> Result<MacroList, String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let mut root = doc.root_element();

    // The version is checked before the ID, because edge and vertex turning puzzles changed both
    // from 2.0 -> 2.1, and the version message makes more sense.
    let saved_version = root.attribute("Version").unwrap_or("");
    if puzzle.config.edge_or_vertex_twisting() && saved_version != puzzle.config.version {
        return Err(format!(
            "Sorry, this macro file is not compatible with this puzzle. The macro file was saved with \
             version {saved_version}, but the puzzle is version {}",
            puzzle.config.version
        ));
    }

    // Saved logs keep their macros in a child element.
    if let Some(macros) = xml::child(root, "Macros") {
        root = macros;
    }

    let puzzle_id = xml::child(root, "PuzzleID").map(xml::text).unwrap_or_default();
    if puzzle_id != puzzle.config.id {
        let puzzle_name = xml::child(root, "PuzzleName").map(xml::text).unwrap_or_default();
        return Err(format!(
            "Sorry, we only support loading macro files which apply to the active puzzle. These macros \
             were saved for the puzzle with display name '{puzzle_name}' and having id '{puzzle_id}'"
        ));
    }

    MacroList::load(root, puzzle.all_twist_data.len())
}
