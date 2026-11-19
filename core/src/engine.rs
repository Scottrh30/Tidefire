//! Owns the simulators + renderer and drives them frame by frame.

use crate::gpu::wgpu;
use crate::params::Scene;
use crate::render::{RenderTarget, Renderer};
use crate::smoke::SmokeSim;
use crate::water::WaterSim;
use web_time::Instant;

pub struct Engine {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub scene: Scene,
    pub smoke: Option<SmokeSim>,
    pub water: Option<WaterSim>,
    pub renderer: Renderer,
    pub frame: u32,
    pub time: f32,
    pub last_step_ms: f32,
    built_smoke: Option<([u32; 3],)>,
    built_water: Option<([u32; 3], u32, u32, bool)>,
}

impl Engine {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, scene: Scene) -> Self {
        let renderer = Renderer::new(&device, &queue);
        let mut e = Self {
            device,
            queue,
            scene,
            smoke: None,
            water: None,
            renderer,
            frame: 0,
            time: 0.0,
            last_step_ms: 0.0,
            built_smoke: None,
            built_water: None,
        };
        e.rebuild_if_needed();
        e.reset();
        e
    }

    /// Recreate GPU resources when resolution / capacities change. Returns true if rebuilt.
    pub fn rebuild_if_needed(&mut self) -> bool {
        let mut rebuilt = false;
        let s = &self.scene.smoke;
        let want_s = s.enabled.then_some((s.res,));
        if want_s != self.built_smoke {
            self.smoke = want_s.map(|_| SmokeSim::new(&self.device, s));
            self.built_smoke = want_s;
            rebuilt = true;
        }
        let w = &self.scene.water;
        let want_w = w.enabled.then_some((w.res, w.max_particles, w.max_spray, w.spray_enabled));
        if want_w != self.built_water {
            self.water = want_w.map(|_| WaterSim::new(&self.device, w));
            self.built_water = want_w;
            rebuilt = true;
        }
        rebuilt
    }

    pub fn reset(&mut self) {
        self.rebuild_if_needed();
        self.frame = 0;
        self.time = 0.0;
        if let Some(s) = &mut self.smoke {
            s.reset(&self.device, &self.queue, &self.scene.smoke);
        }
        if let Some(w) = &mut self.water {
            w.reset(&self.device, &self.queue, &self.scene.water);
        }
    }

    /// Advance one output frame (1/fps seconds) using the configured substeps.
    pub fn step_frame(&mut self) {
        if self.rebuild_if_needed() {
            self.reset();
        }
        let t0 = Instant::now();
        let subs = self.scene.substeps.clamp(1, 16);
        let dt = 1.0 / self.scene.fps.clamp(1.0, 240.0) / subs as f32;
        for _ in 0..subs {
            if let Some(s) = &mut self.smoke {
                let mut sp = self.scene.smoke.clone();
                if sp.emitter_duration > 0.0 && self.time > sp.emitter_duration {
                    sp.emitter_on = false;
                }
                s.step(&self.device, &self.queue, &sp, dt, self.time);
            }
            if let Some(w) = &mut self.water {
                w.step(&self.device, &self.queue, &self.scene.water, dt, self.time);
            }
            self.time += dt;
        }
        self.frame += 1;
        self.last_step_ms = t0.elapsed().as_secs_f32() * 1000.0;
    }

    pub fn render(&mut self, target: &RenderTarget, hide_helpers: bool) {
        self.renderer.render(&self.device, &self.queue, target, &self.scene, self.smoke.as_ref(), self.water.as_ref(), hide_helpers);
    }

    pub fn particle_count(&self) -> u32 {
        self.water.as_ref().map(|w| w.count).unwrap_or(0)
    }

}

struct Readback {
    buf: wgpu::Buffer,
    bpr: u32,
    width: u32,
    height: u32,
}

fn start_readback(device: &wgpu::Device, queue: &wgpu::Queue, t: &RenderTarget) -> Readback {
    let bpr = (t.width * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bpr * t.height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &t.color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(t.height) },
        },
        wgpu::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    Readback { buf, bpr, width: t.width, height: t.height }
}

fn finish_readback(r: Readback) -> image::RgbaImage {
    let data = r.buf.get_mapped_range(..).expect("map readback");
    let mut img = image::RgbaImage::new(r.width, r.height);
    for y in 0..r.height {
        let row = &data[(y * r.bpr) as usize..(y * r.bpr + r.width * 4) as usize];
        for x in 0..r.width {
            let p = &row[(x * 4) as usize..(x * 4 + 4) as usize];
            let a = p[3];
            // stored colour is premultiplied; undo it for PNG (straight alpha)
            let un = |c: u8| -> u8 {
                if a == 0 || a == 255 { c } else { ((c as f32 * 255.0 / a as f32).round()).min(255.0) as u8 }
            };
            img.put_pixel(x, y, image::Rgba([un(p[0]), un(p[1]), un(p[2]), a]));
        }
    }
    drop(data);
    r.buf.unmap();
    img
}

/// Copy a render target back to the CPU as straight-alpha RGBA8 (sRGB), blocking
/// until the GPU is done. Native only (a browser can't block on the GPU).
#[cfg(not(target_arch = "wasm32"))]
pub fn read_target(device: &wgpu::Device, queue: &wgpu::Queue, t: &RenderTarget) -> image::RgbaImage {
    let r = start_readback(device, queue, t);
    r.buf.map_async(wgpu::MapMode::Read, .., |_| {});
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    finish_readback(r)
}

/// Same as `read_target`, without blocking: works in the browser (WebGPU) too.
pub async fn read_target_async(device: &wgpu::Device, queue: &wgpu::Queue, t: &RenderTarget) -> Result<image::RgbaImage, String> {
    let r = start_readback(device, queue, t);
    let (tx, rx) = futures_channel::oneshot::channel();
    r.buf.map_async(wgpu::MapMode::Read, .., move |res| {
        let _ = tx.send(res);
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    match rx.await {
        Ok(Ok(())) => Ok(finish_readback(r)),
        Ok(Err(e)) => Err(format!("could not read the frame back from the GPU: {e}")),
        Err(_) => Err("the GPU readback was cancelled".into()),
    }
}

/// `<prefix>_sequence.json`, written next to an exported PNG sequence so
/// Reelforge (or anything else) knows its frame rate, size and file pattern.
pub fn sequence_manifest(scene: &crate::params::Scene) -> String {
    let e = &scene.export;
    let m = serde_json::json!({
        "generator": "Tidefire",
        "scene": scene.name,
        "fps": scene.fps,
        "frames": e.frames,
        "start_frame": e.start_frame,
        "width": e.width,
        "height": e.height,
        "alpha": scene.render.transparent_background,
        "pattern": format!("{}_%05d.png", e.prefix),
    });
    serde_json::to_string_pretty(&m).unwrap()
}

pub fn manifest_name(prefix: &str) -> String {
    format!("{prefix}_sequence.json")
}

/// File name of one exported frame.
pub fn frame_name(prefix: &str, frame: u32) -> String {
    format!("{prefix}_{frame:05}.png")
}
