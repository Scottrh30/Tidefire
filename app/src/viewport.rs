//! The live viewport: a canvas the engine draws into through WebGPU. Every
//! animation frame it copies scene edits into the engine, steps the simulation
//! (while playing), renders, and presents.

use crate::state::{Stats, Ui};
use leptos::prelude::*;
use std::cell::RefCell;
use tidefire_core::engine::Engine;
use tidefire_core::render::{Presenter, RenderTarget, camera_math};
use wasm_bindgen::JsCast;

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pub engine: Engine,
    presenter: Presenter,
    target: Option<RenderTarget>,
    applied_rev: u64,
    applied_reset: u64,
    applied_step: u64,
    /// Scene revision last drawn; while paused nothing is redrawn until it changes.
    drawn_rev: u64,
    adapter: String,
}

thread_local! {
    /// The GPU engine. The browser runs everything on one thread, so it lives here
    /// rather than in a signal; export borrows it between frames.
    pub static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) };
}

/// Runs `f` on the engine, if the GPU has started.
pub fn with_engine<R>(f: impl FnOnce(&mut Engine) -> R) -> Option<R> {
    GPU.with_borrow_mut(|g| g.as_mut().map(|g| f(&mut g.engine)))
}

fn has_webgpu() -> bool {
    web_sys::window()
        .and_then(|w| js_sys::Reflect::get(&w.navigator(), &"gpu".into()).ok())
        .is_some_and(|g| !g.is_undefined() && !g.is_null())
}

// Natively (only for `cargo check`) the canvas surface can't exist.
#[cfg_attr(not(target_arch = "wasm32"), allow(unreachable_code, unused_variables))]
async fn init_gpu(canvas: web_sys::HtmlCanvasElement, ui: Ui) -> Result<Gpu, String> {
    if !has_webgpu() {
        return Err("This window has no WebGPU. Tidefire simulates on the GPU through WebGPU, which Windows 10 and 11 \
                    provide through Microsoft Edge WebView2: updating WebView2 (or your graphics driver) should fix this."
            .into());
    }
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::BROWSER_WEBGPU;
    let instance = wgpu::util::new_instance_with_webgpu_detection(desc).await;
    #[cfg(target_arch = "wasm32")]
    let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone())).map_err(|e| e.to_string())?;
    #[cfg(not(target_arch = "wasm32"))]
    let surface: wgpu::Surface<'static> = {
        let _ = (&canvas, &instance);
        return Err("the viewport only runs in the app's window".to_owned());
    };
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|e| format!("no WebGPU adapter: {e}"))?;
    let info = adapter.get_info();
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor { label: Some("tidefire"), required_limits: adapter.limits(), ..Default::default() })
        .await
        .map_err(|e| format!("could not open the GPU: {e}"))?;
    device.on_uncaptured_error(std::sync::Arc::new(|e| {
        web_sys::console::error_1(&format!("wgpu: {e}").into());
    }));
    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or_else(|| caps.formats.first().copied())
        .ok_or("the canvas has no formats")?;
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: canvas.width().max(8),
        height: canvas.height().max(8),
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: wgpu::CompositeAlphaMode::Opaque,
        view_formats: vec![],
        color_space: Default::default(),
    };
    surface.configure(&device, &config);
    let presenter = Presenter::new(&device, format);
    let engine = Engine::new(device, queue, ui.scene.get_untracked());
    let adapter = info.name.split(" (").next().unwrap_or(&info.name).chars().take(48).collect::<String>();
    Ok(Gpu {
        surface,
        config,
        engine,
        presenter,
        target: None,
        applied_rev: ui.rev.get_untracked(),
        applied_reset: ui.reset_rev.get_untracked(),
        applied_step: ui.step_rev.get_untracked(),
        drawn_rev: 0,
        adapter: if adapter.is_empty() { "WebGPU".into() } else { adapter },
    })
}

fn frame(ui: Ui, canvas: web_sys::HtmlCanvasElement) {
    let dpr = web_sys::window().map(|w| w.device_pixel_ratio()).unwrap_or(1.0);
    let pw = ((canvas.client_width() as f64) * dpr).round().max(8.0) as u32;
    let ph = ((canvas.client_height() as f64) * dpr).round().max(8.0) as u32;
    GPU.with_borrow_mut(|g| {
        let Some(g) = g.as_mut() else { return };
        let rev = ui.rev.get_untracked();
        if rev != g.applied_rev {
            g.engine.scene = ui.scene.get_untracked();
            g.applied_rev = rev;
        }
        let mut changed = rev != g.drawn_rev;
        let reset = ui.reset_rev.get_untracked();
        if reset != g.applied_reset {
            g.engine.reset();
            g.applied_reset = reset;
            changed = true;
        }
        let step = ui.step_rev.get_untracked();
        if !ui.export.running.get_untracked() && (ui.playing.get_untracked() || step != g.applied_step) {
            g.engine.step_frame();
            changed = true;
        }
        g.applied_step = step;
        let resized = g.config.width != pw || g.config.height != ph;
        // Paused and nothing changed: the canvas keeps showing the last frame.
        if !changed && !resized && !ui.export.running.get_untracked() {
            return;
        }
        g.drawn_rev = rev;

        if resized {
            canvas.set_width(pw);
            canvas.set_height(ph);
            g.config.width = pw;
            g.config.height = ph;
            g.surface.configure(&g.engine.device, &g.config);
        }
        if g.target.as_ref().is_none_or(|t| t.width != pw || t.height != ph) {
            g.target = Some(RenderTarget::new(&g.engine.device, pw, ph));
        }
        let target = g.target.as_ref().unwrap();
        g.engine.render(target, false);
        let out = match g.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                g.surface.configure(&g.engine.device, &g.config);
                return;
            }
        };
        let view = out.texture.create_view(&Default::default());
        let checker = g.engine.scene.render.transparent_background;
        g.presenter.present(&g.engine.device, &g.engine.queue, target, &view, checker);
        g.engine.queue.present(out);

        let stats = Stats {
            frame: g.engine.frame,
            time: g.engine.time,
            step_ms: g.engine.last_step_ms,
            particles: g.engine.particle_count(),
            adapter: g.adapter.clone(),
        };
        if ui.stats.with_untracked(|s| s != &stats) {
            ui.stats.set(stats);
        }
    });
    request_animation_frame(move || frame(ui, canvas));
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Orbit,
    Pan,
}

#[component]
pub fn Viewport(ui: Ui) -> impl IntoView {
    let canvas_ref = NodeRef::<leptos::html::Canvas>::new();
    let drag = StoredValue::new(None::<(Drag, f64, f64)>);
    let started = StoredValue::new(false);

    Effect::new(move |_| {
        let Some(canvas) = canvas_ref.get() else { return };
        if started.get_value() {
            return;
        }
        started.set_value(true);
        let canvas: web_sys::HtmlCanvasElement = canvas.unchecked_into();
        leptos::task::spawn_local(async move {
            match init_gpu(canvas.clone(), ui).await {
                Ok(g) => {
                    GPU.with_borrow_mut(|slot| *slot = Some(g));
                    request_animation_frame(move || frame(ui, canvas));
                }
                Err(e) => {
                    web_sys::console::error_1(&e.clone().into());
                    ui.gpu_error.set(Some(e));
                }
            }
        });
    });

    let on_down = move |ev: web_sys::PointerEvent| {
        let kind = match ev.button() {
            0 => Drag::Orbit,
            1 | 2 => Drag::Pan,
            _ => return,
        };
        if let Some(t) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
            let _ = t.set_pointer_capture(ev.pointer_id());
        }
        ev.prevent_default();
        drag.set_value(Some((kind, ev.client_x() as f64, ev.client_y() as f64)));
    };
    let on_move = move |ev: web_sys::PointerEvent| {
        let Some((kind, x0, y0)) = drag.get_value() else { return };
        let (x, y) = (ev.client_x() as f64, ev.client_y() as f64);
        let (dx, dy) = ((x - x0) as f32, (y - y0) as f32);
        drag.set_value(Some((kind, x, y)));
        ui.edit(|s| {
            let c = &mut s.camera;
            match kind {
                Drag::Orbit => {
                    c.yaw -= dx * 0.008;
                    c.pitch = (c.pitch + dy * 0.008).clamp(-1.45, 1.45);
                }
                Drag::Pan => {
                    let cm = camera_math(c, 1.6);
                    let k = c.distance * 0.0015;
                    let t = glam::Vec3::from(c.target) - cm.right * dx * k + cm.up * dy * k;
                    c.target = t.into();
                }
            }
        });
    };
    let on_up = move |_: web_sys::PointerEvent| drag.set_value(None);
    let on_wheel = move |ev: web_sys::WheelEvent| {
        ev.prevent_default();
        let d = ev.delta_y() as f32;
        ui.edit(|s| s.camera.distance = (s.camera.distance * (1.0 + d * 0.0012)).clamp(0.2, 50.0));
    };

    view! {
        <div class="viewport">
            <canvas
                node_ref=canvas_ref
                on:pointerdown=on_down
                on:pointermove=on_move
                on:pointerup=on_up
                on:pointercancel=on_up
                on:wheel=on_wheel
                on:contextmenu=|ev: web_sys::MouseEvent| ev.prevent_default()
            ></canvas>
            <div class="vp-hud">
                {move || ui.stats.with(|s| s.adapter.clone())}
            </div>
            <Show when=move || ui.gpu_error.with(|e| e.is_some())>
                <div class="vp-error">
                    <b>"The viewport could not start"</b>
                    <p>{move || ui.gpu_error.get().unwrap_or_default()}</p>
                </div>
            </Show>
        </div>
    }
}
