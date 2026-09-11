//! Puzzle configuration: what tiling, coloring and slicing define a puzzle.
//!
//! Mirrors the original's DataContract-serialized classes. Note that the .NET deserializer doesn't
//! run constructors, so anything missing from a file gets a zero/empty value, whereas objects
//! created in code get the constructor defaults (e.g. a slicing thickness of 0.01).

use crate::netfmt::{self, format_g, parse_bool, parse_double, parse_int};
use crate::xml::{self, XElement};
use r3::Geometry;
use r3::geometry2d;
use roxmltree::Node;

pub const VERSION_PREVIEW: &str = "2.0";
pub const VERSION_CURRENT: &str = "2.1";

/// How to identify one cell with another, via a sequence of reflections.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Identification {
    /// The initial edges to reflect across. If empty, all of them.
    pub initial_edges: Vec<i32>,
    /// Subsequent edges to reflect across to get from one cell to another.
    pub edges: Vec<i32>,
    /// First do an in-place reflection of the cell (keeping the initial edge in place). Allows some
    /// otherwise impossible colorings (e.g. an orientable {4,4} 9-color).
    pub in_place_reflection: bool,
    /// CCW symmetry rotation applied after the reflections (1 means 1/p of a turn).
    pub end_rotation: i32,
    /// Also use the mirrored list of edges (mirrored about the first segment's midpoint).
    pub use_mirrored_edge_set: bool,
}

impl Identification {
    pub fn new(edges: &[i32], end_rotation: i32, use_mirrored_edge_set: bool) -> Self {
        Identification { edges: edges.to_vec(), end_rotation, use_mirrored_edge_set, ..Default::default() }
    }

    const MEMBERS: &'static [&'static str] =
        &["EdgeSet", "EndRotation", "InPlaceReflection", "InitialEdgeSet", "UseMirroredEdgeSet"];

    fn read(node: Node) -> Self {
        let mut r = Identification::default();
        for (i, n) in xml::data_contract_members(node, Self::MEMBERS) {
            let t = xml::text(n);
            match i {
                0 => r.edges = load_int_array(&t),
                1 => r.end_rotation = parse_int(&t).unwrap_or(0),
                2 => r.in_place_reflection = parse_bool(&t).unwrap_or(false),
                3 => r.initial_edges = load_int_array(&t),
                _ => r.use_mirrored_edge_set = parse_bool(&t).unwrap_or(false),
            }
        }
        r
    }

    fn write(&self) -> XElement {
        XElement::new("Identification")
            .child(XElement::with_text("EdgeSet", save_int_array(&self.edges)))
            .child(XElement::with_text("EndRotation", self.end_rotation.to_string()))
            .child(XElement::with_text("InPlaceReflection", self.in_place_reflection.to_string()))
            .child(XElement::with_text("InitialEdgeSet", save_int_array(&self.initial_edges)))
            .child(XElement::with_text("UseMirroredEdgeSet", self.use_mirrored_edge_set.to_string()))
    }
}

fn load_int_array(s: &str) -> Vec<i32> {
    s.split(':').filter(|p| !p.is_empty()).filter_map(parse_int).collect()
}

fn save_int_array(v: &[i32]) -> String {
    v.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(":")
}

/// A distance in terms of the edges of the (2,p,q) triangle (plus an absolute amount), measured in
/// the tiling's geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Distance {
    /// Multiplier for the triangle edge opposite the PI/p angle.
    pub p: f64,
    /// Multiplier for the triangle edge opposite the PI/q angle.
    pub q: f64,
    /// Multiplier for the triangle edge opposite the right angle.
    pub r: f64,
    /// An absolute distance.
    pub d: f64,
}

impl Distance {
    pub fn new(p: f64, q: f64, r: f64, d: f64) -> Self {
        Distance { p, q, r, d }
    }

    pub fn is_zero(&self) -> bool {
        r3::tolerance::zero(self.p)
            && r3::tolerance::zero(self.q)
            && r3::tolerance::zero(self.r)
            && r3::tolerance::zero(self.d)
    }

    /// The distance for a particular {p,q} tiling.
    pub fn dist(&self, p: i32, q: i32) -> f64 {
        self.p * geometry2d::triangle_p_side(p, q)
            + self.q * geometry2d::triangle_q_side(p, q)
            + self.r * geometry2d::triangle_hypotenuse(p, q)
            + self.d
    }

    pub fn save_to_string_short(&self) -> String {
        if self.d == 0.0 {
            format!("{}:{}:{}", format_g(self.p), format_g(self.q), format_g(self.r))
        } else {
            self.save_to_string()
        }
    }

    pub fn save_to_string(&self) -> String {
        format!("{}:{}:{}:{}", format_g(self.p), format_g(self.q), format_g(self.r), format_g(self.d))
    }

    /// Parses "p:q:r:d". Anything else leaves the distance zero (as the original does).
    pub fn load_from_string(saved: &str) -> Self {
        let split: Vec<&str> = saved.split(':').collect();
        if split.len() != 4 {
            return Distance::default();
        }
        let v: Vec<f64> = split.iter().map(|s| parse_double(s).unwrap_or(0.0)).collect();
        Distance::new(v[0], v[1], v[2], v[3])
    }

    const MEMBERS: &'static [&'static str] = &["D", "P", "Q", "R"];

    fn read_object(node: Node) -> Self {
        let mut r = Distance::default();
        for (i, n) in xml::data_contract_members(node, Self::MEMBERS) {
            let v = parse_double(&xml::text(n)).unwrap_or(0.0);
            match i {
                0 => r.d = v,
                1 => r.p = v,
                2 => r.q = v,
                _ => r.r = v,
            }
        }
        r
    }

    fn write_object(&self, name: &str) -> XElement {
        XElement::new(name)
            .child(XElement::with_text("D", netfmt::format_xml_double(self.d)))
            .child(XElement::with_text("P", netfmt::format_xml_double(self.p)))
            .child(XElement::with_text("Q", netfmt::format_xml_double(self.q)))
            .child(XElement::with_text("R", netfmt::format_xml_double(self.r)))
    }
}

/// The slicing circles for the template tile.
#[derive(Clone, Debug, PartialEq)]
pub struct SlicingCircles {
    pub face_centered: Vec<Distance>,
    pub edge_centered: Vec<Distance>,
    pub vertex_centered: Vec<Distance>,
    pub systolic: Vec<Distance>,
    /// Thickness of the cuts, in the tiling's geometry.
    pub thickness: f64,
}

impl Default for SlicingCircles {
    /// The constructor default (thickness 0.01).
    fn default() -> Self {
        SlicingCircles { thickness: 0.01, ..SlicingCircles::zeroed() }
    }
}

impl SlicingCircles {
    /// What the deserializer produces for missing values.
    fn zeroed() -> Self {
        SlicingCircles {
            face_centered: Vec::new(),
            edge_centered: Vec::new(),
            vertex_centered: Vec::new(),
            systolic: Vec::new(),
            thickness: 0.0,
        }
    }

    pub fn face_twisting(&self) -> bool {
        !self.face_centered.is_empty()
    }
    pub fn edge_twisting(&self) -> bool {
        !self.edge_centered.is_empty()
    }
    pub fn vertex_twisting(&self) -> bool {
        !self.vertex_centered.is_empty()
    }
    pub fn systolic_twisting(&self) -> bool {
        !self.systolic.is_empty()
    }

    pub fn face_twisting_only(&self) -> bool {
        self.face_twisting() && !self.edge_twisting() && !self.vertex_twisting() && !self.systolic_twisting()
    }
    pub fn edge_twisting_only(&self) -> bool {
        !self.face_twisting() && self.edge_twisting() && !self.vertex_twisting() && !self.systolic_twisting()
    }
    pub fn vertex_twisting_only(&self) -> bool {
        !self.face_twisting() && !self.edge_twisting() && self.vertex_twisting() && !self.systolic_twisting()
    }
    pub fn systolic_twisting_only(&self) -> bool {
        !self.face_twisting() && !self.edge_twisting() && !self.vertex_twisting() && self.systolic_twisting()
    }

    pub fn sliced(&self) -> bool {
        self.face_twisting() || self.edge_twisting() || self.vertex_twisting() || self.systolic_twisting()
    }

    const MEMBERS: &'static [&'static str] =
        &["EdgeCentered", "FaceCentered", "Systolic", "Thickness", "VertexCentered"];

    fn read(node: Node) -> Self {
        let mut r = SlicingCircles::zeroed();
        let list = |n: Node| -> Vec<Distance> {
            xml::children(n, "Distance").map(|d| Distance::load_from_string(&xml::text(d))).collect()
        };
        for (i, n) in xml::data_contract_members(node, Self::MEMBERS) {
            match i {
                0 => r.edge_centered = list(n),
                1 => r.face_centered = list(n),
                2 => r.systolic = list(n),
                3 => r.thickness = parse_double(&xml::text(n)).unwrap_or(0.0),
                _ => r.vertex_centered = list(n),
            }
        }
        r
    }

    fn write(&self) -> XElement {
        let list = |name: &str, v: &[Distance]| {
            let mut e = XElement::new(name);
            for d in v {
                e.push(XElement::with_text("Distance", d.save_to_string()));
            }
            e
        };
        XElement::new("SlicingCircles")
            .child(list("EdgeCentered", &self.edge_centered))
            .child(list("FaceCentered", &self.face_centered))
            .child(list("Systolic", &self.systolic))
            .child(XElement::with_text("Thickness", netfmt::format_xml_double(self.thickness)))
            .child(list("VertexCentered", &self.vertex_centered))
    }
}

/// An IRP (infinite regular polyhedron) to associate with a puzzle. Unused in 2D, but kept so it
/// round-trips through saved files.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IrpConfig {
    pub data_file: Option<String>,
    pub first_tile: i32,
    pub reflect: bool,
    pub rotate: i32,
}

impl IrpConfig {
    const MEMBERS: &'static [&'static str] = &["DataFile", "FirstTile", "Reflect", "Rotate"];

    fn read(node: Node) -> Self {
        let mut r = IrpConfig::default();
        for (i, n) in xml::data_contract_members(node, Self::MEMBERS) {
            let t = xml::text(n);
            match i {
                0 => r.data_file = if xml::is_nil(n) { None } else { Some(t) },
                1 => r.first_tile = parse_int(&t).unwrap_or(0),
                2 => r.reflect = parse_bool(&t).unwrap_or(false),
                _ => r.rotate = parse_int(&t).unwrap_or(0),
            }
        }
        r
    }

    fn write(&self) -> XElement {
        XElement::new("IRPConfig")
            .child(match &self.data_file {
                Some(f) => XElement::with_text("DataFile", f),
                None => XElement::nil("DataFile"),
            })
            .child(XElement::with_text("FirstTile", self.first_tile.to_string()))
            .child(XElement::with_text("Reflect", self.reflect.to_string()))
            .child(XElement::with_text("Rotate", self.rotate.to_string()))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Surface {
    #[default]
    None,
    Sphere,
    Boys,
    CliffordTorus,
    LawsonKleinBottle,
}

impl Surface {
    const NAMES: &'static [(&'static str, Surface)] = &[
        ("None", Surface::None),
        ("Sphere", Surface::Sphere),
        ("Boys", Surface::Boys),
        ("CliffordTorus", Surface::CliffordTorus),
        ("LawsonKleinBottle", Surface::LawsonKleinBottle),
    ];
}

/// Rendering a puzzle on a compact surface. Unused in 2D, but kept so it round-trips.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceConfig {
    pub surface: Surface,
    /// For locally Euclidean puzzles: a rhombus with two basis vectors.
    pub basis1_x: Option<Distance>,
    pub basis1_y: Option<Distance>,
    pub basis2_x: Option<Distance>,
    pub basis2_y: Option<Distance>,
}

impl SurfaceConfig {
    pub fn configured(&self) -> bool {
        self.surface != Surface::None
    }

    const MEMBERS: &'static [&'static str] = &["Basis1X", "Basis1Y", "Basis2X", "Basis2Y", "Surface"];

    fn read(node: Node) -> Self {
        let mut r = SurfaceConfig::default();
        let dist = |n: Node| if xml::is_nil(n) { None } else { Some(Distance::read_object(n)) };
        for (i, n) in xml::data_contract_members(node, Self::MEMBERS) {
            match i {
                0 => r.basis1_x = dist(n),
                1 => r.basis1_y = dist(n),
                2 => r.basis2_x = dist(n),
                3 => r.basis2_y = dist(n),
                _ => r.surface = read_enum(n, Surface::NAMES).unwrap_or_default(),
            }
        }
        r
    }

    fn write(&self) -> XElement {
        let dist = |name: &str, d: &Option<Distance>| match d {
            Some(d) => d.write_object(name),
            None => XElement::nil(name),
        };
        XElement::new("SurfaceConfig")
            .child(dist("Basis1X", &self.basis1_x))
            .child(dist("Basis1Y", &self.basis1_y))
            .child(dist("Basis2X", &self.basis2_x))
            .child(dist("Basis2Y", &self.basis2_y))
            .child(XElement::with_text("Surface", enum_name(self.surface, Surface::NAMES)))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Polytope {
    #[default]
    Duoprism,
    Runcinated5Cell,
    Bitruncated5Cell,
}

impl Polytope {
    const NAMES: &'static [(&'static str, Polytope)] = &[
        ("Duoprism", Polytope::Duoprism),
        ("Runcinated5Cell", Polytope::Runcinated5Cell),
        ("Bitruncated5Cell", Polytope::Bitruncated5Cell),
    ];
}

/// A regular 4D skew polyhedron to associate with a puzzle. Unused in 2D, but kept so it
/// round-trips.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skew4DConfig {
    pub polytope: Polytope,
}

impl Skew4DConfig {
    fn read(node: Node) -> Self {
        let mut r = Skew4DConfig::default();
        for (_, n) in xml::data_contract_members(node, &["Polytope"]) {
            r.polytope = read_enum(n, Polytope::NAMES).unwrap_or_default();
        }
        r
    }

    fn write(&self) -> XElement {
        XElement::new("Skew4DConfig").child(XElement::with_text("Polytope", enum_name(self.polytope, Polytope::NAMES)))
    }
}

fn read_enum<T: Copy>(n: Node, names: &[(&str, T)]) -> Option<T> {
    let t = xml::text(n);
    names.iter().find(|(name, _)| *name == t.trim()).map(|(_, v)| *v)
}

fn enum_name<T: PartialEq>(v: T, names: &[(&'static str, T)]) -> &'static str {
    names.iter().find(|(_, x)| *x == v).map(|(n, _)| *n).unwrap_or("")
}

/// "Lights on" puzzles, where clicking toggles tiles instead of twisting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TogglingMode {
    NeighborsOnly,
    NeighborsAndSelf,
}

/// Everything needed to build a puzzle.
#[derive(Clone, Debug, PartialEq)]
pub struct PuzzleConfig {
    /// Not saved; set by the loader.
    pub version: String,
    pub id: String,
    /// For menus and the title bar.
    pub display_name: String,
    /// A subset of the display name for menus and the tree. Not saved.
    pub menu_name: String,
    /// Just draw a Coxeter complex (no twisting). Not saved.
    pub coxeter_complex: bool,
    /// Set for "lights on" puzzles. Not saved.
    pub toggling_mode: Option<TogglingMode>,
    /// Sides of a polygonal face.
    pub p: i32,
    /// Polygons meeting at each vertex.
    pub q: i32,
    pub slicing_circles: SlicingCircles,
    /// Gap between tiles, applied before slicing (1.0 means none). Part of the puzzle definition.
    pub tile_shrink: f64,
    /// Identifications taking master cells to slaves. `None` if not set (versus empty).
    pub identifications: Option<Vec<Identification>>,
    /// A more succinct alternative to `identifications`: a regular map presentation from
    /// https://www.math.auckland.ac.nz/~conder/OrientableRegularMaps101.txt
    pub group_relations: Option<String>,
    /// Checked during building; cells beyond this are dropped.
    pub expected_num_colors: i32,
    pub num_tiles: i32,
    pub surface_config: Option<SurfaceConfig>,
    pub irp_config: Option<IrpConfig>,
    pub skew4d_config: Option<Skew4DConfig>,
}

impl Default for PuzzleConfig {
    /// The constructor default: the {7,3} Classic puzzle.
    fn default() -> Self {
        let mut slicing_circles = SlicingCircles::default();
        slicing_circles.face_centered.push(Distance::new(2.0 / 3.0, 0.0, 1.0, 0.0));
        PuzzleConfig {
            version: VERSION_CURRENT.into(),
            id: "Puzzle.{7,3}.Classic".into(), // Needs to be coordinated with the {7,3} config.
            display_name: "{7,3} Classic".into(),
            menu_name: String::new(),
            coxeter_complex: false,
            toggling_mode: None,
            p: 7,
            q: 3,
            slicing_circles,
            tile_shrink: 0.94,
            identifications: Some(vec![Identification::new(&[3, 3, 3], 0, true)]),
            group_relations: None,
            expected_num_colors: 24,
            num_tiles: 5000,
            surface_config: None,
            irp_config: None,
            skew4d_config: None,
        }
    }
}

impl PuzzleConfig {
    pub fn geometry(&self) -> Geometry {
        Geometry::from_pq(self.p, self.q)
    }

    pub fn is_toggling(&self) -> bool {
        self.toggling_mode.is_some()
    }

    pub fn systolic(&self) -> bool {
        self.slicing_circles.systolic_twisting()
    }

    /// An "Earthquake" puzzle: systolic, with a single zero-width (geodesic) slice.
    pub fn earthquake(&self) -> bool {
        self.systolic() && self.slicing_circles.systolic.len() == 1 && self.slicing_circles.systolic[0].is_zero()
    }

    pub fn edge_or_vertex_twisting(&self) -> bool {
        self.slicing_circles.edge_twisting() || self.slicing_circles.vertex_twisting()
    }

    pub fn using_relations(&self) -> bool {
        self.group_relations.as_deref().is_some_and(|s| !s.is_empty())
    }

    pub fn has_valid_irp_config(&self) -> bool {
        self.irp_config.as_ref().is_some_and(|c| c.data_file.as_deref().is_some_and(|f| !f.is_empty()))
    }

    pub fn has_surface_config(&self) -> bool {
        self.surface_config.as_ref().is_some_and(|c| c.configured())
    }

    const MEMBERS: &'static [&'static str] = &[
        "DisplayName",
        "ExpectedNumColors",
        "GroupRelations",
        "ID",
        "IRPConfig",
        "Identifications",
        "NumTiles",
        "P",
        "Q",
        "Skew4DConfig",
        "SlicingCircles",
        "SurfaceConfig",
        "TileShrink",
    ];

    /// Reads a DataContract-serialized config (as found in saved puzzle files). Missing values are
    /// zero/empty, since the original deserializer skips constructors.
    pub fn read(node: Node) -> Self {
        let mut r = PuzzleConfig {
            version: VERSION_CURRENT.into(),
            id: String::new(),
            display_name: String::new(),
            menu_name: String::new(),
            coxeter_complex: false,
            toggling_mode: None,
            p: 0,
            q: 0,
            slicing_circles: SlicingCircles::zeroed(),
            tile_shrink: 0.0,
            identifications: None,
            group_relations: None,
            expected_num_colors: 0,
            num_tiles: 0,
            surface_config: None,
            irp_config: None,
            skew4d_config: None,
        };
        for (i, n) in xml::data_contract_members(node, Self::MEMBERS) {
            let nil = xml::is_nil(n);
            let t = xml::text(n);
            match i {
                0 => r.display_name = t,
                1 => r.expected_num_colors = parse_int(&t).unwrap_or(0),
                2 => r.group_relations = (!nil).then_some(t),
                3 => r.id = t,
                4 => r.irp_config = (!nil).then(|| IrpConfig::read(n)),
                5 => r.identifications = (!nil).then(|| read_identifications(n)),
                6 => r.num_tiles = parse_int(&t).unwrap_or(0),
                7 => r.p = parse_int(&t).unwrap_or(0),
                8 => r.q = parse_int(&t).unwrap_or(0),
                9 => r.skew4d_config = (!nil).then(|| Skew4DConfig::read(n)),
                10 => r.slicing_circles = SlicingCircles::read(n),
                11 => r.surface_config = (!nil).then(|| SurfaceConfig::read(n)),
                _ => r.tile_shrink = parse_double(&t).unwrap_or(0.0),
            }
        }
        r
    }

    /// Writes the DataContract form used in saved puzzle files.
    pub fn write(&self) -> XElement {
        let opt = |name: &str, e: Option<XElement>| e.unwrap_or_else(|| XElement::nil(name));
        XElement::new("PuzzleConfig")
            .attr("xmlns:i", xml::XSI_NS)
            .child(XElement::with_text("DisplayName", &self.display_name))
            .child(XElement::with_text("ExpectedNumColors", self.expected_num_colors.to_string()))
            .child(opt(
                "GroupRelations",
                self.group_relations.as_ref().map(|g| XElement::with_text("GroupRelations", g)),
            ))
            .child(XElement::with_text("ID", &self.id))
            .child(opt("IRPConfig", self.irp_config.as_ref().map(|c| c.write())))
            .child(opt("Identifications", self.identifications.as_ref().map(|ids| write_identifications(ids))))
            .child(XElement::with_text("NumTiles", self.num_tiles.to_string()))
            .child(XElement::with_text("P", self.p.to_string()))
            .child(XElement::with_text("Q", self.q.to_string()))
            .child(opt("Skew4DConfig", self.skew4d_config.as_ref().map(|c| c.write())))
            .child(self.slicing_circles.write())
            .child(opt("SurfaceConfig", self.surface_config.as_ref().map(|c| c.write())))
            .child(XElement::with_text("TileShrink", netfmt::format_xml_double(self.tile_shrink)))
    }
}

fn read_identifications(n: Node) -> Vec<Identification> {
    xml::children(n, "Identification").map(Identification::read).collect()
}

fn write_identifications(ids: &[Identification]) -> XElement {
    let mut e = XElement::new("Identifications");
    for id in ids {
        e.push(id.write());
    }
    e
}

/// Puzzle-specific settings within a puzzle class.
#[derive(Clone, Debug, PartialEq)]
pub struct PuzzleSpecific {
    /// If empty, auto-generated. Only set for backward compatibility of macros with old IDs.
    pub id: Option<String>,
    /// If empty, auto-generated.
    pub display_name: Option<String>,
    pub slicing_circles: SlicingCircles,
}

impl PuzzleSpecific {
    const MEMBERS: &'static [&'static str] = &["DisplayName", "ID", "SlicingCircles"];

    fn read(node: Node) -> Self {
        let mut r = PuzzleSpecific { id: None, display_name: None, slicing_circles: SlicingCircles::zeroed() };
        for (i, n) in xml::data_contract_members(node, Self::MEMBERS) {
            match i {
                0 => r.display_name = Some(xml::text(n)),
                1 => r.id = Some(xml::text(n)),
                _ => r.slicing_circles = SlicingCircles::read(n),
            }
        }
        r
    }

    /// Unique up to slicing circles (class IDs take care of the rest).
    pub fn auto_unique_id(&self) -> String {
        format!("T{}{}", format_g(self.slicing_circles.thickness), self.circles_to_string())
    }

    /// Things like "F1:1:0 E0:1:0 V0.6:2:2".
    pub fn auto_display_name(&self) -> String {
        self.circles_to_string().trim_start().to_string()
    }

    fn circles_to_string(&self) -> String {
        let s = &self.slicing_circles;
        let mut result = String::new();
        for (prefix, list) in
            [("F", &s.face_centered), ("E", &s.edge_centered), ("V", &s.vertex_centered), ("S", &s.systolic)]
        {
            for d in list {
                result += &format!(" {prefix}{}", d.save_to_string_short());
            }
        }
        result
    }
}

/// A class of puzzles sharing a tiling and coloring (one file in the config directory), with
/// many slicing variations.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PuzzleConfigClass {
    pub class_id: Option<String>,
    pub class_display_name: String,
    pub p: i32,
    pub q: i32,
    pub puzzle_specific_list: Vec<PuzzleSpecific>,
    pub tile_shrink: f64,
    pub identifications: Option<Vec<Identification>>,
    pub group_relations: Option<String>,
    pub expected_num_colors: i32,
    pub num_tiles: i32,
    pub surface_config: Option<SurfaceConfig>,
    pub irp_config: Option<IrpConfig>,
    pub skew4d_config: Option<Skew4DConfig>,
}

/// The puzzles in a class, grouped as the menus show them.
#[derive(Clone, Debug, Default)]
pub struct PuzzleGroups {
    /// The plain tiling and the Coxeter complex.
    pub tilings: Vec<PuzzleConfig>,
    pub face: Vec<PuzzleConfig>,
    pub edge: Vec<PuzzleConfig>,
    pub vertex: Vec<PuzzleConfig>,
    pub mixed: Vec<PuzzleConfig>,
    pub systolic: Vec<PuzzleConfig>,
    pub toggles: Vec<PuzzleConfig>,
}

impl PuzzleConfigClass {
    const MEMBERS: &'static [&'static str] = &[
        "ClassDisplayName",
        "ClassID",
        "ExpectedNumColors",
        "GroupRelations",
        "IRPConfig",
        "Identifications",
        "NumTiles",
        "P",
        "Q",
        "Skew4DConfig",
        "Specific",
        "SurfaceConfig",
        "TileShrink",
    ];

    pub fn parse(xml_text: &str) -> Result<Self, roxmltree::Error> {
        let doc = roxmltree::Document::parse(xml_text)?;
        Ok(Self::read(doc.root_element()))
    }

    /// Reads a puzzle class file (the root element's name isn't checked, as in the original).
    pub fn read(node: Node) -> Self {
        let mut r = PuzzleConfigClass::default();
        for (i, n) in xml::data_contract_members(node, Self::MEMBERS) {
            let nil = xml::is_nil(n);
            let t = xml::text(n);
            match i {
                0 => r.class_display_name = t,
                1 => r.class_id = (!nil).then_some(t),
                2 => r.expected_num_colors = parse_int(&t).unwrap_or(0),
                3 => r.group_relations = (!nil).then_some(t),
                4 => r.irp_config = (!nil).then(|| IrpConfig::read(n)),
                5 => r.identifications = (!nil).then(|| read_identifications(n)),
                6 => r.num_tiles = parse_int(&t).unwrap_or(0),
                7 => r.p = parse_int(&t).unwrap_or(0),
                8 => r.q = parse_int(&t).unwrap_or(0),
                9 => r.skew4d_config = (!nil).then(|| Skew4DConfig::read(n)),
                10 => r.puzzle_specific_list = xml::children(n, "Puzzle").map(PuzzleSpecific::read).collect(),
                11 => r.surface_config = (!nil).then(|| SurfaceConfig::read(n)),
                _ => r.tile_shrink = parse_double(&t).unwrap_or(0.0),
            }
        }
        r
    }

    pub fn geometry(&self) -> Geometry {
        Geometry::from_pq(self.p, self.q)
    }

    /// True for the IRP classes meant only for viewing as 3D polyhedra (no 2D coloring), which
    /// we don't show since we only support 2D.
    pub fn is_view_only_irp(&self) -> bool {
        self.irp_config.is_some()
            && self.identifications.as_ref().is_none_or(|ids| ids.is_empty())
            && self.group_relations.as_deref().is_none_or(|g| g.is_empty())
    }

    /// All the puzzles in this class.
    pub fn puzzles(&self) -> PuzzleGroups {
        let class_id = self.class_id.clone().unwrap_or_default();

        let mut tiling = self.non_specific();
        tiling.menu_name = "Tiling".into();
        tiling.display_name = format!("{} {}", self.class_display_name, tiling.menu_name);

        let mut coxeter = self.non_specific();
        coxeter.coxeter_complex = true;
        coxeter.menu_name = "Coxeter Complex".into();
        coxeter.display_name = format!("{} {}", self.class_display_name, coxeter.menu_name);
        coxeter.slicing_circles.thickness = 0.01;

        let toggles = [
            ("Toggling Neighbors", TogglingMode::NeighborsOnly),
            ("Toggling Clicked Tile And Neighbors", TogglingMode::NeighborsAndSelf),
        ]
        .into_iter()
        .map(|(name, mode)| {
            let mut c = self.non_specific();
            c.menu_name = name.into();
            c.toggling_mode = Some(mode);
            c.display_name = format!("{} {}", self.class_display_name, name);
            c.id = format!("{class_id}{name}");
            c
        })
        .collect();

        let mut puzzles = Vec::new();
        for specific in &self.puzzle_specific_list {
            let mut config = self.non_specific();
            config.slicing_circles = specific.slicing_circles.clone();

            // Old IDs aren't propagated for edge or vertex turning puzzles (their preview-version
            // macros can't be loaded anyway, due to build changes from 2.0 -> 2.1).
            config.id = match &specific.id {
                Some(id) if !id.is_empty() && !config.edge_or_vertex_twisting() => id.clone(),
                _ => format!("{} {}", class_id, specific.auto_unique_id()),
            };

            config.menu_name = match &specific.display_name {
                Some(name) if !name.is_empty() => format!("{} ({})", name, specific.auto_display_name()),
                _ => specific.auto_display_name(),
            };
            config.display_name = format!("{} {}", self.class_display_name, config.menu_name);
            puzzles.push(config);
        }

        let mut groups = PuzzleGroups { tilings: vec![tiling, coxeter], toggles, ..Default::default() };
        for config in puzzles {
            let s = &config.slicing_circles;
            let target = if s.face_twisting_only() {
                &mut groups.face
            } else if s.edge_twisting_only() {
                &mut groups.edge
            } else if s.vertex_twisting_only() {
                &mut groups.vertex
            } else if s.systolic_twisting_only() {
                &mut groups.systolic
            } else {
                &mut groups.mixed
            };
            target.push(config);
        }
        groups
    }

    /// The class-level parts of a config. (The ID stays the constructor default, as in the
    /// original, for tilings.)
    fn non_specific(&self) -> PuzzleConfig {
        PuzzleConfig {
            p: self.p,
            q: self.q,
            tile_shrink: self.tile_shrink,
            identifications: self.identifications.clone(),
            group_relations: self.group_relations.clone(),
            expected_num_colors: self.expected_num_colors,
            num_tiles: self.num_tiles,
            surface_config: self.surface_config.clone(),
            irp_config: self.irp_config.clone(),
            skew4d_config: self.skew4d_config.clone(),
            slicing_circles: SlicingCircles::default(),
            ..PuzzleConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<PuzzleConfigClass>
   <ClassDisplayName>{4,4|3} 9-Color (Duoprism)</ClassDisplayName>
   <ClassID>Skew.{4,4}.9</ClassID>
   <ExpectedNumColors>9</ExpectedNumColors>
   <Identifications>
      <Identification>
         <EdgeSet>2:2</EdgeSet>
         <EndRotation>2</EndRotation>
         <InPlaceReflection>true</InPlaceReflection>
         <UseMirroredEdgeSet>true</UseMirroredEdgeSet>
      </Identification>
   </Identifications>
   <NumTiles>2500</NumTiles>
   <P>4</P>
   <Q>4</Q>
   <Specific>
      <Puzzle>
         <SlicingCircles>
            <EdgeCentered/>
            <FaceCentered><Distance>0.33:0:1:0</Distance></FaceCentered>
            <Thickness>0.01</Thickness>
            <VertexCentered/>
         </SlicingCircles>
      </Puzzle>
      <Edge/>
      <Puzzle>
         <DisplayName>Harlequin</DisplayName>
         <SlicingCircles>
            <EdgeCentered><Distance>0:1:0:0</Distance></EdgeCentered>
            <FaceCentered/>
            <Thickness>0.01</Thickness>
            <VertexCentered/>
         </SlicingCircles>
      </Puzzle>
   </Specific>
   <TileShrink>0.94</TileShrink>
</PuzzleConfigClass>"#;

    #[test]
    fn reads_class_and_generates_puzzles() {
        let c = PuzzleConfigClass::parse(SAMPLE).unwrap();
        assert_eq!(c.class_id.as_deref(), Some("Skew.{4,4}.9"));
        assert_eq!(c.p, 4);
        let ids = c.identifications.as_ref().unwrap();
        assert_eq!(ids[0].edges, vec![2, 2]);
        assert!(ids[0].in_place_reflection && ids[0].use_mirrored_edge_set);
        assert!(ids[0].initial_edges.is_empty());

        let groups = c.puzzles();
        assert_eq!(groups.face.len(), 1);
        assert_eq!(groups.edge.len(), 1);
        assert_eq!(groups.face[0].id, "Skew.{4,4}.9 T0.01 F0.33:0:1");
        assert_eq!(groups.face[0].menu_name, "F0.33:0:1");
        assert_eq!(groups.edge[0].menu_name, "Harlequin (E0:1:0)");
        assert_eq!(groups.edge[0].display_name, "{4,4|3} 9-Color (Duoprism) Harlequin (E0:1:0)");
        assert_eq!(groups.toggles[0].id, "Skew.{4,4}.9Toggling Neighbors");
        // Tilings keep the constructor's default ID, as in the original.
        assert_eq!(groups.tilings[0].id, "Puzzle.{7,3}.Classic");
    }

    #[test]
    fn config_round_trips() {
        let c = PuzzleConfigClass::parse(SAMPLE).unwrap();
        let mut config = c.puzzles().face[0].clone();
        config.surface_config = Some(SurfaceConfig {
            surface: Surface::CliffordTorus,
            basis1_x: Some(Distance::new(1.0, 0.0, 0.5, 0.0)),
            ..Default::default()
        });
        config.group_relations = Some("[R^4, S^4]".into());
        let text = config.write().to_pretty_string();
        let doc = roxmltree::Document::parse(&text).unwrap();
        let mut back = PuzzleConfig::read(doc.root_element());
        // Not persisted:
        back.menu_name = config.menu_name.clone();
        assert_eq!(back, config);
    }

    #[test]
    fn default_config_ids() {
        let d = PuzzleConfig::default();
        assert_eq!(d.slicing_circles.face_centered[0].save_to_string(), "0.666666666666667:0:1:0");
    }
}
