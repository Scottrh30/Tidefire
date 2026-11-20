# Tidefire

Real-time GPU fire, smoke and water VFX, in Rust. In the spirit of EmberGen, plus liquids.

The app is a Tauri window with a Leptos interface. The simulation engine is compiled into the interface's WebAssembly and runs on the GPU through WebGPU (WebView2 on Windows), the same way Forma3D runs its viewport. The Tauri side only handles native file dialogs and writing files.

## Milestone 1

- **Fire & smoke:** a GPU voxel solver. It handles emission with noise flicker, fuel, combustion (fuel → heat + soot), buoyancy, vorticity confinement, wind, an open or closed top and a red-black pressure projection. The volume is raymarched with filmic tonemapping, a blackbody-style flame ramp, light shadowing and smoke lit by the fire.
- **Realistic flames:** MacCormack (sharp) advection keeps flame edges crisp. Render-time flame detail breaks hot gas into licking tongues, thin bright sheets and dark gaps finer than the grid. A per-channel film tonemap makes bright fire saturate to orange-yellow (as on camera) rather than white. The look was tuned against a reference photo of a real fire.
- **Water:** GPU APIC liquid (Affine Particle-In-Cell, the default) on a staggered grid, with the original FLIP/PIC blend still selectable. Particle-to-grid transfer uses fixed-point atomics, with a free-surface pressure solve, volume preservation and RK2 advection. Sources are fill blocks and jets.
- **Water rendering:** a smooth screen-space surface. Particle depth is blurred with a bilateral, narrow-range filter, then shaded with refraction, Beer–Lambert absorption, Fresnel sky reflection, wind ripples, sun glints and patchy foam. The old per-particle spheres remain available as the Particles look.
- **Open ocean:** an FFT ocean (Tessendorf) out to the horizon, for wide shots no simulation grid could cover.
  - **Waves:** a JONSWAP wind-sea spectrum with direction-dependent spreading. It is evolved on the GPU and turned into displacement with inverse FFTs: about 196,000 wave components per frame, in three cascades (tiles of different sizes, each carrying one band of wavelengths). The tile sizes are irrational ratios apart, so the pattern never visibly repeats.
  - **Detail:** choppy horizontal displacement sharpens the crests. Capillary ripples (2–40 cm) give the surface its fine wrinkles.
  - **Foam:** builds where the surface folds (Jacobian) and fades in streaks.
  - **Filtering:** every layer is mipmapped and anisotropically filtered, so detail stays crisp up close and clean at the horizon.
  - **Settings:** significant wave height and peak wavelength are physical. The height is capped at wavelength / 7, the steepest real seas get. You also control wind direction, spread, choppiness, speed, short-wave energy, ripples, whitecaps and colours.
  - **Shading:** sky reflection, backlit crest glow, foam and aerial haze, tuned against a reference photo.
- **Whitewater:** foam on fast or agitated particles. Ballistic spray droplets are spawned from fast surface particles and get drag, a set lifetime and re-absorption into the water.
- **Sparks & embers:** GPU particles. Sparks are launched in a cone (continuously and in random bursts), fall, drag, bounce off the ground and cool from yellow-white to red, drawn as motion-blurred streaks. Embers are lifted by the fire's smoke currents with a swirling wander and flicker as they rise. There are presets (Campfire crackle, Bonfire embers, Grinder sparks, Forge, Firefly drift) plus the Bonfire, Forge and Grinder Sparks scenes.
- **Combined scenes:** fire and water run side by side, each in its own domain, and depth-composite correctly.
- **Live editing:** every parameter is editable while the sim runs. Orbit camera: drag to orbit, right/middle drag to pan, wheel to zoom. Keys: Space play/pause, `.` step, R reset, Ctrl+S save (Ctrl+Shift+S save as), Ctrl+O open.
- **Scene files:** scenes save and load as `.tidefire` (JSON).
- **Presets:** Campfire, Bonfire, Fireball, Forge, Grinder Sparks, Dam Break, Fountain, Fire & Water, Open Ocean.
- **Export** at any resolution, in three formats:
  - **PNG sequence** for Reelforge or any editor, optionally with transparent alpha. Each export also writes `<prefix>_sequence.json`, which records fps, frame count, size, alpha and the filename pattern.
  - **MP4 (H.264)** with a quality setting, playable anywhere. MP4 has no alpha, so the background is always rendered in.
  - **Apple ProRes .mov** in 422 Proxy, LT, 422, 422 HQ, 4444 or 4444 XQ. The 4444 profiles keep transparency.

  Video goes through a small ffmpeg (H.264 and ProRes only, about 4 MB) that is built into the Windows programs and unpacked on first use. Frames are piped to it while the next one renders. Elsewhere, ffmpeg is taken from `TIDEFIRE_FFMPEG`, from next to the program, or from the PATH. That ffmpeg includes x264, so it is GPL-licensed.

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
tidefire-cli --headless --preset Bonfire --frames 240 --format mp4 --quality 85 --out shots/bonfire.mp4
tidefire-cli --headless --preset Bonfire --frames 240 --prores-profile 4444 --transparent --out shots/bonfire.mov
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
| `core/src/shaders/` | WGSL: `smoke` (fire/smoke solver), `water` (APIC/FLIP, foam, spray), `volume` (raymarcher), `particles` and `fluid` (water surface), `ocean_fft` + `ocean_mips` (FFT ocean) and `ocean` (its surface), `scene` (ground, boxes), `present` (to the screen) |
| `app/` | **tidefire-app**: the Leptos interface and the WebGPU viewport (Trunk builds it to WebAssembly) |
| `src-tauri/` | **tidefire**: the Tauri shell (dialogs, scene files, exported frames, video encoding) |
| `cli/` | **tidefire-cli**: headless renders and the visual tests |
| `video/` | **tidefire-video**: video export, piping frames into ffmpeg (H.264 .mp4, ProRes .mov) |
| `ci/build-ffmpeg.sh` | Builds the small static ffmpeg embedded in the Windows programs |
| `tests/visual/` | Reference frames for the visual tests |
| `ci/preview.mjs` | The interface screenshots taken on CI |
