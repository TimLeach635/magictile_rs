//! Scripted input and window screenshots, for testing the interactive app without a person.
//!
//! `MAGICTILE_SELFTEST=prefix MAGICTILE_SCRIPT="..." magictile [puzzle]` runs a script of steps
//! separated by `;`, saving screenshots as `prefix1.png`, `prefix2.png`, ...
//!
//! Steps (coordinates are fractions of the window size):
//! `built` (wait until no puzzle is building), `wait:FRAMES`, `move:X,Y`, `click:X,Y`,
//! `rclick:X,Y`, `drag:X1,Y1,X2,Y2`, `key:NAME` (press and release), `keydown:NAME`, `keyup:NAME`,
//! `shot`, `quit`.

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, RawInput};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
enum Step {
    Built,
    Wait(u32),
    Move(f32, f32),
    Click(f32, f32, PointerButton),
    Drag(f32, f32, f32, f32),
    /// Whether to press, release, or both.
    Key(Key, Option<bool>),
    Shot,
    Quit,
}

pub struct SelfTest {
    prefix: String,
    steps: VecDeque<Step>,
    pending: VecDeque<Vec<Event>>,
    shots: u32,
    /// Viewport commands to send during the frame (commands sent before it starts are lost).
    commands: Vec<egui::ViewportCommand>,
}

impl SelfTest {
    pub fn from_env() -> Option<SelfTest> {
        let prefix = std::env::var("MAGICTILE_SELFTEST").ok()?;
        let script = std::env::var("MAGICTILE_SCRIPT").unwrap_or_else(|_| "built;wait:10;shot;quit".into());
        let steps = script.split(';').filter(|s| !s.trim().is_empty()).map(parse_step).collect();
        Some(SelfTest { prefix, steps, pending: VecDeque::new(), shots: 0, commands: Vec::new() })
    }

    /// Injects this frame's scripted events. `building` says whether a puzzle is being built.
    pub fn hook(&mut self, ctx: &egui::Context, raw: &mut RawInput, building: bool) {
        ctx.request_repaint();
        let size = raw.screen_rect.map_or(egui::vec2(1280.0, 860.0), |r| r.size());
        let at = |x: f32, y: f32| Pos2::new(x * size.x, y * size.y);

        // Multi-frame events (e.g. a drag) are spread across frames.
        if let Some(events) = self.pending.pop_front() {
            raw.events.extend(events);
            return;
        }

        let Some(step) = self.steps.front().cloned() else {
            return;
        };
        if !matches!(step, Step::Wait(_) | Step::Built) {
            let paints = crate::render::PAINTS.load(std::sync::atomic::Ordering::Relaxed);
            eprintln!("selftest: {step:?} (after {paints} painted frames)");
        }
        match step {
            Step::Built if building => return,
            Step::Wait(n) if n > 0 => {
                self.steps[0] = Step::Wait(n - 1);
                return;
            }
            Step::Move(x, y) => raw.events.push(Event::PointerMoved(at(x, y))),
            Step::Click(x, y, button) => {
                let pos = at(x, y);
                raw.events.push(Event::PointerMoved(pos));
                raw.events.push(pointer(pos, button, true));
                self.pending.push_back(vec![pointer(pos, button, false)]);
            }
            Step::Drag(x1, y1, x2, y2) => {
                let (from, to) = (at(x1, y1), at(x2, y2));
                raw.events.push(Event::PointerMoved(from));
                raw.events.push(pointer(from, PointerButton::Primary, true));
                let steps = 10;
                for i in 1..=steps {
                    let p = from + (to - from) * (i as f32 / steps as f32);
                    self.pending.push_back(vec![Event::PointerMoved(p)]);
                }
                self.pending.push_back(vec![pointer(to, PointerButton::Primary, false)]);
            }
            Step::Key(key, only) => {
                for pressed in only.map_or(vec![true, false], |pressed| vec![pressed]) {
                    raw.events.push(Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    });
                }
            }
            Step::Shot => {
                self.shots += 1;
                *SHOT_REQUEST.lock().unwrap() = Some(format!("{}{}.png", self.prefix, self.shots));
            }
            Step::Quit => self.commands.push(egui::ViewportCommand::Close),
            Step::Built | Step::Wait(_) => {}
        }
        self.steps.pop_front();
    }

    /// Sends queued viewport commands (commands sent before the frame starts are lost).
    pub fn frame(&mut self, ctx: &egui::Context) {
        for cmd in self.commands.drain(..) {
            ctx.send_viewport_cmd(cmd);
        }
    }
}

/// A requested screenshot path, taken by the renderer on its next frame.
static SHOT_REQUEST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Whether this is a self-test run.
pub fn active() -> bool {
    static ACTIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ACTIVE.get_or_init(|| std::env::var_os("MAGICTILE_SELFTEST").is_some())
}

/// The screenshot about to be taken this frame, without claiming it.
pub fn peek_shot_request() -> Option<String> {
    SHOT_REQUEST.lock().unwrap().clone()
}

pub fn take_shot_request() -> Option<String> {
    SHOT_REQUEST.lock().unwrap().take()
}

fn pointer(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
}

fn parse_step(s: &str) -> Step {
    let (name, args) = s.trim().split_once(':').unwrap_or((s.trim(), ""));
    let nums: Vec<f32> = args.split(',').filter_map(|a| a.trim().parse().ok()).collect();
    let n = |i: usize| nums.get(i).copied().unwrap_or(0.5);
    match name {
        "built" => Step::Built,
        "wait" => Step::Wait(n(0) as u32),
        "move" => Step::Move(n(0), n(1)),
        "click" => Step::Click(n(0), n(1), PointerButton::Primary),
        "rclick" => Step::Click(n(0), n(1), PointerButton::Secondary),
        "drag" => Step::Drag(n(0), n(1), n(2), n(3)),
        "key" | "keydown" | "keyup" => {
            let only = match name {
                "keydown" => Some(true),
                "keyup" => Some(false),
                _ => None,
            };
            Step::Key(Key::from_name(args.trim()).unwrap_or(Key::Escape), only)
        }
        "shot" => Step::Shot,
        _ => Step::Quit,
    }
}
