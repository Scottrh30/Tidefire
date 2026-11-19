//! Open-ocean surface (procedural Gerstner waves) and the sky background.
//! No simulation state: everything is a function of the settings and time.

use crate::gpu::{self, wgpu};
use crate::params::OceanParams;
use bytemuck::{Pod, Zeroable};

pub const RINGS: u32 = 256;
pub const SEGMENTS: u32 = 512;
const WAVES: usize = 64;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct OceanU {
    wave_a: [[f32; 4]; WAVES],
    wave_b: [[f32; 4]; WAVES],
    chop: [f32; 4],
    chop2: [f32; 4],
    misc: [f32; 4],
    deep: [f32; 4],
    scatter: [f32; 4],
    foam: [f32; 4],
    grid: [u32; 4],
    grid_f: [f32; 4],
}

/// Wave set for the settings: one dominant swell, then shorter and shorter
/// waves spread around the wind direction (the spread widens as they shorten).
/// `pixel` = metres per pixel at 1 m distance (2·tan(fov/2) / image height).
pub fn uniforms(p: &OceanParams, time: f32, pixel: f32) -> OceanU {
    // fixed pseudo-random directions and phases (same scene = same sea)
    let mut seed: u32 = 0x9E37_79B9;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let wind = p.wind_direction.to_radians();
    let mut u = OceanU::zeroed();
    let longest = p.wavelength.max(0.2) * 1.4;
    // wavelengths fall geometrically from the main swell down to ~8 cm ripples
    let shrink = (0.08 / longest)
        .max(1e-4)
        .powf(1.0 / (WAVES as f32 - 1.0))
        .min(0.97);
    let mut wl = longest;
    // sum of all waves' steepness; above 1 the crests would fold over themselves
    let total_steep = p.choppiness.clamp(0.0, 1.0);
    for i in 0..WAVES {
        // short waves wander further from the wind direction than long swells
        let frac = i as f32 / WAVES as f32;
        let r = rnd();
        let spread = r * r.abs().sqrt() * (0.4 + 1.6 * frac) * p.spread.clamp(0.0, 2.0);
        let phase = (rnd() * 0.5 + 0.5) * std::f32::consts::TAU;
        let ang = wind + spread;
        let k = std::f32::consts::TAU / wl;
        // deep-water dispersion: w^2 = g k
        let omega = (9.81 * k).sqrt() * p.speed.max(0.0);
        // amplitudes fall off with wavelength, keeping each wave's slope similar
        // ocean-like spectrum: short waves are much gentler than the swell
        let amp = p.wave_height
            * 0.5
            * (wl / longest).powf(1.2)
            * (0.75 + 0.5 * (rnd() * 0.5 + 0.5))
            * 0.6;
        let q = total_steep / (k * amp.max(1e-4) * WAVES as f32);
        u.wave_a[i] = [ang.cos(), ang.sin(), k, amp];
        u.wave_b[i] = [omega, phase, q, wl];
        wl *= shrink;
    }
    // sharp wind chop layered over the swell, about half the main wavelength
    u.chop = [
        wind.cos(),
        wind.sin(),
        std::f32::consts::TAU / (p.wavelength.max(0.2) * 0.5),
        p.chop.max(0.0) * p.wave_height * 0.08,
    ];
    u.chop2 = [p.speed.max(0.0), 0.0, 0.0, 0.0];
    u.misc = [time, p.level, p.ripples, p.ripple_scale];
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
        p.wave_height.max(0.01),
    ];
    u.grid = [RINGS, SEGMENTS, 0, 0];
    // ring spacing grows ~2.6% per ring: 4 cm at the camera, ~1.3 km out
    u.grid_f = [0.04, 1.026, 1300.0, pixel];
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
