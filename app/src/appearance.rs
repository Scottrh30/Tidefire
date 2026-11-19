//! The Appearance dialog: interface colours, applied live and remembered.

use crate::fields::{ColorField, from_hex, to_hex};
use crate::prefs::Theme;
use crate::state::Ui;
use leptos::prelude::*;

#[component]
pub fn AppearanceDialog(ui: Ui) -> impl IntoView {
    let field = move |label: &'static str, get: fn(&Theme) -> &String, set: fn(&mut Theme, String)| {
        let value = Signal::derive(move || ui.theme.with(|t| from_hex(get(t)).unwrap_or([1.0; 3])));
        let on_change = Callback::new(move |c: [f32; 3]| ui.set_theme(|t| set(t, to_hex(c))));
        view! { <ColorField label=label value=value on_change=on_change /> }
    };
    let presets = move || Theme::PRESETS
        .iter()
        .map(|&(name, make)| {
            let active = move || ui.theme.with(|t| *t == make());
            view! {
                <button class="seg" class:on=active on:click=move |_| ui.set_theme(|t| *t = make())>{name}</button>
            }
        })
        .collect_view();
    view! {
        <Show when=move || ui.appearance_open.get()>
            <div class="scrim" on:click=move |_| ui.appearance_open.set(false)></div>
            <div class="dialog appearance">
                <h2>"Appearance"</h2>
                <p class="dim">"Interface colours. Changes apply right away and are remembered on this computer."</p>
                <div class="row">
                    <label>"Theme"</label>
                    <div class="ctl segs">{presets()}</div>
                </div>
                {field("Text", |t| &t.text, |t, v| t.text = v)}
                {field("Sliders", |t| &t.slider, |t, v| t.slider = v)}
                {field("Toggles", |t| &t.toggle, |t, v| t.toggle = v)}
                {field("Buttons", |t| &t.button, |t, v| t.button = v)}
                {field("Button text", |t| &t.button_text, |t, v| t.button_text = v)}
                <p class="dim note">"The toggle colour also marks selected options, the Play button while playing, and the main button in dialogs."</p>
                <div class="sample">
                    <span class="sample-label">"Preview"</span>
                    <input type="range" min="0" max="1" step="0.01" value="0.6" style="--p:60%" />
                    <button class="switch on"><i></i></button>
                    <button class="btn">"Button"</button>
                    <button class="btn primary">"Main action"</button>
                </div>
                <div class="actions">
                    <button class="btn" on:click=move |_| ui.set_theme(|t| *t = Theme::default())>"Reset to default"</button>
                    <button class="btn primary" on:click=move |_| ui.appearance_open.set(false)>"Done"</button>
                </div>
            </div>
        </Show>
    }
}
