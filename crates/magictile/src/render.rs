//! wgpu rendering of [`DrawList`]s: into per-master cell textures, and into an offscreen view that
//! is composited into the egui frame by a paint callback.

use crate::scene::{Camera, Cmd, ColorVertex, DrawList, TexVertex};
use eframe::egui;
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu::{self, util::DeviceExt};
use std::sync::Arc;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
const SAMPLES: u32 = 4;
pub const CELL_TEXTURE_SIZE: u32 = 512;
const CELL_MIP_LEVELS: u32 = 10; // 512 -> 1

// Stencil bits: bit 0 for concave polygon fills, bit 1 for the clip region.
const FILL_BIT: u32 = 1;
const CLIP_BIT: u32 = 2;

const SHADER: &str = r#"
struct Camera { m: vec4<f32> };
@group(0) @binding(0) var<uniform> camera: Camera;

fn project(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(camera.m.x * p.x + camera.m.y * p.y, camera.m.z * p.x + camera.m.w * p.y, 0.0, 1.0);
}

struct ColorOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };

@vertex fn vs_color(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> ColorOut {
    return ColorOut(project(pos), color);
}
@fragment fn fs_color(in: ColorOut) -> @location(0) vec4<f32> { return in.color; }

@vertex fn vs_position(@location(0) pos: vec2<f32>) -> @builtin(position) vec4<f32> { return project(pos); }
@fragment fn fs_nothing() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }

@vertex fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@group(1) @binding(0) var cells: texture_2d_array<f32>;
@group(1) @binding(1) var cell_sampler: sampler;

struct CellOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) @interpolate(flat) layer: u32 };

@vertex fn vs_cell(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) layer: u32) -> CellOut {
    return CellOut(project(pos), uv, layer);
}
@fragment fn fs_cell(in: CellOut) -> @location(0) vec4<f32> {
    return textureSample(cells, cell_sampler, in.uv, in.layer);
}
"#;

/// Samples a texture over the whole target (for compositing and mipmaps).
const BLIT_SHADER: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return Out(vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0), vec2<f32>(uv.x, 1.0 - uv.y));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return textureSample(source, source_sampler, in.uv);
}
// For sRGB targets (which re-encode what we write), undo the encoding our colors already have.
@fragment fn fs_linear(in: Out) -> @location(0) vec4<f32> {
    let c = textureSample(source, source_sampler, in.uv);
    return vec4<f32>(to_linear(c.rgb), c.a);
}
"#;

/// A frame to render: cell textures to update, then the view.
pub struct FrameJob {
    /// Changes when the puzzle changes (cell textures are then recreated).
    pub puzzle_generation: u64,
    pub num_layers: u32,
    pub cell_jobs: Vec<(u32, DrawList)>,
    pub mipmaps: bool,
    pub view: DrawList,
    pub size_px: [u32; 2],
}

/// The number of frames painted (for self-tests).
pub static PAINTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub struct PuzzleCallback {
    pub job: Arc<FrameJob>,
}

/// GPU resources, kept in egui's callback resources.
pub struct Renderer {
    fill_fan: wgpu::RenderPipeline,
    fill_cover: wgpu::RenderPipeline,
    solid: wgpu::RenderPipeline,
    solid_clipped: wgpu::RenderPipeline,
    set_clip: wgpu::RenderPipeline,
    clear_clip: wgpu::RenderPipeline,
    cell_pipeline: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    mip: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    cells_layout: wgpu::BindGroupLayout,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    sampler_no_mips: wgpu::Sampler,
    cells: Option<CellTextures>,
    view: Option<ViewTarget>,
}

struct CellTextures {
    generation: u64,
    texture: wgpu::Texture,
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    bind_group_no_mips: wgpu::BindGroup,
}

struct ViewTarget {
    size: [u32; 2],
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    blit_bind_group: wgpu::BindGroup,
}

impl Renderer {
    /// Creates the renderer and stores it in egui's callback resources.
    pub fn install(render_state: &egui_wgpu::RenderState) {
        let renderer = Renderer::new(&render_state.device, render_state.target_format);
        render_state.renderer.write().callback_resources.insert(renderer);
    }

    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("magictile"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT_SHADER.into()),
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_layout = |label, dimension| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: dimension,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            })
        };
        let cells_layout = texture_layout("cells", wgpu::TextureViewDimension::D2Array);
        let blit_layout = texture_layout("blit", wgpu::TextureViewDimension::D2);

        let layout = |layouts: &[&wgpu::BindGroupLayout]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &layouts.iter().map(|l| Some(*l)).collect::<Vec<_>>(),
                immediate_size: 0,
            })
        };
        let camera_pipeline_layout = layout(&[&camera_layout]);
        let cell_pipeline_layout = layout(&[&camera_layout, &cells_layout]);
        let blit_pipeline_layout = layout(&[&blit_layout]);

        let position_layout = wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2],
        };
        let color_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ColorVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
        };
        let cell_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TexVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32],
        };

        let stencil_face =
            |compare, op| wgpu::StencilFaceState { compare, fail_op: op, depth_fail_op: op, pass_op: op };
        let depth_stencil = |front: wgpu::StencilFaceState, read_mask, write_mask| wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState { front, back: front, read_mask, write_mask },
            bias: wgpu::DepthBiasState::default(),
        };
        let keep = || depth_stencil(stencil_face(wgpu::CompareFunction::Always, wgpu::StencilOperation::Keep), 0, 0);

        let pipeline = |label: &str,
                        pipeline_layout: &wgpu::PipelineLayout,
                        vs: &str,
                        fs: &str,
                        buffers: &[wgpu::VertexBufferLayout],
                        write_color: bool,
                        stencil: wgpu::DepthStencilState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: if write_color { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::empty() },
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(stencil),
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                multiview_mask: None,
                cache: None,
            })
        };

        let p = &camera_pipeline_layout;
        let fill_fan = pipeline(
            "fill fan",
            p,
            "vs_position",
            "fs_nothing",
            std::slice::from_ref(&position_layout),
            false,
            depth_stencil(stencil_face(wgpu::CompareFunction::Always, wgpu::StencilOperation::Invert), 0, FILL_BIT),
        );
        let fill_cover = pipeline(
            "fill cover",
            p,
            "vs_color",
            "fs_color",
            std::slice::from_ref(&color_layout),
            true,
            depth_stencil(
                stencil_face(wgpu::CompareFunction::Equal, wgpu::StencilOperation::Zero),
                FILL_BIT | CLIP_BIT,
                FILL_BIT,
            ),
        );
        let solid = pipeline("solid", p, "vs_color", "fs_color", std::slice::from_ref(&color_layout), true, keep());
        let solid_clipped = pipeline(
            "solid clipped",
            p,
            "vs_color",
            "fs_color",
            std::slice::from_ref(&color_layout),
            true,
            depth_stencil(stencil_face(wgpu::CompareFunction::Equal, wgpu::StencilOperation::Keep), CLIP_BIT, 0),
        );
        let set_clip = pipeline(
            "set clip",
            p,
            "vs_color",
            "fs_color",
            std::slice::from_ref(&color_layout),
            false,
            depth_stencil(stencil_face(wgpu::CompareFunction::Always, wgpu::StencilOperation::Replace), 0, CLIP_BIT),
        );
        let clear_clip = pipeline(
            "clear clip",
            p,
            "vs_fullscreen",
            "fs_nothing",
            &[],
            false,
            depth_stencil(stencil_face(wgpu::CompareFunction::Always, wgpu::StencilOperation::Zero), 0, CLIP_BIT),
        );
        let cell_pipeline =
            pipeline("cells", &cell_pipeline_layout, "vs_cell", "fs_cell", &[cell_layout], true, keep());

        let blit_pipeline = |label, format, fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&blit_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &blit_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &blit_shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let blit = blit_pipeline("blit", target_format, if target_format.is_srgb() { "fs_linear" } else { "fs_main" });
        let mip = blit_pipeline("mip", COLOR_FORMAT, "fs_main");

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cells"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let sampler_no_mips = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cells without mipmaps"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            lod_max_clamp: 0.0,
            ..Default::default()
        });

        Renderer {
            fill_fan,
            fill_cover,
            solid,
            solid_clipped,
            set_clip,
            clear_clip,
            cell_pipeline,
            blit,
            mip,
            camera_layout,
            cells_layout,
            blit_layout,
            sampler,
            sampler_no_mips,
            cells: None,
            view: None,
        }
    }

    /// Renders a frame's cell textures and view (into our offscreen view target).
    pub fn render_job(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, job: &FrameJob) {
        self.ensure_cells(device, job.puzzle_generation, job.num_layers);
        self.ensure_view(device, job.size_px);

        let cells = self.cells.as_ref().unwrap();
        for (layer, list) in &job.cell_jobs {
            let resolve = cells.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_mip_level: 0,
                mip_level_count: Some(1),
                base_array_layer: *layer,
                array_layer_count: Some(1),
                ..Default::default()
            });
            self.draw(device, encoder, list, &cells.msaa, &resolve, &cells.depth, None);
            if job.mipmaps {
                self.generate_mipmaps(device, encoder, &cells.texture, *layer);
            }
        }

        let view = self.view.as_ref().unwrap();
        let cell_bind_group = if job.mipmaps { &cells.bind_group } else { &cells.bind_group_no_mips };
        self.draw(device, encoder, &job.view, &view.msaa, &view.resolve_view, &view.depth, Some(cell_bind_group));
    }

    /// Copies the rendered view into a buffer (rows padded to 256 bytes), for screenshots.
    pub fn copy_view(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Option<(wgpu::Buffer, [u32; 2], u32)> {
        let view = self.view.as_ref()?;
        let [w, h] = [view.size[0].max(1), view.size[1].max(1)];
        let padded = (w * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            view.resolve.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        Some((buffer, [w, h], padded))
    }

    /// Reads back the last rendered view and saves it as a PNG (blocking).
    pub fn save_view_png(&self, device: &wgpu::Device, queue: &wgpu::Queue, path: &str) -> Result<(), String> {
        let pixels = self.read_view(device, queue)?;
        let [w, h] = self.view.as_ref().map_or([1, 1], |v| v.size);
        crate::headless::write_png(path, [w.max(1), h.max(1)], &pixels)
    }

    /// Reads back the last rendered view as RGBA rows (blocking).
    pub fn read_view(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Vec<u8>, String> {
        let mut encoder = device.create_command_encoder(&Default::default());
        let (buffer, [w, h], padded) = self.copy_view(device, &mut encoder).ok_or("nothing rendered")?;
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let start = (row * padded) as usize;
            pixels.extend_from_slice(&mapped[start..start + (w * 4) as usize]);
        }
        Ok(pixels)
    }

    fn ensure_cells(&mut self, device: &wgpu::Device, generation: u64, layers: u32) {
        if self.cells.as_ref().is_some_and(|c| c.generation == generation) {
            return;
        }
        let layers = layers.max(1);
        let size =
            wgpu::Extent3d { width: CELL_TEXTURE_SIZE, height: CELL_TEXTURE_SIZE, depth_or_array_layers: layers };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cell textures"),
            size,
            mip_level_count: CELL_MIP_LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let single = wgpu::Extent3d { depth_or_array_layers: 1, ..size };
        let msaa = attachment(device, single, COLOR_FORMAT);
        let depth = attachment(device, single, DEPTH_FORMAT);

        let array_view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let bind = |sampler: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cells"),
                layout: &self.cells_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&array_view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            })
        };
        let bind_group = bind(&self.sampler);
        let bind_group_no_mips = bind(&self.sampler_no_mips);
        self.cells = Some(CellTextures { generation, texture, msaa, depth, bind_group, bind_group_no_mips });
    }

    fn ensure_view(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.view.as_ref().is_some_and(|v| v.size == size) {
            return;
        }
        let extent = wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 };
        let msaa = attachment(device, extent, COLOR_FORMAT);
        let depth = attachment(device, extent, DEPTH_FORMAT);
        let resolve = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("view"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let resolve_view = resolve.create_view(&Default::default());
        let blit_bind_group = self.blit_bind_group(device, &resolve_view, &self.sampler_no_mips);
        self.view = Some(ViewTarget { size, msaa, depth, resolve, resolve_view, blit_bind_group });
    }

    fn blit_bind_group(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    /// Executes a draw list into the given (multisampled) attachments.
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        list: &DrawList,
        color: &wgpu::TextureView,
        resolve: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        cells: Option<&wgpu::BindGroup>,
    ) {
        let camera = camera_bind_group(device, &self.camera_layout, list.camera);
        let buffer = |contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: if contents.is_empty() { &[0; 16] } else { contents },
                usage,
            })
        };
        let solid = buffer(bytemuck::cast_slice(&list.solid), wgpu::BufferUsages::VERTEX);
        let fan = buffer(bytemuck::cast_slice(&list.fan), wgpu::BufferUsages::VERTEX);
        let cell_vertices = buffer(bytemuck::cast_slice(&list.cell_vertices), wgpu::BufferUsages::VERTEX);
        let cell_indices = buffer(bytemuck::cast_slice(&list.cell_indices), wgpu::BufferUsages::INDEX);

        let [r, g, b, a] = list.clear;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("puzzle"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: Some(resolve),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: r as f64, g: g as f64, b: b as f64, a: a as f64 }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &camera, &[]);

        for cmd in &list.cmds {
            match cmd {
                Cmd::Solid { range, clipped } => {
                    if *clipped {
                        pass.set_pipeline(&self.solid_clipped);
                        pass.set_stencil_reference(CLIP_BIT);
                    } else {
                        pass.set_pipeline(&self.solid);
                    }
                    pass.set_vertex_buffer(0, solid.slice(..));
                    pass.draw(range.clone(), 0..1);
                }
                Cmd::Fill { fan: fan_range, cover, inverted, clipped } => {
                    pass.set_pipeline(&self.fill_fan);
                    pass.set_vertex_buffer(0, fan.slice(..));
                    pass.draw(fan_range.clone(), 0..1);

                    pass.set_pipeline(&self.fill_cover);
                    let reference = if *inverted { 0 } else { FILL_BIT } | if *clipped { CLIP_BIT } else { 0 };
                    pass.set_stencil_reference(reference);
                    pass.set_vertex_buffer(0, solid.slice(..));
                    pass.draw(cover.clone(), 0..1);
                }
                Cmd::SetClip(range) => {
                    pass.set_pipeline(&self.set_clip);
                    pass.set_stencil_reference(CLIP_BIT);
                    pass.set_vertex_buffer(0, solid.slice(..));
                    pass.draw(range.clone(), 0..1);
                }
                Cmd::ClearClip => {
                    pass.set_pipeline(&self.clear_clip);
                    pass.draw(0..3, 0..1);
                }
                Cmd::Cells(range) => {
                    let Some(cells) = cells else {
                        continue;
                    };
                    pass.set_pipeline(&self.cell_pipeline);
                    pass.set_bind_group(1, cells, &[]);
                    pass.set_vertex_buffer(0, cell_vertices.slice(..));
                    pass.set_index_buffer(cell_indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(range.clone(), 0, 0..1);
                }
            }
        }
    }

    fn generate_mipmaps(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        layer: u32,
    ) {
        let level_view = |level| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_mip_level: level,
                mip_level_count: Some(1),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        };
        for level in 1..CELL_MIP_LEVELS {
            let source = level_view(level - 1);
            let target = level_view(level);
            let bind_group = self.blit_bind_group(device, &source, &self.sampler_no_mips);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mipmap"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.mip);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

fn attachment(device: &wgpu::Device, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn camera_bind_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, camera: Camera) -> wgpu::BindGroup {
    let [[a, b], [c, d]] = camera.matrix;
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("camera"),
        contents: bytemuck::cast_slice(&[a, b, c, d]),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("camera"),
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
    })
}

impl CallbackTrait for PuzzleCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        PAINTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let Some(renderer) = resources.get_mut::<Renderer>() else {
            return Vec::new();
        };
        if crate::selftest::active() {
            // egui drops the frame's commands when the window is occluded, so self-test runs
            // submit their own to keep screenshots working without a visible window.
            let mut own = device.create_command_encoder(&Default::default());
            renderer.render_job(device, &mut own, &self.job);
            queue.submit([own.finish()]);
            if let Some(path) = crate::selftest::take_shot_request() {
                let result = renderer.save_view_png(device, queue, &path);
                eprintln!(
                    "selftest: {}",
                    result.map_or_else(|e| format!("screenshot failed: {e}"), |_| format!("wrote {path}"))
                );
            }
        } else {
            renderer.render_job(device, encoder, &self.job);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(renderer) = resources.get::<Renderer>() else {
            return;
        };
        let Some(view) = &renderer.view else {
            return;
        };
        render_pass.set_pipeline(&renderer.blit);
        render_pass.set_bind_group(0, &view.blit_bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}
