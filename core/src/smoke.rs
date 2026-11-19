//! GPU smoke / fire simulator.

use crate::gpu::{self, wgpu};
use crate::params::SmokeParams;
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SmokeU {
    res: [u32; 4],
    box_min: [f32; 4],
    emit_pos: [f32; 4],
    emit_vel: [f32; 4],
    emit_rates: [f32; 4],
    combust: [f32; 4],
    diss: [f32; 4],
    forces: [f32; 4],
    wind: [f32; 4],
    shape: [f32; 4],
}

struct Pipes {
    emit: wgpu::ComputePipeline,
    advect_vel: wgpu::ComputePipeline,
    advect_scalars: wgpu::ComputePipeline,
    sc_forward: wgpu::ComputePipeline,
    sc_backward: wgpu::ComputePipeline,
    maccormack: wgpu::ComputePipeline,
    curl: wgpu::ComputePipeline,
    forces: wgpu::ComputePipeline,
    divergence: wgpu::ComputePipeline,
    red: wgpu::ComputePipeline,
    black: wgpu::ComputePipeline,
    project: wgpu::ComputePipeline,
    pack: wgpu::ComputePipeline,
    clear: wgpu::ComputePipeline,
}

pub struct SmokeSim {
    pub res: [u32; 3],
    ubuf: wgpu::Buffer,
    vel: wgpu::Buffer,
    vel_tmp: wgpu::Buffer,
    sc: wgpu::Buffer,
    sc_tmp: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipes: Pipes,
    _volume: wgpu::Texture,
    pub volume_view: wgpu::TextureView,
    frame: u32,
}

impl SmokeSim {
    pub fn new(device: &wgpu::Device, p: &SmokeParams) -> Self {
        let res = p.res.map(|r| r.clamp(8, 512));
        let cells = (res[0] * res[1] * res[2]) as u64;
        let v4 = cells * 16;
        let vel = gpu::buffer(device, "smoke.vel", v4, gpu::STORAGE);
        let vel_tmp = gpu::buffer(device, "smoke.vel_tmp", v4, gpu::STORAGE);
        let sc = gpu::buffer(device, "smoke.sc", v4, gpu::STORAGE);
        let sc_tmp = gpu::buffer(device, "smoke.sc_tmp", v4, gpu::STORAGE);
        let pd = gpu::buffer(device, "smoke.pd", cells * 8, gpu::STORAGE);
        let curl = gpu::buffer(device, "smoke.curl", v4, gpu::STORAGE);
        let ubuf = gpu::uniform_buffer(device, "smoke.params", &SmokeU::zeroed());

        let volume = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("smoke.volume"),
            size: wgpu::Extent3d {
                width: res[0],
                height: res[1],
                depth_or_array_layers: res[2],
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let volume_view = volume.create_view(&Default::default());

        let cs = wgpu::ShaderStages::COMPUTE;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("smoke.layout"),
            entries: &[
                gpu::uniform(0, cs),
                gpu::storage(1, false, cs),
                gpu::storage(2, false, cs),
                gpu::storage(3, false, cs),
                gpu::storage(4, false, cs),
                gpu::storage(5, false, cs),
                gpu::storage(6, false, cs),
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: cs,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba16Float,
                        view_dimension: wgpu::TextureViewDimension::D3,
                    },
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("smoke.bind"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ubuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: vel.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: vel_tmp.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: sc.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: sc_tmp.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: pd.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: curl.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&volume_view),
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("smoke.pl"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = gpu::shader(device, "smoke.wgsl", include_str!("shaders/smoke.wgsl"));
        let c = |e: &str| gpu::compute(device, &pl, &module, e);
        let pipes = Pipes {
            emit: c("emit"),
            advect_vel: c("advect_vel"),
            advect_scalars: c("advect_scalars"),
            sc_forward: c("advect_sc_forward"),
            sc_backward: c("advect_sc_backward"),
            maccormack: c("maccormack_scalars"),
            curl: c("compute_curl"),
            forces: c("forces"),
            divergence: c("divergence"),
            red: c("pressure_red"),
            black: c("pressure_black"),
            project: c("project"),
            pack: c("pack_volume"),
            clear: c("clear_all"),
        };
        Self {
            res,
            ubuf,
            vel,
            vel_tmp,
            sc,
            sc_tmp,
            bind,
            pipes,
            _volume: volume,
            volume_view,
            frame: 0,
        }
    }

    fn uniforms(&self, p: &SmokeParams, dt: f32, time: f32) -> SmokeU {
        let h = p.cell_size;
        let to_grid = |w: [f32; 3]| -> [f32; 3] {
            [
                (w[0] - p.box_min[0]) / h,
                (w[1] - p.box_min[1]) / h,
                (w[2] - p.box_min[2]) / h,
            ]
        };
        let e = to_grid(p.emitter_pos);
        SmokeU {
            res: [self.res[0], self.res[1], self.res[2], self.frame],
            box_min: [p.box_min[0], p.box_min[1], p.box_min[2], h],
            emit_pos: [e[0], e[1], e[2], (p.emitter_radius / h).max(1.0)],
            emit_vel: [
                p.emit_velocity[0],
                p.emit_velocity[1],
                p.emit_velocity[2],
                p.turbulence,
            ],
            emit_rates: [p.fuel_rate, p.temp_rate, p.smoke_rate, dt],
            combust: [
                p.burn_rate,
                p.heat_release,
                p.smoke_from_burn,
                p.ignition_temp,
            ],
            diss: [
                p.cooling,
                p.smoke_dissipation,
                p.fuel_dissipation,
                p.velocity_damping,
            ],
            forces: [p.buoyancy, p.smoke_weight, p.vorticity, time],
            wind: [
                p.wind[0],
                p.wind[1],
                p.wind[2],
                if p.emitter_on { 1.0 } else { 0.0 },
            ],
            shape: [
                p.noise_scale,
                p.flicker_speed,
                if p.open_top { 1.0 } else { 0.0 },
                p.emitter_patchiness.clamp(0.0, 1.0),
            ],
        }
    }

    fn dispatch(&self, pass: &mut wgpu::ComputePass, pipe: &wgpu::ComputePipeline) {
        pass.set_pipeline(pipe);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.dispatch_workgroups(
            gpu::groups(self.res[0], 4),
            gpu::groups(self.res[1], 4),
            gpu::groups(self.res[2], 4),
        );
    }

    pub fn reset(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, p: &SmokeParams) {
        self.frame = 0;
        queue.write_buffer(
            &self.ubuf,
            0,
            bytemuck::bytes_of(&self.uniforms(p, 0.0, 0.0)),
        );
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            self.dispatch(&mut pass, &self.pipes.clear);
            self.dispatch(&mut pass, &self.pipes.pack);
        }
        queue.submit([enc.finish()]);
    }

    /// One solver substep (separate submission so the uniform update is visible).
    pub fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        p: &SmokeParams,
        dt: f32,
        time: f32,
    ) {
        queue.write_buffer(
            &self.ubuf,
            0,
            bytemuck::bytes_of(&self.uniforms(p, dt, time)),
        );
        let bytes = self.vel.size();
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("smoke.step"),
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("smoke.a"),
                timestamp_writes: None,
            });
            self.dispatch(&mut pass, &self.pipes.emit);
            if p.sharp_advection {
                self.dispatch(&mut pass, &self.pipes.sc_forward);
                self.dispatch(&mut pass, &self.pipes.sc_backward);
                self.dispatch(&mut pass, &self.pipes.maccormack);
            } else {
                self.dispatch(&mut pass, &self.pipes.advect_scalars);
            }
            self.dispatch(&mut pass, &self.pipes.advect_vel);
        }
        enc.copy_buffer_to_buffer(&self.sc_tmp, 0, &self.sc, 0, bytes);
        enc.copy_buffer_to_buffer(&self.vel_tmp, 0, &self.vel, 0, bytes);
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("smoke.b"),
                timestamp_writes: None,
            });
            self.dispatch(&mut pass, &self.pipes.curl);
            self.dispatch(&mut pass, &self.pipes.forces);
            self.dispatch(&mut pass, &self.pipes.divergence);
            for _ in 0..p.pressure_iters.max(1) {
                self.dispatch(&mut pass, &self.pipes.red);
                self.dispatch(&mut pass, &self.pipes.black);
            }
            self.dispatch(&mut pass, &self.pipes.project);
            self.dispatch(&mut pass, &self.pipes.pack);
        }
        queue.submit([enc.finish()]);
        self.frame = self.frame.wrapping_add(1);
    }

    /// The velocity field (m/s per cell), for things carried by the smoke.
    pub fn velocity(&self) -> &wgpu::Buffer {
        &self.vel
    }

    pub fn memory_bytes(&self) -> u64 {
        let cells = (self.res[0] * self.res[1] * self.res[2]) as u64;
        cells * (16 * 5 + 8 + 8)
    }
}
