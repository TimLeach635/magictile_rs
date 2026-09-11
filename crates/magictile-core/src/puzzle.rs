//! Building a puzzle from its configuration, and updating its state with twists.
//!
//! A faithful port of the original's `Puzzle.Build`: the order in which cells and twist data are
//! created determines the twist indices stored in saved files, so the steps (and the .NET
//! collection semantics they rely on, see [`r3::nethash`]) follow the original closely.

use crate::cell::{Cell, CellId, Sticker, StickerId};
use crate::config::{self, Distance, PuzzleConfig, TogglingMode};
use crate::group_presentation;
use crate::pants::{self, Pants};
use crate::state::State;
use crate::topology::Topology;
use crate::twist::{SingleTwist, TwistHistory};
use crate::twist_data::{ElementType, IdentifiedTwistData, TwistData, TwistDataId};
use r3::euclidean2d;
use r3::infinity;
use r3::nethash::{self, NetKey};
use r3::slicer;
use r3::texture_helper::{self, TextureHelper};
use r3::util;
use r3::{
    Circle, CircleNE, Complex, Geometry, Isometry, Metric, Mobius, NearTree, NetMap, NetSet, Polygon, Segment, Tile,
    Tiling, TilingConfig, TilingPositions, Transform, Vector3D,
};
use std::f64::consts::PI;
use std::fmt;

/// Receives progress while building (and can cancel it).
pub trait BuildProgress {
    fn status(&mut self, _message: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// Ignores progress.
impl BuildProgress for () {}

#[derive(Debug, Clone, PartialEq)]
pub enum BuildError {
    Cancelled,
    Invalid(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BuildError::Cancelled => write!(f, "puzzle building cancelled"),
            BuildError::Invalid(msg) => write!(f, "puzzle build failure: {msg}"),
        }
    }
}

impl std::error::Error for BuildError {}

/// A built puzzle.
#[derive(Debug)]
pub struct Puzzle {
    pub config: PuzzleConfig,
    /// A description of the topology, e.g. "F=24, E=84, V=56, χ=-4".
    pub topology: String,
    pub cells: Vec<Cell>,
    /// Master cells, in color order.
    pub masters: Vec<CellId>,
    /// Slaves of each master (indexed like `masters`).
    slaves: Vec<Vec<CellId>>,
    /// The border of the fundamental domain.
    pub master_boundary: Vec<Segment>,
    state_calc_cells: Vec<CellId>,
    is_state_calc: Vec<bool>,
    pub stickers: Vec<Sticker>,
    /// The number of stickers per cell.
    pub stickers_per_cell: usize,
    pub twist_data: Vec<TwistData>,
    /// All logical twists. Saved twists refer to these by index.
    pub all_twist_data: Vec<IdentifiedTwistData>,
    twist_data_tree: NearTree<TwistDataId>,
    cell_tree: NearTree<CellId>,
    /// Texture coordinates for the template (first master) cell.
    pub template_texture_coords: Vec<Vector3D>,
    pub texture_helper: TextureHelper,
    pub state: State,
    pub history: TwistHistory,
}

/// Isometries identifying cells, from one configured identification.
struct PuzzleIdentification {
    unmirrored: Isometry,
    mirrored: Option<Isometry>,
    use_mirrored: bool,
}

impl PuzzleIdentification {
    fn isometries(&self) -> impl Iterator<Item = &Isometry> {
        std::iter::once(&self.unmirrored).chain(self.mirrored.iter().filter(|_| self.use_mirrored))
    }
}

fn status(progress: &mut dyn BuildProgress, message: &str) -> Result<(), BuildError> {
    if progress.cancelled() {
        return Err(BuildError::Cancelled);
    }
    progress.status(message);
    Ok(())
}

/// Maps over items, in parallel when the `parallel` feature is on. Results keep their order.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync + Send) -> Vec<R> {
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        items.par_iter().map(f).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        items.iter().map(f).collect()
    }
}

impl Puzzle {
    /// Builds a puzzle.
    pub fn build(config: PuzzleConfig, progress: &mut dyn BuildProgress) -> Result<Puzzle, BuildError> {
        if config.p < 2 || config.q < 2 {
            return Err(BuildError::Invalid(format!("invalid tiling {{{},{}}}", config.p, config.q)));
        }
        let metric = Metric::from(config.geometry());
        let mut p = Puzzle {
            config,
            topology: String::new(),
            cells: Vec::new(),
            masters: Vec::new(),
            slaves: Vec::new(),
            master_boundary: Vec::new(),
            state_calc_cells: Vec::new(),
            is_state_calc: Vec::new(),
            stickers: Vec::new(),
            stickers_per_cell: 0,
            twist_data: Vec::new(),
            all_twist_data: Vec::new(),
            twist_data_tree: NearTree::new(metric),
            cell_tree: NearTree::new(metric),
            template_texture_coords: Vec::new(),
            texture_helper: TextureHelper::default(),
            state: State::new(0, 0),
            history: TwistHistory::default(),
        };
        p.build_internal(progress)?;
        Ok(p)
    }

    fn build_internal(&mut self, progress: &mut dyn BuildProgress) -> Result<(), BuildError> {
        status(progress, "creating underlying tiling...")?;
        let tiling = self.gen_tiling();

        status(progress, "precalculating identification isometries...")?;
        let identifications = self.precalc_identification_isometries(&tiling)?;

        // Tile centers to the cells made from them.
        let mut completed: NetMap<Vector3D, CellId> = NetMap::new();

        status(progress, "adding in cells...")?;
        for t in self.master_candidates(&tiling) {
            if completed.contains_key(&tiling.tiles[t].center()) {
                continue;
            }
            self.add_master(t, &tiling, &identifications, &mut completed);
        }
        if self.masters.is_empty() {
            return Err(BuildError::Invalid("no cells".into()));
        }

        status(progress, "analyzing topology...")?;
        let template = &tiling.tiles[0];
        let spherical = self.is_spherical();
        let topology = Topology::analyze(&self.cells, &self.masters, &self.slaves, template, |v| {
            if spherical { infinity::infinity_safe(v) } else { v }
        });
        self.topology = topology.to_string();

        status(progress, "marking cells for state calcs...")?;
        let mut template_twist_data = self.template_twist_data(template);
        self.mark_cells_for_state_calcs(&tiling, &completed, &template_twist_data, &topology);

        status(progress, "slicing up template tile...")?;
        let template_stickers = self.slice_up_template(&tiling, &mut template_twist_data);

        status(progress, "adding in stickers...")?;
        // Spherical puzzles are drawn directly (without textures), so every cell needs stickers.
        // Otherwise, as a memory optimization, only the cells involved in state calcs do.
        let sticker_cells: Vec<CellId> =
            if spherical { completed.values().copied().collect() } else { self.state_calc_cells.clone() };
        for cell in sticker_cells {
            self.add_stickers_to_cell(cell, &template_stickers);
        }
        self.stickers_per_cell = template_stickers.len();

        status(progress, "setting up texture coordinates...")?;
        let first_master = &self.cells[self.masters[0]].boundary;
        self.template_texture_coords = texture_helper::texture_coords(first_master, self.config.geometry(), 8);
        self.texture_helper.setup_element_indices(first_master);

        status(progress, "preparing twisting...")?;
        self.setup_twist_data_for_full_puzzle(&tiling, &topology, &template_twist_data);
        self.setup_cell_near_tree(&completed);

        status(progress, "calculating fundamental domain boundary...")?;
        self.calc_boundary();

        if self.config.is_toggling() {
            status(progress, "populating neighbors...")?;
            self.populate_neighbors(&tiling, &completed)?;
        }

        self.state = State::new(self.masters.len(), template_stickers.len());
        self.history = TwistHistory::default();

        progress.status(&format!("Number of colors:{}", self.masters.len()));
        progress.status(&format!("Number of tiles:{}", tiling.count()));
        progress.status(&format!("Number of cells:{}", self.all_cells().count()));
        progress.status(&format!("Number of stickers per cell:{}", template_stickers.len()));
        Ok(())
    }

    pub fn is_spherical(&self) -> bool {
        self.config.geometry() == Geometry::Spherical
    }

    /// For things that don't like NaN, infinity or huge values (only matters when spherical).
    pub fn infinity_safe(&self, v: Vector3D) -> Vector3D {
        if self.is_spherical() { infinity::infinity_safe(v) } else { v }
    }

    /// The slaves of a master cell.
    pub fn slave_cells(&self, master: CellId) -> &[CellId] {
        match usize::try_from(self.cells[master].index_of_master) {
            Ok(i) if self.cells[master].is_master() && i < self.slaves.len() => &self.slaves[i],
            _ => &[],
        }
    }

    /// All (master and slave) cells, masters first followed by their slaves.
    pub fn all_cells(&self) -> impl Iterator<Item = CellId> + '_ {
        self.masters.iter().flat_map(move |&m| std::iter::once(m).chain(self.slave_cells(m).iter().copied()))
    }

    pub fn all_slave_cells(&self) -> impl Iterator<Item = CellId> + '_ {
        self.masters.iter().flat_map(move |&m| self.slave_cells(m).iter().copied())
    }

    pub fn is_state_calc_cell(&self, cell: CellId) -> bool {
        self.is_state_calc.get(cell).copied().unwrap_or(false)
    }

    pub fn state_calc_cells(&self) -> &[CellId] {
        &self.state_calc_cells
    }

    /// Whether the puzzle is solved (all lights on, for toggling puzzles).
    pub fn is_solved(&self) -> bool {
        if self.config.is_toggling() { self.state.is_all_on() } else { self.state.is_solved() }
    }

    fn gen_tiling(&self) -> Tiling {
        let mut config = TilingConfig::new(self.config.p, self.config.q, self.config.num_tiles.max(0) as usize);
        config.shrink = self.config.tile_shrink;
        Tiling::generate(config)
    }

    /// Hacks making the fundamental region of some Klein bottle puzzles look better (to match the
    /// Mathologer video).
    fn master_candidates(&self, tiling: &Tiling) -> Vec<usize> {
        let c = &self.config;
        let all = 0..tiling.count();
        if c.geometry() == Geometry::Euclidean {
            if c.p == 4 && c.q == 4 && c.expected_num_colors == 4 {
                return all.filter(|&i| i < 3 || i == 5).collect();
            }
            if c.p == 6 && c.q == 3 && c.expected_num_colors == 9 {
                return all.filter(|&i| i < 8 || i == 15).collect();
            }
        }
        all.collect()
    }

    // ---------------------------------------------------------------------------------------
    // Identifications

    /// Pre-calculates the identification isometries (much cheaper than reflecting everywhere).
    fn precalc_identification_isometries(&self, tiling: &Tiling) -> Result<Vec<PuzzleIdentification>, BuildError> {
        let template = &tiling.tiles[0];
        if self.config.using_relations() {
            return self.calc_isometries_from_relations(template);
        }

        let mut result = Vec::new();
        let Some(identifications) = &self.config.identifications else {
            return Ok(result);
        };
        let p = self.config.p;
        let g = self.config.geometry();
        let num_segments = template.boundary.segments.len();

        for identification in identifications {
            let initial_edges: Vec<i32> = if identification.initial_edges.is_empty() {
                (0..num_segments as i32).collect()
            } else {
                identification.initial_edges.clone()
            };

            // An edge set for identifications, and one for their mirrors.
            let mirrored_edges: Vec<i32> = identification.edges.iter().map(|&e| p - e).collect();
            let edge_sets = [&identification.edges, &mirrored_edges];

            for &init in &initial_edges {
                let mut initial_edge = init;
                let mut isometries = [Isometry::default(), Isometry::default()];
                for (i, edge_set) in edge_sets.iter().enumerate() {
                    let mirror = i == 1;
                    if mirror && initial_edge != 0 {
                        initial_edge = p - initial_edge;
                    }

                    let mut boundary = template.boundary.clone();
                    let k = usize::try_from(initial_edge)
                        .ok()
                        .filter(|&k| k < num_segments)
                        .ok_or_else(|| BuildError::Invalid(format!("invalid initial edge {initial_edge}")))?;

                    if identification.in_place_reflection {
                        let reflect = Segment::line(Vector3D::ORIGIN, boundary.segments[k].midpoint());
                        boundary.reflect(&reflect);
                    }

                    // (The original passes the boundary's own segment here; see
                    // `reflect_in_own_segment`.)
                    boundary.reflect_in_own_segment(k);

                    // All the configured reflections.
                    let mut s_index = initial_edge;
                    let mut even = boundary.orientation();
                    let count = boundary.segments.len() as i32;
                    for &offset in edge_set.iter() {
                        s_index += if even { offset } else { -offset };
                        even = !even;
                        if s_index < 0 {
                            s_index += count;
                        }
                        if s_index >= count {
                            s_index -= count;
                        }
                        let k = usize::try_from(s_index)
                            .ok()
                            .filter(|&k| k < num_segments)
                            .ok_or_else(|| BuildError::Invalid(format!("invalid edge set {edge_set:?}")))?;
                        boundary.reflect_in_own_segment(k);
                    }

                    if identification.end_rotation != 0 {
                        let mut angle = identification.end_rotation as f64 * 2.0 * PI / p as f64;
                        if mirror {
                            angle *= -1.0;
                        }
                        let mut rotate = Mobius::default();
                        // The origin case was required for hemi-puzzles.
                        if self.is_spherical() && infinity::is_infinite(boundary.center) {
                            rotate.elliptic(g, Vector3D::ORIGIN, -angle);
                        } else {
                            rotate.elliptic(g, boundary.center, angle);
                        }
                        boundary.transform(&rotate);
                    }

                    let mut isometry = Isometry::default();
                    isometry.calculate_from_two_polygons(template, &boundary, g);
                    isometries[i] = isometry.inverse();
                }

                let [unmirrored, mirrored] = isometries;
                result.push(PuzzleIdentification {
                    unmirrored,
                    mirrored: Some(mirrored),
                    use_mirrored: identification.use_mirrored_edge_set,
                });
            }
        }
        Ok(result)
    }

    /// Identifications from a group presentation of a regular map.
    fn calc_isometries_from_relations(&self, template: &Tile) -> Result<Vec<PuzzleIdentification>, BuildError> {
        // The fundamental triangle.
        let seg = template.boundary.segments[0];
        let source = [Vector3D::ORIGIN, seg.midpoint(), seg.p1];
        let mirrors =
            [Circle::from_2_points(source[0], source[1]), Circle::from_2_points(source[0], source[2]), seg.circle()];

        let relations = group_presentation::read_relations(self.config.group_relations.as_deref().unwrap_or(""))
            .map_err(|e| BuildError::Invalid(e.to_string()))?;

        let id = Mobius::identity();
        let mut relation_transforms: NetSet<Mobius> = NetSet::new();
        for reflections in &relations {
            let mut r = source;
            for &reflection in reflections {
                let Some(mirror) = mirrors.get(reflection) else {
                    return Err(BuildError::Invalid(format!("invalid mirror {reflection}")));
                };
                for v in &mut r {
                    *v = mirror.reflect_point(*v);
                }
            }
            let mut m = Mobius::default();
            m.map_points(r[0], r[1], r[2], source[0], source[1], source[2]);
            if !m.net_eq(&id) {
                relation_transforms.insert(m);
            }
        }

        // We need conjugations of the relations as well.
        let rot = |mut v: Vector3D, n: usize, m1: usize, m2: usize| {
            for _ in 0..n {
                v = mirrors[m1].reflect_point(v);
                v = mirrors[m2].reflect_point(v);
            }
            v
        };

        let max = (self.config.p * self.config.expected_num_colors * 2).max(0) as usize;
        while relation_transforms.len() < max {
            let before = relation_transforms.len();
            for p in 0..self.config.p as usize {
                add_conjugations(&mut relation_transforms, &source, |v| rot(v, p, 0, 1), max);
            }
            add_conjugations(&mut relation_transforms, &source, |v| mirrors[2].reflect_point(v), max);

            // The original would loop forever here.
            if relation_transforms.len() == before {
                return Err(BuildError::Invalid("group relations don't generate enough identifications".into()));
            }
        }

        Ok(relation_transforms
            .into_vec()
            .into_iter()
            .map(|m| PuzzleIdentification { unmirrored: Isometry::new(m, None), mirrored: None, use_mirrored: false })
            .collect())
    }

    // ---------------------------------------------------------------------------------------
    // Cells

    fn add_master(
        &mut self,
        tile: usize,
        tiling: &Tiling,
        identifications: &[PuzzleIdentification],
        completed: &mut NetMap<Vector3D, CellId>,
    ) {
        let template = &tiling.tiles[0];
        let master = self.setup_cell(template, tiling.tiles[tile].boundary.clone(), completed);

        let index = self.masters.len();
        // Paranoia: the cell stays in `completed`, but without a valid index.
        if self.config.expected_num_colors != 0 && index >= self.config.expected_num_colors as usize {
            self.cells[master].index_of_master = -1;
            return;
        }
        self.cells[master].index_of_master = index as i32;
        self.masters.push(master);
        self.slaves.push(Vec::new());

        // To help recentering puzzles built from group relations, we go deeper for the slaves of
        // the central tile.
        let positions = (index == 0 && self.config.using_relations()).then(|| {
            TilingPositions::build(&TilingConfig::new(
                self.config.p,
                self.config.q,
                (self.config.num_tiles.max(0) * 5) as usize,
            ))
        });

        // Breadth first.
        let mut parents = vec![master];
        while !parents.is_empty() && !identifications.is_empty() {
            let mut added = Vec::new();
            for &parent in &parents {
                for identification in identifications {
                    for isometry in identification.isometries() {
                        let check_tiling = if positions.is_some() { None } else { Some(tiling) };
                        if let Some(slave) = self.apply_one_isometry(
                            master,
                            parent,
                            isometry,
                            check_tiling,
                            positions.as_ref(),
                            template,
                            completed,
                        ) {
                            added.push(slave);
                        }
                    }
                }
            }
            parents = added;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_one_isometry(
        &mut self,
        master: CellId,
        parent: CellId,
        isometry: &Isometry,
        tiling: Option<&Tiling>,
        positions: Option<&TilingPositions>,
        template: &Tile,
        completed: &mut NetMap<Vector3D, CellId>,
    ) -> Option<CellId> {
        // NOTE: The original tried conjugating the identification by the parent's isometry, but
        // that caused problems (extraneous mirroring for Klein bottles, bad spherical puzzles).
        let mut new_center = isometry.apply_infinite_safe(self.cells[parent].vertex_circle.center_ne);

        // Some spherical centers project to very large values rather than infinity.
        if infinity::is_infinite(new_center) {
            new_center = infinity::INFINITY_VECTOR_2D;
        }

        if let Some(tiling) = tiling
            && !tiling.tile_positions.contains_key(&new_center)
        {
            return None;
        }
        if let Some(positions) = positions
            && !positions.positions.contains(&new_center)
        {
            return None;
        }
        if completed.contains_key(&new_center) {
            return None;
        }

        let mut boundary = self.cells[parent].boundary.clone();
        boundary.transform(isometry);
        let slave = self.setup_cell(template, boundary, completed);
        self.cells[slave].master = Some(master);
        self.cells[slave].index_of_master = self.cells[master].index_of_master;
        self.slaves[self.cells[master].index_of_master as usize].push(slave);
        Some(slave)
    }

    /// Creates a cell (marking its tile completed). The boundary should come from applying
    /// identifications, not from a tile in the tiling.
    fn setup_cell(&mut self, home: &Tile, boundary: Polygon, completed: &mut NetMap<Vector3D, CellId>) -> CellId {
        let mut isometry = Isometry::default();
        // This may differ from the tiling's isometries, so it must be recalculated.
        isometry.calculate_from_two_polygons(home, &boundary, self.config.geometry());
        let center = boundary.center;
        let vertex_circle = boundary.circum_circle();
        let mut cell = Cell::new(boundary, vertex_circle);
        cell.isometry = isometry;
        let id = self.cells.len();
        self.cells.push(cell);
        completed.insert(center, id);
        id
    }

    fn master_or_self(&self, cell: CellId) -> CellId {
        self.cells[cell].master.unwrap_or(cell)
    }

    // ---------------------------------------------------------------------------------------
    // Twist data

    fn setup_template_slicing_circles(
        &self,
        template: &Tile,
        center: Vector3D,
        distances: &[Distance],
    ) -> Vec<CircleNE> {
        let g = self.config.geometry();
        let mut m = Mobius::default();
        m.isometry(g, 0.0, center);
        distances
            .iter()
            .map(|d| {
                let radius_in_geometry = d.dist(self.config.p, self.config.q);
                let radius = match g {
                    Geometry::Spherical => r3::spherical2d::s2e_norm(radius_in_geometry),
                    Geometry::Euclidean => radius_in_geometry,
                    Geometry::Hyperbolic => r3::donhatch::h2e_norm(radius_in_geometry),
                };
                let mut circle = CircleNE::new(Circle::new(template.center(), radius), template.center());
                circle.transform(&m);
                circle
            })
            .collect()
    }

    /// Twist data for the template tile.
    fn template_twist_data(&self, template: &Tile) -> Vec<TwistData> {
        let circles = &self.config.slicing_circles;
        let (p, q) = (self.config.p, self.config.q);
        let mut result = Vec::new();

        if circles.face_twisting() {
            let mut td = TwistData::new(ElementType::Face, template.center(), p);
            td.circles = self.setup_template_slicing_circles(template, td.center, &circles.face_centered);
            result.push(td);
        }

        for s in &template.boundary.segments {
            if circles.edge_twisting() {
                let mut td = TwistData::new(ElementType::Edge, s.midpoint(), 2);
                td.circles = self.setup_template_slicing_circles(template, td.center, &circles.edge_centered);
                result.push(td);
            }

            if circles.vertex_twisting() {
                let mut td = TwistData::new(ElementType::Vertex, s.p1, q);
                td.circles = self.setup_template_slicing_circles(template, td.center, &circles.vertex_centered);
                result.push(td);
            }

            // A long way to go to make this general.
            if self.config.systolic() {
                // The order only controls twist speed; the three "slices" mark the directions.
                let mut td = TwistData::new(ElementType::Vertex, s.p1, 3);
                td.set_num_slices(3);

                let mut m = Mobius::default();
                let angle = euclidean2d::angle_to_counter_clock(Vector3D::new(1.0, 0.0), s.p1);
                m.elliptic(Geometry::Hyperbolic, Vector3D::ORIGIN, angle);

                let mut pants = Pants::for_klein_quartic();
                pants.transform_mobius(&m);
                td.pants = Some(pants);

                // The actual twist may use 1 of 3 Möbius transforms, depending on the pants edge.
                let mut pants_circles = pants::systoles_for_kq();
                let mut cut_circles = Vec::new();
                for d in &circles.systolic {
                    let temp: Vec<CircleNE> = if !d.is_zero() {
                        pants_circles
                            .iter()
                            .flat_map(|pc| {
                                let (c1, c2) = slicer::offset_hyperbolic_geodesic(pc, d.dist(p, q));
                                // The circles' NE center is the pants center, the reference for
                                // systolic twists (it's "outside" the circles, which is accounted
                                // for when finding affected masters and stickers).
                                [CircleNE::new(c1, pc.center_ne), CircleNE::new(c2, pc.center_ne)]
                            })
                            .map(|mut c| {
                                c.transform(&m);
                                c
                            })
                            .collect()
                    } else {
                        // The original transforms these circle objects in place.
                        for c in &mut pants_circles {
                            c.transform(&m);
                        }
                        pants_circles.clone()
                    };
                    cut_circles.extend(temp);
                }
                td.circles = cut_circles;
                result.push(td);
            }
        }
        result
    }

    /// The isometry taking the template to a cell, for twist data.
    fn isometry_from_origin_to_cell(
        &self,
        tiling: &Tiling,
        topology: &Topology,
        cell: CellId,
        template_center: Vector3D,
    ) -> Option<Isometry> {
        if !self.config.systolic() {
            return Some(self.cells[cell].isometry_inverse());
        }

        // The isometry needs to keep the pants hexagon oriented correctly (and not reversed): it
        // takes the twist center to the template's, consistently across the tiling. So we get the
        // 3 cells around the center (a tile vertex) and always map them in the same order.
        let vertex_at_cell = self.cells[cell].isometry_inverse().apply(template_center);
        let incident = tiling.vertex_incidences.get(&vertex_at_cell)?;
        if incident.len() != 3 {
            return None;
        }

        // Order them consistently, starting with the lowest master index.
        let mut lowest = usize::MAX;
        let mut index_with_lowest = 0;
        for (i, &t) in incident.iter().enumerate() {
            let idx = topology.logical_element_index(ElementType::Face, tiling.tiles[t].center())?;
            if idx < lowest {
                lowest = idx;
                index_with_lowest = i;
            }
        }

        let e1 = tiling.tiles[incident[index_with_lowest]].center();
        let mut e2 = tiling.tiles[incident[(index_with_lowest + 1) % 3]].center();
        let mut e3 = tiling.tiles[incident[(index_with_lowest + 2) % 3]].center();

        // We may need to swap the latter two to have CCW order.
        let d1 = e1 - vertex_at_cell;
        let d2 = e2 - vertex_at_cell;
        let d3 = e3 - vertex_at_cell;
        if euclidean2d::angle_to_counter_clock(d1, d2) > euclidean2d::angle_to_counter_clock(d1, d3) {
            std::mem::swap(&mut e2, &mut e3);
        }

        // Map the 3 centers defining the original pants to these, in CCW order.
        // (Hardcoded for the Klein quartic.)
        let masters = &self.masters;
        if masters.len() < 8 {
            return None;
        }
        let amount_template_rotated = euclidean2d::angle_to_counter_clock(Vector3D::new(1.0, 0.0), template_center);
        let rotated = |c: CellId| {
            let mut v = self.cells[c].center();
            v.rotate_xy(amount_template_rotated);
            v
        };
        let (s1, s2, s3) = (rotated(masters[0]), rotated(masters[7]), rotated(masters[1]));

        let mut m = Mobius::default();
        m.map_points(s1, s2, s3, e1, e2, e3);
        Some(Isometry::new(m, None))
    }

    fn transformed_twist_data_for_cell(
        &self,
        tiling: &Tiling,
        topology: &Topology,
        cell: CellId,
        untransformed: &TwistData,
        reverse: bool,
    ) -> Option<TwistData> {
        let isometry = self.isometry_from_origin_to_cell(tiling, topology, cell, untransformed.center)?;
        Some(untransformed.transformed(&isometry, reverse))
    }

    /// Marks the cells needed for state calculations: masters, plus any slaves a twist touching a
    /// master can affect.
    fn mark_cells_for_state_calcs(
        &mut self,
        tiling: &Tiling,
        cells: &NetMap<Vector3D, CellId>,
        template_twist_data: &[TwistData],
        topology: &Topology,
    ) {
        let spherical = self.is_spherical();
        let mut result: Vec<CellId> = self.masters.clone();
        let mut complete = NetSet::new();
        let slaves: Vec<CellId> = self.all_slave_cells().collect();

        // (The original added to a shared list from parallel loops, a data race. We collect the
        // results in order instead.)
        let affected_slaves = |twists: &[TwistData]| -> Vec<CellId> {
            let hits =
                par_map(&slaves, |&slave| twists.iter().any(|td| td.will_affect_cell(&self.cells[slave], spherical)));
            slaves.iter().zip(hits).filter(|(_, hit)| *hit).map(|(&s, _)| s).collect()
        };

        if self.config.systolic() {
            // All twist data attached to master cells.
            let mut to_check = Vec::new();
            for td in template_twist_data {
                for &master in &self.masters {
                    let Some(transformed) = self.transformed_twist_data_for_cell(tiling, topology, master, td, false)
                    else {
                        continue;
                    };
                    if complete.insert(transformed.center) {
                        to_check.push(transformed);
                    }
                }
            }
            result.extend(affected_slaves(&to_check));
        } else {
            let mut hot_twists = Vec::new();
            for td in template_twist_data {
                for &slave in &slaves {
                    let Some(transformed) = self.transformed_twist_data_for_cell(tiling, topology, slave, td, false)
                    else {
                        continue;
                    };
                    if !complete.insert(transformed.center) {
                        continue;
                    }
                    if self.masters.iter().any(|&m| transformed.will_affect_cell(&self.cells[m], spherical)) {
                        result.push(slave);
                        hot_twists.push(transformed);
                    }
                }
            }

            // Any slaves these twists touch can also affect the masters.
            result.extend(affected_slaves(&hot_twists));

            // IRP puzzles with no slicing need all cells adjacent to masters too.
            if self.config.has_valid_irp_config() && self.masters.len() == result.len() {
                for &master in &self.masters {
                    let Some(&master_tile) = tiling.tile_positions.get(&self.cells[master].center()) else {
                        continue;
                    };
                    let t = &tiling.tiles[master_tile];
                    for &tile in std::iter::once(&master_tile).chain(&t.edge_incidences).chain(&t.vertex_incidences) {
                        if let Some(&c) = cells.get(&tiling.tiles[tile].center()) {
                            result.push(c);
                        }
                    }
                }
            }
        }

        self.is_state_calc = vec![false; self.cells.len()];
        self.state_calc_cells.clear();
        for c in result {
            if !self.is_state_calc[c] {
                self.is_state_calc[c] = true;
                self.state_calc_cells.push(c);
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Slicing

    /// All the circles to slice the template cell with.
    fn slicers(&self, tiling: &Tiling, template_twist_data: &mut [TwistData]) -> Vec<CircleNE> {
        let template = &tiling.tiles[0];
        let g = self.config.geometry();

        if self.config.coxeter_complex {
            // There's no twisting, so just slice up the fundamental triangles.
            let c = Circle::from_2_points(
                template.boundary.start().unwrap_or_default(),
                template.boundary.mid().unwrap_or_default(),
            );
            return (0..self.config.p)
                .map(|i| {
                    let mut m = Mobius::default();
                    m.elliptic(Geometry::Spherical, Complex::ZERO, PI * i as f64 / self.config.p as f64);
                    let mut c_ne = CircleNE::new(c, template.boundary.segments[0].p2);
                    c_ne.transform(&m);
                    c_ne
                })
                .collect();
        }

        let mut complete: NetSet<CircleNE> = NetSet::new();
        let mut result = Vec::new();
        for td in template_twist_data.iter_mut() {
            for slicing_circle in td.circles.iter_mut() {
                // For spherical puzzles, just use all the tiles (there aren't many). Otherwise
                // use the edge and vertex incident tiles.
                let mut isometries: Vec<Isometry> = if g == Geometry::Spherical {
                    tiling.tiles.iter().map(|t| t.isometry.clone()).collect()
                } else {
                    std::iter::once(0)
                        .chain(template.edge_incidences.iter().copied())
                        .chain(template.vertex_incidences.iter().copied())
                        .map(|t| tiling.tiles[t].isometry.clone())
                        .collect()
                };

                // Euclidean and hyperbolic puzzles may need more tiles (but this doesn't work for
                // systolic puzzles, whose circles cover so many tiles).
                if (g == Geometry::Euclidean || g == Geometry::Hyperbolic) && !self.config.systolic() {
                    let cutoff = slicing_circle.radius;
                    for t in &tiling.tiles {
                        if t.center().abs() <= cutoff || t.boundary.vertices().iter().any(|v| v.abs() <= cutoff) {
                            // Recalculated, since one of the stored tile isometries is mirrored.
                            let mut isometry = Isometry::default();
                            isometry.calculate_from_two_polygons(template, &t.boundary, g);
                            isometries.push(isometry.inverse());
                        }
                    }

                    // Avoids a slicer limitation (no tangent arc slices). This changes the template
                    // twist data too, as in the original.
                    let d = &self.config.slicing_circles.face_centered;
                    if d.len() == 1 && d[0].p == 1.0 && d[0].q == 1.0 && d[0].r == 1.0 {
                        slicing_circle.radius *= 0.999;
                    }
                }

                for isometry in &isometries {
                    let mut c = slicing_circle.clone();
                    c.transform(isometry);
                    if complete.insert(c.clone()) {
                        result.push(c);
                    }
                }
            }
        }
        result
    }

    fn slice_up_template(&self, tiling: &Tiling, template_twist_data: &mut [TwistData]) -> Vec<Polygon> {
        let mut slicers = self.slicers(tiling, template_twist_data);
        let mut sliced = self.slice_recursive(vec![tiling.tiles[0].drawn.clone()], &mut slicers);

        if self.config.coxeter_complex {
            // Order the stickers, so we can color them appropriately.
            sliced = util::sort_by_f64_key(sliced, |p| {
                euclidean2d::angle_to_counter_clock(p.center, Vector3D::new(1.0, 0.0))
            });
        }

        // A hacky special case.
        if self.config.earthquake() && sliced.len() > 4 {
            let mut slicers: Vec<CircleNE> = (0..7)
                .map(|i| {
                    let mut m = Mobius::default();
                    m.elliptic(Geometry::Hyperbolic, Complex::ZERO, PI * 2.0 * i as f64 / 7.0);
                    let mut c = CircleNE::new(
                        Circle {
                            center: Vector3D::ORIGIN,
                            radius: f64::INFINITY,
                            p1: Vector3D::ORIGIN,
                            p2: Vector3D::new(1.0, 0.0),
                        },
                        Vector3D::new(0.0, 0.5),
                    );
                    c.transform(&m);
                    c
                })
                .collect();

            let center_sticker = sliced.remove(4);
            let mut result = vec![center_sticker];
            result.extend(self.slice_recursive(sliced, &mut slicers));
            return result;
        }

        // Some slicing is complicated enough to leave zero-area stickers, which we remove.
        sliced.retain(|p| !r3::tolerance::zero(p.signed_area()));
        sliced
    }

    /// Slices with the last circle, then the next to last, and so on.
    fn slice_recursive(&self, mut slicees: Vec<Polygon>, slicers: &mut Vec<CircleNE>) -> Vec<Polygon> {
        let thickness = self.config.slicing_circles.thickness;
        while let Some(slicer) = slicers.pop() {
            let mut sliced = Vec::new();
            for mut slicee in slicees {
                if self.config.systolic() {
                    // Cuts may be hypercycles (not geodesic); hopefully close enough.
                    sliced.extend(slicer::slice_polygon_with_hyperbolic_geodesic(&mut slicee, &slicer, thickness));
                } else {
                    sliced.extend(slicer::slice_polygon_thick(&mut slicee, &slicer, self.config.geometry(), thickness));
                }
            }
            slicees = sliced;
        }
        slicees
    }

    fn add_stickers_to_cell(&mut self, cell: CellId, template_stickers: &[Polygon]) {
        let inverse = self.cells[cell].isometry_inverse();
        let cell_index = self.cells[cell].index_of_master;
        for (i, poly) in template_stickers.iter().enumerate() {
            let mut transformed = poly.clone();
            transformed.transform(&inverse);
            let id = self.stickers.len();
            self.stickers.push(Sticker { cell_index, sticker_index: i, poly: transformed, twisting: false });
            self.cells[cell].stickers.push(id);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Setting up twisting on the full puzzle

    fn setup_twist_data_for_full_puzzle(
        &mut self,
        tiling: &Tiling,
        topology: &Topology,
        template_twist_data: &[TwistData],
    ) {
        let spherical = self.is_spherical();

        // Twist centers to twist data.
        let mut twist_data_map: NetMap<Vector3D, TwistDataId> = NetMap::new();
        if self.config.version == config::VERSION_PREVIEW {
            self.setup_twist_data_preview_version(tiling, topology, template_twist_data, &mut twist_data_map);
        } else if self.config.version == config::VERSION_CURRENT {
            self.setup_twist_data_current_version(tiling, topology, template_twist_data, &mut twist_data_map);
        }

        self.add_opp_twisters(&twist_data_map);

        // Mark the affected master cells and stickers. Spherical puzzles need this for all twist
        // data (they're drawn directly); otherwise just for the twist data used in state calcs.
        let work: Vec<TwistDataId> = self
            .all_twist_data
            .iter()
            .flat_map(
                |c| if spherical { c.twist_data_for_drawing.clone() } else { c.twist_data_for_state_calcs.clone() },
            )
            .collect();
        let sticker_cells: Vec<CellId> =
            if spherical { self.all_cells().collect() } else { self.state_calc_cells.clone() };
        let results = par_map(&work, |&id| {
            let td = &self.twist_data[id];
            let masters: Vec<CellId> =
                self.masters.iter().copied().filter(|&m| td.will_affect_cell(&self.cells[m], spherical)).collect();
            let mut stickers = vec![Vec::new(); td.num_slices().max(0) as usize];
            for &cell in &sticker_cells {
                for &sticker in &self.cells[cell].stickers {
                    for slice in td.affected_slices_for_sticker(&self.stickers[sticker], spherical) {
                        if let Some(list) = stickers.get_mut(slice) {
                            list.push(sticker);
                        }
                    }
                }
            }
            (masters, stickers)
        });
        for (&id, (masters, stickers)) in work.iter().zip(results) {
            self.twist_data[id].affected_master_cells = Some(masters);
            self.twist_data[id].affected_stickers = Some(stickers);
        }

        // We only need to keep twist data for state calcs that touches masters.
        for c in &mut self.all_twist_data {
            let twist_data = &self.twist_data;
            c.twist_data_for_state_calcs
                .retain(|&id| twist_data[id].affected_master_cells.as_ref().is_some_and(|m| !m.is_empty()));
        }

        // Now build the near tree (which doesn't like NaN or infinity).
        for (&center, &id) in twist_data_map.iter() {
            self.twist_data_tree.insert(id, self.infinity_safe(center));
        }
    }

    /// The current way of setting up twist data (version 2.1).
    fn setup_twist_data_current_version(
        &mut self,
        tiling: &Tiling,
        topology: &Topology,
        template_twist_data: &[TwistData],
        twist_data_map: &mut NetMap<Vector3D, TwistDataId>,
    ) {
        // Collections for all elements; unused ones are removed at the end.
        let mut collections = vec![IdentifiedTwistData::default(); topology.f() + topology.e() + topology.v()];

        for &master in &self.masters.clone() {
            for td in template_twist_data {
                self.setup_twist_data_for_cell(tiling, topology, master, td, false, twist_data_map, &mut collections);
                for &slave in &self.slave_cells(master).to_vec() {
                    let reverse = self.cells[master].reflected() ^ self.cells[slave].reflected();
                    self.setup_twist_data_for_cell(
                        tiling,
                        topology,
                        slave,
                        td,
                        reverse,
                        twist_data_map,
                        &mut collections,
                    );
                }
            }
        }

        // Remove empty collections and assign indices.
        let mut remap = vec![None; collections.len()];
        for (old, mut c) in collections.into_iter().enumerate() {
            if c.twist_data_for_drawing.is_empty() {
                continue;
            }
            c.index = self.all_twist_data.len();
            remap[old] = Some(c.index);
            self.all_twist_data.push(c);
        }
        for td in &mut self.twist_data {
            td.identified = td.identified.and_then(|i| remap[i]);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn setup_twist_data_for_cell(
        &mut self,
        tiling: &Tiling,
        topology: &Topology,
        cell: CellId,
        template: &TwistData,
        reverse: bool,
        twist_data_map: &mut NetMap<Vector3D, TwistDataId>,
        collections: &mut [IdentifiedTwistData],
    ) {
        // Already done? (This isometry isn't right for systolic puzzles, but maps the center fine.)
        let mapped = self.cells[cell].isometry_inverse().apply(template.center);
        let center_safe = self.infinity_safe(mapped);
        if twist_data_map.contains_key(&center_safe) {
            return;
        }

        // This can fail near the edges for systolic puzzles.
        let Some(mut transformed) = self.transformed_twist_data_for_cell(tiling, topology, cell, template, reverse)
        else {
            return;
        };
        let Some(index) = topology.logical_element_index(template.twist_type, center_safe) else {
            return;
        };

        transformed.identified = Some(index);
        let id = self.twist_data.len();
        self.twist_data.push(transformed);
        collections[index].twist_data_for_drawing.push(id);
        if self.is_state_calc_cell(cell) {
            collections[index].twist_data_for_state_calcs.push(id);
        }
        twist_data_map.insert(center_safe, id);
    }

    /// The old way of setting up twist data (version 2.0), for loading old saved files.
    fn setup_twist_data_preview_version(
        &mut self,
        tiling: &Tiling,
        topology: &Topology,
        template_twist_data: &[TwistData],
        twist_data_map: &mut NetMap<Vector3D, TwistDataId>,
    ) {
        let mut used_master_location = NetSet::new();
        for &master in &self.masters.clone() {
            for td in template_twist_data {
                // Avoid duplicate vertex/edge circles.
                let center_ne = self.cells[master].isometry.inverse().apply(td.center);
                if !used_master_location.insert(center_ne) {
                    continue;
                }

                let index = self.all_twist_data.len();
                self.all_twist_data.push(IdentifiedTwistData { index, ..Default::default() });

                self.setup_twist_data_for_cell_preview(tiling, topology, master, td, false, twist_data_map, index);
                for &slave in &self.slave_cells(master).to_vec() {
                    let reverse = self.cells[master].reflected() ^ self.cells[slave].reflected();
                    self.setup_twist_data_for_cell_preview(tiling, topology, slave, td, reverse, twist_data_map, index);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn setup_twist_data_for_cell_preview(
        &mut self,
        tiling: &Tiling,
        topology: &Topology,
        cell: CellId,
        template: &TwistData,
        reverse: bool,
        twist_data_map: &mut NetMap<Vector3D, TwistDataId>,
        collection: usize,
    ) {
        let Some(mut transformed) = self.transformed_twist_data_for_cell(tiling, topology, cell, template, reverse)
        else {
            return;
        };
        transformed.identified = Some(collection);
        let center = self.infinity_safe(transformed.center);
        let id = self.twist_data.len();
        self.twist_data.push(transformed);
        self.all_twist_data[collection].twist_data_for_drawing.push(id);
        if self.is_state_calc_cell(cell) {
            self.all_twist_data[collection].twist_data_for_state_calcs.push(id);
        }
        twist_data_map.insert(center, id);
    }

    /// For spherical puzzles, appends antipodal twisting circles (when they exist).
    fn add_opp_twisters(&mut self, twist_data_map: &NetMap<Vector3D, TwistDataId>) {
        let spherical = self.is_spherical();
        for &id in twist_data_map.values() {
            let num_circles = self.twist_data[id].circles.len() as i32;
            self.twist_data[id].num_slices_no_opp = num_circles;
            if !spherical || num_circles == 0 {
                continue;
            }

            let first_circle = self.twist_data[id].circles[0].clone();
            let antipode = self.infinity_safe(first_circle.reflect_point(self.twist_data[id].center));
            let Some(&anti) = twist_data_map.get(&antipode) else {
                // One more layer, for the slice beyond the last circle.
                self.twist_data[id].set_num_slices(num_circles + 1);
                continue;
            };

            // Puzzles with non-regular colorings can be really weird with slicing (e.g. {3,5} 8C),
            // so we don't allow slicing if the antipodal twist has identified twists other than
            // itself or us. (This still allows hemi-puzzles to have slices.) The {3,4} 4CA also
            // had an identified antipodal twist with the same orientation, which makes slice-2
            // twists undefined, so we require opposite orientation.
            let td = &self.twist_data[id];
            let anti_td = &self.twist_data[anti];
            let identified = anti_td.identified.map(|i| &self.all_twist_data[i].twist_data_for_drawing);
            let allowed = identified.is_none_or(|list| {
                list.iter().all(|&other| {
                    other == anti
                        || (self.infinity_safe(self.twist_data[other].center) == self.infinity_safe(td.center)
                            && (anti_td.reverse ^ td.reverse))
                })
            });
            if !allowed {
                continue;
            }

            // Add the opposite circles.
            let mut list = td.circles.clone();
            for opp in &anti_td.circles {
                let mut c = opp.clone();
                c.center_ne = first_circle.center_ne;
                list.push(c);
            }

            // Sort by radius after moving to the origin, then remove duplicates. (Lines must be
            // normalized for comparisons to work.)
            let mut to_origin = Mobius::default();
            to_origin.isometry(Geometry::Spherical, 0.0, -first_circle.center_ne);
            let mut list = util::sort_by_f64_key(list, |c| {
                let mut c = c.clone();
                c.transform(&to_origin);
                c.radius
            });
            for c in &mut list {
                c.normalize_line();
            }
            let circles = nethash::distinct(list);

            let td = &mut self.twist_data[id];
            td.set_num_slices(circles.len() as i32 + 1);
            td.circles = circles;
        }
    }

    fn setup_cell_near_tree(&mut self, cell_map: &NetMap<Vector3D, CellId>) {
        for (&center, &cell) in cell_map.iter() {
            self.cell_tree.insert(cell, self.infinity_safe(center));
        }
    }

    fn calc_boundary(&mut self) {
        let mut segments: NetMap<Vector3D, Vec<Segment>> = NetMap::new();
        for &master in &self.masters {
            for s in &self.cells[master].boundary.segments {
                segments.get_or_insert_with(s.midpoint(), Vec::new).push(*s);
            }
        }
        self.master_boundary = segments.values().filter(|l| l.len() == 1).map(|l| l[0]).collect();
    }

    /// For toggling puzzles: master cells sharing an edge (a cell is its own neighbor).
    fn populate_neighbors(&mut self, tiling: &Tiling, completed: &NetMap<Vector3D, CellId>) -> Result<(), BuildError> {
        let missing = || BuildError::Invalid("cell missing from tiling".into());
        for &master in &self.masters.clone() {
            add_unique(&mut self.cells[master].neighbors, master);
            let &master_tile = tiling.tile_positions.get(&self.cells[master].center()).ok_or_else(missing)?;
            for &neighbor_tile in &tiling.tiles[master_tile].edge_incidences {
                let &neighbor = completed.get(&tiling.tiles[neighbor_tile].center()).ok_or_else(missing)?;

                // This can happen for cells near the boundary of our recursion.
                if self.cells[neighbor].index_of_master < 0 {
                    continue;
                }

                let neighbor_master = self.master_or_self(neighbor);
                add_unique(&mut self.cells[master].neighbors, neighbor_master);
                add_unique(&mut self.cells[neighbor_master].neighbors, master);
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // Queries

    /// The twist data with circles closest to a location (untransformed by any view motion).
    pub fn closest_twisting_circles(&self, location: Vector3D) -> Option<TwistDataId> {
        self.twist_data_tree.find_nearest_neighbor(location, f64::MAX)
    }

    /// The cell closest to a location (untransformed by any view motion).
    pub fn closest_cell(&self, location: Vector3D) -> Option<CellId> {
        self.cell_tree.find_nearest_neighbor(location, f64::MAX)
    }

    /// The order of a twist (the number of twists to get back to the start).
    pub fn twist_order(&self, identified: usize) -> i32 {
        let c = &self.all_twist_data[identified];
        let td = c.twist_data_for_state_calcs.first().or(c.twist_data_for_drawing.first());
        td.map_or(1, |&id| self.twist_data[id].order)
    }

    /// The magnitude of a twist, in radians.
    pub fn twist_magnitude(&self, twist: &SingleTwist) -> f64 {
        2.0 * PI / self.twist_order(twist.identified) as f64
    }

    /// The twist data involved in the state calculations for a twist.
    pub fn state_calc_twist_data(&self, twist: &SingleTwist) -> Vec<TwistDataId> {
        let mut result = self.all_twist_data[twist.identified].twist_data_for_state_calcs.clone();
        if let Some(s) = twist.identified_systolic {
            result.extend(&self.all_twist_data[s].twist_data_for_state_calcs);
        }
        result
    }

    /// Marks the stickers moving in a twist (for animation), or clears them.
    pub fn set_twisting(&mut self, twist: &SingleTwist, twisting: bool) {
        let spherical = self.is_spherical();
        for identified in std::iter::once(twist.identified).chain(twist.identified_systolic) {
            let c = &self.all_twist_data[identified];
            let list = if spherical { &c.twist_data_for_drawing } else { &c.twist_data_for_state_calcs };
            for &td in list {
                for stickers in self.twist_data[td].affected_stickers_for_slice_mask(twist.slice_mask) {
                    for &s in stickers {
                        self.stickers[s].twisting = twisting;
                    }
                }
            }
        }
    }

    /// Master cells whose appearance a twist changes.
    pub fn affected_master_cells(&self, identified: usize) -> Vec<CellId> {
        let mut result = Vec::new();
        for &td in &self.all_twist_data[identified].twist_data_for_state_calcs {
            for &m in self.twist_data[td].affected_master_cells.iter().flatten() {
                add_unique(&mut result, m);
            }
        }
        result
    }

    // ---------------------------------------------------------------------------------------
    // State

    /// Applies a twist to the state. Returns the stickers that moved (old -> new position).
    pub fn update_state(&mut self, twist: &SingleTwist) -> Vec<(StickerId, StickerId)> {
        let systolic = self.config.systolic();
        let mut rotation = if systolic { 1.0 } else { self.twist_magnitude(twist) };
        if !twist.left_click {
            rotation *= -1.0;
        }

        let spherical = self.is_spherical();
        let num_primary = self.all_twist_data[twist.identified].twist_data_for_state_calcs.len();

        // Old sticker positions to stickers, and stickers to new positions.
        let mut old_map: NetMap<Vector3D, StickerId> = NetMap::new();
        let mut new_map: Vec<(StickerId, Vector3D)> = Vec::new();
        let mut new_index = std::collections::HashMap::new();

        for (count, td_id) in self.state_calc_twist_data(twist).into_iter().enumerate() {
            let td = &self.twist_data[td_id];
            let mobius = td.mobius_for_twist(&self.config, twist, rotation, count + 1 > num_primary);

            for list in td.affected_stickers_for_slice_mask(twist.slice_mask) {
                for &sticker in list {
                    let mut center = self.stickers[sticker].poly.center;
                    let infinite = spherical && infinity::is_infinite(center);
                    if infinite {
                        center = infinity::INFINITY_VECTOR;
                    }
                    old_map.insert(center, sticker);

                    let mut transformed = if infinite { mobius.apply_to_infinite() } else { mobius.apply(center) };
                    if spherical && infinity::is_infinite(transformed) {
                        transformed = infinity::INFINITY_VECTOR;
                    }
                    match new_index.get(&sticker) {
                        Some(&i) => new_map[i] = (sticker, transformed),
                        None => {
                            new_index.insert(sticker, new_map.len());
                            new_map.push((sticker, transformed));
                        }
                    }
                }
            }
        }

        let mut updated = Vec::new();
        for (sticker1, position) in new_map {
            // (This happens for moves on the puzzle boundary for p >= 6, and at infinity.)
            let Some(&sticker2) = old_map.get(&position) else {
                continue;
            };
            let (s1, s2) = (&self.stickers[sticker1], &self.stickers[sticker2]);
            let (Ok(c1), Ok(c2)) = (usize::try_from(s1.cell_index), usize::try_from(s2.cell_index)) else {
                continue;
            };

            // The sticker has moved from sticker1 -> sticker2.
            let color = self.state.sticker_color_index(c1, s1.sticker_index);
            self.state.set_sticker_color_index(c2, s2.sticker_index, color);
            if c1 != c2 || s1.sticker_index != s2.sticker_index {
                updated.push((sticker1, sticker2));
            }
        }

        self.state.commit_changes();
        updated
    }

    /// A toggling ("lights on") move on a cell.
    pub fn toggle(&mut self, cell: CellId) {
        let master = self.master_or_self(cell);
        let mode = self.config.toggling_mode;
        for &neighbor in &self.cells[master].neighbors {
            if (neighbor != master || mode == Some(TogglingMode::NeighborsAndSelf))
                && let Ok(i) = usize::try_from(self.cells[neighbor].index_of_master)
            {
                self.state.toggle_sticker_color_index(i, 0);
            }
        }
        self.state.commit_changes();
        self.history.update_toggle(master);
    }
}

fn add_unique(v: &mut Vec<CellId>, c: CellId) {
    if !v.contains(&c) {
        v.push(c);
    }
}

/// Adds conjugations of the relation transforms by a transformation (up to `max` in total).
fn add_conjugations(
    relation_transforms: &mut NetSet<Mobius>,
    source: &[Vector3D; 3],
    transform: impl Fn(Vector3D) -> Vector3D,
    max: usize,
) {
    let mut conjugations = relation_transforms.clone();
    for m in relation_transforms.iter() {
        let points = [source[0], source[1], source[2], m.apply(source[0]), m.apply(source[1]), m.apply(source[2])]
            .map(&transform);
        let mut m2 = Mobius::default();
        m2.map_points(points[0], points[1], points[2], points[3], points[4], points[5]);
        conjugations.insert(m2);
        if conjugations.len() >= max {
            break;
        }
    }
    for m in conjugations.into_vec() {
        relation_transforms.insert(m);
    }
}
