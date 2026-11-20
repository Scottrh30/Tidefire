//! Tidefire's command line tool (the app itself is the Tauri window).
//!
//! Headless render:  tidefire-cli --headless [--preset NAME | --scene FILE] [--frames N] [--size WxH] [--out DIR|FILE]
//!                                [--format png|mp4|prores] [--quality 0-100] [--prores-profile P]
//!                                [--transfer apic|flip] [--transparent] [--stats]
//! Visual tests:     tidefire-cli --visual-test tests/visual [--update]

mod visual_test;

use std::path::PathBuf;
use tidefire_core::{engine, params, render, wgpu};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(dir) = arg(&args, "--visual-test") {
        let update = args.iter().any(|a| a == "--update");
        let result = headless_device()
            .and_then(|(d, q)| visual_test::run(&d, &q, std::path::Path::new(dir), update));
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
                eprintln!(
                    "tidefire: unknown preset \"{name}\" (try: {})",
                    params::PRESETS.join(", ")
                );
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
        "Tidefire {} command line tool\n\nUsage:\n  tidefire-cli --headless [--preset NAME | --scene FILE] [--frames N] [--size WxH] [--out DIR|FILE]\n                 [--format png|mp4|prores] [--quality 0-100] [--prores-profile proxy|lt|422|hq|4444|4444xq]\n                 [--transfer apic|flip] [--transparent] [--stats]\n  tidefire-cli --visual-test DIR [--update]\n  tidefire-cli --dump-preset NAME > scene.tidefire\n\nPresets: {}",
        env!("CARGO_PKG_VERSION"),
        params::PRESETS.join(", ")
    );
}

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

fn headless(args: &[String]) -> Result<(), String> {
    let mut scene = if let Some(path) = arg(args, "--scene") {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?
    } else {
        let name = arg(args, "--preset").unwrap_or("Campfire");
        params::preset(name).ok_or_else(|| {
            format!(
                "unknown preset \"{name}\" (try: {})",
                params::PRESETS.join(", ")
            )
        })?
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
    if let Some(f) = arg(args, "--format") {
        scene.export.format = match f.to_ascii_lowercase().as_str() {
            "png" => params::ExportFormat::PngSequence,
            "mp4" | "h264" => params::ExportFormat::Mp4,
            "prores" | "mov" => params::ExportFormat::ProRes,
            _ => return Err("--format expects png, mp4 or prores".into()),
        };
    }
    if let Some(q) = arg(args, "--quality") {
        scene.export.mp4_quality = q
            .parse::<u8>()
            .map_err(|_| "--quality expects 0-100")?
            .min(100);
    }
    if let Some(p) = arg(args, "--prores-profile") {
        scene.export.prores_profile = match p.to_ascii_lowercase().as_str() {
            "proxy" => params::ProResProfile::Proxy,
            "lt" => params::ProResProfile::Lt,
            "422" | "standard" => params::ProResProfile::Standard,
            "hq" | "422hq" => params::ProResProfile::Hq,
            "4444" => params::ProResProfile::P4444,
            "4444xq" | "xq" => params::ProResProfile::P4444Xq,
            _ => return Err("--prores-profile expects proxy, lt, 422, hq, 4444 or 4444xq".into()),
        };
        if arg(args, "--format").is_none() {
            scene.export.format = params::ExportFormat::ProRes;
        }
    }
    let codec = scene.export.codec();
    if codec.is_some() && !scene.export.keeps_alpha() {
        // this format can't store transparency: render over the background
        scene.render.transparent_background = false;
    }
    let stats = args.iter().any(|a| a == "--stats");
    let frames = scene.export.frames;
    let (w, h) = (scene.export.width, scene.export.height);

    // PNGs go into a folder; a video is one file.
    let mut video = None;
    let out = match codec {
        None => {
            let out = PathBuf::from(arg(args, "--out").unwrap_or("tidefire_out"));
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            std::fs::write(
                out.join(engine::manifest_name(&scene.export.prefix)),
                engine::sequence_manifest(&scene),
            )
            .map_err(|e| e.to_string())?;
            out
        }
        Some(codec) => {
            let mut out = PathBuf::from(
                arg(args, "--out")
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("tidefire_out.{}", codec.extension())),
            );
            if out.extension().is_none() {
                out.set_extension(codec.extension());
            }
            if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let settings = tidefire_video::Settings {
                codec,
                width: w,
                height: h,
                fps: scene.fps,
            };
            video = Some(tidefire_video::VideoWriter::new(&out, settings)?);
            out
        }
    };

    let (device, queue) = headless_device()?;
    let prefix = scene.export.prefix.clone();
    let mut eng = engine::Engine::new(device, queue, scene);
    let target = render::RenderTarget::new(&eng.device, w, h);
    let t0 = std::time::Instant::now();
    for f in 0..frames {
        eng.step_frame();
        eng.render(&target, true);
        let img = engine::read_target(&eng.device, &eng.queue, &target);
        match video.as_mut() {
            Some(v) => v.push(img.into_raw())?,
            None => {
                let path = out.join(engine::frame_name(&prefix, f));
                img.save(&path).map_err(|e| e.to_string())?;
            }
        }
        if stats {
            eprintln!(
                "frame {f:4}  particles {:7}  step {:.1} ms",
                eng.particle_count(),
                eng.last_step_ms
            );
        }
    }
    if let Some(v) = video {
        v.finish()?;
    }
    eprintln!(
        "wrote {frames} frames to {} in {:.1}s",
        out.display(),
        t0.elapsed().as_secs_f32()
    );
    Ok(())
}

/// GPU device for headless rendering (no window). Uses whatever adapter the
/// system offers, including software Vulkan (lavapipe) on CI.
fn headless_device() -> Result<(wgpu::Device, wgpu::Queue), String> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("no GPU adapter: {e}"))?;
    eprintln!(
        "adapter: {} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend
    );
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
