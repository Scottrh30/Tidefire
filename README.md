# Tidefire

Real-time GPU fire, smoke and water VFX, in Rust. In the spirit of EmberGen, plus liquids.

The app is a Tauri window with a Leptos interface. The simulation engine is compiled into the interface's WebAssembly and runs on the GPU through WebGPU (WebView2 on Windows), the same way Forma3D runs its viewport. The Tauri side only handles native file dialogs and writing files.

## Milestone 1

- **Fire & smoke:** a GPU voxel solver. It handles emission with noise flicker, fuel, combustion (fuel → heat + soot), buoyancy, vorticity confinement, wind, an open or closed top and a red-black pressure projection. The volume is raymarched with filmic tonemapping, a blackbody-style flame ramp, light shadowing and smoke lit by the fire.
- **Realistic flames:** MacCormack (sharp) advection keeps flame edges crisp. Render-time flame detail breaks hot gas into licking tongues, thin bright sheets and dark gaps finer than the grid. A per-channel film tonemap makes bright fire saturate to orange-yellow (as on camera) rather than white. The look was tuned against a reference photo of a real fire.
- **Water:** GPU APIC liquid (Affine Particle-In-Cell, the default) on a staggered grid, with the original FLIP/PIC blend still selectable. Particle-to-grid transfer uses fixed-point atomics, with a free-surface pressure solve, volume preservation and RK2 advection. Sources are fill blocks and jets.
- **Water rendering:** a smooth screen-space surface. Particle depth is blurred with a bilateral, narrow-range filter, then shaded with refraction, Beer–Lambert absorption, Fresnel environment reflection, sun glints and foam. The old per-particle spheres remain available as the Particles look.
- **Whitewater:** foam on fast or agitated particles. Ballistic spray droplets are spawned from fast surface particles and get drag, a set lifetime and re-absorption into the water.
- **Sparks & embers:** GPU particles. Sparks are launched in a cone (continuously and in random bursts), fall, drag, bounce off the ground and cool from yellow-white to red, drawn as motion-blurred streaks. Embers are lifted by the fire's smoke currents with a swirling wander and flicker as they rise. There are presets (Campfire crackle, Bonfire embers, Grinder sparks, Forge, Firefly drift) plus the Bonfire, Forge and Grinder Sparks scenes.
- **Combined scenes:** fire and water run side by side, each in its own domain, and depth-composite correctly.
- **Live editing:** every parameter is editable while the sim runs. Orbit camera: drag to orbit, right/middle drag to pan, wheel to zoom. Keys: Space play/pause, `.` step, R reset, Ctrl+S save (Ctrl+Shift+S save as), Ctrl+O open.
- **Scene files:** scenes save and load as `.tidefire` (JSON).
- **Presets:** Campfire, Fireball, Dam Break, Fountain, Fire & Water.
- **Export to Reelforge:** PNG sequences at any resolution, optionally with transparent alpha. Each export also writes `<prefix>_sequence.json`, which records fps, frame count, size, alpha and the filename pattern.

## Run

Download `Tidefire.exe` (or `Tidefire-setup.exe`) from the **latest** release. It needs Windows 10/11 with WebView2, which both include, and a GPU with WebGPU support.

From source (needs the `wasm32-unknown-unknown` target and [Trunk](https://trunkrs.dev)):

```
npx @tauri-apps/cli@2 dev      # the app, with live reload of the interface
npx @tauri-apps/cli@2 build    # Tidefire.exe and its installer
```

Headless batch render with the command line tool (no window; also works on CI or a render box):

```
tidefire-cli --headless --preset "Fire & Water" --frames 120 --size 1920x1080 --out shots/fw --transparent --stats
tidefire-cli --headless --scene my_shot.tidefire --out shots/my_shot
```

## Visual tests

Every preset is rendered headless and compared with the reference frames in `tests/visual/`. The test checks the frame layout plus colour, brightness and texture statistics. These are tolerant of the sims' natural chaotic drift but catch real look regressions.

```
tidefire-cli --visual-test tests/visual            # compare; renders and diffs go to tests/visual/out/
tidefire-cli --visual-test tests/visual --update   # accept the current look as the new reference
```

`.github/workflows/visual-tests.yml` runs this on every push and pull request, on an Ubuntu runner using Mesa's software Vulkan driver. The results table appears on the run's summary page. When a test fails, the renders and diffs are uploaded for 3 days. After an intentional look change, run `--update` and commit the new references.

## Windows builds

`.github/workflows/windows.yml` builds on every push. For `main`, it publishes `Tidefire.exe`, `Tidefire-setup.exe` and `tidefire-cli.exe` as the **latest** release. A second job opens the interface in Chromium with software WebGPU and publishes screenshots plus the browser console as the **preview** release.

## Layout

| Path | What |
|---|---|
| `core/` | **tidefire-core**: the engine. Fire/smoke and water solvers, renderer, scene model and presets. Builds natively and for WebAssembly. |
| `core/src/shaders/` | WGSL: `smoke` (fire/smoke solver), `water` (APIC/FLIP, foam, spray), `volume` (raymarcher), `particles` and `fluid` (water surface), `scene` (ground, boxes), `present` (to the screen) |
| `app/` | **tidefire-app**: the Leptos interface and the WebGPU viewport (Trunk builds it to WebAssembly) |
| `src-tauri/` | **tidefire**: the Tauri shell (dialogs, scene files, exported frames) |
| `cli/` | **tidefire-cli**: headless renders and the visual tests |
| `tests/visual/` | Reference frames for the visual tests |
| `ci/preview.mjs` | The interface screenshots taken on CI |
