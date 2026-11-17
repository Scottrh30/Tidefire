//! GPU FLIP liquid + whitewater spray simulator.

use crate::gpu::{self, wgpu};
use crate::params::WaterParams;
use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct WaterU {
    res: [u32; 4],
    box_min: [f32; 4],
    misc: [f32; 4],
    spray: [f32; 4],
    spray2: [u32; 4],
    foam: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Particle {
    pub pos: [f32; 4],
    pub vel: [f32; 4],
}

struct Pipes {
    clear_grid: wgpu::ComputePipeline,
    p2g: wgpu::ComputePipeline,
    normalize: wgpu::ComputePipeline,
    divergence: wgpu::ComputePipeline,
    red: wgpu::ComputePipeline,
    black: wgpu::ComputePipeline,
    project: wgpu::ComputePipeline,
    g2p: wgpu::ComputePipeline,
    spray: wgpu::ComputePipeline,
    clear_spray: wgpu::ComputePipeline,
}

/// Tiny deterministic RNG so the same scene always seeds the same particles.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
}

pub struct WaterSim {
    pub res: [u32; 3],
    pub max_particles: u32,
    pub max_spray: u32,
    pub count: u32,
    ubuf: wgpu::Buffer,
    pub particles: wgpu::Buffer,
    pub spray: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipes: Pipes,
    rng: Rng,
    jet_carry: Vec<f32>,
    frame: u32,
}

impl WaterSim {
    pub fn new(device: &wgpu::Device, p: &WaterParams) -> Self {
        let res = p.res.map(|r| r.clamp(8, 384));
        let nodes = ((res[0] + 1) * (res[1] + 1) * (res[2] + 1)) as u64;
        let max_particles = p.max_particles.clamp(1024, 4_000_000);
        let max_spray = if p.spray_enabled { p.max_spray.clamp(1024, 4_000_000) } else { 1024 };
        let particles = gpu::buffer(device, "water.particles", max_particles as u64 * 32, gpu::STORAGE);
        let accum = gpu::buffer(device, "water.accum", nodes * 7 * 4, gpu::STORAGE);
        let gvel = gpu::buffer(device, "water.gvel", nodes * 16, gpu::STORAGE);
        let gvel_old = gpu::buffer(device, "water.gvel_old", nodes * 16, gpu::STORAGE);
        let cell = gpu::buffer(device, "water.cell", nodes * 16, gpu::STORAGE);
        let spray = gpu::buffer(device, "water.spray", max_spray as u64 * 32, gpu::STORAGE);
        let counter = gpu::buffer(device, "water.counter", 16, gpu::STORAGE);
        let ubuf = gpu::uniform_buffer(device, "water.params", &WaterU::zeroed());

        let cs = wgpu::ShaderStages::COMPUTE;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("water.layout"),
            entries: &[
                gpu::uniform(0, cs),
                gpu::storage(1, false, cs),
                gpu::storage(2, false, cs),
                gpu::storage(3, false, cs),
                gpu::storage(4, false, cs),
                gpu::storage(5, false, cs),
                gpu::storage(6, false, cs),
                gpu::storage(7, false, cs),
            ],
        });
        let entries: Vec<wgpu::BindGroupEntry> = [&ubuf, &particles, &accum, &gvel, &gvel_old, &cell, &spray, &counter]
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry { binding: i as u32, resource: b.as_entire_binding() })
            .collect();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("water.bind"), layout: &layout, entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("water.pl"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = gpu::shader(device, "water.wgsl", include_str!("shaders/water.wgsl"));
        let c = |e: &str| gpu::compute(device, &pl, &module, e);
        let pipes = Pipes {
            clear_grid: c("clear_grid"),
            p2g: c("p2g"),
            normalize: c("normalize_grid"),
            divergence: c("divergence"),
            red: c("pressure_red"),
            black: c("pressure_black"),
            project: c("project"),
            g2p: c("g2p"),
            spray: c("update_spray"),
            clear_spray: c("clear_spray"),
        };
        Self {
            res,
            max_particles,
            max_spray,
            count: 0,
            ubuf,
            particles,
            spray,
            bind,
            pipes,
            rng: Rng(0x9E3779B97F4A7C15),
            jet_carry: vec![],
            frame: 0,
        }
    }

    fn uniforms(&self, p: &WaterParams, dt: f32, time: f32) -> WaterU {
        WaterU {
            res: [self.res[0], self.res[1], self.res[2], self.count],
            box_min: [p.box_min[0], p.box_min[1], p.box_min[2], p.cell_size],
            misc: [dt, p.flip_ratio, p.gravity, time],
            spray: [p.spray_threshold, p.spray_amount, p.spray_drag, p.spray_lifetime],
            spray2: [if p.spray_enabled { self.max_spray } else { 0 }, self.frame, 0, 0],
            foam: [p.foam_threshold, p.foam_decay, p.density_correction, 8.0],
        }
    }

    fn to_grid(p: &WaterParams, w: Vec3) -> Vec3 {
        (w - Vec3::from(p.box_min)) / p.cell_size
    }

    fn inside(&self, g: Vec3) -> bool {
        g.x > 1.02 && g.y > 1.02 && g.z > 1.02
            && g.x < self.res[0] as f32 - 1.02
            && g.y < self.res[1] as f32 - 0.02
            && g.z < self.res[2] as f32 - 1.02
    }

    fn upload(&mut self, queue: &wgpu::Queue, new: &[Particle]) {
        let room = (self.max_particles - self.count) as usize;
        let n = new.len().min(room);
        if n == 0 {
            return;
        }
        queue.write_buffer(&self.particles, self.count as u64 * 32, bytemuck::cast_slice(&new[..n]));
        self.count += n as u32;
    }

    pub fn reset(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, p: &WaterParams) {
        self.count = 0;
        self.frame = 0;
        self.rng = Rng(0x9E3779B97F4A7C15);
        self.jet_carry = vec![0.0; p.jets.len()];
        // seed blocks with 2x2x2 jittered particles per cell
        let mut seeds = Vec::new();
        for b in &p.blocks {
            let lo = Self::to_grid(p, Vec3::from(b.min)).max(Vec3::splat(1.0));
            let hi = Self::to_grid(p, Vec3::from(b.max)).min(Vec3::new(self.res[0] as f32 - 1.0, self.res[1] as f32, self.res[2] as f32 - 1.0));
            let (i0, i1) = (lo.x.floor() as i32, hi.x.ceil() as i32);
            let (j0, j1) = (lo.y.floor() as i32, hi.y.ceil() as i32);
            let (k0, k1) = (lo.z.floor() as i32, hi.z.ceil() as i32);
            for k in k0..k1 {
                for j in j0..j1 {
                    for i in i0..i1 {
                        for s in 0..8 {
                            let o = Vec3::new((s & 1) as f32, ((s >> 1) & 1) as f32, ((s >> 2) & 1) as f32) * 0.5
                                + Vec3::new(self.rng.next(), self.rng.next(), self.rng.next()) * 0.5;
                            let g = Vec3::new(i as f32, j as f32, k as f32) + o;
                            if g.cmpge(lo).all() && g.cmplt(hi).all() && self.inside(g) {
                                seeds.push(Particle { pos: [g.x, g.y, g.z, 0.0], vel: [b.velocity[0], b.velocity[1], b.velocity[2], 0.0] });
                            }
                        }
                    }
                }
            }
        }
        self.upload(queue, &seeds);
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&self.uniforms(p, 0.0, 0.0)));
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipes.clear_spray);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.dispatch_workgroups(gpu::groups(self.max_spray, 64), 1, 1);
        }
        queue.submit([enc.finish()]);
    }

    fn emit_jets(&mut self, queue: &wgpu::Queue, p: &WaterParams, dt: f32, time: f32) {
        if self.jet_carry.len() != p.jets.len() {
            self.jet_carry = vec![0.0; p.jets.len()];
        }
        let h = p.cell_size;
        let mut out = Vec::new();
        for (ji, jet) in p.jets.iter().enumerate() {
            if !jet.enabled || (jet.duration > 0.0 && time > jet.duration) {
                continue;
            }
            let v = Vec3::from(jet.velocity);
            let speed = v.length();
            if speed < 1e-3 {
                continue;
            }
            // particle volume = h^3 / 8 ; flow = area * speed
            let flow = std::f32::consts::PI * jet.radius * jet.radius * speed;
            let per_step = flow * dt / (h * h * h / 8.0) + self.jet_carry[ji];
            let n = per_step.floor() as usize;
            self.jet_carry[ji] = per_step - n as f32;
            let dir = v / speed;
            let a = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
            let t1 = dir.cross(a).normalize();
            let t2 = dir.cross(t1);
            for _ in 0..n {
                let ang = self.rng.next() * std::f32::consts::TAU;
                let rr = self.rng.next().sqrt() * jet.radius;
                let along = self.rng.next() * speed * dt;
                let w = Vec3::from(jet.pos) + (t1 * ang.cos() + t2 * ang.sin()) * rr + dir * along;
                let g = Self::to_grid(p, w);
                if self.inside(g) {
                    out.push(Particle { pos: [g.x, g.y, g.z, 0.0], vel: [v.x, v.y, v.z, time] });
                }
            }
        }
        self.upload(queue, &out);
    }

    pub fn step(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, p: &WaterParams, dt: f32, time: f32) {
        self.emit_jets(queue, p, dt, time);
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&self.uniforms(p, dt, time)));
        let (nx, ny, nz) = (self.res[0] + 1, self.res[1] + 1, self.res[2] + 1);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water.step") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("water"), timestamp_writes: None });
            pass.set_bind_group(0, &self.bind, &[]);
            let grid = |pass: &mut wgpu::ComputePass, pipe: &wgpu::ComputePipeline| {
                pass.set_pipeline(pipe);
                pass.dispatch_workgroups(gpu::groups(nx, 4), gpu::groups(ny, 4), gpu::groups(nz, 4));
            };
            grid(&mut pass, &self.pipes.clear_grid);
            if self.count > 0 {
                pass.set_pipeline(&self.pipes.p2g);
                pass.dispatch_workgroups(gpu::groups(self.count, 64), 1, 1);
            }
            grid(&mut pass, &self.pipes.normalize);
            grid(&mut pass, &self.pipes.divergence);
            for _ in 0..p.pressure_iters.max(1) {
                grid(&mut pass, &self.pipes.red);
                grid(&mut pass, &self.pipes.black);
            }
            grid(&mut pass, &self.pipes.project);
            if self.count > 0 {
                pass.set_pipeline(&self.pipes.g2p);
                pass.dispatch_workgroups(gpu::groups(self.count, 64), 1, 1);
            }
            if p.spray_enabled {
                pass.set_pipeline(&self.pipes.spray);
                pass.dispatch_workgroups(gpu::groups(self.max_spray, 64), 1, 1);
            }
        }
        queue.submit([enc.finish()]);
        self.frame = self.frame.wrapping_add(1);
    }
}
