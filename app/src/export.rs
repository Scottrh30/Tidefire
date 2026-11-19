//! PNG-sequence export (for Reelforge): steps the simulation frame by frame,
//! renders each frame at the export size, reads it back from the GPU and has
//! the Tauri shell write it to the chosen folder.

use crate::fields::Switch;
use crate::host;
use crate::state::Ui;
use crate::viewport::with_engine;
use crate::{number, switch};
use leptos::prelude::*;
use tidefire_core::engine;
use tidefire_core::render::RenderTarget;

async fn run(ui: Ui) -> Result<u32, String> {
    let folder = ui.export.folder.get_untracked().ok_or("choose a folder first")?;
    let scene = ui.scene.get_untracked();
    let e = scene.export.clone();
    // Make sure the engine has the latest edits, then optionally start over.
    let (device, queue) = with_engine(|eng| {
        eng.scene = scene.clone();
        if ui.export.reset_first.get_untracked() {
            eng.reset();
        }
        (eng.device.clone(), eng.queue.clone())
    })
    .ok_or("the viewport's GPU has not started")?;
    host::write_text(&folder, &engine::manifest_name(&e.prefix), &engine::sequence_manifest(&scene)).await?;
    let target = RenderTarget::new(&device, e.width, e.height);
    ui.export.total.set(e.frames);
    for i in 0..e.frames {
        if ui.export.cancel.get_untracked() {
            return Ok(i);
        }
        with_engine(|eng| {
            eng.step_frame();
            eng.render(&target, true);
        });
        let img = engine::read_target_async(&device, &queue, &target).await?;
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
        host::write_file(&folder, &engine::frame_name(&e.prefix, e.start_frame + i), &png).await?;
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
        let folder = ui.export.folder.get_untracked().unwrap_or_default();
        match result {
            Ok(n) if ui.export.cancel.get_untracked() => ui.status.set(format!("Export cancelled after {n} frames")),
            Ok(n) => {
                ui.status.set(format!("Exported {n} frames to {folder}"));
                ui.export.open.set(false);
            }
            Err(e) => ui.status.set(format!("Export failed: {e}")),
        }
    });
}

#[component]
pub fn ExportDialog(ui: Ui) -> impl IntoView {
    let x = ui.export;
    let size_preset = move |w: u32, h: u32| ui.edit(|s| {
        s.export.width = w;
        s.export.height = h;
    });
    let show_progress = move || x.running.get() || x.done.get() != 0;
    let pick = move |_| {
        leptos::task::spawn_local(async move {
            match host::pick_folder().await {
                Ok(Some(f)) => x.folder.set(Some(f)),
                Ok(None) => {}
                Err(e) => ui.status.set(e),
            }
        });
    };
    view! {
        <Show when=move || x.open.get()>
            <div class="scrim" on:click=move |_| if !x.running.get_untracked() { x.open.set(false) }></div>
            <div class="dialog">
                <h2>"Export PNG sequence"</h2>
                <p class="dim">"Frames for Reelforge or any editor, plus a "<code>"_sequence.json"</code>" with the frame rate and file pattern."</p>
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
                <div class="row">
                    <label>"Folder"</label>
                    <div class="ctl">
                        <button class="btn" on:click=pick>"Choose\u{2026}"</button>
                        <span class="path">{move || x.folder.get().unwrap_or_else(|| "none chosen".into())}</span>
                    </div>
                </div>
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
                            <button class="btn primary" disabled=move || x.folder.get().is_none() on:click=move |_| start(ui)>"Start export"</button>
                        }
                    >
                        <button class="btn" on:click=move |_| x.cancel.set(true)>"Cancel"</button>
                    </Show>
                </div>
            </div>
        </Show>
    }
}
