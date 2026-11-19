//! Input widgets for the parameter panels, plus macros that bind them to a
//! field of the scene (`slider!(ui, "Buoyancy", 0.0, 15.0, smoke.buoyancy)`).

use crate::prefs::Folds;
use leptos::prelude::*;

fn fmt(v: f32, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.starts_with('-') && s.trim_start_matches(['-', '0', '.']).is_empty() { s.trim_start_matches('-').to_owned() } else { s }
}

fn decimals_for(min: f32, max: f32) -> usize {
    let span = (max - min).abs();
    if span >= 200.0 {
        0
    } else if span >= 10.0 {
        1
    } else if span >= 1.0 {
        2
    } else if span >= 0.1 {
        3
    } else {
        4
    }
}

fn parse(text: &str) -> Option<f32> {
    let t: String = text.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')).collect();
    t.parse::<f32>().ok().filter(|v| v.is_finite())
}

/// A labelled slider with a number box. `integer` rounds the value.
#[component]
pub fn Slider(
    label: &'static str,
    min: f32,
    max: f32,
    #[prop(optional)] unit: &'static str,
    #[prop(optional)] integer: bool,
    #[prop(into)] value: Signal<f32>,
    on_change: Callback<f32>,
) -> impl IntoView {
    let decimals = if integer { 0 } else { decimals_for(min, max) };
    let step = if integer { 1.0 } else { (max - min) / 1000.0 };
    let set = move |v: f32| {
        let v = if integer { v.round() } else { v };
        on_change.run(v);
    };
    view! {
        <div class="row">
            <label>{label}</label>
            <div class="ctl slider">
                <input
                    type="range"
                    min=min.to_string()
                    max=max.to_string()
                    step=step.to_string()
                    style=move || format!("--p:{:.2}%", 100.0 * ((value.get() - min) / (max - min)).clamp(0.0, 1.0))
                    prop:value=move || value.get().clamp(min, max).to_string()
                    on:input=move |ev| if let Some(v) = parse(&event_target_value(&ev)) { set(v) }
                />
                <input
                    class="numbox"
                    prop:value=move || format!("{}{}", fmt(value.get(), decimals), unit)
                    on:keydown=move |ev: web_sys::KeyboardEvent| ev.stop_propagation()
                    on:change=move |ev| if let Some(v) = parse(&event_target_value(&ev)) { set(v) }
                />
            </div>
        </div>
    }
}

/// A labelled number box (no slider): resolutions, counts, sizes.
#[component]
pub fn Number(
    label: &'static str,
    #[prop(optional)] unit: &'static str,
    #[prop(default = 3)] decimals: usize,
    #[prop(optional)] integer: bool,
    #[prop(default = f32::MIN)] min: f32,
    #[prop(default = f32::MAX)] max: f32,
    #[prop(into)] value: Signal<f32>,
    on_change: Callback<f32>,
) -> impl IntoView {
    let decimals = if integer { 0 } else { decimals };
    view! {
        <div class="row">
            <label>{label}</label>
            <div class="ctl">
                <input
                    class="numbox wide"
                    prop:value=move || format!("{}{}", fmt(value.get(), decimals), unit)
                    on:keydown=move |ev: web_sys::KeyboardEvent| ev.stop_propagation()
                    on:change=move |ev| {
                        if let Some(v) = parse(&event_target_value(&ev)) {
                            let v = v.clamp(min, max);
                            on_change.run(if integer { v.round() } else { v });
                        }
                    }
                />
            </div>
        </div>
    }
}

/// Three numbers with coloured X / Y / Z (or R / G / B for resolutions).
#[component]
pub fn Vec3Field(
    label: &'static str,
    #[prop(optional)] integer: bool,
    #[prop(into)] value: Signal<[f32; 3]>,
    on_change: Callback<[f32; 3]>,
) -> impl IntoView {
    let decimals = if integer { 0 } else { 3 };
    let axis = move |i: usize| {
        let name = ["x", "y", "z"][i];
        view! {
            <span class="axis">
                <b class=format!("ax-{name}")>{name}</b>
                <input
                    class="numbox"
                    prop:value=move || fmt(value.get()[i], decimals)
                    on:keydown=move |ev: web_sys::KeyboardEvent| ev.stop_propagation()
                    on:change=move |ev| {
                        if let Some(v) = parse(&event_target_value(&ev)) {
                            let mut a = value.get_untracked();
                            a[i] = if integer { v.round() } else { v };
                            on_change.run(a);
                        }
                    }
                />
            </span>
        }
    };
    view! {
        <div class="row">
            <label>{label}</label>
            <div class="ctl vec3">{axis(0)}{axis(1)}{axis(2)}</div>
        </div>
    }
}

pub fn to_hex(c: [f32; 3]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

pub fn from_hex(s: &str) -> Option<[f32; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
    Some([p(0)?, p(2)?, p(4)?])
}

#[component]
pub fn ColorField(label: &'static str, #[prop(into)] value: Signal<[f32; 3]>, on_change: Callback<[f32; 3]>) -> impl IntoView {
    view! {
        <div class="row">
            <label>{label}</label>
            <div class="ctl">
                <input
                    type="color"
                    class="color"
                    prop:value=move || to_hex(value.get())
                    on:input=move |ev| if let Some(c) = from_hex(&event_target_value(&ev)) { on_change.run(c) }
                />
                <span class="hex">{move || to_hex(value.get())}</span>
            </div>
        </div>
    }
}

#[component]
pub fn Switch(label: &'static str, #[prop(into)] value: Signal<bool>, on_change: Callback<bool>) -> impl IntoView {
    view! {
        <div class="row">
            <label>{label}</label>
            <div class="ctl">
                <button
                    class="switch"
                    class:on=move || value.get()
                    role="switch"
                    on:click=move |_| on_change.run(!value.get_untracked())
                >
                    <i></i>
                </button>
            </div>
        </div>
    }
}

/// Segmented buttons for a small enum.
#[component]
pub fn Choice<T: Copy + PartialEq + Send + Sync + 'static>(
    label: &'static str,
    options: Vec<(T, &'static str)>,
    #[prop(into)] value: Signal<T>,
    on_change: Callback<T>,
) -> impl IntoView {
    let buttons = options
        .into_iter()
        .map(|(v, name)| {
            view! {
                <button class="seg" class:on=move || value.get() == v on:click=move |_| on_change.run(v)>
                    {name}
                </button>
            }
        })
        .collect_view();
    view! {
        <div class="row">
            <label>{label}</label>
            <div class="ctl segs">{buttons}</div>
        </div>
    }
}

fn folds() -> Folds {
    use_context::<Folds>().expect("Folds context")
}

/// A collapsible panel section (Fire & smoke, Water...). Remembers whether it
/// was open between runs, under `key`.
#[component]
pub fn Section(
    title: &'static str,
    key: &'static str,
    #[prop(optional)] accent: &'static str,
    #[prop(default = true)] open: bool,
    children: Children,
) -> impl IntoView {
    let f = folds();
    let is_open = move || f.is_open(key, open);
    view! {
        <section class="section" class:closed=move || !is_open()>
            <header on:click=move |_| f.set(key, !is_open())>
                <span class="chev">{move || if is_open() { "\u{25be}" } else { "\u{25b8}" }}</span>
                <span class="dot" style=format!("background:{}", if accent.is_empty() { "var(--dim)" } else { accent })></span>
                {title}
            </header>
            <div class="body">{children()}</div>
        </section>
    }
}

/// A dropdown inside a section (Domain, Emitter, Combustion...), also
/// remembered between runs under `key`.
#[component]
pub fn Fold(title: &'static str, key: &'static str, #[prop(optional)] open: bool, children: Children) -> impl IntoView {
    let f = folds();
    let is_open = move || f.is_open(key, open);
    view! {
        <div class="fold" class:closed=move || !is_open()>
            <button class="fold-head" on:click=move |_| f.set(key, !is_open())>
                <span class="chev">{move || if is_open() { "\u{25be}" } else { "\u{25b8}" }}</span>
                {title}
            </button>
            <div class="fold-body">{children()}</div>
        </div>
    }
}

#[component]
pub fn Hint(text: &'static str) -> impl IntoView {
    view! { <div class="hint">{text}</div> }
}

/// `slider!(ui, "Label", min, max, path.to.field)` and optional `unit = "s"`, `int`.
#[macro_export]
macro_rules! slider {
    ($ui:expr, $label:expr, $min:expr, $max:expr, $($f:ident).+) => {{
        let ui = $ui;
        view! { <$crate::fields::Slider label=$label min=$min as f32 max=$max as f32
            value=Signal::derive(move || ui.scene.with(|s| s.$($f).+ as f32))
            on_change=Callback::new(move |v: f32| ui.edit(|s| s.$($f).+ = v as _)) /> }
    }};
    ($ui:expr, $label:expr, $min:expr, $max:expr, $($f:ident).+, unit = $unit:expr) => {{
        let ui = $ui;
        view! { <$crate::fields::Slider label=$label min=$min as f32 max=$max as f32 unit=$unit
            value=Signal::derive(move || ui.scene.with(|s| s.$($f).+ as f32))
            on_change=Callback::new(move |v: f32| ui.edit(|s| s.$($f).+ = v as _)) /> }
    }};
    ($ui:expr, $label:expr, $min:expr, $max:expr, $($f:ident).+, int) => {{
        let ui = $ui;
        view! { <$crate::fields::Slider label=$label min=$min as f32 max=$max as f32 integer=true
            value=Signal::derive(move || ui.scene.with(|s| s.$($f).+ as f32))
            on_change=Callback::new(move |v: f32| ui.edit(|s| s.$($f).+ = v as _)) /> }
    }};
}

/// `number!(ui, "Label", path.to.field)` with optional `unit = ".."`, `int`.
#[macro_export]
macro_rules! number {
    ($ui:expr, $label:expr, $($f:ident).+, unit = $unit:expr) => {{
        let ui = $ui;
        view! { <$crate::fields::Number label=$label unit=$unit min=0.0
            value=Signal::derive(move || ui.scene.with(|s| s.$($f).+ as f32))
            on_change=Callback::new(move |v: f32| ui.edit(|s| s.$($f).+ = v as _)) /> }
    }};
    ($ui:expr, $label:expr, $($f:ident).+, int) => {{
        let ui = $ui;
        view! { <$crate::fields::Number label=$label integer=true min=0.0
            value=Signal::derive(move || ui.scene.with(|s| s.$($f).+ as f32))
            on_change=Callback::new(move |v: f32| ui.edit(|s| s.$($f).+ = v as _)) /> }
    }};
}

/// `vec3!(ui, "Label", path.to.field)` for `[f32; 3]`; `vec3!(.., int)` for `[u32; 3]`.
#[macro_export]
macro_rules! vec3 {
    ($ui:expr, $label:expr, $($f:ident).+) => {{
        let ui = $ui;
        view! { <$crate::fields::Vec3Field label=$label
            value=Signal::derive(move || ui.scene.with(|s| s.$($f).+))
            on_change=Callback::new(move |v: [f32; 3]| ui.edit(|s| s.$($f).+ = v)) /> }
    }};
    ($ui:expr, $label:expr, $($f:ident).+, int) => {{
        let ui = $ui;
        view! { <$crate::fields::Vec3Field label=$label integer=true
            value=Signal::derive(move || ui.scene.with(|s| s.$($f).+.map(|x| x as f32)))
            on_change=Callback::new(move |v: [f32; 3]| ui.edit(|s| s.$($f).+ = v.map(|x| x.max(8.0) as u32))) /> }
    }};
}

#[macro_export]
macro_rules! color {
    ($ui:expr, $label:expr, $($f:ident).+) => {{
        let ui = $ui;
        view! { <$crate::fields::ColorField label=$label
            value=Signal::derive(move || ui.scene.with(|s| s.$($f).+))
            on_change=Callback::new(move |v: [f32; 3]| ui.edit(|s| s.$($f).+ = v)) /> }
    }};
}

#[macro_export]
macro_rules! switch {
    ($ui:expr, $label:expr, $($f:ident).+) => {{
        let ui = $ui;
        view! { <$crate::fields::Switch label=$label
            value=Signal::derive(move || ui.scene.with(|s| s.$($f).+))
            on_change=Callback::new(move |v: bool| ui.edit(|s| s.$($f).+ = v)) /> }
    }};
}
