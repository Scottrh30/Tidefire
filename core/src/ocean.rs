//! Open-ocean surface grid and shading settings, and the sky background.
//! The waves are simulated by the FFT ocean in `ocean_fft`.

use crate::gpu::{self, wgpu};
use crate::params::OceanParams;
use bytemuck::{Pod, Zeroable};

pub const RINGS: u32 = 256;
pub const SEGMENTS: u32 = 512;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct OceanU {
    cascade: [f32; 4],
    misc: [f32; 4],
    deep: [f32; 4],
    scatter: [f32; 4],
    foam: [f32; 4],
    grid: [u32; 4],
    grid_f: [f32; 4],
    light: [f32; 4],
}

/// Shading and grid settings; the waves themselves come from the FFT
/// cascades (see `ocean_fft`). `pixel` = metres per pixel at 1 m distance.
pub fn uniforms(p: &OceanParams, time: f32, pixel: f32) -> OceanU {
    let l = crate::ocean_fft::cascade_lengths(p);
    let mut u = OceanU::zeroed();
    u.cascade = [l[0], l[1], l[2], crate::ocean_fft::lambda(p)];
    u.misc = [time, p.level, crate::ocean_fft::foam_threshold(p), 0.0];
    u.deep = [
        p.deep_color[0],
        p.deep_color[1],
        p.deep_color[2],
        p.whitecaps.clamp(0.0, 1.0),
    ];
    u.scatter = [
        p.scatter_color[0],
        p.scatter_color[1],
        p.scatter_color[2],
        p.haze_distance,
    ];
    u.foam = [
        p.foam_color[0],
        p.foam_color[1],
        p.foam_color[2],
        crate::ocean_fft::effective_height(p).max(0.01),
    ];
    // grid.z = 1 multiplies the shader's loop counts: a uniform bound stops shader
    // compilers fully unrolling the nested light-path loops (which made the software
    // WebGPU used in CI take minutes to build the pipeline)
    u.grid = [RINGS, SEGMENTS, 1, 0];
    // ring spacing grows ~2.6% per ring: 4 cm at the camera, ~1.3 km out
    u.grid_f = [0.04, 1.026, 1300.0, pixel];
    // clarity 1 = clear ocean water; lower = murkier (more absorption)
    u.light = [
        p.scattering.max(0.0),
        1.0 / p.clarity.max(0.05),
        p.shadows.clamp(0.0, 1.0),
        p.sun_intensity.max(0.0),
    ];
    u
}

pub fn index_buffer(device: &wgpu::Device) -> (wgpu::Buffer, u32) {
    use wgpu::util::DeviceExt;
    let mut idx: Vec<u32> = Vec::with_capacity(((RINGS - 1) * SEGMENTS * 6) as usize);
    for i in 0..RINGS - 1 {
        for j in 0..SEGMENTS {
            let a = i * SEGMENTS + j;
            let b = i * SEGMENTS + (j + 1) % SEGMENTS;
            let c = (i + 1) * SEGMENTS + j;
            let d = (i + 1) * SEGMENTS + (j + 1) % SEGMENTS;
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ocean.indices"),
        contents: bytemuck::cast_slice(&idx),
        usage: wgpu::BufferUsages::INDEX,
    });
    (buf, idx.len() as u32)
}

pub fn uniform_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    gpu::uniform_buffer(device, "ocean.params", &OceanU::zeroed())
}
