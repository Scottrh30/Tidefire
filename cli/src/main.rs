//! Tidefire's command line tool (the app itself is the Tauri window).
//!
//! Headless render:  tidefire-cli --headless [--preset NAME | --scene FILE] [--frames N] [--size WxH] [--out DIR]
//!                                [--transfer apic|flip] [--transparent] [--stats]
//! Visual tests:     tidefire-cli --visual-test tests/visual [--update]

mod visual_test;

use std::path::PathBuf;
use tidefire_core::{engine, params, render, wgpu};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(dir) = arg(&args, "--visual-test") {
        let update = args.iter().any(|a| a == "--update");
        let result = headless_device().and_then(|(d, q)| visual_test::run(&d, &q, std::path::Path::new(dir), update));
        match result {
            Ok(true) => eprintln!("visual tests passed"),
            Ok(false) => {
                eprintln!("visual tests FAILED (renders and diffs in {dir}/out)");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("tidefire: {e}");
                std::process::exit(2);
            }
        }
        return;
    }
    if let Some(name) = arg(&args, "--dump-preset") {
        // Prints a preset as a .tidefire scene, to start a custom scene from.
        match params::preset(name) {
            Some(s) => println!("{}", serde_json::to_string_pretty(&s).unwrap()),
            None => {
                eprintln!("tidefire: unknown preset \"{name}\" (try: {})", params::PRESETS.join(", "));
                std::process::exit(1);
            }
        }
        return;
    }
    if args.iter().any(|a| a == "--headless") {
        if let Err(e) = headless(&args) {
            eprintln!("tidefire: {e}");
            std::process::exit(1);
        }
        return;
    }
    println!(
        "Tidefire {} command line tool\n\nUsage:\n  tidefire-cli --headless [--preset NAME | --scene FILE] [--frames N] [--size WxH] [--out DIR] [--transfer apic|flip] [--transparent] [--stats]\n  tidefire-cli --visual-test DIR [--update]\n  tidefire-cli --dump-preset NAME > scene.tidefire\n\nPresets: {}",
        env!("CARGO_PKG_VERSION"),
        params::PRESETS.join(", ")
    );
}

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(|s| s.as_str())
}

fn headless(args: &[String]) -> Result<(), String> {
    let mut scene = if let Some(path) = arg(args, "--scene") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?
    } else {
        let name = arg(args, "--preset").unwrap_or("Campfire");
        params::preset(name).ok_or_else(|| format!("unknown preset \"{name}\" (try: {})", params::PRESETS.join(", ")))?
    };
    if let Some(n) = arg(args, "--frames") {
        scene.export.frames = n.parse().map_err(|_| "bad --frames")?;
    }
    if let Some(sz) = arg(args, "--size") {
        let (w, h) = sz.split_once('x').ok_or("--size expects WxH")?;
        scene.export.width = w.parse().map_err(|_| "bad width")?;
        scene.export.height = h.parse().map_err(|_| "bad height")?;
    }
    if let Some(t) = arg(args, "--transfer") {
        scene.water.transfer = match t {
            "apic" | "APIC" => params::Transfer::Apic,
            "flip" | "FLIP" => params::Transfer::Flip,
            _ => return Err("--transfer expects apic or flip".into()),
        };
    }
    if args.iter().any(|a| a == "--transparent") {
        scene.render.transparent_background = true;
    }
    let out = PathBuf::from(arg(args, "--out").unwrap_or("tidefire_out"));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let stats = args.iter().any(|a| a == "--stats");

    let (device, queue) = headless_device()?;

    let frames = scene.export.frames;
    let (w, h) = (scene.export.width, scene.export.height);
    std::fs::write(out.join(engine::manifest_name(&scene.export.prefix)), engine::sequence_manifest(&scene)).map_err(|e| e.to_string())?;
    let prefix = scene.export.prefix.clone();
    let mut eng = engine::Engine::new(device, queue, scene);
    let target = render::RenderTarget::new(&eng.device, w, h);
    let t0 = std::time::Instant::now();
    for f in 0..frames {
        eng.step_frame();
        eng.render(&target, true);
        let img = engine::read_target(&eng.device, &eng.queue, &target);
        let path = out.join(engine::frame_name(&prefix, f));
        img.save(&path).map_err(|e| e.to_string())?;
        if stats {
            eprintln!("frame {f:4}  particles {:7}  step {:.1} ms", eng.particle_count(), eng.last_step_ms);
        }
    }
    eprintln!("wrote {frames} frames to {} in {:.1}s", out.display(), t0.elapsed().as_secs_f32());
    Ok(())
}

/// GPU device for headless rendering (no window). Uses whatever adapter the
/// system offers, including software Vulkan (lavapipe) on CI.
fn headless_device() -> Result<(wgpu::Device, wgpu::Queue), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("no GPU adapter: {e}"))?;
    eprintln!("adapter: {} ({:?})", adapter.get_info().name, adapter.get_info().backend);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tidefire"),
        ..Default::default()
    }))
    .map_err(|e| e.to_string())?;
    device.on_uncaptured_error(std::sync::Arc::new(|e| {
        eprintln!("wgpu error: {e}");
        std::process::exit(2);
    }));
    Ok((device, queue))
}
