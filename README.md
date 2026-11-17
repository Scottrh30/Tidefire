# Tidefire

Real-time GPU fire, smoke and water VFX, in Rust (egui + wgpu). In the spirit of EmberGen, plus liquids.

## Milestone 1

- **Fire & smoke:** a GPU voxel solver. It handles emission with noise flicker, fuel, combustion (fuel → heat + soot), buoyancy, vorticity confinement, wind, an open or closed top and a red-black pressure projection. The volume is raymarched with filmic tonemapping, a blackbody-style flame ramp, light shadowing and smoke lit by the fire.
- **Water:** GPU APIC liquid (Affine Particle-In-Cell, the default) on a staggered grid, with the original FLIP/PIC blend still selectable. Particle-to-grid transfer uses fixed-point atomics, with a free-surface pressure solve, volume preservation and RK2 advection. Sources are fill blocks and jets.
- **Water rendering:** a smooth screen-space surface. Particle depth is blurred with a bilateral, narrow-range filter, then shaded with refraction, Beer–Lambert absorption, Fresnel environment reflection, sun glints and foam. The old per-particle spheres remain available as the Particles look.
- **Whitewater:** foam on fast or agitated particles. Ballistic spray droplets are spawned from fast surface particles and get drag, a set lifetime and re-absorption into the water.
- **Combined scenes:** fire and water run side by side, each in its own domain, and depth-composite correctly.
- **Live editing:** every parameter is editable while the sim runs. Orbit camera: drag to orbit, right/middle drag to pan, wheel to zoom.
- **Scene files:** scenes save and load as `.tidefire` (JSON).
- **Presets:** Campfire, Fireball, Dam Break, Fountain, Fire & Water.
- **Export to Reelforge:** PNG sequences at any resolution, optionally with transparent alpha. Each export also writes `<prefix>_sequence.json`, which records fps, frame count, size, alpha and the filename pattern.

## Run

```
cargo run --release
```

Headless batch render (no window). It also works on CI or a render box:

```
tidefire --headless --preset "Fire & Water" --frames 120 --size 1920x1080 --out shots/fw --transparent --stats
tidefire --headless --scene my_shot.tidefire --out shots/my_shot
```

## Windows .exe

`.github/workflows/windows.yml` builds `tidefire-windows-x64.zip` on every push to `main`, and attaches it to the GitHub release when you push a `v*` tag.

## Layout

| File | What |
|---|---|
| `src/shaders/smoke.wgsl` | Fire/smoke solver kernels |
| `src/shaders/water.wgsl` | FLIP, foam and spray kernels |
| `src/shaders/volume.wgsl` | Volume raymarcher |
| `src/shaders/particles.wgsl` | Water/spray sphere impostors, plus the fluid depth and thickness passes |
| `src/shaders/fluid.wgsl` | Screen-space fluid depth smoothing and surface shading |
| `src/shaders/scene.wgsl` | Ground and domain boxes |
| `src/smoke.rs`, `src/water.rs` | GPU resources and dispatch per solver |
| `src/render.rs` | Offscreen renderer (shared by the viewport and export) |
| `src/engine.rs` | Frame stepping and readback |
| `src/app.rs` | egui UI |
| `src/params.rs` | Scene model and presets |
