//! Export: steps the simulation frame by frame, renders each frame at the
//! export size and reads it back from the GPU. A PNG sequence (for Reelforge)
//! is written by the Tauri shell into a folder; .mp4 (H.264) and ProRes .mov
//! frames go to the shell as raw pixels and on into ffmpeg.

use crate::fields::{Choice, Switch};
use crate::host;
use crate::state::Ui;
use crate::viewport::with_engine;
use crate::{number, slider, switch};
use leptos::prelude::*;
use tidefire_core::engine;
use tidefire_core::params::{ExportFormat, ProResProfile, VideoSettings};
use tidefire_core::render::RenderTarget;

/// Where the export goes, as shown in the dialog.
fn destination(ui: Ui) -> Option<String> {
    match ui.scene.with(|s| s.export.format) {
        ExportFormat::PngSequence => ui.export.folder.get(),
        _ => ui.export.video_path.get(),
    }
}

async fn run(ui: Ui) -> Result<u32, String> {
    let mut scene = ui.scene.get_untracked();
    let e = scene.export.clone();
    if let Some(codec) = e.codec() {
        let path = ui
            .export
            .video_path
            .get_untracked()
            .ok_or("choose a file first")?;
        if !e.keeps_alpha() {
            // this format has no alpha channel: render over the background
            scene.render.transparent_background = false;
        }
        let settings = VideoSettings {
            codec,
            width: e.width,
            height: e.height,
            fps: scene.fps,
        };
        let (device, queue) = prepare(ui, &scene)?;
        let id = host::video_begin(&path, &settings).await?;
        let r = frames(ui, &device, &queue, &e, async |_, img: image::RgbaImage| {
            host::video_frame(id, img.as_raw()).await
        })
        .await;
        return match r {
            Ok(n) if ui.export.cancel.get_untracked() => {
                host::video_abort(id).await?;
                Ok(n)
            }
            Ok(_) => host::video_finish(id).await,
            Err(err) => {
                let _ = host::video_abort(id).await;
                Err(err)
            }
        };
    }
    let folder = ui
        .export
        .folder
        .get_untracked()
        .ok_or("choose a folder first")?;
    let (device, queue) = prepare(ui, &scene)?;
    host::write_text(
        &folder,
        &engine::manifest_name(&e.prefix),
        &engine::sequence_manifest(&scene),
    )
    .await?;
    let folder = &folder;
    let prefix = &e.prefix;
    frames(ui, &device, &queue, &e, async |i, img: image::RgbaImage| {
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        host::write_file(folder, &engine::frame_name(prefix, e.start_frame + i), &png).await
    })
    .await
}

/// Gives the engine the scene being exported (optionally starting over).
fn prepare(
    ui: Ui,
    scene: &tidefire_core::params::Scene,
) -> Result<(tidefire_core::wgpu::Device, tidefire_core::wgpu::Queue), String> {
    with_engine(|eng| {
        eng.scene = scene.clone();
        if ui.export.reset_first.get_untracked() {
            eng.reset();
        }
        (eng.device.clone(), eng.queue.clone())
    })
    .ok_or_else(|| "the viewport's GPU has not started".to_owned())
}

/// Steps, renders and reads back each frame, handing it to `out`.
/// Returns how many frames were done (fewer if cancelled).
async fn frames(
    ui: Ui,
    device: &tidefire_core::wgpu::Device,
    queue: &tidefire_core::wgpu::Queue,
    e: &tidefire_core::params::ExportParams,
    out: impl AsyncFn(u32, image::RgbaImage) -> Result<(), String>,
) -> Result<u32, String> {
    let target = RenderTarget::new(device, e.width, e.height);
    ui.export.total.set(e.frames);
    for i in 0..e.frames {
        if ui.export.cancel.get_untracked() {
            return Ok(i);
        }
        with_engine(|eng| {
            eng.step_frame();
            eng.render(&target, true);
        });
        let img = engine::read_target_async(device, queue, &target).await?;
        out(i, img).await?;
        ui.export.done.set(i + 1);
    }
    Ok(e.frames)
}

fn start(ui: Ui) {
    if ui.export.running.get_untracked() {
        return;
    }
    ui.export.running.set(true);
    ui.export.cancel.set(false);
    ui.export.done.set(0);
    ui.playing.set(false);
    leptos::task::spawn_local(async move {
        let result = run(ui).await;
        ui.export.running.set(false);
        // The engine's scene is the UI's again on the next frame.
        ui.rev.update(|r| *r += 1);
        let dest = destination(ui).unwrap_or_default();
        match result {
            Ok(n) if ui.export.cancel.get_untracked() => {
                ui.status.set(format!("Export cancelled after {n} frames"))
            }
            Ok(n) => {
                ui.status.set(format!("Exported {n} frames to {dest}"));
                ui.export.open.set(false);
            }
            Err(e) => ui.status.set(format!("Export failed: {e}")),
        }
    });
}

#[component]
pub fn ExportDialog(ui: Ui) -> impl IntoView {
    let x = ui.export;
    let size_preset = move |w: u32, h: u32| {
        ui.edit(|s| {
            s.export.width = w;
            s.export.height = h;
        })
    };
    let show_progress = move || x.running.get() || x.done.get() != 0;
    let format = Signal::derive(move || ui.scene.with(|s| s.export.format));
    let profile = Signal::derive(move || ui.scene.with(|s| s.export.prores_profile));
    let is_video = move || format.get() != ExportFormat::PngSequence;
    let ext = move || {
        if format.get_untracked() == ExportFormat::ProRes {
            "mov"
        } else {
            "mp4"
        }
    };
    let pick = move |_| {
        leptos::task::spawn_local(async move {
            let r = if format.get_untracked() == ExportFormat::PngSequence {
                host::pick_folder()
                    .await
                    .map(|f| f.map(|f| x.folder.set(Some(f))))
            } else {
                let prefix = ui.scene.with_untracked(|s| s.export.prefix.clone());
                host::pick_video_file(&prefix, ext())
                    .await
                    .map(|f| f.map(|f| x.video_path.set(Some(f))))
            };
            if let Err(e) = r {
                ui.status.set(e);
            }
        });
    };
    // Switching between .mp4 and .mov fixes the chosen file's extension.
    Effect::new(move |_| {
        let e = ext();
        if !is_video() {
            return;
        }
        x.video_path.update(|p| {
            if let Some(p) = p {
                if let Some(dot) = p
                    .rfind('.')
                    .filter(|&d| d > p.rfind(['/', '\\']).map_or(0, |s| s + 1))
                {
                    p.truncate(dot);
                }
                p.push('.');
                p.push_str(e);
            }
        });
    });
    // Look for ffmpeg once, the first time a video format is chosen.
    Effect::new(move |_| {
        if is_video() && host::in_tauri() && x.ffmpeg.get_untracked().is_none() {
            x.ffmpeg.set(Some(Err("checking\u{2026}".into())));
            leptos::task::spawn_local(async move {
                x.ffmpeg.set(Some(host::ffmpeg_status().await));
            });
        }
    });
    let alpha_lost = move || {
        ui.scene
            .with(|s| s.render.transparent_background && !s.export.keeps_alpha())
    };
    let ffmpeg_missing =
        move || matches!(x.ffmpeg.get(), Some(Err(e)) if !e.starts_with("checking"));
    view! {
        <Show when=move || x.open.get()>
            <div class="scrim" on:click=move |_| if !x.running.get_untracked() { x.open.set(false) }></div>
            <div class="dialog">
                <h2>"Export"</h2>
                <Choice
                    label="Format"
                    options=vec![
                        (ExportFormat::PngSequence, "PNG sequence"),
                        (ExportFormat::Mp4, "MP4 (H.264)"),
                        (ExportFormat::ProRes, "ProRes"),
                    ]
                    value=format
                    on_change=Callback::new(move |f| ui.edit(|s| s.export.format = f))
                />
                <p class="dim">{move || match format.get() {
                    ExportFormat::PngSequence => "Numbered PNG frames for Reelforge or any editor, plus a _sequence.json with the frame rate and file pattern. Keeps transparency.",
                    ExportFormat::Mp4 => "One H.264 .mp4 that plays anywhere. No transparency: the background is rendered in.",
                    ExportFormat::ProRes => "One Apple ProRes .mov for editing and compositing. 4444 and 4444 XQ keep transparency.",
                }}</p>
                <Show when=move || format.get() == ExportFormat::Mp4>
                    {slider!(ui, "Quality", 0, 100, export.mp4_quality, int)}
                </Show>
                <Show when=move || format.get() == ExportFormat::ProRes>
                    <Choice
                        label="Profile"
                        options=ProResProfile::ALL.iter().map(|p| (*p, p.label())).collect()
                        value=profile
                        on_change=Callback::new(move |p| ui.edit(|s| s.export.prores_profile = p))
                    />
                </Show>
                {number!(ui, "Width", export.width, int)}
                {number!(ui, "Height", export.height, int)}
                <div class="row">
                    <label>"Presets"</label>
                    <div class="ctl segs">
                        <button class="seg" on:click=move |_| size_preset(1280, 720)>"720p"</button>
                        <button class="seg" on:click=move |_| size_preset(1920, 1080)>"1080p"</button>
                        <button class="seg" on:click=move |_| size_preset(3840, 2160)>"4K"</button>
                        <button class="seg" on:click=move |_| size_preset(1080, 1080)>"Square"</button>
                    </div>
                </div>
                {number!(ui, "Frames", export.frames, int)}
                <div class="row">
                    <label>"File prefix"</label>
                    <div class="ctl">
                        <input class="text" prop:value=move || ui.scene.with(|s| s.export.prefix.clone())
                            on:keydown=|ev: web_sys::KeyboardEvent| ev.stop_propagation()
                            on:change=move |ev| { let v = event_target_value(&ev); ui.edit(|s| s.export.prefix = v) } />
                    </div>
                </div>
                <Switch label="Reset simulation first" value=Signal::derive(move || x.reset_first.get()) on_change=Callback::new(move |v| x.reset_first.set(v)) />
                {switch!(ui, "Transparent background", render.transparent_background)}
                <Show when=alpha_lost>
                    <p class="dim">"This format can't store transparency, so the background will be rendered in. Use PNG or ProRes 4444 to keep it."</p>
                </Show>
                <div class="row">
                    <label>{move || if is_video() { "File" } else { "Folder" }}</label>
                    <div class="ctl">
                        <button class="btn" on:click=pick>"Choose\u{2026}"</button>
                        <span class="path">{move || destination(ui).unwrap_or_else(|| "none chosen".into())}</span>
                    </div>
                </div>
                <Show when=move || is_video() && ffmpeg_missing()>
                    <p class="warn">{move || match x.ffmpeg.get() { Some(Err(e)) => e, _ => String::new() }}</p>
                </Show>
                <Show when=move || !host::in_tauri()>
                    <p class="warn">"Exporting writes files, so it needs the Tidefire desktop app."</p>
                </Show>
                <Show when=show_progress>
                    <div class="progress">
                        <div class="bar" style=move || format!("width:{}%", 100.0 * x.done.get() as f32 / x.total.get().max(1) as f32)></div>
                        <span>{move || format!("{} / {}", x.done.get(), x.total.get())}</span>
                    </div>
                </Show>
                <div class="actions">
                    <Show
                        when=move || x.running.get()
                        fallback=move || view! {
                            <button class="btn" on:click=move |_| x.open.set(false)>"Close"</button>
                            <button class="btn primary" disabled=move || destination(ui).is_none() || (is_video() && ffmpeg_missing()) on:click=move |_| start(ui)>"Start export"</button>
                        }
                    >
                        <button class="btn" on:click=move |_| x.cancel.set(true)>"Cancel"</button>
                    </Show>
                </div>
            </div>
        </Show>
    }
}
