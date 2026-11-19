//! Visual regression tests: render every preset headless and compare the final
//! frame with a committed reference image.
//!
//!   tidefire --visual-test tests/visual            compare (exit 1 on failure)
//!   tidefire --visual-test tests/visual --update   re-generate the references
//!
//! The simulations are chaotic, so frames are never bit-identical across GPUs or
//! driver versions. Each frame is therefore checked two ways:
//!
//! 1. Layout: a blurred, downsampled comparison that ignores pixel-level turbulence
//!    but catches missing water or fire, black frames and sims that blow up.
//! 2. Look: statistics that chaotic drift barely moves but a change in appearance
//!    does: average colour of the content, its brightness distribution, and its
//!    fine texture (which catches e.g. the smooth water surface turning into
//!    beads).
//!
//! Limits were calibrated by nudging the physics up to 0.3% (worst drift: colour 0.9,
//! histogram 0.06, texture 4%) against deliberate breaks such as flames at 70%,
//! water absorption, foam strength, smoke colour and smoothing off (the smallest
//! was caught at colour 2.6, histogram 0.13, texture 20%).

use image::{RgbaImage, imageops};
use std::fmt::Write as _;
use std::path::Path;
use tidefire_core::engine::{self, Engine};
use tidefire_core::params;
use tidefire_core::render::RenderTarget;
use tidefire_core::wgpu;

const WIDTH: u32 = 320;
const HEIGHT: u32 = 200;
/// Comparison grid (after blurring) — coarse on purpose.
const CMP_W: u32 = 64;
const CMP_H: u32 = 40;
/// Mean absolute difference allowed (0–255 scale, RGB averaged).
const MAX_MEAN: f32 = 6.0;
/// 99th-percentile per-cell difference allowed.
const MAX_P99: f32 = 48.0;
/// Largest per-channel shift in the average colour of the content (0–255).
const MAX_COLOR: f32 = 2.0;
/// Earth mover's distance between 16-bin brightness histograms of the content.
const MAX_HIST: f32 = 0.10;
/// Relative change in fine texture (mean |Laplacian| of brightness).
const MAX_TEXTURE: f32 = 0.12;
/// Relative change in how much of the frame is covered by content.
const MAX_COVER: f32 = 0.15;

struct Case {
    preset: &'static str,
    frames: u32,
    /// Optional tweak on top of the preset, and the name the test goes by.
    tweak: Option<(&'static str, fn(&mut params::Scene))>,
}

/// The Bonfire sim with the flame and smoke made invisible, so the embers
/// (which ride the smoke) are all that's in the frame.
fn embers_only(s: &mut params::Scene) {
    s.smoke.flame_intensity = 0.0;
    s.smoke.smoke_absorption = 0.0;
    s.sparks.sparks = false;
}

impl Case {
    fn name(&self) -> &'static str {
        self.tweak.map(|t| t.0).unwrap_or(self.preset)
    }
}

const CASES: &[Case] = &[
    Case {
        preset: "Campfire",
        frames: 40,
        tweak: None,
    },
    Case {
        preset: "Fireball",
        frames: 24,
        tweak: None,
    },
    Case {
        preset: "Dam Break",
        frames: 24,
        tweak: None,
    },
    Case {
        preset: "Fountain",
        frames: 30,
        tweak: None,
    },
    Case {
        preset: "Fire & Water",
        frames: 40,
        tweak: None,
    },
    Case {
        preset: "Bonfire",
        frames: 45,
        tweak: None,
    },
    Case {
        preset: "Forge",
        frames: 40,
        tweak: None,
    },
    Case {
        preset: "Grinder Sparks",
        frames: 30,
        tweak: None,
    },
    Case {
        preset: "Bonfire",
        frames: 45,
        tweak: Some(("Embers only", embers_only)),
    },
    Case {
        preset: "Open Ocean",
        frames: 10,
        tweak: None,
    },
];

pub fn slug(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn render_case(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    case: &Case,
) -> Result<RgbaImage, String> {
    let mut scene =
        params::preset(case.preset).ok_or_else(|| format!("unknown preset {}", case.preset))?;
    scene.render.show_bounds = false;
    if let Some((_, tweak)) = case.tweak {
        tweak(&mut scene);
    }
    // TIDEFIRE_PERTURB=<eps> (default 1e-5) nudges the physics slightly to mimic another GPU/driver's
    // floating-point differences; used to check the thresholds tolerate normal drift.
    if let Some(v) = std::env::var_os("TIDEFIRE_PERTURB") {
        let eps: f32 = v.to_str().and_then(|t| t.parse().ok()).unwrap_or(1e-5);
        scene.water.gravity *= 1.0 + eps;
        scene.smoke.buoyancy *= 1.0 + eps;
        scene.smoke.vorticity *= 1.0 + eps;
    }
    let mut eng = Engine::new(device.clone(), queue.clone(), scene);
    let target = RenderTarget::new(&eng.device, WIDTH, HEIGHT);
    for _ in 0..case.frames {
        eng.step_frame();
    }
    eng.render(&target, true);
    Ok(engine::read_target(&eng.device, &eng.queue, &target))
}

fn reduce(img: &RgbaImage) -> RgbaImage {
    let small = imageops::resize(img, CMP_W * 2, CMP_H * 2, imageops::FilterType::Triangle);
    let blurred = imageops::blur(&small, 1.5);
    imageops::resize(&blurred, CMP_W, CMP_H, imageops::FilterType::Triangle)
}

struct Score {
    mean: f32,
    p99: f32,
    color: f32,
    hist: f32,
    texture: f32,
    cover: f32,
}

impl Score {
    fn failures(&self) -> Vec<String> {
        let mut f = Vec::new();
        let mut check = |name: &str, v: f32, max: f32| {
            if v > max {
                f.push(format!("{name} {v:.3} > {max}"));
            }
        };
        check("layout mean", self.mean, MAX_MEAN);
        check("layout p99", self.p99, MAX_P99);
        check("colour", self.color, MAX_COLOR);
        check("brightness histogram", self.hist, MAX_HIST);
        check("texture", self.texture, MAX_TEXTURE);
        check("coverage", self.cover, MAX_COVER);
        f
    }
}

struct Stats {
    cover: f32,
    rgb: [f32; 3],
    hist: [f32; 16],
    texture: f32,
}

fn luma(p: &image::Rgba<u8>) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

fn stats(img: &RgbaImage) -> Stats {
    let (w, h) = img.dimensions();
    // background = median of the top-left 8x8 corner
    let mut corner: Vec<[u8; 3]> = (0..8)
        .flat_map(|y| (0..8).map(move |x| (x, y)))
        .map(|(x, y)| {
            let p = img.get_pixel(x, y);
            [p[0], p[1], p[2]]
        })
        .collect();
    let mut bg = [0f32; 3];
    for (c, b) in bg.iter_mut().enumerate() {
        corner.sort_by_key(|p| p[c]);
        *b = corner[corner.len() / 2][c] as f32;
    }
    let is_content = |x: u32, y: u32| {
        let p = img.get_pixel(x, y);
        (0..3)
            .map(|c| (p[c] as f32 - bg[c]).abs())
            .fold(0.0, f32::max)
            > 12.0
    };
    let mut n = 0usize;
    let mut rgb = [0f64; 3];
    let mut hist = [0f32; 16];
    let mut tex = 0f64;
    let mut tex_n = 0usize;
    for y in 0..h {
        for x in 0..w {
            if !is_content(x, y) {
                continue;
            }
            let p = img.get_pixel(x, y);
            n += 1;
            for c in 0..3 {
                rgb[c] += p[c] as f64;
            }
            hist[((luma(p) / 16.0) as usize).min(15)] += 1.0;
            if x > 0 && y > 0 && x + 1 < w && y + 1 < h {
                let l = 4.0 * luma(p)
                    - luma(img.get_pixel(x - 1, y))
                    - luma(img.get_pixel(x + 1, y))
                    - luma(img.get_pixel(x, y - 1))
                    - luma(img.get_pixel(x, y + 1));
                tex += l.abs() as f64;
                tex_n += 1;
            }
        }
    }
    let nf = n.max(1) as f32;
    for b in hist.iter_mut() {
        *b /= nf;
    }
    Stats {
        cover: n as f32 / (w * h) as f32,
        rgb: rgb.map(|v| (v / n.max(1) as f64) as f32),
        hist,
        texture: (tex / tex_n.max(1) as f64) as f32,
    }
}

fn compare(a: &RgbaImage, b: &RgbaImage) -> Score {
    let (ra, rb) = (reduce(a), reduce(b));
    let mut diffs: Vec<f32> = ra
        .pixels()
        .zip(rb.pixels())
        .map(|(p, q)| {
            (0..3)
                .map(|c| (p[c] as f32 - q[c] as f32).abs())
                .sum::<f32>()
                / 3.0
        })
        .collect();
    let mean = diffs.iter().sum::<f32>() / diffs.len() as f32;
    diffs.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let p99 = diffs[((diffs.len() as f32 * 0.99) as usize).min(diffs.len() - 1)];
    let (sa, sb) = (stats(a), stats(b));
    let color = (0..3)
        .map(|c| (sa.rgb[c] - sb.rgb[c]).abs())
        .fold(0.0, f32::max);
    let mut ca = 0.0;
    let mut cb = 0.0;
    let mut hist = 0.0;
    for i in 0..16 {
        ca += sa.hist[i];
        cb += sb.hist[i];
        hist += (ca - cb).abs();
    }
    let rel = |x: f32, y: f32, floor: f32| (x - y).abs() / y.max(floor);
    Score {
        mean,
        p99,
        color,
        hist,
        texture: rel(sa.texture, sb.texture, 1.0),
        cover: rel(sa.cover, sb.cover, 0.01),
    }
}

fn diff_image(a: &RgbaImage, b: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(a.width(), a.height(), |x, y| {
        let (p, q) = (a.get_pixel(x, y), b.get_pixel(x, y));
        let d = (0..3)
            .map(|c| (p[c] as i32 - q[c] as i32).unsigned_abs())
            .max()
            .unwrap();
        let v = (d * 4).min(255) as u8;
        image::Rgba([v, v / 3, 0, 255])
    })
}

pub fn run(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    dir: &Path,
    update: bool,
) -> Result<bool, String> {
    let out = dir.join("out");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let gha = std::env::var_os("GITHUB_ACTIONS").is_some();
    let mut summary = String::from(
        "| Preset | Layout mean / p99 | Colour | Histogram | Texture | Result |\n|---|---|---|---|---|---|\n",
    );
    let mut all_ok = true;
    for case in CASES {
        let t0 = std::time::Instant::now();
        let img = render_case(device, queue, case)?;
        let name = slug(case.name());
        img.save(out.join(format!("{name}.png")))
            .map_err(|e| e.to_string())?;
        let ref_path = dir.join(format!("{name}.png"));
        if update {
            img.save(&ref_path).map_err(|e| e.to_string())?;
            eprintln!(
                "updated  {:<14} ({:.1}s)",
                case.name(),
                t0.elapsed().as_secs_f32()
            );
            continue;
        }
        let reference = match image::open(&ref_path) {
            Ok(r) => r.to_rgba8(),
            Err(e) => {
                all_ok = false;
                eprintln!("MISSING  {:<14} {}: {e}", case.name(), ref_path.display());
                if gha {
                    println!(
                        "::error title=Visual test::{}: missing reference {}",
                        case.name(),
                        ref_path.display()
                    );
                }
                let _ = writeln!(
                    summary,
                    "| {} | – | – | – | – | ❌ missing reference |",
                    case.name()
                );
                continue;
            }
        };
        if reference.dimensions() != img.dimensions() {
            return Err(format!(
                "{}: reference is {:?}, render is {:?}",
                case.name(),
                reference.dimensions(),
                img.dimensions()
            ));
        }
        let s = compare(&img, &reference);
        let failures = s.failures();
        let ok = failures.is_empty();
        all_ok &= ok;
        if !ok {
            diff_image(&img, &reference)
                .save(out.join(format!("{name}_diff.png")))
                .map_err(|e| e.to_string())?;
        }
        let line = format!(
            "layout {:.2}/{:.1}  colour {:.2}  hist {:.3}  texture {:.1}%  coverage {:.1}%",
            s.mean,
            s.p99,
            s.color,
            s.hist,
            s.texture * 100.0,
            s.cover * 100.0
        );
        eprintln!(
            "{}  {:<14} {line}  {:.1}s",
            if ok { "ok  " } else { "FAIL" },
            case.name(),
            t0.elapsed().as_secs_f32()
        );
        for f in &failures {
            eprintln!("        -> {f}");
        }
        if gha {
            let level = if ok { "notice" } else { "error" };
            let why = if ok {
                line.clone()
            } else {
                failures.join("; ")
            };
            println!("::{level} title=Visual test {}::{why}", case.name());
        }
        let _ = writeln!(
            summary,
            "| {} | {:.2} / {:.1} | {:.2} | {:.3} | {:.1}% | {} |",
            case.name(),
            s.mean,
            s.p99,
            s.color,
            s.hist,
            s.texture * 100.0,
            if ok {
                "✅".to_string()
            } else {
                format!("❌ {}", failures.join("; "))
            }
        );
    }
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let _ = std::fs::write(path, format!("## Tidefire visual tests\n\n{summary}"));
    }
    Ok(all_ok)
}
