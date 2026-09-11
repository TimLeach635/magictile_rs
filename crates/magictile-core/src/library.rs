//! The library of puzzles: class files plus `menu.xml`, organized into the menu tree the
//! original showed (mirroring its `MenuBuilder`).

use crate::config::{PuzzleConfig, PuzzleConfigClass};
use crate::xml;
use include_dir::{Dir, include_dir};
use roxmltree::Node;
use std::collections::HashMap;

/// The standard puzzle definitions, compiled in.
static CONFIG_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/config");

#[derive(Clone, Debug)]
pub struct MenuNode {
    /// Text for the puzzle tree.
    pub label: String,
    pub kind: MenuKind,
}

#[derive(Clone, Debug)]
pub enum MenuKind {
    Group(Vec<MenuNode>),
    /// An index into [`Library::configs`].
    Puzzle(usize),
}

impl MenuNode {
    fn group(label: impl Into<String>, children: Vec<MenuNode>) -> Self {
        MenuNode { label: label.into(), kind: MenuKind::Group(children) }
    }

    pub fn children(&self) -> &[MenuNode] {
        match &self.kind {
            MenuKind::Group(c) => c,
            MenuKind::Puzzle(_) => &[],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Library {
    pub configs: Vec<PuzzleConfig>,
    /// Top level: "Start Here!", the configured groups, then "User" (if there are user puzzles).
    pub root: Vec<MenuNode>,
    pub num_puzzles: usize,
    pub num_tilings: usize,
    by_id: HashMap<String, usize>,
    /// Problems encountered while loading (unparseable files, duplicate IDs).
    pub warnings: Vec<String>,
}

impl Library {
    /// Loads the built-in puzzles.
    pub fn load_standard() -> Library {
        Library::load(&[])
    }

    /// Loads the built-in puzzles plus user puzzle class files, given as (name, contents).
    pub fn load(user_files: &[(String, String)]) -> Library {
        let mut lib = Library::default();

        let mut standard_files: Vec<(&str, &str)> = Vec::new();
        collect_xml(CONFIG_DIR.get_dir("puzzles").expect("embedded puzzles dir"), &mut standard_files);
        standard_files.sort_by(|a, b| a.0.cmp(b.0));

        let standard = lib.parse_classes(standard_files.into_iter());
        let user = lib.parse_classes(user_files.iter().map(|(n, c)| (n.as_str(), c.as_str())));

        let menu_text = CONFIG_DIR.get_file("menu.xml").and_then(|f| f.contents_utf8()).expect("embedded menu.xml");
        let menu_doc = roxmltree::Document::parse(menu_text).expect("menu.xml is valid");
        let puzzle_menu = menu_doc.root_element().children().find(|c| c.is_element()).expect("menu has a root group");

        let mut root = Vec::new();
        for child in puzzle_menu.children().filter(|c| c.is_element()) {
            if let Some(node) = lib.build_menu_node(child, &standard) {
                root.push(node);
            }
        }

        // "Start Here!" refers to puzzles by ID, so it's filled in after everything else is loaded.
        let start = xml::child(puzzle_menu, "Start").map(|s| lib.start_here(s)).unwrap_or_default();
        root.insert(0, MenuNode::group("Start Here!", start));

        if !user.is_empty() {
            let groups = user.iter().map(|c| MenuNode::group(c.class_display_name.clone(), lib.add_class(c))).collect();
            root.push(MenuNode::group("User", groups));
        }

        lib.root = root;
        lib
    }

    pub fn config_by_id(&self, id: &str) -> Option<&PuzzleConfig> {
        self.by_id.get(id).map(|&i| &self.configs[i])
    }

    /// Finds a puzzle by ID, full display name, or (the first match of) its menu name with or
    /// without the parenthesised slicing parameters, e.g. "Professor's Cube".
    pub fn find(&self, name: &str) -> Option<&PuzzleConfig> {
        let short = |c: &&PuzzleConfig| c.menu_name == name || c.menu_name.split(" (").next() == Some(name);
        self.config_by_id(name)
            .or_else(|| self.configs.iter().find(|c| c.display_name == name))
            .or_else(|| self.configs.iter().find(short))
    }

    fn parse_classes<'a>(&mut self, files: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<PuzzleConfigClass> {
        let mut classes = Vec::new();
        for (name, contents) in files {
            match PuzzleConfigClass::parse(contents) {
                Ok(c) if c.is_view_only_irp() => {}
                Ok(c) => classes.push(c),
                Err(e) => self.warnings.push(format!("Failed to load puzzle config class {name}: {e}")),
            }
        }
        classes
    }

    fn build_menu_node(&mut self, node: Node, classes: &[PuzzleConfigClass]) -> Option<MenuNode> {
        match node.tag_name().name() {
            "Item" => {
                let class_id = xml::text(node);
                let class = classes.iter().find(|c| c.class_id.as_deref() == Some(class_id.as_str()))?;
                Some(MenuNode::group(class.class_display_name.clone(), self.add_class(class)))
            }
            "Start" => None,
            name => {
                let group_name = if name == "Group" { node.attribute("Name").unwrap_or_default() } else { name };
                let children = node
                    .children()
                    .filter(|c| c.is_element())
                    .filter_map(|c| self.build_menu_node(c, classes))
                    .collect();
                Some(MenuNode::group(group_name, children))
            }
        }
    }

    fn add_class(&mut self, class: &PuzzleConfigClass) -> Vec<MenuNode> {
        let groups = class.puzzles();
        let mut nodes = Vec::new();
        for tiling in groups.tilings {
            if tiling.coxeter_complex {
                self.num_tilings += 1;
            }
            nodes.push(self.add_config(tiling, None));
        }

        for (name, list) in [
            ("Face Twisting", groups.face),
            ("Edge Twisting", groups.edge),
            ("Vertex Twisting", groups.vertex),
            ("Systolic", groups.systolic),
            ("Mixed Twisting", groups.mixed),
            ("Lights On", groups.toggles),
        ] {
            if list.is_empty() {
                continue;
            }
            let children = list
                .into_iter()
                .map(|config| {
                    self.num_puzzles += 1;
                    let id = config.id.clone();
                    let node = self.add_config(config, None);
                    let MenuKind::Puzzle(index) = node.kind else { unreachable!() };
                    if self.by_id.insert(id.clone(), index).is_some() {
                        self.warnings.push(format!("Duplicate puzzle id: {id}"));
                    }
                    node
                })
                .collect();
            nodes.push(MenuNode::group(name, children));
        }
        nodes
    }

    fn add_config(&mut self, config: PuzzleConfig, label: Option<String>) -> MenuNode {
        let label = label.unwrap_or_else(|| config.menu_name.clone());
        self.configs.push(config);
        MenuNode { label, kind: MenuKind::Puzzle(self.configs.len() - 1) }
    }

    fn start_here(&self, node: Node) -> Vec<MenuNode> {
        let mut items = self.start_here_refs(node);
        for group in xml::children(node, "Group") {
            let name = group.attribute("Name").unwrap_or_default();
            items.push(MenuNode::group(name, self.start_here_refs(group)));
        }
        items
    }

    fn start_here_refs(&self, node: Node) -> Vec<MenuNode> {
        node.children()
            .filter(|c| c.is_element() && c.tag_name().name() != "Group")
            .filter_map(|r| {
                let id = xml::text(xml::child(r, "ID")?);
                let &index = self.by_id.get(&id)?;
                let label = xml::child(r, "DisplayName").map(xml::text).unwrap_or_default();
                Some(MenuNode { label, kind: MenuKind::Puzzle(index) })
            })
            .collect()
    }
}

fn collect_xml<'a>(dir: &'a Dir<'a>, out: &mut Vec<(&'a str, &'a str)>) {
    for f in dir.files() {
        if f.path().extension().is_some_and(|e| e == "xml")
            && let (Some(path), Some(contents)) = (f.path().to_str(), f.contents_utf8())
        {
            out.push((path, contents));
        }
    }
    for d in dir.dirs() {
        collect_xml(d, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(nodes: &'a [MenuNode], label: &str) -> Option<&'a MenuNode> {
        nodes.iter().find(|n| n.label == label)
    }

    #[test]
    fn loads_the_standard_library() {
        let lib = Library::load_standard();
        // The shipped {6,3} 7C file really does define this puzzle twice (the original only
        // asserted in debug builds, and the later one wins).
        assert_eq!(lib.warnings, vec!["Duplicate puzzle id: {6,3}.7 T0.01 F0.85:0:0 F0.75:0:1"]);
        assert!(lib.num_puzzles > 1000, "{}", lib.num_puzzles);
        assert!(lib.num_tilings > 100, "{}", lib.num_tilings);

        let start = find(&lib.root, "Start Here!").unwrap();
        let rubik = find(start.children(), "Rubik's Cube (F6 Toggles Surface View)").unwrap();
        let MenuKind::Puzzle(i) = rubik.kind else { panic!() };
        assert_eq!(lib.configs[i].id, "RubikCube");
        assert_eq!((lib.configs[i].p, lib.configs[i].q), (4, 3));

        let classics = find(start.children(), "Classics").unwrap();
        assert_eq!(classics.children().len(), 8);

        let hyperbolic = lib.config_by_id("Puzzle.{7,3}.Classic").unwrap();
        assert_eq!(hyperbolic.expected_num_colors, 24);
    }

    #[test]
    fn finds_puzzles_by_id_or_name() {
        let lib = Library::load_standard();
        let id = |name| lib.find(name).map(|c| c.id.as_str());
        assert_eq!(id("ProfessorsCube"), Some("ProfessorsCube"));
        assert_eq!(id("Cube Professor's Cube (F0.4:0:1 F0.8:0:1)"), Some("ProfessorsCube"));
        assert_eq!(id("Professor's Cube"), Some("ProfessorsCube"));
        assert_eq!(id("No Such Puzzle"), None);
    }

    #[test]
    fn reproduces_dropped_out_of_order_elements() {
        let lib = Library::load_standard();
        // This puzzle's DisplayName comes after its ID in the file, so the original ignores it.
        let klein = lib.config_by_id("Puzzle.{6,3}.Klein.F.9").unwrap();
        assert!(!klein.menu_name.contains('('), "{}", klein.menu_name);
    }

    #[test]
    fn hides_view_only_irp_classes() {
        let lib = Library::load_standard();
        assert!(!lib.configs.iter().any(|c| c.display_name.contains("runcrhomb")));
        assert!(
            lib.configs
                .iter()
                .all(|c| c.irp_config.is_none() || c.identifications.is_some() || c.group_relations.is_some())
        );
    }
}
