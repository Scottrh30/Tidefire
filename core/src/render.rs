//! Offscreen renderer: ground, domain bounds, water/spray particles and the fire/smoke volume.
//! The same path is used for the live viewport and for PNG export.

use crate::gpu::{self, wgpu};
use crate::params::{CameraParams, Scene, WaterLook};
use crate::smoke::SmokeSim;
use crate::sparks::SparkSim;
use crate::water::WaterSim;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraU {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    cam_right: [f32; 4],
    cam_up: [f32; 4],
    light_dir: [f32; 4],
    viewport: [f32; 4],
    bg_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct VolU {
    box_min: [f32; 4],
    box_max: [f32; 4],
    smoke_color: [f32; 4],
    fire: [f32; 4],
    ambient: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SparkStyleU {
    spark_hot: [f32; 4],
    spark_cool: [f32; 4],
    ember_hot: [f32; 4],
    ember_cool: [f32; 4],
    size: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct StyleU {
    box_min: [f32; 4],
    deep: [f32; 4],
    shallow: [f32; 4],
    foam: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FluidU {
    blur: [f32; 4],
    dir: [f32; 4],
    shade: [f32; 4],
    deep: [f32; 4],
    shallow: [f32; 4],
    foam: [f32; 4],
}

pub const FLUID_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
pub const THICK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const NO_WATER: f64 = 1.0e6;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineV {
    pos: [f32; 3],
    color: [f32; 4],
}

pub struct RenderTarget {
    pub width: u32,
    pub height: u32,
    pub color: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
    // screen-space fluid buffers
    fluid_a: wgpu::TextureView,
    fluid_b: wgpu::TextureView,
    fluid_z: wgpu::TextureView,
    thick: wgpu::TextureView,
    scene_copy: wgpu::Texture,
    scene_copy_view: wgpu::TextureView,
}

impl RenderTarget {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let (width, height) = (width.max(8), height.max(8));
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target.color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target.depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let tex = |label: &str, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let view = |t: wgpu::Texture| t.create_view(&Default::default());
        let scene_copy = tex("target.scene_copy", COLOR_FORMAT, wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING);
        Self {
            width,
            height,
            color_view: color.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
            color,
            fluid_a: view(tex("fluid.a", FLUID_DEPTH_FORMAT, rt)),
            fluid_b: view(tex("fluid.b", FLUID_DEPTH_FORMAT, rt)),
            fluid_z: view(tex("fluid.z", DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT)),
            thick: view(tex("fluid.thickness", THICK_FORMAT, rt)),
            scene_copy_view: scene_copy.create_view(&Default::default()),
            scene_copy,
        }
    }
}

pub struct CameraMath {
    pub eye: Vec3,
    pub view_proj: Mat4,
    pub right: Vec3,
    pub up: Vec3,
}

pub fn camera_math(c: &CameraParams, aspect: f32) -> CameraMath {
    let target = Vec3::from(c.target);
    let dir = Vec3::new(c.pitch.cos() * c.yaw.sin(), c.pitch.sin(), c.pitch.cos() * c.yaw.cos());
    let eye = target + dir * c.distance;
    let view = glam::camera::rh::view::look_at_mat4(eye, target, Vec3::Y);
    let proj = glam::camera::rh::proj::directx::perspective(c.fov_deg.to_radians(), aspect.max(0.01), 0.02, 200.0);
    let fwd = (target - eye).normalize();
    let right = fwd.cross(Vec3::Y).normalize();
    let up = right.cross(fwd);
    CameraMath { eye, view_proj: proj * view, right, up }
}

pub struct Renderer {
    cam_buf: wgpu::Buffer,
    vol_buf: wgpu::Buffer,
    style_buf: wgpu::Buffer,
    scene_layout: wgpu::BindGroupLayout,
    vol_layout: wgpu::BindGroupLayout,
    part_layout: wgpu::BindGroupLayout,
    ground: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    water: wgpu::RenderPipeline,
    spray: wgpu::RenderPipeline,
    volume: wgpu::RenderPipeline,
    fluid_layout: wgpu::BindGroupLayout,
    fluid_depth: wgpu::RenderPipeline,
    fluid_thick: wgpu::RenderPipeline,
    fluid_blur: wgpu::RenderPipeline,
    fluid_shade: wgpu::RenderPipeline,
    fluid_h: wgpu::Buffer,
    fluid_v: wgpu::Buffer,
    spark_layout: wgpu::BindGroupLayout,
    spark_pipe: wgpu::RenderPipeline,
    spark_style: wgpu::Buffer,
    sampler: wgpu::Sampler,
    dummy_volume: wgpu::TextureView,
    dummy_storage: wgpu::Buffer,
    time: f32,
}

fn premul_blend() -> Option<wgpu::BlendState> {
    Some(wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    })
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let cam_buf = gpu::uniform_buffer(device, "camera", &CameraU::zeroed());
        let vol_buf = gpu::uniform_buffer(device, "volume.params", &VolU::zeroed());
        let style_buf = gpu::uniform_buffer(device, "particle.style", &StyleU::zeroed());

        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene.layout"),
            entries: &[gpu::uniform(0, vf)],
        });
        let vol_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("volume.layout"),
            entries: &[
                gpu::uniform(0, vf),
                gpu::uniform(1, vf),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let part_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particles.layout"),
            entries: &[
                gpu::uniform(0, vf),
                gpu::uniform(1, vf),
                gpu::storage(2, true, wgpu::ShaderStages::VERTEX),
                gpu::storage(3, true, wgpu::ShaderStages::VERTEX),
            ],
        });

        let layout = |l: &wgpu::BindGroupLayout| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(l)],
                immediate_size: 0,
            })
        };
        let scene_pl = layout(&scene_layout);
        let vol_pl = layout(&vol_layout);
        let part_pl = layout(&part_layout);

        let scene_mod = gpu::shader(device, "scene.wgsl", include_str!("shaders/scene.wgsl"));
        let vol_mod = gpu::shader(device, "volume.wgsl", include_str!("shaders/volume.wgsl"));
        let part_mod = gpu::shader(device, "particles.wgsl", include_str!("shaders/particles.wgsl"));

        let depth_write = |write: bool| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let make = |label: &str,
                    pl: &wgpu::PipelineLayout,
                    module: &wgpu::ShaderModule,
                    vs: &str,
                    fs: &str,
                    buffers: &[Option<wgpu::VertexBufferLayout>],
                    topology: wgpu::PrimitiveTopology,
                    depth: Option<wgpu::DepthStencilState>,
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(pl),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: depth,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let tri = wgpu::PrimitiveTopology::TriangleList;
        let ground = make("ground", &scene_pl, &scene_mod, "vs_ground", "fs_ground", &[], tri, depth_write(true), premul_blend());
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineV>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &line_attrs,
        };
        let lines = make("lines", &scene_pl, &scene_mod, "vs_line", "fs_line", &[Some(line_layout)], wgpu::PrimitiveTopology::LineList, depth_write(false), premul_blend());
        let water = make("water", &part_pl, &part_mod, "vs_water", "fs_particle", &[], tri, depth_write(true), None);
        let spray = make("spray", &part_pl, &part_mod, "vs_spray", "fs_particle", &[], tri, depth_write(true), None);
        let volume = make("volume", &vol_pl, &vol_mod, "vs_main", "fs_main", &[], tri, None, premul_blend());

        // ---- screen-space fluid
        let tex_entry = |binding: u32, filterable: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let fluid_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fluid.layout"),
            entries: &[
                gpu::uniform(0, vf),
                gpu::uniform(1, vf),
                tex_entry(2, false),
                tex_entry(3, false),
                tex_entry(4, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let fluid_pl = layout(&fluid_layout);
        let fluid_mod = gpu::shader(device, "fluid.wgsl", include_str!("shaders/fluid.wgsl"));
        let pipe = |label: &str,
                    pl: &wgpu::PipelineLayout,
                    module: &wgpu::ShaderModule,
                    vs: &str,
                    fs: &str,
                    format: wgpu::TextureFormat,
                    depth: Option<wgpu::DepthStencilState>,
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(pl),
                vertex: wgpu::VertexState { module, entry_point: Some(vs), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState { topology: tri, ..Default::default() },
                depth_stencil: depth,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive = Some(wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        });
        let fluid_depth = pipe("fluid.depth", &part_pl, &part_mod, "vs_water", "fs_fluid_depth", FLUID_DEPTH_FORMAT, depth_write(true), None);
        let fluid_thick = pipe("fluid.thickness", &part_pl, &part_mod, "vs_water", "fs_thickness", THICK_FORMAT, depth_write(false), additive);
        let fluid_blur = pipe("fluid.blur", &fluid_pl, &fluid_mod, "vs_full", "fs_blur", FLUID_DEPTH_FORMAT, None, None);
        let fluid_shade = pipe("fluid.shade", &fluid_pl, &fluid_mod, "vs_full", "fs_shade", COLOR_FORMAT, depth_write(true), None);
        let fluid_h = gpu::uniform_buffer(device, "fluid.h", &FluidU::zeroed());
        let fluid_v = gpu::uniform_buffer(device, "fluid.v", &FluidU::zeroed());

        // ---- sparks & embers: additive light, depth-tested, no depth writes
        let spark_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sparks.draw.layout"),
            entries: &[gpu::uniform(0, vf), gpu::uniform(1, vf), gpu::storage(2, true, wgpu::ShaderStages::VERTEX)],
        });
        let spark_pl = layout(&spark_layout);
        let spark_mod = gpu::shader(device, "sparks_draw.wgsl", include_str!("shaders/sparks_draw.wgsl"));
        let spark_pipe = pipe("sparks.draw", &spark_pl, &spark_mod, "vs_main", "fs_main", COLOR_FORMAT, depth_write(false), additive);
        let spark_style = gpu::uniform_buffer(device, "sparks.style", &SparkStyleU::zeroed());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("volume.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let dummy = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("dummy.volume"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            Default::default(),
            &[0u8; 8],
        );
        let dummy_storage = gpu::buffer(device, "dummy.storage", 64, gpu::STORAGE);
        Self {
            cam_buf,
            vol_buf,
            style_buf,
            scene_layout,
            vol_layout,
            part_layout,
            ground,
            lines,
            water,
            spray,
            volume,
            fluid_layout,
            fluid_depth,
            fluid_thick,
            fluid_blur,
            fluid_shade,
            fluid_h,
            fluid_v,
            spark_layout,
            spark_pipe,
            spark_style,
            sampler,
            dummy_volume: dummy.create_view(&Default::default()),
            dummy_storage,
            time: 0.0,
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        scene: &Scene,
        smoke: Option<&SmokeSim>,
        water: Option<&WaterSim>,
        sparks: Option<&SparkSim>,
        sim_time: f32,
        hide_helpers: bool,
    ) {
        self.time += 1.0 / 60.0;
        let r = &scene.render;
        let aspect = target.width as f32 / target.height as f32;
        let cm = camera_math(&scene.camera, aspect);
        let ld = Vec3::from(r.light_dir).normalize_or(Vec3::Y);
        let bg = if r.transparent_background { [0.0; 4] } else { [r.background[0], r.background[1], r.background[2], 1.0] };
        let cu = CameraU {
            view_proj: cm.view_proj.to_cols_array_2d(),
            inv_view_proj: cm.view_proj.inverse().to_cols_array_2d(),
            cam_pos: cm.eye.extend(1.0).into(),
            cam_right: cm.right.extend(0.0).into(),
            cam_up: cm.up.extend(0.0).into(),
            light_dir: [ld.x, ld.y, ld.z, r.exposure],
            viewport: [target.width as f32, target.height as f32, self.time, if r.transparent_background { 1.0 } else { 0.0 }],
            bg_color: bg,
        };
        queue.write_buffer(&self.cam_buf, 0, bytemuck::bytes_of(&cu));

        let sp = &scene.smoke;
        let smin = Vec3::from(sp.box_min);
        let smax = smin + Vec3::from(sp.box_size());
        let vu = VolU {
            box_min: smin.extend(sp.step_scale.clamp(0.25, 4.0)).into(),
            box_max: smax.extend(sp.max_steps.clamp(16, 512) as f32).into(),
            smoke_color: [sp.smoke_color[0], sp.smoke_color[1], sp.smoke_color[2], sp.smoke_absorption],
            fire: [sp.flame_intensity, sp.temperature_scale, sp.self_illumination, sp.shadow_density],
            ambient: [0.12, 0.13, 0.16, sp.fuel_glow],
        };
        queue.write_buffer(&self.vol_buf, 0, bytemuck::bytes_of(&vu));

        let wp = &scene.water;
        let su = StyleU {
            box_min: [wp.box_min[0], wp.box_min[1], wp.box_min[2], wp.cell_size],
            deep: [wp.deep_color[0], wp.deep_color[1], wp.deep_color[2], wp.particle_size],
            shallow: [wp.shallow_color[0], wp.shallow_color[1], wp.shallow_color[2], wp.spray_size],
            foam: [wp.foam_color[0], wp.foam_color[1], wp.foam_color[2], wp.foam_strength],
        };
        queue.write_buffer(&self.style_buf, 0, bytemuck::bytes_of(&su));

        // bounds
        let mut lines: Vec<LineV> = Vec::new();
        if r.show_bounds && !hide_helpers {
            if smoke.is_some() {
                push_box(&mut lines, smin, smax, [1.0, 0.45, 0.15, 0.55]);
            }
            if water.is_some() {
                let wmin = Vec3::from(wp.box_min);
                let wmax = wmin + Vec3::new(wp.res[0] as f32, wp.res[1] as f32, wp.res[2] as f32) * wp.cell_size;
                push_box(&mut lines, wmin, wmax, [0.25, 0.65, 1.0, 0.55]);
            }
        }
        let line_buf = (!lines.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lines"),
                contents: bytemuck::cast_slice(&lines),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });

        let scene_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.scene_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.cam_buf.as_entire_binding() }],
        });
        let part_bind = water.map(|w| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.part_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.cam_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.style_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: w.particles.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: w.spray.as_entire_binding() },
                ],
            })
        });
        let _ = &self.dummy_storage;
        let vol_view = smoke.map(|s| &s.volume_view).unwrap_or(&self.dummy_volume);
        let vol_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.vol_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.cam_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.vol_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(vol_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&target.depth_view) },
            ],
        });

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("render") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("opaque"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: bg[3] as f64 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if r.show_ground && !(hide_helpers && r.transparent_background) {
                pass.set_pipeline(&self.ground);
                pass.set_bind_group(0, &scene_bind, &[]);
                pass.draw(0..6, 0..1);
            }
            if let (Some(w), Some(b)) = (water, &part_bind) {
                pass.set_bind_group(0, b, &[]);
                if w.count > 0 && wp.look == WaterLook::Particles {
                    pass.set_pipeline(&self.water);
                    pass.draw(0..6, 0..w.count);
                }
                if wp.spray_enabled {
                    pass.set_pipeline(&self.spray);
                    pass.draw(0..6, 0..w.max_spray);
                }
            }
            if let Some(lb) = &line_buf {
                pass.set_pipeline(&self.lines);
                pass.set_bind_group(0, &scene_bind, &[]);
                pass.set_vertex_buffer(0, lb.slice(..));
                pass.draw(0..lines.len() as u32, 0..1);
            }
        }
        if let (Some(w), Some(pb)) = (water, &part_bind) {
            if w.count > 0 && wp.look == WaterLook::Surface {
                self.fluid_passes(device, queue, &mut enc, target, scene, w, pb);
            }
        }
        if smoke.is_some() {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("volume"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.volume);
            pass.set_bind_group(0, &vol_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(k) = sparks {
            let kp = &scene.sparks;
            let px_per_unit = target.height as f32 / (2.0 * (scene.camera.fov_deg.to_radians() * 0.5).tan());
            let st = SparkStyleU {
                spark_hot: [kp.spark_hot_color[0], kp.spark_hot_color[1], kp.spark_hot_color[2], kp.spark_brightness],
                spark_cool: [kp.spark_cool_color[0], kp.spark_cool_color[1], kp.spark_cool_color[2], kp.spark_streak.max(0.0)],
                ember_hot: [kp.ember_hot_color[0], kp.ember_hot_color[1], kp.ember_hot_color[2], kp.ember_brightness],
                ember_cool: [kp.ember_cool_color[0], kp.ember_cool_color[1], kp.ember_cool_color[2], kp.ember_flicker],
                size: [kp.spark_size, kp.ember_size, px_per_unit, sim_time],
            };
            queue.write_buffer(&self.spark_style, 0, bytemuck::bytes_of(&st));
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.spark_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.cam_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.spark_style.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: k.parts.as_entire_binding() },
                ],
            });
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sparks"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.spark_pipe);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..6, 0..k.max);
        }
        queue.submit([enc.finish()]);
    }
}

impl Renderer {
    #[allow(clippy::too_many_arguments)]
    fn fluid_passes(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        scene: &Scene,
        w: &WaterSim,
        part_bind: &wgpu::BindGroup,
    ) {
        let wp = &scene.water;
        let radius = wp.cell_size * 0.5 * wp.particle_size;
        let px_per_unit = target.height as f32 / (2.0 * (scene.camera.fov_deg.to_radians() * 0.5).tan());
        let make = |dir: [f32; 2]| FluidU {
            blur: [radius, px_per_unit * wp.smoothing_radius, wp.smoothing_depth, 40.0],
            dir: [dir[0], dir[1], 0.0, 0.0],
            shade: [wp.refraction, wp.absorption, wp.thickness_scale, wp.foam_strength],
            deep: [wp.deep_color[0], wp.deep_color[1], wp.deep_color[2], 0.0],
            shallow: [wp.shallow_color[0], wp.shallow_color[1], wp.shallow_color[2], 0.0],
            foam: [wp.foam_color[0], wp.foam_color[1], wp.foam_color[2], 0.0],
        };
        queue.write_buffer(&self.fluid_h, 0, bytemuck::bytes_of(&make([1.0, 0.0])));
        queue.write_buffer(&self.fluid_v, 0, bytemuck::bytes_of(&make([0.0, 1.0])));

        fn color_att(view: &wgpu::TextureView, clear: Option<wgpu::Color>) -> Option<wgpu::RenderPassColorAttachment<'_>> {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: clear.map(wgpu::LoadOp::Clear).unwrap_or(wgpu::LoadOp::Load),
                    store: wgpu::StoreOp::Store,
                },
            })
        }
        let none = wgpu::Color { r: NO_WATER, g: NO_WATER, b: NO_WATER, a: NO_WATER };

        // 1. nearest-sphere eye depth
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fluid.depth"),
                color_attachments: &[color_att(&target.fluid_a, Some(none))],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.fluid_z,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.fluid_depth);
            pass.set_bind_group(0, part_bind, &[]);
            pass.draw(0..6, 0..w.count);
        }
        // 2. accumulated thickness (+ foam), occluded by the opaque scene
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fluid.thickness"),
                color_attachments: &[color_att(&target.thick, Some(wgpu::Color::TRANSPARENT))],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.fluid_thick);
            pass.set_bind_group(0, part_bind, &[]);
            pass.draw(0..6, 0..w.count);
        }
        // 3. copy what is behind the water, for refraction
        enc.copy_texture_to_texture(
            target.color.as_image_copy(),
            target.scene_copy.as_image_copy(),
            wgpu::Extent3d { width: target.width, height: target.height, depth_or_array_layers: 1 },
        );

        let bind = |depth: &wgpu::TextureView, u: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.fluid_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.cam_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: u.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(depth) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&target.thick) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&target.scene_copy_view) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            })
        };
        // 4. separable bilateral smoothing: a -> b (horizontal), b -> a (vertical)
        let bh = bind(&target.fluid_a, &self.fluid_h);
        let bv = bind(&target.fluid_b, &self.fluid_v);
        for _ in 0..wp.smoothing_passes.min(8) {
            for (src_bind, dst) in [(&bh, &target.fluid_b), (&bv, &target.fluid_a)] {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("fluid.blur"),
                    color_attachments: &[color_att(dst, None)],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.fluid_blur);
                pass.set_bind_group(0, src_bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        // 5. shade the surface into the target, depth-tested against the scene
        let bs = bind(&target.fluid_a, &self.fluid_h);
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fluid.shade"),
            color_attachments: &[color_att(&target.color_view, None)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.fluid_shade);
        pass.set_bind_group(0, &bs, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn push_box(out: &mut Vec<LineV>, a: Vec3, b: Vec3, color: [f32; 4]) {
    let c = |x: bool, y: bool, z: bool| [if x { b.x } else { a.x }, if y { b.y } else { a.y }, if z { b.z } else { a.z }];
    let edges = [
        ((0, 0, 0), (1, 0, 0)), ((0, 1, 0), (1, 1, 0)), ((0, 0, 1), (1, 0, 1)), ((0, 1, 1), (1, 1, 1)),
        ((0, 0, 0), (0, 1, 0)), ((1, 0, 0), (1, 1, 0)), ((0, 0, 1), (0, 1, 1)), ((1, 0, 1), (1, 1, 1)),
        ((0, 0, 0), (0, 0, 1)), ((1, 0, 0), (1, 0, 1)), ((0, 1, 0), (0, 1, 1)), ((1, 1, 0), (1, 1, 1)),
    ];
    for ((x0, y0, z0), (x1, y1, z1)) in edges {
        out.push(LineV { pos: c(x0 == 1, y0 == 1, z0 == 1), color });
        out.push(LineV { pos: c(x1 == 1, y1 == 1, z1 == 1), color });
    }
}

/// Draws a finished `RenderTarget` onto a window or canvas surface.
pub struct Presenter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    ubuf: wgpu::Buffer,
    pub format: wgpu::TextureFormat,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PresentU {
    flags: [f32; 4],
}

impl Presenter {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let fs = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present.layout"),
            entries: &[
                gpu::uniform(0, fs),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: fs,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: fs,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("present.pl"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = gpu::shader(device, "present.wgsl", include_str!("shaders/present.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("present.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let ubuf = gpu::uniform_buffer(device, "present.params", &PresentU::zeroed());
        Self { pipeline, layout, sampler, ubuf, format }
    }

    /// Draws `src` over the whole of `dst`. `checker` shows a checkerboard
    /// where the frame is transparent.
    pub fn present(&self, device: &wgpu::Device, queue: &wgpu::Queue, src: &RenderTarget, dst: &wgpu::TextureView, checker: bool) {
        let u = PresentU { flags: [if self.format.is_srgb() { 0.0 } else { 1.0 }, if checker { 1.0 } else { 0.0 }, 0.0, 0.0] };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&src.color_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("present") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([enc.finish()]);
    }
}
