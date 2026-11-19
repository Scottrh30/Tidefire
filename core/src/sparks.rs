//! GPU sparks and embers. The CPU only decides how many to spawn each step
//! (continuous rate plus random bursts); the GPU spawns, moves and draws them.

use crate::gpu::{self, wgpu};
use crate::params::{SmokeParams, SparkParams};
use crate::smoke::SmokeSim;
use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SparkU {
    emit: [u32; 4],
    misc: [f32; 4],
    emitter: [f32; 4],
    spark_dir: [f32; 4],
    spark: [f32; 4],
    spark2: [f32; 4],
    ember: [f32; 4],
    ember2: [f32; 4],
    smoke_min: [f32; 4],
    smoke_res: [u32; 4],
}

/// Small deterministic RNG (the same scene always produces the same bursts).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
}

pub struct SparkSim {
    pub max: u32,
    pub parts: wgpu::Buffer,
    ubuf: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    emit: wgpu::ComputePipeline,
    update: wgpu::ComputePipeline,
    clear: wgpu::ComputePipeline,
    dummy_vel: wgpu::Buffer,
    next_slot: u32,
    carry_spark: f32,
    carry_ember: f32,
    burst_timer: f32,
    rng: Rng,
    step_index: u32,
}

const PARTICLE_BYTES: u64 = 48;

impl SparkSim {
    pub fn new(device: &wgpu::Device, p: &SparkParams) -> Self {
        let max = p.max_particles.clamp(256, 2_000_000);
        let parts = gpu::buffer(device, "sparks.parts", max as u64 * PARTICLE_BYTES, gpu::STORAGE);
        let ubuf = gpu::uniform_buffer(device, "sparks.params", &SparkU::zeroed());
        let dummy_vel = gpu::buffer(device, "sparks.no_smoke", 64, gpu::STORAGE);
        let cs = wgpu::ShaderStages::COMPUTE;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sparks.layout"),
            entries: &[gpu::uniform(0, cs), gpu::storage(1, false, cs), gpu::storage(2, true, cs)],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sparks.pl"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = gpu::shader(device, "sparks.wgsl", include_str!("shaders/sparks.wgsl"));
        Self {
            max,
            parts,
            ubuf,
            emit: gpu::compute(device, &pl, &module, "emit"),
            update: gpu::compute(device, &pl, &module, "update"),
            clear: gpu::compute(device, &pl, &module, "clear"),
            layout,
            dummy_vel,
            next_slot: 0,
            carry_spark: 0.0,
            carry_ember: 0.0,
            burst_timer: 0.0,
            rng: Rng(0xC0FFEE1234567),
            step_index: 0,
        }
    }

    fn bind(&self, device: &wgpu::Device, smoke: Option<&SmokeSim>) -> wgpu::BindGroup {
        let vel = smoke.map(|s| s.velocity()).unwrap_or(&self.dummy_vel);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sparks.bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.parts.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: vel.as_entire_binding() },
            ],
        })
    }

    pub fn reset(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.next_slot = 0;
        self.carry_spark = 0.0;
        self.carry_ember = 0.0;
        self.burst_timer = 0.25;
        self.rng = Rng(0xC0FFEE1234567);
        self.step_index = 0;
        let u = SparkU { emit: [0, 0, 0, self.max], ..SparkU::zeroed() };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let bind = self.bind(device, None);
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.clear);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(gpu::groups(self.max, 64), 1, 1);
        }
        queue.submit([enc.finish()]);
    }

    /// One substep. `smoke` lets embers ride the fire's currents.
    pub fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        p: &SparkParams,
        smoke: Option<(&SmokeSim, &SmokeParams)>,
        dt: f32,
        time: f32,
    ) {
        // Where they come from: the fire's emitter, or the spark emitter's own.
        let fire = smoke.map(|(_, sp)| sp).filter(|_| p.use_fire_emitter);
        let (pos, radius) = match fire {
            Some(sp) => (Vec3::from(sp.emitter_pos), sp.emitter_radius * 0.7),
            None => (Vec3::from(p.emitter_pos), p.emitter_radius),
        };
        let mut emitting = p.duration <= 0.0 || time < p.duration;
        if let Some(sp) = fire {
            emitting &= sp.emitter_on && (sp.emitter_duration <= 0.0 || time < sp.emitter_duration + 0.3);
        }

        let mut n_spark = 0u32;
        let mut n_ember = 0u32;
        if emitting {
            if p.sparks {
                let want = p.spark_rate.max(0.0) * dt + self.carry_spark;
                n_spark = want.floor() as u32;
                self.carry_spark = want - n_spark as f32;
                if p.burst_count > 0 && p.burst_interval > 0.0 {
                    self.burst_timer -= dt;
                    if self.burst_timer <= 0.0 {
                        n_spark += (p.burst_count as f32 * (0.5 + self.rng.next())) as u32;
                        self.burst_timer = p.burst_interval * (0.35 + 1.3 * self.rng.next());
                    }
                }
            }
            if p.embers {
                let want = p.ember_rate.max(0.0) * dt + self.carry_ember;
                n_ember = want.floor() as u32;
                self.carry_ember = want - n_ember as f32;
            }
        }
        let room = self.max / 2;
        n_spark = n_spark.min(room);
        n_ember = n_ember.min(room);

        let dir = Vec3::from(p.spark_direction).normalize_or(Vec3::Y);
        let (smoke_min, smoke_res, has_smoke) = match smoke {
            Some((s, sp)) => ([sp.box_min[0], sp.box_min[1], sp.box_min[2], sp.cell_size], s.res, 1.0),
            None => ([0.0; 4], [1, 1, 1], 0.0),
        };
        self.step_index = self.step_index.wrapping_add(1);
        let u = SparkU {
            emit: [self.next_slot, n_spark, n_ember, self.max],
            misc: [dt, time, 0.0, has_smoke],
            emitter: [pos.x, pos.y, pos.z, radius.max(0.0)],
            spark_dir: [dir.x, dir.y, dir.z, p.spark_spread.clamp(0.0, 180.0).to_radians()],
            spark: [p.spark_speed, p.spark_speed_variation.clamp(0.0, 1.0), p.spark_lifetime.max(0.01), p.spark_gravity],
            spark2: [p.spark_drag.max(0.0), p.spark_bounce.clamp(0.0, 1.0), p.spark_follow_smoke.clamp(0.0, 1.0), 0.6],
            ember: [p.ember_lifetime.max(0.01), p.ember_buoyancy, p.ember_follow_smoke.clamp(0.0, 1.0), p.ember_turbulence.max(0.0)],
            ember2: [p.ember_launch, 0.0, 0.0, 0.0],
            smoke_min,
            smoke_res: [smoke_res[0], smoke_res[1], smoke_res[2], self.step_index.wrapping_mul(0x9E3779B9)],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));
        let bind = self.bind(device, smoke.map(|(s, _)| s));
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sparks.step") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sparks"), timestamp_writes: None });
            pass.set_bind_group(0, &bind, &[]);
            let n = n_spark + n_ember;
            if n > 0 {
                pass.set_pipeline(&self.emit);
                pass.dispatch_workgroups(gpu::groups(n, 64), 1, 1);
            }
            pass.set_pipeline(&self.update);
            pass.dispatch_workgroups(gpu::groups(self.max, 64), 1, 1);
        }
        queue.submit([enc.finish()]);
        self.next_slot = (self.next_slot + n_spark + n_ember) % self.max;
    }
}
