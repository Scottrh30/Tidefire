//! Tidefire's interface: a Leptos app in the Tauri window. The fire and water
//! engine (`tidefire-core`) is compiled into the same WebAssembly module and
//! runs on the GPU through WebGPU.

mod export;
mod fields;
mod host;
mod panels;
mod state;
mod viewport;

use leptos::prelude::*;
use state::Ui;
use tidefire_core::params::{self, Scene};

fn open(ui: Ui) {
    leptos::task::spawn_local(async move {
        match host::open_scene().await {
            Ok(Some(f)) => match serde_json::from_str::<Scene>(&f.text) {
                Ok(s) => {
                    ui.load(s);
                    ui.status.set(format!("Opened {}", f.path));
                    ui.scene_path.set(Some(f.path));
                }
                Err(e) => ui.status.set(format!("Could not read {}: {e}", f.path)),
            },
            Ok(None) => {}
            Err(e) => ui.status.set(e),
        }
    });
}

fn save(ui: Ui, save_as: bool) {
    let text = ui.scene.with_untracked(|s| serde_json::to_string_pretty(s).unwrap());
    let suggested = ui.scene.with_untracked(|s| format!("{}.tidefire", s.name));
    let path = if save_as { None } else { ui.scene_path.get_untracked() };
    leptos::task::spawn_local(async move {
        match host::save_scene(text, path, suggested).await {
            Ok(Some(p)) => {
                ui.status.set(format!("Saved {p}"));
                ui.scene_path.set(Some(p));
            }
            Ok(None) => {}
            Err(e) => ui.status.set(e),
        }
    });
}

#[component]
fn TopBar(ui: Ui) -> impl IntoView {
    let stats = move || ui.stats.get();
    view! {
        <header class="top">
            <div class="brand"><span class="logo"></span>"Tidefire"</div>
            <select
                class="preset"
                title="Start from a preset"
                prop:value=move || ui.scene.with(|s| s.name.clone())
                on:change=move |ev| ui.load_preset(&event_target_value(&ev))
            >
                {params::PRESETS.iter().map(|p| view! { <option value=*p>{*p}</option> }).collect_view()}
            </select>
            <button class="btn" title="Open a .tidefire scene (Ctrl+O)" on:click=move |_| open(ui)>"Open"</button>
            <button class="btn" title="Save the scene (Ctrl+S; Shift for Save As)" on:click=move |ev: web_sys::MouseEvent| save(ui, ev.shift_key())>"Save"</button>
            <button class="btn" title="Export a PNG sequence" on:click=move |_| ui.export.open.set(true)>"Export\u{2026}"</button>
            <div class="sep"></div>
            <button class="btn play" class:on=move || ui.playing.get() title="Play / pause (Space)"
                on:click=move |_| ui.playing.update(|p| *p = !*p)>
                {move || if ui.playing.get() { "\u{275a}\u{275a} Pause" } else { "\u{25b6} Play" }}
            </button>
            <button class="btn" title="Advance one frame (.)" on:click=move |_| { ui.playing.set(false); ui.step_rev.update(|r| *r += 1) }>"Step"</button>
            <button class="btn" title="Restart the simulation (R)" on:click=move |_| ui.reset()>"Reset"</button>
            <div class="stats">
                <span>{move || format!("Frame {}", stats().frame)}</span>
                <span>{move || format!("{:.2} s", stats().time)}</span>
                <span title="Simulated frames per second in the viewport">{move || {
                    let f = stats().fps;
                    if f > 0.0 { format!("{f:.0} fps") } else { "paused".to_string() }
                }}</span>
                <Show when=move || ui.scene.with(|s| s.water.enabled)>
                    <span>{move || format!("{} particles", stats().particles)}</span>
                </Show>
            </div>
            <div class="status" title=move || ui.status.get()>{move || ui.status.get()}</div>
        </header>
    }
}

#[component]
fn Root() -> impl IntoView {
    let ui = Ui::new();
    // Keyboard: Space play/pause, . step, R reset, Ctrl+S / Ctrl+O.
    let handle = window_event_listener(leptos::ev::keydown, move |ev| {
        if ui.export.open.get_untracked() {
            if ev.key() == "Escape" && !ui.export.running.get_untracked() {
                ui.export.open.set(false);
            }
            return;
        }
        let ctrl = ev.ctrl_key() || ev.meta_key();
        match ev.key().as_str() {
            " " => {
                ev.prevent_default();
                ui.playing.update(|p| *p = !*p);
            }
            "." => {
                ui.playing.set(false);
                ui.step_rev.update(|r| *r += 1);
            }
            "r" | "R" if !ctrl => ui.reset(),
            "s" | "S" if ctrl => {
                ev.prevent_default();
                save(ui, ev.shift_key());
            }
            "o" | "O" if ctrl => {
                ev.prevent_default();
                open(ui);
            }
            _ => {}
        }
    });
    on_cleanup(move || handle.remove());
    view! {
        <div class="app">
            <TopBar ui=ui />
            <div class="main">
                <panels::Panels ui=ui />
                <viewport::Viewport ui=ui />
            </div>
            <export::ExportDialog ui=ui />
        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(Root);
}
