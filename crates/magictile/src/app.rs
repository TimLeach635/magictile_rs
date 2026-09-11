//! The application: puzzle picking and building, input handling, and driving rendering.

use crate::render::{FrameJob, PuzzleCallback, Renderer};
use crate::scene::{self, RenderData, SceneContext};
use crate::selftest::SelfTest;
use crate::settings::Settings;
use crate::view::{DragButton, DragData, Model, View};
use eframe::egui::{self, Color32, Key, PointerButton, Pos2, Rect, Sense};
use eframe::egui_wgpu;
use magictile_core::controller::rotation_step;
use magictile_core::library::{MenuKind, MenuNode};
use magictile_core::macros::Macro;
use magictile_core::twist_data::TwistDataId;
use magictile_core::{
    BuildError, BuildProgress, Library, Puzzle, PuzzleConfig, SingleTwist, TwistController, slice_mask,
};
use r3::Geometry;
use r3::models::{HyperbolicModel, SphericalModel};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// Seconds per twist animation step (the original's timer interval).
const TWIST_INTERVAL: f64 = 0.030;

struct LoadedPuzzle {
    puzzle: Puzzle,
    data: RenderData,
    generation: u64,
}

enum BuildMessage {
    Status(String),
    Done(Box<Result<(Puzzle, RenderData), BuildError>>),
}

struct BuildJob {
    receiver: Receiver<BuildMessage>,
    cancel: Arc<AtomicBool>,
    status: String,
    started: Instant,
}

struct ThreadProgress {
    sender: std::sync::mpsc::Sender<BuildMessage>,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
}

impl BuildProgress for ThreadProgress {
    fn status(&mut self, message: &str) {
        let _ = self.sender.send(BuildMessage::Status(message.to_string()));
        self.ctx.request_repaint();
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Mouse state for distinguishing clicks from drags (as the original's `MouseHandler`).
#[derive(Default)]
struct Mouse {
    /// When the last drag movement happened (a release soon after is a flick).
    last_drag: Option<Instant>,
    /// Pressing while gliding stops it, and shouldn't also count as a click.
    skip_click: bool,
    hover: Option<Pos2>,
}

pub struct MagicTileApp {
    library: Library,
    settings: Settings,
    loaded: Option<LoadedPuzzle>,
    controller: TwistController,
    view: View,
    macros: Vec<Macro>,
    building: Option<BuildJob>,
    message: Option<(String, Instant)>,
    generation: u64,
    textures_valid: Vec<bool>,
    closest_twist: Option<TwistDataId>,
    closest_geodesic_seg: i32,
    mouse: Mouse,
    twist_accumulator: f64,
    last_frame: Instant,
    wait_radius: f64,
    show_tree: bool,
    rng: rand::rngs::StdRng,
    selftest: Option<SelfTest>,
}

impl MagicTileApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        if let Some(render_state) = &cc.wgpu_render_state {
            Renderer::install(render_state);
        }

        let mut app = MagicTileApp {
            library: Library::load_standard(),
            settings: Settings::default(),
            loaded: None,
            controller: TwistController::default(),
            view: View::default(),
            macros: Vec::new(),
            building: None,
            message: None,
            generation: 0,
            textures_valid: Vec::new(),
            closest_twist: None,
            closest_geodesic_seg: -1,
            mouse: Mouse::default(),
            twist_accumulator: 0.0,
            last_frame: Instant::now(),
            wait_radius: 0.0,
            show_tree: true,
            rng: rand::make_rng(),
            selftest: SelfTest::from_env(),
        };

        // Start with the puzzle given on the command line (by ID or name), or the classic {7,3}.
        let requested = std::env::args().nth(1).filter(|a| !a.starts_with("--"));
        let config = requested.as_deref().and_then(|r| app.library.find(r)).cloned().unwrap_or_default();
        app.start_build(&cc.egui_ctx, config);
        app
    }

    fn start_build(&mut self, ctx: &egui::Context, config: PuzzleConfig) {
        if let Some(job) = &self.building {
            job.cancel.store(true, Ordering::Relaxed);
        }
        let (sender, receiver) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut progress = ThreadProgress { sender: sender.clone(), cancel: cancel.clone(), ctx: ctx.clone() };
        let name = config.display_name.clone();
        std::thread::spawn(move || {
            let result = Puzzle::build(config, &mut progress).map(|p| {
                let data = RenderData::new(&p);
                (p, data)
            });
            let _ = sender.send(BuildMessage::Done(Box::new(result)));
            progress.ctx.request_repaint();
        });
        self.building =
            Some(BuildJob { receiver, cancel, status: format!("Building {name}..."), started: Instant::now() });
        self.wait_radius = 0.0;
    }

    fn poll_build(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.building else {
            return;
        };
        while let Ok(message) = job.receiver.try_recv() {
            match message {
                BuildMessage::Status(s) => job.status = s,
                BuildMessage::Done(result) => {
                    self.building = None;
                    match *result {
                        Ok((puzzle, data)) => self.puzzle_loaded(ctx, puzzle, data),
                        Err(BuildError::Cancelled) => {}
                        Err(e) => self.show_message(format!("So sorry, there was a puzzle build failure: {e}")),
                    }
                    return;
                }
            }
        }
    }

    fn puzzle_loaded(&mut self, ctx: &egui::Context, puzzle: Puzzle, data: RenderData) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("MagicTile - {}", puzzle.config.display_name)));
        self.generation += 1;
        self.view.reset(puzzle.config.geometry());
        self.controller.reset();
        self.macros.clear();
        self.textures_valid = vec![false; puzzle.masters.len()];
        self.closest_twist = None;
        self.loaded = Some(LoadedPuzzle { puzzle, data, generation: self.generation });
        if let Some(pos) = self.mouse.hover {
            self.update_closest_twist(pos);
        }
    }

    fn show_message(&mut self, message: String) {
        self.message = Some((message, Instant::now()));
    }

    fn model(&self) -> Model {
        let geometry = self.loaded.as_ref().map_or(Geometry::Hyperbolic, |l| l.puzzle.config.geometry());
        Model::for_puzzle(geometry, self.settings.hyperbolic_model, self.settings.spherical_model)
    }

    fn invalidate_all(&mut self) {
        self.textures_valid.iter_mut().for_each(|v| *v = false);
    }

    fn invalidate_twist(&mut self) {
        let (Some(loaded), Some(twist)) = (&self.loaded, self.controller.current_twist()) else {
            return;
        };
        for identified in std::iter::once(twist.identified).chain(twist.identified_systolic) {
            for master in loaded.puzzle.affected_master_cells(identified) {
                let index = loaded.puzzle.cells[master].index_of_master;
                if let Some(v) = usize::try_from(index).ok().and_then(|i| self.textures_valid.get_mut(i)) {
                    *v = false;
                }
            }
        }
    }

    /// The slice mask from the number keys held down (slice 1 if none).
    fn slice_mask(&self, ctx: &egui::Context) -> i32 {
        let keys = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
            Key::Num0,
        ];
        let mask = ctx.input(|i| {
            keys.iter()
                .enumerate()
                .filter(|(_, k)| i.key_down(**k) && !i.modifiers.command)
                .fold(0, |m, (s, _)| m | slice_mask::slice_to_mask(s as i32 + 1))
        });
        if mask == 0 { 1 } else { mask }
    }

    fn update_closest_twist(&mut self, pos: Pos2) -> bool {
        let model = self.model();
        let Some(loaded) = &self.loaded else {
            return false;
        };
        let previous = (self.closest_twist, self.closest_geodesic_seg);
        let puzzle = &loaded.puzzle;
        match self.view.space_coords_no_view(puzzle, model, pos.x, pos.y) {
            Some(p) => {
                self.closest_twist = puzzle.closest_twisting_circles(p);
                if puzzle.config.systolic()
                    && let Some(td) = self.closest_twist
                    && let Some(pants) = &puzzle.twist_data[td].pants
                {
                    self.closest_geodesic_seg = pants.closest_geodesic_seg(p);
                }
            }
            None => self.closest_twist = None,
        }
        previous != (self.closest_twist, self.closest_geodesic_seg)
    }

    fn click(&mut self, ctx: &egui::Context, pos: Pos2, button: PointerButton) {
        let model = self.model();
        let modifiers = ctx.input(|i| i.modifiers);
        let slice_mask = self.slice_mask(ctx);
        let Some(loaded) = &mut self.loaded else {
            return;
        };
        let puzzle = &mut loaded.puzzle;
        let left = button == PointerButton::Primary;

        // Macros.
        if modifiers.alt {
            let Some(space) = self.view.space_coords_no_view(puzzle, model, pos.x, pos.y) else {
                return;
            };
            let Some(cell) = puzzle.closest_cell(space) else {
                return;
            };
            let reflected = self.view.isometry.reflected();
            if modifiers.command && left {
                self.controller.working_macro.setup_mobius(puzzle, cell, space, reflected);
                self.controller.working_macro.start_recording();
            } else if let Some(m) = self.macros.last() {
                let transformed = m.transform(puzzle, cell, space, reflected);
                self.controller.apply_macro(puzzle, &transformed, !left);
                self.invalidate_all();
            } else {
                self.show_message("No macros recorded yet (Ctrl+Alt+click starts recording one).".into());
            }
            return;
        }

        // Lights on.
        if puzzle.config.is_toggling() {
            if let Some(space) = self.view.space_coords_no_view(puzzle, model, pos.x, pos.y)
                && let Some(cell) = puzzle.closest_cell(space)
            {
                if self.controller.toggle(puzzle, cell).solved {
                    self.show_message("Solved!".into());
                }
                self.invalidate_all();
            }
            return;
        }

        if !self.update_closest_twist(pos) && self.closest_twist.is_none() {
            return;
        }
        let Some(loaded) = &mut self.loaded else {
            return;
        };
        let puzzle = &mut loaded.puzzle;
        let Some(td_id) = self.closest_twist else {
            return;
        };
        let td = &puzzle.twist_data[td_id];
        let Some(identified) = td.identified else {
            return;
        };

        let mut twist = SingleTwist { identified, left_click: left, ..Default::default() };
        if puzzle.config.systolic() {
            twist.slice_mask = slice_mask::dir_seg_to_mask(self.closest_geodesic_seg);

            // Earthquakes chop off a pants leg, which moves with a second set of twist data.
            if puzzle.config.earthquake()
                && let Some((identified, mask)) = puzzle.earthquake_companion(td_id, self.closest_geodesic_seg)
            {
                twist.identified_systolic = Some(identified);
                twist.slice_mask_systolic = mask;
            }
        } else {
            twist.slice_mask = slice_mask;
        }

        // Clicking mirrored tiles (non-orientable puzzles) should still turn the tiles you
        // left-click counterclockwise.
        if self.view.isometry.reflected() ^ td.reverse {
            twist.reverse_twist();
        }
        self.controller.start_rotate(puzzle, twist);
    }

    fn handle_view_input(&mut self, ctx: &egui::Context, response: &egui::Response, rect: Rect) {
        let model = self.model();
        let to_view = |p: Pos2| p - rect.min.to_vec2();

        // Hover highlighting.
        if let Some(pos) = response.hover_pos() {
            let pos = to_view(pos);
            if self.mouse.hover != Some(pos) {
                self.mouse.hover = Some(pos);
                if !self.controller.twisting() && !response.dragged() && !self.view.spinning() {
                    self.update_closest_twist(pos);
                }
            }
        } else if self.mouse.hover.take().is_some() {
            self.closest_twist = None;
        }

        if response.is_pointer_button_down_on() && ctx.input(|i| i.pointer.any_pressed()) {
            // Pressing while gliding stops it, and doesn't count as a click.
            self.mouse.skip_click = self.view.spinning();
            self.view.stop_spinning();
        }

        // Drags.
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
            self.view.drag(self.loaded.as_ref().map(|l| &l.puzzle), model, drag);
            self.mouse.last_drag = Some(Instant::now());
        }
        if response.drag_stopped() {
            // Using elapsed time works much better than how far we moved.
            let flick = self.mouse.last_drag.is_some_and(|t| t.elapsed() < Duration::from_millis(50));
            self.view.release(flick, self.settings.gliding);
            self.mouse.skip_click = false;
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
                self.view.drag(None, model, drag);
            }
        }

        // Clicks.
        for button in [PointerButton::Primary, PointerButton::Secondary] {
            if response.clicked_by(button)
                && !std::mem::take(&mut self.mouse.skip_click)
                && let Some(pos) = response.interact_pointer_pos()
            {
                self.click(ctx, to_view(pos), button);
            }
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let (undo, redo, f7, escape, scramble1, scramble5) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, Key::Z),
                i.consume_key(egui::Modifiers::COMMAND, Key::Y),
                i.consume_key(egui::Modifiers::NONE, Key::F7),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
                i.consume_key(egui::Modifiers::COMMAND, Key::Num1),
                i.consume_key(egui::Modifiers::COMMAND, Key::Num5),
            )
        });
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if f7 {
            self.cycle_model();
        }
        if escape {
            self.controller.solving = false;
            self.controller.setup_moves.reset();
            self.controller.working_macro.reset();
        }
        if scramble1 {
            self.scramble(1);
        }
        if scramble5 {
            self.scramble(5);
        }
    }

    fn cycle_model(&mut self) {
        let Some(loaded) = &self.loaded else {
            return;
        };
        match loaded.puzzle.config.geometry() {
            Geometry::Spherical => {
                use SphericalModel::*;
                self.settings.spherical_model = match self.settings.spherical_model {
                    Sterographic => Gnomonic,
                    Gnomonic => Fisheye,
                    Fisheye => HemisphereDisks,
                    HemisphereDisks => Sterographic,
                };
            }
            Geometry::Hyperbolic => {
                use HyperbolicModel::*;
                self.settings.hyperbolic_model = match self.settings.hyperbolic_model {
                    Poincare => Klein,
                    Klein => UpperHalfPlane,
                    UpperHalfPlane => Orthographic,
                    Orthographic => Poincare,
                };
            }
            Geometry::Euclidean => {}
        }
    }

    fn scramble(&mut self, n: usize) {
        if let Some(loaded) = &mut self.loaded
            && !self.controller.twisting()
        {
            self.controller.scramble(&mut loaded.puzzle, n, &mut self.rng);
            self.invalidate_all();
        }
    }

    fn undo(&mut self) {
        if let Some(loaded) = &mut self.loaded {
            self.controller.undo(&mut loaded.puzzle);
            self.invalidate_all();
        }
    }

    fn redo(&mut self) {
        if let Some(loaded) = &mut self.loaded {
            self.controller.redo(&mut loaded.puzzle);
            self.invalidate_all();
        }
    }

    fn solve(&mut self) {
        if let Some(loaded) = &mut self.loaded {
            self.controller.solve(&mut loaded.puzzle);
            self.invalidate_all();
        }
    }

    fn reset_state(&mut self) {
        if let Some(loaded) = &mut self.loaded
            && !self.controller.twisting()
        {
            self.controller.reset_state(&mut loaded.puzzle);
            self.invalidate_all();
        }
    }

    fn stop_recording_macro(&mut self) {
        let m = &mut self.controller.working_macro;
        if !m.recording {
            return;
        }
        m.stop_recording();
        if m.twists().is_empty() {
            self.show_message("There were no twists recorded for the macro.".into());
            return;
        }
        let mut clone = m.clone();
        clone.display_name = format!("Macro {}", self.macros.len() + 1);
        clone.clear_start_end_markings();
        let name = clone.display_name.clone();
        self.macros.push(clone);
        self.show_message(format!("Recorded {name}. Alt+click applies it (Alt+right click reverses)."));
    }

    /// Advances twist animation and gliding.
    fn animate(&mut self, dt: f64) -> bool {
        let model = self.model();
        let mut active = false;
        if let Some(loaded) = &mut self.loaded
            && self.controller.twisting()
        {
            active = true;
            let order = self.controller.current_twist_order(&loaded.puzzle);
            let step = rotation_step(self.settings.rotation_rate, order);
            self.twist_accumulator += dt;
            let ticks = (self.twist_accumulator / TWIST_INTERVAL).floor();
            self.twist_accumulator -= ticks * TWIST_INTERVAL;
            if ticks > 0.0 {
                self.invalidate_twist();
                let Some(loaded) = &mut self.loaded else { unreachable!() };
                if let Some(done) = self.controller.advance(&mut loaded.puzzle, step * ticks) {
                    if done.solved {
                        self.show_message("Solved!".into());
                    }
                    self.invalidate_all();
                }
            }
        } else {
            self.twist_accumulator = 0.0;
        }

        if self.view.spinning() {
            active = true;
            self.view.step_spin(self.loaded.as_ref().map(|l| &l.puzzle), model, dt, self.settings.gliding);
        }
        active
    }

    fn menu_bar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                ui.checkbox(&mut self.show_tree, "Show puzzle tree");
                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Undo (Ctrl+Z)").clicked() {
                    self.undo();
                }
                if ui.button("Redo (Ctrl+Y)").clicked() {
                    self.redo();
                }
                ui.separator();
                if ui.button("Solve").clicked() {
                    self.solve();
                }
                if ui.button("Reset state").clicked() {
                    self.reset_state();
                }
            });
            ui.menu_button("Scramble", |ui| {
                for n in [1, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000] {
                    let label = match n {
                        1 => "1 (Ctrl+1)".to_string(),
                        5 => "5 (Ctrl+5)".to_string(),
                        n => n.to_string(),
                    };
                    if ui.button(label).clicked() {
                        self.scramble(n);
                    }
                }
            });
            ui.menu_button("Macros", |ui| {
                ui.label("Ctrl+Alt+click: start recording a macro");
                if ui.button("Stop recording macro").clicked() {
                    self.stop_recording_macro();
                }
                ui.label("Alt+click: apply the last macro (Alt+right click reverses)");
                ui.separator();
                if ui.button("Start setup moves").clicked() {
                    self.controller.setup_moves.start_recording();
                }
                if ui.button("End setup moves").clicked() {
                    self.controller.setup_moves.stop_recording();
                }
                if ui.button("Unwind setup moves").clicked()
                    && let Some(loaded) = &mut self.loaded
                {
                    self.controller.unwind(&mut loaded.puzzle);
                    self.invalidate_all();
                }
                if ui.button("Commutator").clicked()
                    && let Some(loaded) = &mut self.loaded
                {
                    self.controller.commutator(&mut loaded.puzzle);
                    self.invalidate_all();
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Reset view").clicked()
                    && let Some(loaded) = &self.loaded
                {
                    self.view.reset(loaded.puzzle.config.geometry());
                }
                ui.separator();
                ui.label("Hyperbolic model (F7 cycles):");
                for (name, m) in [
                    ("Poincaré disk", HyperbolicModel::Poincare),
                    ("Klein", HyperbolicModel::Klein),
                    ("Upper half plane", HyperbolicModel::UpperHalfPlane),
                    ("Orthographic", HyperbolicModel::Orthographic),
                ] {
                    ui.radio_value(&mut self.settings.hyperbolic_model, m, name);
                }
                ui.label("Spherical model (F7 cycles):");
                for (name, m) in [
                    ("Stereographic", SphericalModel::Sterographic),
                    ("Gnomonic", SphericalModel::Gnomonic),
                    ("Fisheye", SphericalModel::Fisheye),
                    ("Hemisphere disks", SphericalModel::HemisphereDisks),
                ] {
                    ui.radio_value(&mut self.settings.spherical_model, m, name);
                }
                ui.separator();
                ui.checkbox(&mut self.settings.highlight_twisting_circles, "Highlight twisting circles");
                ui.checkbox(&mut self.settings.show_only_fundamental, "Show only fundamental");
                ui.checkbox(&mut self.settings.enable_texture_mipmaps, "Texture mipmaps");
                ui.add(egui::Slider::new(&mut self.settings.rotation_rate, 0.0..=1.0).text("Rotation rate"));
                ui.add(egui::Slider::new(&mut self.settings.gliding, 0.0..=1.0).text("Gliding"));
            });
        });
    }

    fn puzzle_tree(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let mut chosen = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for node in &self.library.root {
                tree_node(ui, node, &mut chosen);
            }
        });
        if let Some(i) = chosen {
            let config = self.library.configs[i].clone();
            self.start_build(ctx, config);
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (twists, scrambles, total) = self.loaded.as_ref().map_or((0, 0, 0), |l| {
                let h = &l.puzzle.history;
                let total = h.all_moves_count();
                (total.saturating_sub(h.scrambles), h.scrambles, total)
            });
            ui.label(format!("Twists: {twists}    ●    Scrambles: {scrambles}    ●    Total: {total}"));
            ui.separator();
            let status = if let Some(job) = &self.building {
                job.status.clone()
            } else if self.controller.setup_moves.recording_setup() {
                "Recording Setup Moves".into()
            } else if self.controller.setup_moves.recording_commutator() {
                "Recording Commutator Moves".into()
            } else if self.controller.working_macro.recording {
                "Recording Macro".into()
            } else {
                self.loaded.as_ref().map(|l| l.puzzle.topology.clone()).unwrap_or_default()
            };
            ui.label(status);
            if let Some((message, _)) = &self.message {
                ui.separator();
                ui.colored_label(Color32::from_rgb(255, 200, 80), message);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{} Puzzles and {} Tilings Available",
                    self.library.num_puzzles, self.library.num_tilings
                ));
            });
        });
    }

    fn puzzle_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        self.view.set_size(rect.width(), rect.height());
        self.handle_view_input(ctx, &response, rect);

        if let Some(job) = &self.building
            && (self.loaded.is_none() || job.started.elapsed() > Duration::from_millis(300))
        {
            self.paint_waiting(ui, rect);
            return;
        }
        let Some(loaded) = &self.loaded else {
            ui.painter().rect_filled(rect, 0.0, self.settings.color_bg);
            return;
        };

        let model = self.model();
        let ctx_scene = SceneContext {
            puzzle: &loaded.puzzle,
            data: &loaded.data,
            controller: &self.controller,
            settings: &self.settings,
            model,
            slice_mask: self.slice_mask(ctx),
            closest_twist: if self.controller.twisting() { None } else { self.closest_twist },
            closest_geodesic_seg: self.closest_geodesic_seg,
        };

        let cell_jobs: Vec<(u32, scene::DrawList)> = if loaded.puzzle.is_spherical() {
            Vec::new()
        } else {
            self.textures_valid
                .iter()
                .enumerate()
                .filter(|(_, valid)| !**valid)
                .map(|(i, _)| (i as u32, scene::build_cell_texture(&ctx_scene, i)))
                .collect()
        };
        let ppp = ctx.pixels_per_point();
        let (view_list, closest) = scene::build_view(&ctx_scene, &self.view, ppp);
        self.view.closest = closest;
        self.textures_valid.iter_mut().for_each(|v| *v = true);

        let size_px = [(rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32];
        let job = FrameJob {
            puzzle_generation: loaded.generation,
            num_layers: loaded.puzzle.masters.len() as u32,
            cell_jobs,
            mipmaps: self.settings.enable_texture_mipmaps,
            view: view_list,
            size_px,
        };
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, PuzzleCallback { job: Arc::new(job) }));
    }

    /// Expanding circles while a puzzle builds.
    fn paint_waiting(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, self.settings.color_bg);
        self.wait_radius += 0.01;
        if self.wait_radius > 5.0 {
            self.wait_radius -= 1.0;
        }
        let pixels_per_unit = rect.height() / (2.0 * 1.1);
        let mut radius = self.wait_radius;
        while radius >= 0.0 {
            painter.circle_stroke(
                rect.center(),
                radius as f32 * pixels_per_unit,
                egui::Stroke::new(2.0, Color32::from_rgb(0x48, 0x3D, 0x8B)),
            );
            radius -= 0.25;
        }
    }
}

fn tree_node(ui: &mut egui::Ui, node: &MenuNode, chosen: &mut Option<usize>) {
    match &node.kind {
        MenuKind::Group(children) => {
            egui::CollapsingHeader::new(&node.label).id_salt(node as *const MenuNode).show(ui, |ui| {
                for child in children {
                    tree_node(ui, child, chosen);
                }
            });
        }
        MenuKind::Puzzle(index) => {
            if ui.selectable_label(false, &node.label).double_clicked() || ui.ctx().input(|_| false) {
                *chosen = Some(*index);
            }
        }
    }
}

impl eframe::App for MagicTileApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let building = self.building.is_some();
        if let Some(selftest) = &mut self.selftest {
            selftest.hook(ctx, raw_input, building);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().min(0.25);
        self.last_frame = now;

        if let Some(selftest) = &mut self.selftest {
            selftest.frame(&ctx);
        }
        self.poll_build(&ctx);
        self.handle_keys(&ctx);
        if self.message.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(6)) {
            self.message = None;
        }

        egui::Panel::top("menu").show_inside(ui, |ui| self.menu_bar(&ctx, ui));
        egui::Panel::bottom("status").show_inside(ui, |ui| self.status_bar(ui));
        if self.show_tree {
            egui::Panel::left("tree").resizable(true).default_size(320.0).show_inside(ui, |ui| {
                ui.heading("Puzzles");
                ui.label("Double-click a puzzle to load it.");
                ui.separator();
                self.puzzle_tree(&ctx, ui);
            });
        }

        let animating = self.animate(dt);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(ui, |ui| self.puzzle_view(&ctx, ui));

        if animating || self.building.is_some() || self.message.is_some() {
            ctx.request_repaint();
        }
    }
}
