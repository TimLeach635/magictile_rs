//! The 2D view: panning, rotating and zooming (the original's `MouseMotion`, 2D parts).

use magictile_core::Puzzle;
use magictile_core::cell::CellId;
use r3::models::{self, HyperbolicModel, SphericalModel};
use r3::{Geometry, Isometry, Mobius, Transform, Vector3D};
use std::collections::VecDeque;

/// Which kind of drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragButton {
    /// Pan.
    Primary,
    /// Rotate.
    Middle,
    /// Zoom.
    Secondary,
}

/// One step of a drag, in screen points (y down) relative to the view's top left.
#[derive(Clone, Copy, Debug)]
pub struct DragData {
    pub x: f32,
    pub y: f32,
    pub x_diff: f32,
    pub y_diff: f32,
    pub y_percent: f32,
    /// Rotation about the view center, in radians.
    pub rotation: f32,
    pub button: DragButton,
}

/// The model (projection) currently in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Plain,
    Hyperbolic(HyperbolicModel),
    Spherical(SphericalModel),
}

impl Model {
    pub fn for_puzzle(geometry: Geometry, hyperbolic: HyperbolicModel, spherical: SphericalModel) -> Model {
        match geometry {
            Geometry::Hyperbolic if hyperbolic != HyperbolicModel::Poincare => Model::Hyperbolic(hyperbolic),
            Geometry::Spherical if spherical != SphericalModel::Sterographic => Model::Spherical(spherical),
            _ => Model::Plain,
        }
    }

    /// Maps from the standard model (Poincaré / stereographic) for drawing. Hemisphere disks are
    /// handled separately.
    pub fn apply(self, v: Vector3D) -> Vector3D {
        match self {
            Model::Hyperbolic(HyperbolicModel::Klein) => models::poincare_to_klein(v),
            Model::Hyperbolic(HyperbolicModel::UpperHalfPlane) => models::poincare_to_upper(v),
            Model::Hyperbolic(HyperbolicModel::Orthographic) => models::poincare_to_ortho(v),
            Model::Spherical(SphericalModel::Gnomonic) => models::stereo_to_gnomonic(v),
            Model::Spherical(SphericalModel::Fisheye) => models::gnomonic_to_stereo(v),
            _ => v,
        }
    }

    /// Maps screen (model) coordinates back to the standard model.
    pub fn to_standard(self, v: Vector3D) -> Vector3D {
        match self {
            Model::Hyperbolic(HyperbolicModel::Klein) => models::klein_to_poincare(v),
            Model::Hyperbolic(HyperbolicModel::UpperHalfPlane) => models::upper_to_poincare(v),
            Model::Hyperbolic(HyperbolicModel::Orthographic) => models::ortho_to_poincare(v),
            Model::Spherical(SphericalModel::Gnomonic) => models::gnomonic_to_stereo(v),
            Model::Spherical(SphericalModel::Fisheye) => models::stereo_to_gnomonic(v),
            Model::Spherical(SphericalModel::HemisphereDisks) => models::from_disks(v * 2.0, false),
            _ => v,
        }
    }

    pub fn is_hemisphere_disks(self) -> bool {
        self == Model::Spherical(SphericalModel::HemisphereDisks)
    }
}

/// Seconds between gliding steps (the original's timer interval).
const SPIN_INTERVAL: f64 = 0.030;

#[derive(Debug)]
pub struct View {
    pub isometry: Isometry,
    /// Rotation of the whole view, in radians.
    pub rotation: f64,
    /// Half the visible height, in model units.
    pub view_scale: f64,
    geometry: Geometry,
    /// View size in points.
    pub width: f32,
    pub height: f32,
    /// The cell (a copy of the first master) drawn closest to the center, found while
    /// rendering. Used to recenter infinite tilings.
    pub closest: Option<CellId>,
    // Gliding after a flick.
    recent_drags: VecDeque<DragData>,
    spin: Option<DragData>,
    spin_accumulator: f64,
}

impl Default for View {
    fn default() -> Self {
        View {
            isometry: Isometry::identity(),
            rotation: 0.0,
            view_scale: 1.1,
            geometry: Geometry::Hyperbolic,
            width: 1.0,
            height: 1.0,
            closest: None,
            recent_drags: VecDeque::new(),
            spin: None,
            spin_accumulator: 0.0,
        }
    }
}

impl View {
    /// Resets for a new puzzle.
    pub fn reset(&mut self, geometry: Geometry) {
        *self = View { geometry, width: self.width, height: self.height, ..View::default() };
    }

    pub fn set_size(&mut self, width: f32, height: f32) {
        if width != self.width || height != self.height {
            // Resizing messes up gliding, so just stop.
            self.spin = None;
        }
        self.width = width.max(1.0);
        self.height = height.max(1.0);
    }

    /// Screen point (y down, relative to the view) to model coordinates, before the model is
    /// undone.
    pub fn screen_to_model(&self, x: f32, y: f32) -> Vector3D {
        let aspect = self.width as f64 / self.height as f64;
        let (x_min, x_max) = (-aspect * self.view_scale, aspect * self.view_scale);
        let (y_min, y_max) = (-self.view_scale, self.view_scale);
        let mut p = Vector3D::new(
            x_min + (x as f64 / self.width as f64) * (x_max - x_min),
            y_max - (y as f64 / self.height as f64) * (y_max - y_min),
        );
        p.rotate_xy(-self.rotation);
        p
    }

    /// Screen point to the standard model (Poincaré disk / stereographic plane).
    pub fn screen_to_gl(&self, model: Model, x: f32, y: f32) -> Vector3D {
        model.to_standard(self.screen_to_model(x, y))
    }

    /// Screen point to puzzle coordinates (undoing the view isometry). `None` outside the
    /// Poincaré disk.
    pub fn space_coords_no_view(&mut self, puzzle: &Puzzle, model: Model, x: f32, y: f32) -> Option<Vector3D> {
        self.recenter(puzzle);
        let space = self.screen_to_gl(model, x, y);

        // Same clamp as for panning.
        if puzzle.config.geometry() == Geometry::Hyperbolic && space.abs() > 0.98 {
            return None;
        }
        Some(self.isometry.inverse().apply(space))
    }

    /// Moves the view to the copy of the home cell nearest the center, keeping numbers well
    /// behaved on infinite tilings.
    pub fn recenter(&mut self, puzzle: &Puzzle) {
        let Some(closest) = self.closest.take() else {
            return;
        };
        if puzzle.cells.get(closest).is_some_and(|c| !c.is_master()) {
            let recenter = puzzle.cells[closest].isometry.inverse();
            self.isometry = &self.isometry * &recenter;
        }
    }

    pub fn drag(&mut self, puzzle: Option<&Puzzle>, model: Model, drag: DragData) {
        self.perform_drag(puzzle, model, drag);
        self.recent_drags.push_back(drag);
        if self.recent_drags.len() > 2 {
            self.recent_drags.pop_front();
        }
    }

    fn perform_drag(&mut self, puzzle: Option<&Puzzle>, model: Model, drag: DragData) {
        match drag.button {
            DragButton::Primary => {
                if let Some(p) = puzzle {
                    self.recenter(p);
                }
                let p1 = self.screen_to_gl(model, drag.x - drag.x_diff, drag.y - drag.y_diff);
                let p2 = self.screen_to_gl(model, drag.x, drag.y);
                match self.geometry {
                    Geometry::Hyperbolic => {
                        // Clamp it.
                        const MAX: f64 = 0.98;
                        if p1.abs() > MAX || p2.abs() > MAX {
                            return;
                        }
                        // Don Hatch's pure translation, applied first.
                        let mut pan = Mobius::default();
                        pan.pure_translation(Geometry::Hyperbolic, p1, p2);
                        let mut m = pan * self.isometry.mobius;
                        // Numerical stability hack: things explode after panning for a while
                        // otherwise.
                        m.round(5);
                        self.isometry.mobius = m;
                    }
                    Geometry::Euclidean | Geometry::Spherical => self.geodesic_pan(p1, p2),
                }
            }
            DragButton::Middle => self.rotation += drag.rotation as f64,
            DragButton::Secondary => {
                self.view_scale += 3.0 * self.view_scale * drag.y_percent as f64;
                let smallest = 0.02;
                let mut largest = 3.0;
                if self.geometry == Geometry::Spherical {
                    largest *= 20.0;
                }
                if model == Model::Hyperbolic(HyperbolicModel::Orthographic) {
                    largest *= 5.0;
                }
                self.view_scale = self.view_scale.clamp(smallest, largest);
            }
        }
    }

    fn geodesic_pan(&mut self, p1: Vector3D, p2: Vector3D) {
        let inverse = self.isometry.inverse();
        let mut pan = Mobius::default();
        pan.geodesic(self.geometry, inverse.apply(p1), inverse.apply(p2), 1.0);
        self.isometry.mobius = self.isometry.mobius * pan;
    }

    /// Called when a drag ends: starts gliding if the drag was a flick.
    pub fn release(&mut self, flick: bool, gliding: f64) {
        let drags = std::mem::take(&mut self.recent_drags);
        if !flick || drags.len() < 2 || glide_factor(gliding) == 0.0 {
            return;
        }
        let n = drags.len() as f32;
        let mut spin = *drags.back().unwrap();
        spin.x_diff = drags.iter().map(|d| d.x_diff).sum::<f32>() / n;
        spin.y_diff = drags.iter().map(|d| d.y_diff).sum::<f32>() / n;
        spin.y_percent = drags.iter().map(|d| d.y_percent).sum::<f32>() / n;
        self.spin = Some(spin);
        self.spin_accumulator = 0.0;
    }

    pub fn stop_spinning(&mut self) {
        self.spin = None;
    }

    pub fn spinning(&self) -> bool {
        self.spin.is_some()
    }

    /// Advances gliding by some seconds. Returns true if the view moved.
    pub fn step_spin(&mut self, puzzle: Option<&Puzzle>, model: Model, dt: f64, gliding: f64) -> bool {
        let mut moved = false;
        self.spin_accumulator += dt;
        while self.spin_accumulator >= SPIN_INTERVAL {
            self.spin_accumulator -= SPIN_INTERVAL;
            let Some(mut spin) = self.spin else {
                return moved;
            };

            let glide = glide_factor(gliding) as f32;
            spin.x_diff *= glide;
            spin.y_diff *= glide;
            spin.y_percent *= glide;

            // (The original stopped when either component was small, which ended most
            // horizontal or vertical flicks immediately.)
            if spin.x_diff.hypot(spin.y_diff) < 0.01 {
                self.spin = None;
                return moved;
            }
            self.spin = Some(spin);
            self.perform_drag(puzzle, model, spin);
            moved = true;
        }
        moved
    }
}

fn glide_factor(gliding: f64) -> f64 {
    gliding.powf(0.15).clamp(0.0, 1.0)
}
