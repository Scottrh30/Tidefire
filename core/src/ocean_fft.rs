//! FFT ocean (Tessendorf). A JONSWAP wind-sea spectrum with directional
//! spreading is sampled once on the CPU (whenever the wave settings change) for
//! three cascades: square tiles of different sizes that each carry one band of
//! wavelengths. Every frame the GPU evolves the spectrum in time, inverse-FFTs
//! it and writes displacement, slope/Jacobian and foam textures with full mip
//! chains, which the ocean shader samples.

use crate::gpu::{self, wgpu};
use crate::params::OceanParams;
use bytemuck::{Pod, Zeroable};

/// FFT size per cascade (must match `N` in ocean_fft.wgsl).
pub const N: u32 = 256;
pub const CASCADES: u32 = 3;
const MIPS: u32 = 9; // 256 .. 1
const FIELDS: u32 = 4;
const G: f32 = 9.81;
const TAU: f32 = std::f32::consts::TAU;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FftU {
    lengths: [f32; 4],
    misc: [f32; 4],
    misc2: [f32; 4],
}

/// Tile sizes of the three cascades for a peak wavelength. The ratios are
/// deliberately not whole numbers, so the tiles never line up and repeat.
pub fn cascade_lengths(p: &OceanParams) -> [f32; 3] {
    let l0 = (p.wavelength.max(0.5) * 10.0).max(30.0);
    let l1 = l0 / 5.13;
    let l2 = l1 / 5.71;
    [l0, l1, l2]
}

/// The wave height actually used: real wind seas are never steeper than about
/// height = wavelength / 7, beyond which they break apart. Bigger seas need longer waves.
pub fn effective_height(p: &OceanParams) -> f32 {
    p.wave_height.max(0.0).min(p.wavelength.max(0.5) / 7.0)
}

/// Jacobian below which the surface counts as breaking and makes foam
/// (1 = undisturbed, 0 = folding over). 0 whitecaps = no foam.
pub fn foam_threshold(p: &OceanParams) -> f32 {
    let wc = p.whitecaps.clamp(0.0, 1.0);
    if wc <= 0.0 { -1.0 } else { 0.15 + wc * 0.7 }
}

/// Choppiness slider (0..1) to the horizontal displacement factor.
pub fn lambda(p: &OceanParams) -> f32 {
    p.choppiness.clamp(0.0, 1.0) * 0.75
}

fn spectrum_key(p: &OceanParams) -> [u32; 6] {
    [
        p.wave_height.to_bits(),
        p.wavelength.to_bits(),
        p.wind_direction.to_bits(),
        p.spread.to_bits(),
        p.chop.to_bits(),
        p.ripples.to_bits(),
    ]
}

/// Deterministic standard normal pair for one spectrum sample (same scene = same sea,
/// and changing a slider reshapes the waves without reshuffling them).
fn gauss(c: u32, x: u32, z: u32) -> (f32, f32) {
    let mut h =
        c.wrapping_mul(0x9E37_79B1) ^ x.wrapping_mul(0x85EB_CA6B) ^ z.wrapping_mul(0xC2B2_AE35);
    let mut next = || {
        h ^= h >> 16;
        h = h.wrapping_mul(0x7FEB_352D);
        h ^= h >> 15;
        h = h.wrapping_mul(0x846C_A68B);
        h ^= h >> 16;
        (h as f32 + 0.5) / 4_294_967_296.0
    };
    let u1 = next().max(1e-7);
    let u2 = next();
    let r = (-2.0 * u1.ln()).sqrt();
    (r * (TAU * u2).cos(), r * (TAU * u2).sin())
}

/// h0(k) and conj(h0(-k)) for every cascade, as vec4s (`cascade, z, x` order).
pub fn initial_spectrum(p: &OceanParams) -> Vec<[f32; 4]> {
    let lengths = cascade_lengths(p);
    let n = N as usize;
    // wavenumber where one cascade hands over to the next: waves at least
    // a sixth of the next tile long stay in the bigger tile
    let cut = [TAU / lengths[1] * 6.0, TAU / lengths[2] * 6.0];
    let wp = (G * TAU / p.wavelength.max(0.5)).sqrt(); // peak angular frequency
    let kp = wp * wp / G;
    let wind = p.wind_direction.to_radians();
    // directional spreading cos^(2s)(dtheta/2): narrow at the peak, wider for
    // shorter and longer waves (Mitsuyasu)
    let sp = 2.0 + 20.0 / (0.2 + p.spread.clamp(0.0, 2.0) * 2.0);
    let gamma: f32 = 3.3;

    // variance density per sample (before scaling), and its total
    let mut amp2 = vec![0.0f32; CASCADES as usize * n * n];
    let mut total = 0.0f64;
    for c in 0..CASCADES as usize {
        let dk = TAU / lengths[c];
        let lo = if c == 0 { 0.0 } else { cut[c - 1] };
        let hi = if c + 1 < CASCADES as usize {
            cut[c]
        } else {
            f32::INFINITY
        };
        for z in 0..n {
            for x in 0..n {
                let kx = (x as f32 - (n / 2) as f32) * dk;
                let kz = (z as f32 - (n / 2) as f32) * dk;
                let k = (kx * kx + kz * kz).sqrt();
                if k < 1e-6 || k < lo || k >= hi {
                    continue;
                }
                let w = (G * k).sqrt();
                let sigma = if w <= wp { 0.07 } else { 0.09 };
                let r = (-((w - wp) * (w - wp)) / (2.0 * sigma * sigma * wp * wp)).exp();
                let s_w = G * G / w.powi(5) * (-1.25 * (wp / w).powi(4)).exp() * gamma.powf(r);
                let s_k = s_w * G / (2.0 * w); // dw/dk
                let ratio = w / wp;
                let s = if ratio < 1.0 {
                    sp * ratio.powi(5)
                } else {
                    sp * ratio.powf(-2.5)
                }
                .max(1.0);
                let th = kz.atan2(kx) - wind;
                let norm = (s / (4.0 * std::f32::consts::PI))
                    .sqrt()
                    .max(1.0 / std::f32::consts::PI);
                let d = norm * (th * 0.5).cos().abs().powf(2.0 * s);
                // surface tension smooths out waves shorter than a few centimetres
                let capillary = (-(k / 70.0).powi(2)).exp();
                // and the short-wave tail falls faster than JONSWAP's: when the sea is
                // scaled to a chosen height its tail would otherwise be far too steep
                let tail = (k / (4.0 * kp)).max(1.0).powf(-0.75);
                let e = s_k * d / k * dk * dk * capillary * tail; // variance carried by this sample
                amp2[(c * n + z) * n + x] = e;
                total += e as f64;
            }
        }
    }
    // significant wave height = 4 standard deviations of the surface height
    let target = (effective_height(p) / 4.0).powi(2) as f64;
    let scale = if total > 0.0 {
        (target / total) as f32
    } else {
        0.0
    };

    // ripples add about 0.15 * ripples to the RMS slope (the band integrates
    // to ~4 B of slope variance); in real seas much of the slope is in them
    let ripple_b = (0.15 * p.ripples.max(0.0)).powi(2) / 4.0;
    let mut h0 = vec![[0.0f32; 2]; CASCADES as usize * n * n];
    for c in 0..CASCADES as usize {
        let dk = TAU / lengths[c];
        for z in 0..n {
            for x in 0..n {
                let i = (c * n + z) * n + x;
                let kx = (x as f32 - (n / 2) as f32) * dk;
                let kz = (z as f32 - (n / 2) as f32) * dk;
                let k = (kx * kx + kz * kz).sqrt();
                // extra short-wave energy: crisper chop without making the swell bigger
                let t = ((k / kp - 3.0) / 9.0).clamp(0.0, 1.0);
                let boost = 1.0 + p.chop.max(0.0) * 4.0 * t * t * (3.0 - 2.0 * t);
                // capillary-gravity ripples (2-40 cm): a band with constant slope
                // energy per octave, loosely aligned with the wind. They give the
                // surface its fine, crisp wrinkles and add almost nothing to the height.
                let rip = if k > 1e-6 && k >= lo_k(c, &cut) && k < hi_k(c, &cut) {
                    let band = smooth(12.0, 30.0, k) * (1.0 - smooth(220.0, 320.0, k));
                    let th = kz.atan2(kx) - wind;
                    // mostly along and against the wind: long ripple crests across it
                    let d = 0.45 * (0.15 + 0.85 * th.cos().abs().powi(6) * 3.2);
                    ripple_b * band * d / (k * k * k * k) * dk * dk
                } else {
                    0.0
                };
                let a = ((amp2[i] * scale * boost + rip) * 0.5).sqrt();
                let (g1, g2) = gauss(c as u32, x as u32, z as u32);
                h0[i] = [
                    a * g1 * std::f32::consts::FRAC_1_SQRT_2,
                    a * g2 * std::f32::consts::FRAC_1_SQRT_2,
                ];
            }
        }
    }
    let mut out = vec![[0.0f32; 4]; h0.len()];
    for c in 0..CASCADES as usize {
        for z in 0..n {
            for x in 0..n {
                let i = (c * n + z) * n + x;
                let mx = (n - x) % n;
                let mz = (n - z) % n;
                let m = h0[(c * n + mz) * n + mx];
                out[i] = [h0[i][0], h0[i][1], m[0], -m[1]];
            }
        }
    }
    out
}

/// RMS slope of the surface (realistic seas: about 0.1 to 0.25).
pub fn spectrum_slope_rms(p: &OceanParams, h0: &[[f32; 4]]) -> f32 {
    let l = cascade_lengths(p);
    let n = N as usize;
    let mut var = 0.0f64;
    for (i, v) in h0.iter().enumerate() {
        let c = i / (n * n);
        let z = (i / n) % n;
        let x = i % n;
        let dk = TAU / l[c];
        let kx = (x as f32 - (n / 2) as f32) * dk;
        let kz = (z as f32 - (n / 2) as f32) * dk;
        let k2 = (kx * kx + kz * kz) as f64;
        var += 2.0 * k2 * (v[0] as f64 * v[0] as f64 + v[1] as f64 * v[1] as f64);
    }
    var.sqrt() as f32
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lo_k(c: usize, cut: &[f32; 2]) -> f32 {
    if c == 0 { 0.0 } else { cut[c - 1] }
}

fn hi_k(c: usize, cut: &[f32; 2]) -> f32 {
    if c + 1 < CASCADES as usize {
        cut[c]
    } else {
        f32::INFINITY
    }
}

/// Significant wave height actually present in a spectrum (for tests).
pub fn spectrum_hs(h0: &[[f32; 4]]) -> f32 {
    let var: f64 = h0
        .iter()
        .map(|v| 2.0 * (v[0] as f64 * v[0] as f64 + v[1] as f64 * v[1] as f64))
        .sum();
    4.0 * var.sqrt() as f32
}

struct Tex {
    tex: wgpu::Texture,
    /// all mips, for sampling
    view: wgpu::TextureView,
}

pub struct OceanFft {
    uniform: wgpu::Buffer,
    h0: wgpu::Buffer,
    foam_state: wgpu::Buffer,
    main_bind: wgpu::BindGroup,
    mip_binds: Vec<wgpu::BindGroup>,
    evolve: wgpu::ComputePipeline,
    rows: wgpu::ComputePipeline,
    cols: wgpu::ComputePipeline,
    assemble: wgpu::ComputePipeline,
    downsample: wgpu::ComputePipeline,
    disp: Tex,
    deriv: Tex,
    foam: Tex,
    pub sampler: wgpu::Sampler,
    key: Option<[u32; 6]>,
    last_time: Option<f32>,
}

impl OceanFft {
    pub fn new(device: &wgpu::Device) -> Self {
        let c = wgpu::ShaderStages::COMPUTE;
        let n2 = (N * N) as u64;
        let uniform = gpu::uniform_buffer(device, "ocean.fft.u", &FftU::zeroed());
        let h0 = gpu::buffer(device, "ocean.h0", n2 * CASCADES as u64 * 16, gpu::STORAGE);
        let spec = gpu::buffer(
            device,
            "ocean.spectrum",
            n2 * (CASCADES * FIELDS) as u64 * 8,
            gpu::STORAGE,
        );
        let foam_state = gpu::buffer(device, "ocean.foam", n2 * CASCADES as u64 * 4, gpu::STORAGE);
        let make_tex = |label: &str| {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: N,
                    height: N,
                    depth_or_array_layers: CASCADES,
                },
                mip_level_count: MIPS,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
            Tex { tex, view }
        };
        let disp = make_tex("ocean.disp");
        let deriv = make_tex("ocean.deriv");
        let foam = make_tex("ocean.foam");
        let level = |t: &Tex, l: u32| {
            t.tex.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                base_mip_level: l,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let storage_tex = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: c,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::Rgba16Float,
                view_dimension: wgpu::TextureViewDimension::D2Array,
            },
            count: None,
        };
        let main_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ocean.fft.layout"),
            entries: &[
                gpu::uniform(0, c),
                gpu::storage(1, true, c),
                gpu::storage(2, false, c),
                gpu::storage(3, false, c),
                storage_tex(4),
                storage_tex(5),
                storage_tex(6),
            ],
        });
        let (d0, v0, f0) = (level(&disp, 0), level(&deriv, 0), level(&foam, 0));
        let main_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ocean.fft.bind"),
            layout: &main_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: h0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: spec.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: foam_state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&d0),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&v0),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&f0),
                },
            ],
        });
        let module = gpu::shader(
            device,
            "ocean_fft.wgsl",
            include_str!("shaders/ocean_fft.wgsl"),
        );
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&main_layout)],
            immediate_size: 0,
        });
        let mip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ocean.mips.layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: c,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                storage_tex(1),
            ],
        });
        let mip_module = gpu::shader(
            device,
            "ocean_mips.wgsl",
            include_str!("shaders/ocean_mips.wgsl"),
        );
        let mip_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&mip_layout)],
            immediate_size: 0,
        });
        let mut mip_binds = Vec::new();
        for t in [&disp, &deriv, &foam] {
            for l in 1..MIPS {
                let src = level(t, l - 1);
                let dst = level(t, l);
                mip_binds.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &mip_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&src),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&dst),
                        },
                    ],
                }));
            }
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ocean.sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        Self {
            uniform,
            h0,
            foam_state,
            main_bind,
            mip_binds,
            evolve: gpu::compute(device, &pl, &module, "evolve"),
            rows: gpu::compute(device, &pl, &module, "fft_rows"),
            cols: gpu::compute(device, &pl, &module, "fft_cols"),
            assemble: gpu::compute(device, &pl, &module, "assemble"),
            downsample: gpu::compute(device, &mip_pl, &mip_module, "downsample"),
            disp,
            deriv,
            foam,
            sampler,
            key: None,
            last_time: None,
        }
    }

    pub fn disp_view(&self) -> &wgpu::TextureView {
        &self.disp.view
    }
    pub fn deriv_view(&self) -> &wgpu::TextureView {
        &self.deriv.view
    }
    pub fn foam_view(&self) -> &wgpu::TextureView {
        &self.foam.view
    }

    /// Brings the textures to time `time`. Foam builds up only as time moves
    /// forward, so redrawing a paused frame leaves it as it is.
    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        p: &OceanParams,
        time: f32,
    ) {
        let key = spectrum_key(p);
        if self.key != Some(key) {
            queue.write_buffer(&self.h0, 0, bytemuck::cast_slice(&initial_spectrum(p)));
            self.key = Some(key);
        }
        if self.last_time == Some(time) {
            return;
        }
        let dt = match self.last_time {
            Some(t0) if time > t0 => (time - t0).min(0.25),
            // first frame, or time went back (reset / scrub): start without foam
            _ => {
                queue.write_buffer(
                    &self.foam_state,
                    0,
                    &vec![0u8; (N * N * CASCADES * 4) as usize],
                );
                0.0
            }
        };
        self.last_time = Some(time);
        let lengths = cascade_lengths(p);
        let u = FftU {
            lengths: [lengths[0], lengths[1], lengths[2], 0.0],
            // foam fades with a ~1.5 s half-life; it forms where the surface is
            // squeezed hard (Jacobian below the threshold)
            misc: [time, lambda(p), (-dt * 0.46).exp(), foam_threshold(p)],
            misc2: [p.speed.max(0.0), 2.5, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));

        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("ocean.fft"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &self.main_bind, &[]);
        pass.set_pipeline(&self.evolve);
        pass.dispatch_workgroups(N / 8, N / 8, CASCADES);
        pass.set_pipeline(&self.rows);
        pass.dispatch_workgroups(N, FIELDS, CASCADES);
        pass.set_pipeline(&self.cols);
        pass.dispatch_workgroups(N, FIELDS, CASCADES);
        pass.set_pipeline(&self.assemble);
        pass.dispatch_workgroups(N / 8, N / 8, CASCADES);
        pass.set_pipeline(&self.downsample);
        for (i, b) in self.mip_binds.iter().enumerate() {
            let l = (i as u32 % (MIPS - 1)) + 1;
            let size = (N >> l).max(1);
            pass.set_bind_group(0, b, &[]);
            pass.dispatch_workgroups(gpu::groups(size, 8), gpu::groups(size, 8), CASCADES);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectrum_matches_requested_wave_height() {
        let mut p = OceanParams::default();
        p.chop = 0.0;
        p.ripples = 0.0;
        p.wavelength = 60.0; // long enough that no height here hits the steepness cap
        for hs in [0.5f32, 2.0, 6.0] {
            p.wave_height = hs;
            let got = spectrum_hs(&initial_spectrum(&p));
            assert!((got - hs).abs() / hs < 0.15, "asked {hs}, got {got}");
        }
    }

    #[test]
    fn report_slopes() {
        let mut p = OceanParams::default();
        for (hs, wl, chop) in [
            (0.55, 3.0, 0.25),
            (1.2, 10.0, 0.25),
            (1.2, 10.0, 0.0),
            (0.8, 12.0, 0.25),
            (1.1, 9.0, 0.6),
            (1.0, 9.0, 0.2),
            (1.0, 10.0, 0.0),
            (1.4, 11.0, 0.1),
            (1.0, 16.0, 0.25),
            (3.0, 40.0, 0.25),
        ] {
            p.wave_height = hs;
            p.wavelength = wl;
            p.chop = chop;
            let h = initial_spectrum(&p);
            println!(
                "hs {hs} wl {wl} chop {chop}: Hs {:.2} slope {:.3}",
                spectrum_hs(&h),
                spectrum_slope_rms(&p, &h)
            );
        }
    }

    #[test]
    fn height_is_capped_by_steepness() {
        let mut p = OceanParams::default();
        p.wave_height = 5.0;
        p.wavelength = 14.0;
        assert_eq!(effective_height(&p), 2.0);
    }

    #[test]
    fn spectrum_is_hermitian_paired() {
        let p = OceanParams::default();
        let h = initial_spectrum(&p);
        let n = N as usize;
        // conj(h0(-k)) stored at k equals conj of h0 stored at -k
        let (x, z) = (37usize, 201usize);
        let a = h[z * n + x];
        let b = h[((n - z) % n) * n + (n - x) % n];
        assert_eq!(a[2], b[0]);
        assert_eq!(a[3], -b[1]);
    }
}
