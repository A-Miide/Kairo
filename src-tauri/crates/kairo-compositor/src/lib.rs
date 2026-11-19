//! Kairo's GPU compositor (docs/DIRECTOR_COMPOSITOR.md): video, cameras and the page put
//! together on the graphics card, as OBS does, instead of pixel by pixel on the CPU.
//!
//! A [`Source`] is a picture on the GPU: NV12 (what decoders and cameras produce; the GPU
//! converts it) or premultiplied BGRA (the page's snapshots). A [`Canvas`] is one output's
//! picture: [`Canvas::render`] draws a list of [`Layer`]s into it, and [`Canvas::read_bgra`]
//! hands the result back for the outputs that need bytes (NDI, DeckLink, streaming), through
//! two buffers read asynchronously, so the CPU never waits on the GPU.
//!
//! wgpu: Metal on macOS, Direct3D 12 on Windows, Vulkan on Linux.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// The canvas's pixel format: what NDI, DeckLink and `capture::Frame` already use.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// Per-layer uniforms are written into one buffer, a slot per layer (dynamic offsets).
const SLOT: u64 = 256;
const MAX_LAYERS: u64 = 32;

/// The GPU, and the two pipelines every canvas draws with.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    nv12: wgpu::RenderPipeline,
    bgra: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// Bound as the "faded from" picture of a BGRA layer with no fade.
    blank: wgpu::TextureView,
    pub adapter: String,
}

impl Gpu {
    /// The system's GPU, or why there is none (a VM without one, an old driver): callers keep
    /// the CPU path then.
    pub fn new() -> Result<Arc<Gpu>, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL | wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no GPU: {e}"))?;
        let name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("kairo-compositor"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("GPU unusable: {e}"))?;
        // wgpu's default is to panic on any GPU error: on air, a logged error and a frame that
        // didn't change are far better than an output that stops.
        device.on_uncaptured_error(Arc::new(|e: wgpu::Error| eprintln!("[Compositor] GPU error: {e}")));

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kairo-compositor"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<LayerUniform>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex(2),
                tex(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layer"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // Premultiplied "over": src + dst × (1 − src alpha).
        let over = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: Some(wgpu::BlendState { color: over, alpha: over }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let nv12 = pipeline("fs_nv12");
        let bgra = pipeline("fs_bgra");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("layer"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blank = texture(&device, 1, 1, wgpu::TextureFormat::Bgra8Unorm, "blank").create_view(&Default::default());
        Ok(Arc::new(Gpu { device, queue, layout, nv12, bgra, sampler, blank, adapter: name }))
    }
}

fn texture(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// How a source's YUV is to be read (NV12 only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matrix {
    Bt709,
    Bt601,
}

enum Planes {
    Nv12 { y: wgpu::Texture, uv: wgpu::Texture, matrix: Matrix, full_range: bool },
    Bgra(wgpu::Texture),
}

/// A picture on the GPU, replaced in place by [`Source::upload_nv12`] / [`Source::upload_bgra`].
pub struct Source {
    pub width: u32,
    pub height: u32,
    planes: Planes,
    views: (wgpu::TextureView, wgpu::TextureView),
}

impl Source {
    /// An NV12 picture (`width`, `height` even): BT.709 from 720 lines up, BT.601 below, as
    /// decoders and players assume when a file doesn't say.
    pub fn nv12(gpu: &Gpu, width: u32, height: u32) -> Source {
        let y = texture(&gpu.device, width, height, wgpu::TextureFormat::R8Unorm, "y");
        let uv = texture(&gpu.device, width.div_ceil(2), height.div_ceil(2), wgpu::TextureFormat::Rg8Unorm, "uv");
        let views = (y.create_view(&Default::default()), uv.create_view(&Default::default()));
        let matrix = if height >= 720 { Matrix::Bt709 } else { Matrix::Bt601 };
        Source { width, height, planes: Planes::Nv12 { y, uv, matrix, full_range: false }, views }
    }

    /// A premultiplied BGRA picture (the page).
    pub fn bgra(gpu: &Gpu, width: u32, height: u32) -> Source {
        let t = texture(&gpu.device, width, height, wgpu::TextureFormat::Bgra8Unorm, "bgra");
        let v = t.create_view(&Default::default());
        Source { width, height, views: (v.clone(), v), planes: Planes::Bgra(t) }
    }

    /// Overrides the YUV reading of an NV12 source.
    pub fn set_yuv(&mut self, m: Matrix, full: bool) {
        if let Planes::Nv12 { matrix, full_range, .. } = &mut self.planes {
            *matrix = m;
            *full_range = full;
        }
    }

    /// One NV12 frame, tightly packed: `width × height` bytes of Y, then the interleaved UV.
    pub fn upload_nv12(&self, gpu: &Gpu, frame: &[u8]) -> Result<(), String> {
        let Planes::Nv12 { y, uv, .. } = &self.planes else { return Err("not an NV12 source".into()) };
        let (w, h) = (self.width, self.height);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let y_len = (w * h) as usize;
        let uv_len = (cw * ch * 2) as usize;
        if frame.len() < y_len + uv_len {
            return Err(format!("NV12 frame is {} bytes, {}x{} needs {}", frame.len(), w, h, y_len + uv_len));
        }
        write(gpu, y, &frame[..y_len], w, w, h);
        write(gpu, uv, &frame[y_len..y_len + uv_len], cw * 2, cw, ch);
        Ok(())
    }

    /// One premultiplied BGRA frame, tightly packed.
    pub fn upload_bgra(&self, gpu: &Gpu, frame: &[u8]) -> Result<(), String> {
        let Planes::Bgra(t) = &self.planes else { return Err("not a BGRA source".into()) };
        let need = (self.width * self.height * 4) as usize;
        if frame.len() < need {
            return Err(format!("BGRA frame is {} bytes, {}x{} needs {need}", frame.len(), self.width, self.height));
        }
        write(gpu, t, &frame[..need], self.width * 4, self.width, self.height);
        Ok(())
    }
}

fn write(gpu: &Gpu, t: &wgpu::Texture, data: &[u8], bytes_per_row: u32, width: u32, height: u32) {
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        data,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(height) },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
}

/// A rectangle in canvas pixels (or, for [`Layer::crop`], source pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn full(w: u32, h: u32) -> Rect {
        Rect { x: 0.0, y: 0.0, w: w as f32, h: h as f32 }
    }
}

/// How a source fills its box, as the page fits a background (`fill`, `cover`, `contain`):
/// the rectangle to draw into and the part of the source to show.
pub fn fit(mode: &str, src_w: u32, src_h: u32, into: Rect) -> (Rect, Rect) {
    let (sw, sh) = (src_w as f32, src_h as f32);
    let whole = Rect::full(src_w, src_h);
    match mode {
        "fill" => (into, whole),
        "cover" => {
            // Scale to cover, then crop the overflow from the source, centred.
            let s = (into.w / sw).max(into.h / sh);
            let (cw, ch) = (into.w / s, into.h / s);
            (into, Rect { x: (sw - cw) / 2.0, y: (sh - ch) / 2.0, w: cw, h: ch })
        }
        _ => {
            let s = (into.w / sw).min(into.h / sh);
            let (w, h) = (sw * s, sh * s);
            (Rect { x: into.x + (into.w - w) / 2.0, y: into.y + (into.h - h) / 2.0, w, h }, whole)
        }
    }
}

/// One thing drawn on a canvas, in order, each over the ones before.
pub struct Layer<'a> {
    pub source: &'a Source,
    /// Where on the canvas.
    pub rect: Rect,
    /// The part of the source shown (source pixels); `None` = all of it.
    pub crop: Option<Rect>,
    pub opacity: f32,
    /// BGRA only: the picture this one is fading in over, and how far the fade is (0..1).
    pub fade_from: Option<(&'a Source, f32)>,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LayerUniform {
    dst: [f32; 4],
    uv: [f32; 4],
    params: [f32; 4],
}

const IDLE: u8 = 0;
const PENDING: u8 = 1;
const READY: u8 = 2;
const FAILED: u8 = 3;

struct Readback {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
}

/// One output's picture on the GPU.
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    target: wgpu::Texture,
    view: wgpu::TextureView,
    uniforms: wgpu::Buffer,
    padded_row: u32,
    readback: [Readback; 2],
    /// The readback buffer the next copy goes into.
    next: usize,
}

impl Canvas {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Canvas {
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layers"),
            size: SLOT * MAX_LAYERS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let rb = || Readback {
            buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size: (padded_row * height) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(IDLE)),
        };
        Canvas { width, height, target, view, uniforms, padded_row, readback: [rb(), rb()], next: 0 }
    }

    /// Draws `layers` (at most 32) over `background` (premultiplied RGBA).
    pub fn render(&mut self, gpu: &Gpu, background: [f64; 4], layers: &[Layer]) {
        let layers = &layers[..layers.len().min(MAX_LAYERS as usize)];
        let (cw, ch) = (self.width as f32, self.height as f32);
        let mut bytes = vec![0u8; (SLOT as usize) * layers.len().max(1)];
        for (i, l) in layers.iter().enumerate() {
            let crop = l.crop.unwrap_or(Rect::full(l.source.width, l.source.height));
            let (sw, sh) = (l.source.width as f32, l.source.height as f32);
            let (matrix, full) = match &l.source.planes {
                Planes::Nv12 { matrix, full_range, .. } => ((*matrix == Matrix::Bt601) as u8 as f32, *full_range as u8 as f32),
                Planes::Bgra(_) => (0.0, 0.0),
            };
            let u = LayerUniform {
                dst: [
                    l.rect.x / cw * 2.0 - 1.0,
                    1.0 - l.rect.y / ch * 2.0,
                    (l.rect.x + l.rect.w) / cw * 2.0 - 1.0,
                    1.0 - (l.rect.y + l.rect.h) / ch * 2.0,
                ],
                uv: [crop.x / sw, crop.y / sh, (crop.x + crop.w) / sw, (crop.y + crop.h) / sh],
                params: [l.opacity.clamp(0.0, 1.0), l.fade_from.map_or(1.0, |(_, t)| t.clamp(0.0, 1.0)), matrix, full],
            };
            let at = i * SLOT as usize;
            bytes[at..at + std::mem::size_of::<LayerUniform>()].copy_from_slice(bytemuck::bytes_of(&u));
        }
        gpu.queue.write_buffer(&self.uniforms, 0, &bytes);

        let groups: Vec<wgpu::BindGroup> = layers
            .iter()
            .map(|l| {
                let (p0, p1) = match (&l.source.planes, l.fade_from) {
                    (Planes::Nv12 { .. }, _) => (&l.source.views.0, &l.source.views.1),
                    (Planes::Bgra(_), Some((from, _))) => (&l.source.views.0, &from.views.0),
                    (Planes::Bgra(_), None) => (&l.source.views.0, &gpu.blank),
                };
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("layer"),
                    layout: &gpu.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &self.uniforms,
                                offset: 0,
                                size: wgpu::BufferSize::new(std::mem::size_of::<LayerUniform>() as u64),
                            }),
                        },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&gpu.sampler) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(p0) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(p1) },
                    ],
                })
            })
            .collect();

        let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("canvas") });
        {
            let [r, g, b, a] = background;
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            for (i, (l, group)) in layers.iter().zip(&groups).enumerate() {
                if l.opacity <= 0.0 {
                    continue;
                }
                pass.set_pipeline(match l.source.planes { Planes::Nv12 { .. } => &gpu.nv12, Planes::Bgra(_) => &gpu.bgra });
                pass.set_bind_group(0, group, &[(i as u64 * SLOT) as u32]);
                pass.draw(0..6, 0..1);
            }
        }
        gpu.queue.submit([enc.finish()]);
    }

    /// Copies the canvas out for reading, and hands back the previous copy if it has arrived:
    /// `out` gets that picture as tightly packed BGRA and the result is true. The picture is one
    /// frame behind the canvas, so neither side ever waits for the other.
    pub fn read_bgra(&mut self, gpu: &Gpu, out: &mut Vec<u8>) -> bool {
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        let mut got = false;
        let prev = 1 - self.next;
        if self.readback[prev].state.load(Ordering::Acquire) == READY {
            got = self.take(prev, out);
        }
        let i = self.next;
        // Still being read from the frame before last (a slow GPU): skip this copy.
        if self.readback[i].state.load(Ordering::Acquire) == PENDING {
            return got;
        }
        if self.readback[i].state.load(Ordering::Acquire) == READY {
            self.readback[i].buffer.unmap();
        }
        let mut enc = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback[i].buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.padded_row), rows_per_image: Some(self.height) },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        let state = self.readback[i].state.clone();
        state.store(PENDING, Ordering::Release);
        self.readback[i].buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            state.store(if r.is_ok() { READY } else { FAILED }, Ordering::Release);
        });
        self.next = prev;
        got
    }

    /// Like [`Canvas::read_bgra`], but waits for this frame (tests, a first frame).
    pub fn read_bgra_now(&mut self, gpu: &Gpu, out: &mut Vec<u8>) -> bool {
        let i = self.next;
        self.read_bgra(gpu, &mut Vec::new());
        let _ = gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        self.readback[i].state.load(Ordering::Acquire) == READY && self.take(i, out)
    }

    fn take(&mut self, i: usize, out: &mut Vec<u8>) -> bool {
        let row = (self.width * 4) as usize;
        out.resize(row * self.height as usize, 0);
        let ok = match self.readback[i].buffer.get_mapped_range(..) {
            Ok(view) => {
                for (y, dst) in out.chunks_exact_mut(row).enumerate() {
                    let at = y * self.padded_row as usize;
                    dst.copy_from_slice(&view[at..at + row]);
                }
                true
            }
            Err(_) => false,
        };
        self.readback[i].buffer.unmap();
        self.readback[i].state.store(IDLE, Ordering::Release);
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu() -> Option<Arc<Gpu>> {
        match Gpu::new() {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("skipped: {e}");
                None
            }
        }
    }

    fn px(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn fit_matches_the_page() {
        let into = Rect::full(1920, 1080);
        // A 4:3 video contained: pillarboxed.
        let (r, _) = fit("contain", 640, 480, into);
        assert_eq!((r.x, r.w, r.h), (240.0, 1440.0, 1080.0));
        // Covered: fills the box, top and bottom cropped from the source.
        let (r, c) = fit("cover", 640, 480, into);
        assert_eq!(r, into);
        assert_eq!((c.w, c.y), (640.0, 60.0));
    }

    #[test]
    fn nv12_grey_and_page_over_it() {
        let Some(g) = gpu() else { return };
        let (w, h) = (64, 36);
        let mut canvas = Canvas::new(&g, w, h);
        // Video: mid grey (Y 126 limited range ≈ 128 full), neutral chroma.
        let video = Source::nv12(&g, w, h);
        let mut frame = vec![126u8; (w * h) as usize];
        frame.extend(std::iter::repeat_n(128u8, (w * h / 2) as usize));
        video.upload_nv12(&g, &frame).unwrap();
        // Page: left half opaque white, right half clear (premultiplied BGRA).
        let page = Source::bgra(&g, w, h);
        let mut p = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w / 2 {
                let i = ((y * w + x) * 4) as usize;
                p[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        page.upload_bgra(&g, &p).unwrap();
        canvas.render(&g, [0.0, 0.0, 0.0, 1.0], &[
            Layer { source: &video, rect: Rect::full(w, h), crop: None, opacity: 1.0, fade_from: None },
            Layer { source: &page, rect: Rect::full(w, h), crop: None, opacity: 1.0, fade_from: None },
        ]);
        let mut out = Vec::new();
        assert!(canvas.read_bgra_now(&g, &mut out));
        assert_eq!(px(&out, w, 5, 5), [255, 255, 255, 255], "page over video");
        let grey = px(&out, w, w - 5, 5);
        assert!(grey[..3].iter().all(|&c| (126..=130).contains(&c)) && grey[3] == 255, "video shows through: {grey:?}");
    }

    #[test]
    fn page_fade_is_a_mix() {
        let Some(g) = gpu() else { return };
        let (w, h) = (16, 16);
        let mut canvas = Canvas::new(&g, w, h);
        let a = Source::bgra(&g, w, h);
        let b = Source::bgra(&g, w, h);
        a.upload_bgra(&g, &[0, 0, 255, 255].repeat((w * h) as usize)).unwrap(); // red
        b.upload_bgra(&g, &[255, 0, 0, 255].repeat((w * h) as usize)).unwrap(); // blue
        canvas.render(&g, [0.0, 0.0, 0.0, 1.0], &[Layer { source: &b, rect: Rect::full(w, h), crop: None, opacity: 1.0, fade_from: Some((&a, 0.5)) }]);
        let mut out = Vec::new();
        assert!(canvas.read_bgra_now(&g, &mut out));
        let p = px(&out, w, 8, 8);
        assert!((126..=129).contains(&p[0]) && (126..=129).contains(&p[2]) && p[1] == 0, "half red, half blue: {p:?}");
    }

    #[test]
    fn readback_is_one_frame_behind_and_never_waits() {
        let Some(g) = gpu() else { return };
        let (w, h) = (8, 8);
        let mut canvas = Canvas::new(&g, w, h);
        let mut out = Vec::new();
        canvas.render(&g, [1.0, 0.0, 0.0, 1.0], &[]);
        canvas.read_bgra(&g, &mut out);
        let _ = g.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
        canvas.render(&g, [0.0, 1.0, 0.0, 1.0], &[]);
        assert!(canvas.read_bgra(&g, &mut out), "the first frame arrives on the second call");
        assert_eq!(px(&out, w, 0, 0), [0, 0, 255, 255], "red, as BGRA");
    }
}
