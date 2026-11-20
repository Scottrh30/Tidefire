//! The parameter panel on the left: simulation, fire & smoke, water, render
//! and camera settings. Every control edits the scene live.

use crate::fields::{Choice, Fold, Hint, Section, Slider, Vec3Field};
use crate::state::Ui;
use crate::{color, number, slider, switch, vec3};
use leptos::prelude::*;
use tidefire_core::params::{
    SPARK_PRESETS, SparkParams, Transfer, WaterBlock, WaterJet, WaterLook,
};

#[component]
pub fn Panels(ui: Ui) -> impl IntoView {
    view! {
        <aside class="panel">
            <div class="scene-name">{move || ui.scene.with(|s| s.name.clone())}</div>
            <Simulation ui=ui />
            <Fire ui=ui />
            <Sparks ui=ui />
            <Water ui=ui />
            <Ocean ui=ui />
            <Render ui=ui />
            <Camera ui=ui />
        </aside>
    }
}

#[component]
fn Simulation(ui: Ui) -> impl IntoView {
    view! {
        <Section title="Simulation" key="simulation">
            {slider!(ui, "Frame rate", 1, 120, fps, unit = " fps")}
            {slider!(ui, "Substeps", 1, 16, substeps, int)}
        </Section>
    }
}

#[component]
fn Fire(ui: Ui) -> impl IntoView {
    let on = Signal::derive(move || ui.scene.with(|s| s.smoke.enabled));
    view! {
        <Section title="Fire & smoke" key="fire" accent="#ff7a2f">
            {switch!(ui, "Enabled", smoke.enabled)}
            <div class="group" class:off=move || !on.get()>
                <Fold title="Domain" key="fire.domain">
                    {vec3!(ui, "Resolution", smoke.res, int)}
                    {number!(ui, "Voxel size", smoke.cell_size, unit = " m")}
                    {vec3!(ui, "Position", smoke.box_min)}
                    {switch!(ui, "Open top", smoke.open_top)}
                </Fold>
                <Fold title="Emitter" key="fire.emitter" open=true>
                    {switch!(ui, "Emitting", smoke.emitter_on)}
                    {vec3!(ui, "Position", smoke.emitter_pos)}
                    {slider!(ui, "Radius", 0.02, 1.0, smoke.emitter_radius)}
                    {slider!(ui, "Stop after", 0, 30, smoke.emitter_duration, unit = " s")}
                    {vec3!(ui, "Velocity", smoke.emit_velocity)}
                    {slider!(ui, "Turbulence", 0, 5, smoke.turbulence)}
                    {slider!(ui, "Noise scale", 0.02, 1.0, smoke.noise_scale)}
                    {slider!(ui, "Flicker", 0, 10, smoke.flicker_speed)}
                    {slider!(ui, "Patchiness", 0, 1, smoke.emitter_patchiness)}
                    {slider!(ui, "Fuel", 0, 60, smoke.fuel_rate)}
                    {slider!(ui, "Heat", 0, 20, smoke.temp_rate)}
                    {slider!(ui, "Smoke", 0, 10, smoke.smoke_rate)}
                </Fold>
                <Fold title="Combustion" key="fire.combustion">
                    {slider!(ui, "Burn rate", 0, 20, smoke.burn_rate)}
                    {slider!(ui, "Heat release", 0, 6, smoke.heat_release)}
                    {slider!(ui, "Soot from burn", 0, 4, smoke.smoke_from_burn)}
                    {slider!(ui, "Ignition temp", 0, 2, smoke.ignition_temp)}
                    {slider!(ui, "Cooling", 0, 6, smoke.cooling)}
                    {slider!(ui, "Smoke fade", 0, 3, smoke.smoke_dissipation)}
                    {slider!(ui, "Fuel fade", 0, 6, smoke.fuel_dissipation)}
                </Fold>
                <Fold title="Forces" key="fire.forces">
                    {slider!(ui, "Buoyancy", 0, 15, smoke.buoyancy)}
                    {slider!(ui, "Smoke weight", 0, 3, smoke.smoke_weight)}
                    {slider!(ui, "Vorticity", 0, 30, smoke.vorticity)}
                    {slider!(ui, "Damping", 0, 2, smoke.velocity_damping)}
                    {vec3!(ui, "Wind", smoke.wind)}
                    {slider!(ui, "Pressure iterations", 4, 100, smoke.pressure_iters, int)}
                    {switch!(ui, "Sharp advection", smoke.sharp_advection)}
                </Fold>
                <Fold title="Shading" key="fire.shading">
                    {slider!(ui, "Flame intensity", 0, 40, smoke.flame_intensity)}
                    {slider!(ui, "Temperature scale", 0.05, 3.0, smoke.temperature_scale)}
                    {slider!(ui, "Flame detail", 0, 1, smoke.flame_detail)}
                    {slider!(ui, "Detail scale", 2, 40, smoke.detail_scale)}
                    {slider!(ui, "Detail speed", 0, 5, smoke.detail_speed, unit = " m/s")}
                    {slider!(ui, "Smoke density", 0, 100, smoke.smoke_absorption)}
                    {color!(ui, "Smoke colour", smoke.smoke_color)}
                    {slider!(ui, "Fire lights smoke", 0, 2, smoke.self_illumination)}
                    {slider!(ui, "Shadow density", 0, 4, smoke.shadow_density)}
                    {slider!(ui, "Quality (steps)", 32, 512, smoke.max_steps, int)}
                </Fold>
            </div>
        </Section>
    }
}

#[component]
fn Sparks(ui: Ui) -> impl IntoView {
    let on = Signal::derive(move || ui.scene.with(|s| s.sparks.enabled));
    let own_emitter = Signal::derive(move || ui.scene.with(|s| !s.sparks.use_fire_emitter));
    let presets = move || {
        SPARK_PRESETS
            .iter()
            .map(|&name| {
                view! {
                    <button class="seg" title="Replace the sparks & embers settings with this preset"
                        on:click=move |_| ui.edit(|s| if let Some(p) = SparkParams::preset(name) { s.sparks = p })>
                        {name}
                    </button>
                }
            })
            .collect_view()
    };
    view! {
        <Section title="Sparks & embers" key="sparks" accent="#ffb347">
            {switch!(ui, "Enabled", sparks.enabled)}
            <div class="group" class:off=move || !on.get()>
                <Fold title="Presets" key="sparks.presets" open=true>
                    <div class="preset-grid">{presets()}</div>
                    <Hint text="Presets replace the settings below; tweak from there." />
                </Fold>
                <Fold title="Emitter" key="sparks.emitter">
                    {switch!(ui, "Follow fire emitter", sparks.use_fire_emitter)}
                    <Show when=move || own_emitter.get()>
                        {vec3!(ui, "Position", sparks.emitter_pos)}
                        {slider!(ui, "Radius", 0, 1, sparks.emitter_radius)}
                    </Show>
                    {slider!(ui, "Stop after", 0, 30, sparks.duration, unit = " s")}
                    {number!(ui, "Max particles", sparks.max_particles, int)}
                </Fold>
                <Fold title="Sparks" key="sparks.sparks" open=true>
                    {switch!(ui, "Sparks", sparks.sparks)}
                    {slider!(ui, "Rate", 0, 1000, sparks.spark_rate, unit = " /s")}
                    {slider!(ui, "Burst size", 0, 300, sparks.burst_count, int)}
                    {slider!(ui, "Burst every", 0.1, 10.0, sparks.burst_interval, unit = " s")}
                    {vec3!(ui, "Direction", sparks.spark_direction)}
                    {slider!(ui, "Spread", 0, 180, sparks.spark_spread, unit = "\u{b0}")}
                    {slider!(ui, "Speed", 0, 20, sparks.spark_speed, unit = " m/s")}
                    {slider!(ui, "Speed variation", 0, 1, sparks.spark_speed_variation)}
                    {slider!(ui, "Lifetime", 0.1, 5.0, sparks.spark_lifetime, unit = " s")}
                    {slider!(ui, "Gravity", 0, 20, sparks.spark_gravity)}
                    {slider!(ui, "Air drag", 0, 5, sparks.spark_drag)}
                    {slider!(ui, "Bounce", 0, 1, sparks.spark_bounce)}
                    {slider!(ui, "Follow smoke", 0, 1, sparks.spark_follow_smoke)}
                    {slider!(ui, "Size", 0.001, 0.03, sparks.spark_size, unit = " m")}
                    {slider!(ui, "Streak", 0, 0.1, sparks.spark_streak, unit = " s")}
                    {slider!(ui, "Brightness", 0, 20, sparks.spark_brightness)}
                    {color!(ui, "Hot colour", sparks.spark_hot_color)}
                    {color!(ui, "Cool colour", sparks.spark_cool_color)}
                </Fold>
                <Fold title="Embers" key="sparks.embers" open=true>
                    {switch!(ui, "Embers", sparks.embers)}
                    {slider!(ui, "Rate", 0, 500, sparks.ember_rate, unit = " /s")}
                    {slider!(ui, "Lifetime", 0.2, 10.0, sparks.ember_lifetime, unit = " s")}
                    {slider!(ui, "Launch speed", 0, 3, sparks.ember_launch, unit = " m/s")}
                    {slider!(ui, "Buoyancy", 0, 2, sparks.ember_buoyancy)}
                    {slider!(ui, "Follow smoke", 0, 1, sparks.ember_follow_smoke)}
                    {slider!(ui, "Turbulence", 0, 4, sparks.ember_turbulence)}
                    {slider!(ui, "Size", 0.002, 0.05, sparks.ember_size, unit = " m")}
                    {slider!(ui, "Flicker", 0, 20, sparks.ember_flicker, unit = " /s")}
                    {slider!(ui, "Brightness", 0, 20, sparks.ember_brightness)}
                    {color!(ui, "Hot colour", sparks.ember_hot_color)}
                    {color!(ui, "Cool colour", sparks.ember_cool_color)}
                </Fold>
            </div>
        </Section>
    }
}

#[component]
fn Water(ui: Ui) -> impl IntoView {
    let on = Signal::derive(move || ui.scene.with(|s| s.water.enabled));
    let transfer = Signal::derive(move || ui.scene.with(|s| s.water.transfer));
    let look = Signal::derive(move || ui.scene.with(|s| s.water.look));
    view! {
        <Section title="Water" key="water" accent="#3d9bff">
            {switch!(ui, "Enabled", water.enabled)}
            <div class="group" class:off=move || !on.get()>
                <Fold title="Domain" key="water.domain">
                    {vec3!(ui, "Resolution", water.res, int)}
                    {number!(ui, "Cell size", water.cell_size, unit = " m")}
                    {vec3!(ui, "Position", water.box_min)}
                    {number!(ui, "Max particles", water.max_particles, int)}
                </Fold>
                <Fold title="Solver" key="water.solver">
                    <Choice
                        label="Transfer"
                        options=vec![(Transfer::Apic, "APIC"), (Transfer::Flip, "FLIP")]
                        value=transfer
                        on_change=Callback::new(move |t| ui.edit(|s| s.water.transfer = t))
                    />
                    <Show when=move || transfer.get() == Transfer::Flip>
                        {slider!(ui, "FLIP / PIC", 0, 1, water.flip_ratio)}
                    </Show>
                    {slider!(ui, "Gravity", 0, 30, water.gravity)}
                    {slider!(ui, "Pressure iterations", 4, 150, water.pressure_iters, int)}
                    {slider!(ui, "Volume keep", 0, 0.3, water.density_correction)}
                </Fold>
                <Fold title="Sources" key="water.sources" open=true>
                    <Sources ui=ui />
                </Fold>
                <Fold title="Whitewater" key="water.whitewater">
                    {switch!(ui, "Spray", water.spray_enabled)}
                    {slider!(ui, "Spray speed", 0.2, 10.0, water.spray_threshold, unit = " m/s")}
                    {slider!(ui, "Spray amount", 0, 0.5, water.spray_amount)}
                    {slider!(ui, "Air drag", 0, 5, water.spray_drag)}
                    {slider!(ui, "Lifetime", 0.1, 5.0, water.spray_lifetime, unit = " s")}
                    {slider!(ui, "Foam speed", 0.1, 8.0, water.foam_threshold)}
                    {slider!(ui, "Foam decay", 0, 8, water.foam_decay)}
                </Fold>
                <Fold title="Shading" key="water.shading">
                    <Choice
                        label="Look"
                        options=vec![(WaterLook::Surface, "Surface"), (WaterLook::Particles, "Particles")]
                        value=look
                        on_change=Callback::new(move |l| ui.edit(|s| s.water.look = l))
                    />
                    <Show when=move || look.get() == WaterLook::Surface>
                        {slider!(ui, "Smoothing passes", 0, 6, water.smoothing_passes, int)}
                        {slider!(ui, "Smoothing radius", 0.5, 8.0, water.smoothing_radius)}
                        {slider!(ui, "Depth blend", 0.5, 10.0, water.smoothing_depth)}
                        {slider!(ui, "Refraction", 0, 4, water.refraction)}
                        {slider!(ui, "Absorption", 0, 20, water.absorption)}
                        {slider!(ui, "Thickness scale", 0.1, 4.0, water.thickness_scale)}
                    </Show>
                    {color!(ui, "Deep colour", water.deep_color)}
                    {color!(ui, "Shallow colour", water.shallow_color)}
                    {color!(ui, "Foam colour", water.foam_color)}
                    {slider!(ui, "Foam strength", 0, 2, water.foam_strength)}
                    {slider!(ui, "Particle size", 0.3, 3.0, water.particle_size)}
                    {slider!(ui, "Spray size", 0.1, 2.0, water.spray_size)}
                    {slider!(ui, "Ripples", 0, 0.15, water.ripple_strength)}
                    {slider!(ui, "Ripple scale", 1, 20, water.ripple_scale)}
                </Fold>
            </div>
        </Section>
    }
}

/// Fill blocks and jets: a list that can grow and shrink.
#[component]
fn Sources(ui: Ui) -> impl IntoView {
    let blocks = move || (0..ui.scene.with(|s| s.water.blocks.len())).collect::<Vec<_>>();
    let jets = move || (0..ui.scene.with(|s| s.water.jets.len())).collect::<Vec<_>>();
    let block_vec =
        move |i: usize, f: fn(&WaterBlock) -> [f32; 3], set: fn(&mut WaterBlock, [f32; 3])| {
            (
                Signal::derive(move || {
                    ui.scene
                        .with(|s| s.water.blocks.get(i).map(f).unwrap_or_default())
                }),
                Callback::new(move |v| {
                    ui.edit(|s| {
                        if let Some(b) = s.water.blocks.get_mut(i) {
                            set(b, v)
                        }
                    })
                }),
            )
        };
    let jet_vec =
        move |i: usize, f: fn(&WaterJet) -> [f32; 3], set: fn(&mut WaterJet, [f32; 3])| {
            (
                Signal::derive(move || {
                    ui.scene
                        .with(|s| s.water.jets.get(i).map(f).unwrap_or_default())
                }),
                Callback::new(move |v| {
                    ui.edit(|s| {
                        if let Some(j) = s.water.jets.get_mut(i) {
                            set(j, v)
                        }
                    })
                }),
            )
        };
    let jet_f = move |i: usize, f: fn(&WaterJet) -> f32, set: fn(&mut WaterJet, f32)| {
        (
            Signal::derive(move || {
                ui.scene
                    .with(|s| s.water.jets.get(i).map(f).unwrap_or_default())
            }),
            Callback::new(move |v| {
                ui.edit(|s| {
                    if let Some(j) = s.water.jets.get_mut(i) {
                        set(j, v)
                    }
                })
            }),
        )
    };
    view! {
        <For each=blocks key=|i| *i let:i>
            {
                let (mn, set_mn) = block_vec(i, |b| b.min, |b, v| b.min = v);
                let (mx, set_mx) = block_vec(i, |b| b.max, |b, v| b.max = v);
                let (vel, set_vel) = block_vec(i, |b| b.velocity, |b, v| b.velocity = v);
                view! {
                    <div class="item">
                        <div class="item-head">
                            <span>{format!("Fill block {}", i + 1)}</span>
                            <button class="x" title="Remove" on:click=move |_| ui.edit(|s| { if i < s.water.blocks.len() { s.water.blocks.remove(i); } })>"\u{2715}"</button>
                        </div>
                        <Vec3Field label="Min" value=mn on_change=set_mn />
                        <Vec3Field label="Max" value=mx on_change=set_mx />
                        <Vec3Field label="Velocity" value=vel on_change=set_vel />
                    </div>
                }
            }
        </For>
        <For each=jets key=|i| *i let:i>
            {
                let (pos, set_pos) = jet_vec(i, |j| j.pos, |j, v| j.pos = v);
                let (vel, set_vel) = jet_vec(i, |j| j.velocity, |j, v| j.velocity = v);
                let (rad, set_rad) = jet_f(i, |j| j.radius, |j, v| j.radius = v);
                let (dur, set_dur) = jet_f(i, |j| j.duration, |j, v| j.duration = v);
                let enabled = Signal::derive(move || ui.scene.with(|s| s.water.jets.get(i).is_some_and(|j| j.enabled)));
                view! {
                    <div class="item">
                        <div class="item-head">
                            <button class="switch small" class:on=move || enabled.get() title="On / off"
                                on:click=move |_| ui.edit(|s| if let Some(j) = s.water.jets.get_mut(i) { j.enabled = !j.enabled })><i></i></button>
                            <span>{format!("Jet {}", i + 1)}</span>
                            <button class="x" title="Remove" on:click=move |_| ui.edit(|s| { if i < s.water.jets.len() { s.water.jets.remove(i); } })>"\u{2715}"</button>
                        </div>
                        <Vec3Field label="Position" value=pos on_change=set_pos />
                        <Slider label="Radius" min=0.01 max=0.4 value=rad on_change=set_rad />
                        <Vec3Field label="Velocity" value=vel on_change=set_vel />
                        <Slider label="Stop after" min=0.0 max=60.0 unit=" s" value=dur on_change=set_dur />
                    </div>
                }
            }
        </For>
        <div class="add-row">
            <button class="btn" on:click=move |_| ui.edit(|s| s.water.blocks.push(WaterBlock::default()))>"+ Block"</button>
            <button class="btn" on:click=move |_| ui.edit(|s| s.water.jets.push(WaterJet { enabled: true, ..Default::default() }))>"+ Jet"</button>
        </div>
        <Hint text="Blocks fill when the simulation resets; jets emit every frame." />
    }
}

#[component]
fn Ocean(ui: Ui) -> impl IntoView {
    let on = Signal::derive(move || ui.scene.with(|s| s.ocean.enabled));
    view! {
        <Section title="Ocean" key="ocean" accent="#2bb3c0" open=false>
            {switch!(ui, "Enabled", ocean.enabled)}
            <div class="group" class:off=move || !on.get()>
                <Fold title="Waves" key="ocean.waves" open=true>
                    <Hint text="An FFT ocean: a real wind-sea spectrum (JONSWAP) simulated in three detail layers. Seas steeper than height = wavelength / 7 don't exist, so raise the wavelength for bigger waves." />
                    {slider!(ui, "Sea level", -5, 5, ocean.level, unit = " m")}
                    {slider!(ui, "Wave height", 0, 8, ocean.wave_height, unit = " m")}
                    {slider!(ui, "Wavelength", 2, 120, ocean.wavelength, unit = " m")}
                    {slider!(ui, "Wind direction", 0, 360, ocean.wind_direction, unit = "\u{b0}")}
                    {slider!(ui, "Spread", 0, 2, ocean.spread)}
                    {slider!(ui, "Choppiness", 0, 1, ocean.choppiness)}
                    {slider!(ui, "Speed", 0, 3, ocean.speed)}
                </Fold>
                <Fold title="Detail" key="ocean.detail">
                    {slider!(ui, "Short waves", 0, 1, ocean.chop)}
                    {slider!(ui, "Ripples", 0, 2, ocean.ripples)}
                    {slider!(ui, "Whitecaps", 0, 1, ocean.whitecaps)}
                </Fold>
                <Fold title="Shading" key="ocean.shading">
                    {color!(ui, "Deep colour", ocean.deep_color)}
                    {color!(ui, "Crest glow", ocean.scatter_color)}
                    {color!(ui, "Foam colour", ocean.foam_color)}
                    {slider!(ui, "Haze distance", 50, 3000, ocean.haze_distance, unit = " m")}
                </Fold>
            </div>
        </Section>
    }
}

#[component]
fn Render(ui: Ui) -> impl IntoView {
    view! {
        <Section title="Render" key="render" open=false>
            {slider!(ui, "Exposure", 0.1, 4.0, render.exposure)}
            {vec3!(ui, "Light direction", render.light_dir)}
            {color!(ui, "Background", render.background)}
            {switch!(ui, "Sky background", render.sky_background)}
            {switch!(ui, "Transparent background", render.transparent_background)}
            {switch!(ui, "Ground", render.show_ground)}
            {switch!(ui, "Domain boxes", render.show_bounds)}
        </Section>
    }
}

#[component]
fn Camera(ui: Ui) -> impl IntoView {
    view! {
        <Section title="Camera" key="camera" open=false>
            {vec3!(ui, "Target", camera.target)}
            {number!(ui, "Distance", camera.distance, unit = " m")}
            {slider!(ui, "Field of view", 10, 100, camera.fov_deg, unit = "\u{b0}")}
            <Hint text="In the viewport: drag to orbit, right or middle drag to pan, wheel to zoom." />
        </Section>
    }
}
