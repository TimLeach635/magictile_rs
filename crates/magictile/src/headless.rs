//! Rendering a puzzle to a PNG without a window, for screenshots and visual testing.
//!
//! Usage: magictile --screenshot out.png [--size 900x900] [--scramble N] [--model NAME]
//!        [--twist FRACTION] [--highlight] [--pan DX,DY] [--zoom SCALE] [puzzle ID or name]

use crate::render::{FrameJob, Renderer};
use crate::scene::{self, RenderData, SceneContext};
use crate::settings::Settings;
use crate::view::{DragButton, DragData, Model, View};
use eframe::wgpu;
use magictile_core::{Library, Puzzle, PuzzleConfig, SingleTwist, TwistController};
use r3::models::{HyperbolicModel, SphericalModel};
use rand::SeedableRng;
use std::path::PathBuf;

struct Options {
    output: PathBuf,
    size: [u32; 2],
    scramble: usize,
    model: Option<String>,
    twist: Option<f64>,
    highlight: bool,
    pan: Option<(f32, f32)>,
    zoom: Option<f64>,
    puzzle: Option<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        output: PathBuf::from("output.png"),
        size: [900, 900],
        scramble: 0,
        model: None,
        twist: None,
        highlight: false,
        pan: None,
        zoom: None,
        puzzle: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--screenshot" => o.output = value()?.into(),
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size is WIDTHxHEIGHT")?;
                o.size = [w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?];
            }
            "--scramble" => o.scramble = value()?.parse().map_err(|_| "bad scramble count")?,
            "--model" => o.model = Some(value()?),
            "--twist" => o.twist = Some(value()?.parse().map_err(|_| "bad twist fraction")?),
            "--highlight" => o.highlight = true,
            "--pan" => {
                let v = value()?;
                let (x, y) = v.split_once(',').ok_or("--pan is DX,DY")?;
                o.pan = Some((x.parse().map_err(|_| "bad pan")?, y.parse().map_err(|_| "bad pan")?));
            }
            "--zoom" => o.zoom = Some(value()?.parse().map_err(|_| "bad zoom")?),
            other => o.puzzle = Some(other.to_string()),
        }
    }
    Ok(o)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let o = parse(args)?;

    let library = Library::load_standard();
    let config: PuzzleConfig = match &o.puzzle {
        Some(p) => library.find(p).cloned().ok_or(format!("no puzzle with ID or name '{p}'"))?,
        None => PuzzleConfig::default(),
    };
    eprintln!("Building {}...", config.display_name);
    let mut puzzle = Puzzle::build(config, &mut ()).map_err(|e| e.to_string())?;
    let data = RenderData::new(&puzzle);

    let mut settings = Settings::default();
    if let Some(m) = &o.model {
        match m.to_lowercase().as_str() {
            "poincare" => settings.hyperbolic_model = HyperbolicModel::Poincare,
            "klein" => settings.hyperbolic_model = HyperbolicModel::Klein,
            "upper" => settings.hyperbolic_model = HyperbolicModel::UpperHalfPlane,
            "ortho" => settings.hyperbolic_model = HyperbolicModel::Orthographic,
            "stereographic" => settings.spherical_model = SphericalModel::Sterographic,
            "gnomonic" => settings.spherical_model = SphericalModel::Gnomonic,
            "fisheye" => settings.spherical_model = SphericalModel::Fisheye,
            "disks" => settings.spherical_model = SphericalModel::HemisphereDisks,
            other => return Err(format!("unknown model {other}")),
        }
    }
    let model = Model::for_puzzle(puzzle.config.geometry(), settings.hyperbolic_model, settings.spherical_model);

    let mut controller = TwistController::default();
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    controller.scramble(&mut puzzle, o.scramble, &mut rng);

    let mut view = View::default();
    view.set_size(o.size[0] as f32, o.size[1] as f32);
    view.reset(puzzle.config.geometry());
    if let Some(z) = o.zoom {
        view.view_scale = z;
    }
    if let Some((dx, dy)) = o.pan {
        let (cx, cy) = (o.size[0] as f32 / 2.0, o.size[1] as f32 / 2.0);
        let drag = DragData {
            x: cx + dx,
            y: cy + dy,
            x_diff: dx,
            y_diff: dy,
            y_percent: 0.0,
            rotation: 0.0,
            button: DragButton::Primary,
        };
        view.drag(model, drag);
    }

    // Part way through a twist of the first logical twist.
    if let Some(fraction) = o.twist
        && !puzzle.all_twist_data.is_empty()
    {
        let mut twist = SingleTwist { identified: 0, left_click: true, slice_mask: 1, ..Default::default() };
        let td = puzzle.all_twist_data[0].twist_data_for_drawing[0];
        if puzzle.config.earthquake()
            && let Some((identified, mask)) = puzzle.earthquake_companion(td, 1)
        {
            twist.identified_systolic = Some(identified);
            twist.slice_mask_systolic = mask;
        }
        let degrees = puzzle.twist_magnitude(&twist).to_degrees() * fraction;
        controller.start_rotate(&mut puzzle, twist);
        controller.advance(&mut puzzle, degrees);
    }

    let closest_twist =
        o.highlight.then(|| puzzle.all_twist_data.first().map(|c| c.twist_data_for_drawing[0])).flatten();
    let ctx = SceneContext {
        puzzle: &puzzle,
        data: &data,
        controller: &controller,
        settings: &settings,
        model,
        slice_mask: 1,
        closest_twist,
        closest_geodesic_seg: 1,
    };
    let start = std::time::Instant::now();
    let cell_jobs = if puzzle.is_spherical() {
        Vec::new()
    } else {
        (0..puzzle.masters.len()).map(|i| (i as u32, scene::build_cell_texture(&ctx, i))).collect()
    };
    let textures = start.elapsed();
    let (view_list, _) = scene::build_view(&ctx, &view, 1.0);
    eprintln!(
        "Scene: cell textures {:.1} ms, view {:.1} ms ({} fills, {} cell triangles)",
        textures.as_secs_f64() * 1000.0,
        (start.elapsed() - textures).as_secs_f64() * 1000.0,
        view_list.cmds.iter().filter(|c| matches!(c, crate::draw::Cmd::Fill { .. })).count(),
        view_list.cell_indices.len() / 3
    );
    let job = FrameJob {
        puzzle_generation: 1,
        num_layers: puzzle.masters.len() as u32,
        cell_jobs,
        mipmaps: settings.enable_texture_mipmaps,
        view: view_list,
        size_px: o.size,
    };

    let pixels = pollster::block_on(render(&job))?;
    crate::render::write_png(&o.output.to_string_lossy(), o.size, &pixels)?;
    eprintln!("Wrote {}", o.output.display());
    Ok(())
}

async fn render(job: &FrameJob) -> Result<Vec<u8>, String> {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .map_err(|e| format!("no GPU adapter: {e}"))?;
    let (device, queue) =
        adapter.request_device(&wgpu::DeviceDescriptor::default()).await.map_err(|e| format!("no GPU device: {e}"))?;

    let mut renderer = Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer.render_job(&device, &mut encoder, job);
    queue.submit([encoder.finish()]);
    renderer.read_view(&device, &queue)
}
