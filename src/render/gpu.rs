//! Presents the CPU canvas on a window: uploads it as a texture and draws one
//! nearest-filtered full-screen triangle. Picks a transparent composite mode
//! when the platform offers one (overlay mode), opaque otherwise.

use super::canvas::Canvas;
use anyhow::{Context, Result};
use std::sync::Arc;
use winit::window::Window;

const SHADER: &str = r#"
struct Params { uv_max: vec2<f32>, _pad: vec2<f32> };
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    // Full-screen triangle.
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VsOut;
    out.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y) * params.uv_max;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, smp, in.uv);
    // Art pixels are fully opaque or fully clear, so straight == premultiplied.
    return vec4<f32>(c.rgb * c.a, c.a);
}
"#;

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    texture: Option<(wgpu::Texture, wgpu::BindGroup, u32, u32)>,
    /// True when the compositor will show the desktop through clear pixels.
    pub transparent: bool,
}

impl Gpu {
    pub fn new(window: Arc<Window>, want_transparent: bool) -> Result<Gpu> {
        let size = window.inner_size();
        let (surface, adapter) = pick_adapter(&window)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("snowlearner"),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .context("creating GPU device")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(caps.formats[0]);
        let alpha_mode = pick_alpha_mode(&caps.alpha_modes, want_transparent);
        let transparent = want_transparent && alpha_mode != wgpu::CompositeAlphaMode::Opaque;
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("surface not supported by adapter")?;
        config.format = format;
        config.alpha_mode = alpha_mode;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("upscale"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Gpu { surface, device, queue, config, pipeline, layout, sampler, uniform, texture: None, transparent })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    fn ensure_texture(&mut self, w: u32, h: u32) {
        if matches!(&self.texture, Some((_, _, tw, th)) if *tw == w && *th == h) {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("canvas"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniform.as_entire_binding() },
            ],
        });
        self.texture = Some((texture, bind, w, h));
    }

    /// Uploads `canvas` and draws it scaled by `scale` from the top-left corner.
    pub fn present(&mut self, canvas: &Canvas, scale: u32) {
        let (w, h) = (canvas.w as u32, canvas.h as u32);
        self.ensure_texture(w, h);
        let Some((texture, bind, _, _)) = &self.texture else { return };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            canvas.bytes(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        // The canvas may be slightly larger than surface/scale; map exactly one
        // canvas pixel to `scale` screen pixels.
        let uv =
            [self.config.width as f32 / (w * scale) as f32, self.config.height as f32 / (h * scale) as f32, 0.0, 0.0];
        self.queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&uv));

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canvas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(frame);
    }
}

/// Vulkan/Metal/DX12 first, GL only when none of those work: enabling GL
/// alongside makes the driver load a second graphics stack (~20 MB resident)
/// that is never used. `WGPU_BACKEND` (e.g. `gl`) overrides the choice.
fn pick_adapter(window: &Arc<Window>) -> Result<(wgpu::Surface<'static>, wgpu::Adapter)> {
    let tries = if std::env::var_os("WGPU_BACKEND").is_some() {
        vec![wgpu::InstanceDescriptor::new_without_display_handle_from_env().backends]
    } else {
        vec![wgpu::Backends::PRIMARY, wgpu::Backends::GL]
    };
    let mut last = None;
    for backends in tries {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = backends;
        let instance = wgpu::Instance::new(desc);
        let surface = instance.create_surface(window.clone()).context("creating GPU surface")?;
        match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            ..Default::default()
        })) {
            Ok(adapter) => return Ok((surface, adapter)),
            Err(e) => last = Some(e),
        }
    }
    Err(last.map(anyhow::Error::from).unwrap_or_else(|| anyhow::anyhow!("no GPU backend enabled")))
        .context("no compatible GPU adapter")
}

/// Prefers a mode that composites our alpha over the desktop.
pub fn pick_alpha_mode(available: &[wgpu::CompositeAlphaMode], want_transparent: bool) -> wgpu::CompositeAlphaMode {
    use wgpu::CompositeAlphaMode as M;
    if want_transparent {
        for m in [M::PreMultiplied, M::PostMultiplied, M::Inherit] {
            if available.contains(&m) {
                return m;
            }
        }
    }
    if available.contains(&M::Opaque) { M::Opaque } else { available.first().copied().unwrap_or(M::Auto) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::CompositeAlphaMode as M;

    #[test]
    fn overlay_prefers_premultiplied_alpha() {
        assert_eq!(pick_alpha_mode(&[M::Opaque, M::PostMultiplied, M::PreMultiplied], true), M::PreMultiplied);
        assert_eq!(pick_alpha_mode(&[M::Opaque, M::PostMultiplied], true), M::PostMultiplied);
    }

    #[test]
    fn falls_back_to_opaque_when_no_alpha_mode_exists() {
        assert_eq!(pick_alpha_mode(&[M::Opaque], true), M::Opaque);
    }

    #[test]
    fn window_mode_is_always_opaque_when_possible() {
        assert_eq!(pick_alpha_mode(&[M::PreMultiplied, M::Opaque], false), M::Opaque);
    }
}
