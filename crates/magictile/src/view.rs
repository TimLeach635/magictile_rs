//! The 2D view: panning, rotating and zooming (the original's `MouseMotion`, 2D parts).

use eframe::egui::{self, PointerButton, Pos2, Rect};
use r3::models::{self, HyperbolicModel, SphericalModel};
use r3::{Geometry, Isometry, Mobius, Transform, Vector3D};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

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
    /// A symmetry of the tiling that moves the home tile back near the center, found while
    /// rendering (from the copy of the home tile drawn closest to the center). Applied before
    /// the next pan, keeping numbers well behaved on infinite tilings.
    pub recenter: Option<Isometry>,
    /// When the last drag movement happened (a release soon after is a flick).
    last_drag: Option<Instant>,
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
            recenter: None,
            last_drag: None,
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

    /// Model coordinates to a screen point, the inverse of [`View::screen_to_model`].
    pub fn model_to_screen(&self, mut p: Vector3D) -> (f32, f32) {
        p.rotate_xy(self.rotation);
        let aspect = self.width as f64 / self.height as f64;
        let x = (p.x + aspect * self.view_scale) / (2.0 * aspect * self.view_scale) * self.width as f64;
        let y = (self.view_scale - p.y) / (2.0 * self.view_scale) * self.height as f64;
        (x as f32, y as f32)
    }

    /// Screen point to the standard model (Poincaré disk / stereographic plane).
    pub fn screen_to_gl(&self, model: Model, x: f32, y: f32) -> Vector3D {
        model.to_standard(self.screen_to_model(x, y))
    }

    /// Screen point to tiling coordinates (undoing the view isometry). `None` outside the
    /// Poincaré disk.
    pub fn space_coords_no_view(&mut self, model: Model, x: f32, y: f32) -> Option<Vector3D> {
        self.apply_recenter();
        let space = self.screen_to_gl(model, x, y);

        // Same clamp as for panning.
        if self.geometry == Geometry::Hyperbolic && space.abs() > 0.98 {
            return None;
        }
        Some(self.isometry.inverse().apply(space))
    }

    /// Applies a pending recentering (see [`View::recenter`]). The picture doesn't change.
    pub fn apply_recenter(&mut self) {
        if let Some(recenter) = self.recenter.take() {
            self.isometry = &self.isometry * &recenter;
        }
    }

    pub fn drag(&mut self, model: Model, drag: DragData) {
        self.perform_drag(model, drag);
        self.recent_drags.push_back(drag);
        if self.recent_drags.len() > 2 {
            self.recent_drags.pop_front();
        }
    }

    fn perform_drag(&mut self, model: Model, drag: DragData) {
        match drag.button {
            DragButton::Primary => {
                self.apply_recenter();
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
    pub fn step_spin(&mut self, model: Model, dt: f64, gliding: f64) -> bool {
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
            self.perform_drag(model, spin);
            moved = true;
        }
        moved
    }
}

/// What [`View::navigate`] saw happen this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Navigation {
    /// A button was pressed on the view; the flag says whether it stopped gliding.
    pub pressed: Option<bool>,
    /// A drag just ended.
    pub drag_stopped: bool,
}

impl View {
    /// Mouse navigation for a view occupying `rect`: left drag pans, middle drag rotates, right
    /// drag and the scroll wheel zoom, and flicks glide.
    pub fn navigate(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        rect: Rect,
        model: Model,
        gliding: f64,
    ) -> Navigation {
        let mut nav = Navigation::default();
        let to_view = |p: Pos2| p - rect.min.to_vec2();

        if response.is_pointer_button_down_on() && ctx.input(|i| i.pointer.any_pressed()) {
            // Pressing while gliding stops it.
            nav.pressed = Some(self.spinning());
            self.stop_spinning();
        }

        for (egui_button, button) in [
            (PointerButton::Primary, DragButton::Primary),
            (PointerButton::Middle, DragButton::Middle),
            (PointerButton::Secondary, DragButton::Secondary),
        ] {
            if !response.dragged_by(egui_button) {
                continue;
            }
            let delta = response.drag_delta();
            if delta == egui::Vec2::ZERO {
                continue;
            }
            let Some(pos) = response.interact_pointer_pos().map(to_view) else {
                continue;
            };
            let (w, h) = (rect.width(), rect.height());
            let (x1, y1) = (pos.x - delta.x - w / 2.0, h / 2.0 - (pos.y - delta.y));
            let (x2, y2) = (pos.x - w / 2.0, h / 2.0 - pos.y);
            let drag = DragData {
                x: pos.x,
                y: pos.y,
                x_diff: delta.x,
                y_diff: delta.y,
                y_percent: delta.y / h,
                rotation: y2.atan2(x2) - y1.atan2(x1),
                button,
            };
            self.drag(model, drag);
            self.last_drag = Some(Instant::now());
        }
        if response.drag_stopped() {
            // Using elapsed time works much better than how far we moved.
            let flick = self.last_drag.is_some_and(|t| t.elapsed() < Duration::from_millis(50));
            self.release(flick, gliding);
            nav.drag_stopped = true;
        }

        // Zooming with the scroll wheel.
        if response.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let drag = DragData {
                    x: 0.0,
                    y: 0.0,
                    x_diff: 0.0,
                    y_diff: 0.0,
                    y_percent: -scroll / rect.height(),
                    rotation: 0.0,
                    button: DragButton::Secondary,
                };
                self.drag(model, drag);
            }
        }
        nav
    }
}

fn glide_factor(gliding: f64) -> f64 {
    gliding.powf(0.15).clamp(0.0, 1.0)
}
