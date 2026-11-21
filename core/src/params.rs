//! Scene settings (serialisable to .tidefire JSON) and built-in presets.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SmokeParams {
    pub enabled: bool,
    /// Grid resolution in cells. X and Z are rounded to multiples of 4.
    pub res: [u32; 3],
    /// World-space size of one voxel (metres).
    pub cell_size: f32,
    /// World position of the domain's minimum corner.
    pub box_min: [f32; 3],
    pub emitter_on: bool,
    /// Emitter centre in world space.
    pub emitter_pos: [f32; 3],
    pub emitter_radius: f32,
    /// Stop emitting after this many seconds (0 = forever).
    pub emitter_duration: f32,
    pub emit_velocity: [f32; 3],
    pub turbulence: f32,
    pub noise_scale: f32,
    pub flicker_speed: f32,
    /// 0 = fuel comes out evenly; 1 = in separate patches that grow separate flame tongues.
    pub emitter_patchiness: f32,
    pub fuel_rate: f32,
    pub temp_rate: f32,
    pub smoke_rate: f32,
    pub burn_rate: f32,
    pub heat_release: f32,
    pub smoke_from_burn: f32,
    pub ignition_temp: f32,
    pub cooling: f32,
    pub smoke_dissipation: f32,
    pub fuel_dissipation: f32,
    pub velocity_damping: f32,
    pub buoyancy: f32,
    pub smoke_weight: f32,
    pub vorticity: f32,
    pub wind: [f32; 3],
    pub open_top: bool,
    pub pressure_iters: u32,
    /// MacCormack advection: keeps flame edges and smoke detail sharp (a little slower).
    pub sharp_advection: bool,
    // shading
    pub flame_intensity: f32,
    pub temperature_scale: f32,
    pub smoke_color: [f32; 3],
    pub smoke_absorption: f32,
    pub self_illumination: f32,
    pub shadow_density: f32,
    pub fuel_glow: f32,
    pub step_scale: f32,
    pub max_steps: u32,
    /// Render-time detail that breaks flames into tongues and filaments finer than the grid.
    pub flame_detail: f32,
    /// Size of that detail (bigger = finer filaments).
    pub detail_scale: f32,
    /// How fast the detail rushes upward (m/s).
    pub detail_speed: f32,
}

impl Default for SmokeParams {
    fn default() -> Self {
        Self {
            enabled: true,
            res: [64, 96, 64],
            cell_size: 0.03125,
            box_min: [-1.0, 0.0, -1.0],
            emitter_on: true,
            emitter_pos: [0.0, 0.14, 0.0],
            emitter_radius: 0.5,
            emitter_duration: 0.0,
            emit_velocity: [0.0, 0.8, 0.0],
            turbulence: 2.2,
            noise_scale: 0.2,
            flicker_speed: 2.5,
            emitter_patchiness: 0.8,
            fuel_rate: 6.0,
            temp_rate: 2.0,
            smoke_rate: 0.0,
            burn_rate: 3.0,
            heat_release: 1.6,
            smoke_from_burn: 0.0,
            ignition_temp: 0.3,
            cooling: 3.0,
            smoke_dissipation: 1.2,
            fuel_dissipation: 1.0,
            velocity_damping: 0.05,
            buoyancy: 3.0,
            smoke_weight: 0.3,
            vorticity: 14.0,
            wind: [0.0, 0.0, 0.0],
            open_top: true,
            pressure_iters: 24,
            sharp_advection: true,
            flame_intensity: 16.0,
            temperature_scale: 0.3,
            smoke_color: [0.16, 0.16, 0.17],
            smoke_absorption: 2.0,
            self_illumination: 0.0,
            shadow_density: 1.0,
            fuel_glow: 0.0,
            step_scale: 1.0,
            max_steps: 320,
            flame_detail: 1.0,
            detail_scale: 20.0,
            detail_speed: 1.6,
        }
    }
}

impl SmokeParams {
    pub fn box_size(&self) -> [f32; 3] {
        let r = self.res;
        [
            r[0] as f32 * self.cell_size,
            r[1] as f32 * self.cell_size,
            r[2] as f32 * self.cell_size,
        ]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterBlock {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub velocity: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterJet {
    pub enabled: bool,
    pub pos: [f32; 3],
    pub radius: f32,
    pub velocity: [f32; 3],
    /// Stop emitting after this many seconds (0 = forever).
    pub duration: f32,
}

impl Default for WaterBlock {
    fn default() -> Self {
        Self {
            min: [-1.2, 0.05, -1.2],
            max: [-0.4, 1.0, 1.2],
            velocity: [0.0; 3],
        }
    }
}

impl Default for WaterJet {
    fn default() -> Self {
        Self {
            enabled: false,
            pos: [0.0, 0.2, 0.0],
            radius: 0.08,
            velocity: [0.0, 5.0, 0.0],
            duration: 0.0,
        }
    }
}

/// How velocity moves between particles and the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Transfer {
    /// Affine Particle-In-Cell: keeps rotation and detail without FLIP noise.
    #[default]
    Apic,
    /// FLIP/PIC blend (the original solver).
    Flip,
}

/// How the liquid is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WaterLook {
    /// Smooth screen-space surface with refraction, absorption and reflections.
    #[default]
    Surface,
    /// Every particle as a shaded sphere (debug / stylised look).
    Particles,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterParams {
    pub enabled: bool,
    pub res: [u32; 3],
    pub cell_size: f32,
    pub box_min: [f32; 3],
    pub max_particles: u32,
    pub blocks: Vec<WaterBlock>,
    pub jets: Vec<WaterJet>,
    pub transfer: Transfer,
    pub flip_ratio: f32,
    pub gravity: f32,
    pub pressure_iters: u32,
    pub density_correction: f32,
    // whitewater
    pub spray_enabled: bool,
    pub max_spray: u32,
    pub spray_threshold: f32,
    pub spray_amount: f32,
    pub spray_drag: f32,
    pub spray_lifetime: f32,
    pub foam_threshold: f32,
    pub foam_decay: f32,
    // shading
    pub look: WaterLook,
    pub smoothing_passes: u32,
    pub smoothing_radius: f32,
    pub smoothing_depth: f32,
    pub refraction: f32,
    pub absorption: f32,
    pub thickness_scale: f32,
    /// Fine wind ripples on the surface (normal detail finer than the particles).
    pub ripple_strength: f32,
    /// Ripples per metre (bigger = finer).
    pub ripple_scale: f32,
    pub deep_color: [f32; 3],
    pub shallow_color: [f32; 3],
    pub foam_color: [f32; 3],
    pub foam_strength: f32,
    pub particle_size: f32,
    pub spray_size: f32,
}

impl Default for WaterParams {
    fn default() -> Self {
        Self {
            enabled: true,
            res: [64, 40, 64],
            cell_size: 0.04,
            box_min: [-1.28, 0.0, -1.28],
            max_particles: 400_000,
            blocks: vec![WaterBlock::default()],
            jets: vec![],
            transfer: Transfer::Apic,
            flip_ratio: 0.96,
            gravity: 9.81,
            pressure_iters: 40,
            density_correction: 0.06,
            spray_enabled: true,
            max_spray: 120_000,
            spray_threshold: 2.4,
            spray_amount: 0.05,
            spray_drag: 0.6,
            spray_lifetime: 1.2,
            foam_threshold: 3.0,
            foam_decay: 1.8,
            look: WaterLook::Surface,
            smoothing_passes: 2,
            smoothing_radius: 2.5,
            smoothing_depth: 3.0,
            refraction: 1.0,
            absorption: 3.0,
            thickness_scale: 1.0,
            ripple_strength: 0.035,
            ripple_scale: 7.0,
            deep_color: [0.01, 0.05, 0.09],
            shallow_color: [0.04, 0.24, 0.32],
            foam_color: [0.9, 0.95, 1.0],
            foam_strength: 0.85,
            particle_size: 1.55,
            spray_size: 0.3,
        }
    }
}

/// Sparks (fast glowing streaks that fall, bounce and cool) and embers
/// (glowing flecks carried up by the fire's smoke).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SparkParams {
    pub enabled: bool,
    /// Most sparks and embers alive at once.
    pub max_particles: u32,
    // ---- where they come from
    /// Emit from the fire's emitter (follows its position and radius).
    pub use_fire_emitter: bool,
    pub emitter_pos: [f32; 3],
    pub emitter_radius: f32,
    /// Stop emitting after this many seconds (0 = forever).
    pub duration: f32,
    // ---- sparks
    pub sparks: bool,
    /// Sparks per second, emitted continuously.
    pub spark_rate: f32,
    /// Extra bursts of sparks ("pops"): how many per burst and the average gap between bursts.
    pub burst_count: u32,
    pub burst_interval: f32,
    pub spark_direction: [f32; 3],
    /// Cone half-angle around the direction, in degrees.
    pub spark_spread: f32,
    pub spark_speed: f32,
    /// 0 = every spark at full speed, 1 = anywhere from 0 to full.
    pub spark_speed_variation: f32,
    pub spark_lifetime: f32,
    pub spark_gravity: f32,
    pub spark_drag: f32,
    /// How much of their speed sparks keep when they hit the ground (0 = none).
    pub spark_bounce: f32,
    /// How strongly the smoke's currents push sparks around (0–1).
    pub spark_follow_smoke: f32,
    pub spark_size: f32,
    /// Streak length, as seconds of motion blur.
    pub spark_streak: f32,
    pub spark_brightness: f32,
    pub spark_hot_color: [f32; 3],
    pub spark_cool_color: [f32; 3],
    // ---- embers
    pub embers: bool,
    pub ember_rate: f32,
    pub ember_lifetime: f32,
    /// Initial upward speed.
    pub ember_launch: f32,
    /// Constant lift (m/s) on top of the smoke's currents.
    pub ember_buoyancy: f32,
    /// How strongly embers ride the smoke's currents (0–1).
    pub ember_follow_smoke: f32,
    /// Wandering, swirling motion.
    pub ember_turbulence: f32,
    pub ember_size: f32,
    /// Flickers per second.
    pub ember_flicker: f32,
    pub ember_brightness: f32,
    pub ember_hot_color: [f32; 3],
    pub ember_cool_color: [f32; 3],
}

impl Default for SparkParams {
    fn default() -> Self {
        let mut p = SparkParams::preset("Campfire crackle").unwrap();
        p.enabled = false;
        p
    }
}

pub const SPARK_PRESETS: &[&str] = &[
    "Campfire crackle",
    "Bonfire embers",
    "Grinder sparks",
    "Forge",
    "Firefly drift",
];

impl SparkParams {
    pub fn preset(name: &str) -> Option<Self> {
        let base = SparkParams {
            enabled: true,
            max_particles: 60_000,
            use_fire_emitter: true,
            emitter_pos: [0.0, 0.3, 0.0],
            emitter_radius: 0.2,
            duration: 0.0,
            sparks: true,
            spark_rate: 25.0,
            burst_count: 30,
            burst_interval: 1.4,
            spark_direction: [0.0, 1.0, 0.0],
            spark_spread: 35.0,
            spark_speed: 3.2,
            spark_speed_variation: 0.6,
            spark_lifetime: 1.1,
            spark_gravity: 3.0,
            spark_drag: 1.2,
            spark_bounce: 0.3,
            spark_follow_smoke: 0.4,
            spark_size: 0.004,
            spark_streak: 0.025,
            spark_brightness: 3.0,
            spark_hot_color: [1.0, 0.82, 0.45],
            spark_cool_color: [0.85, 0.18, 0.03],
            embers: true,
            ember_rate: 40.0,
            ember_lifetime: 3.0,
            ember_launch: 0.6,
            ember_buoyancy: 0.35,
            ember_follow_smoke: 0.8,
            ember_turbulence: 0.6,
            ember_size: 0.01,
            ember_flicker: 6.0,
            ember_brightness: 3.5,
            ember_hot_color: [1.0, 0.62, 0.2],
            ember_cool_color: [0.6, 0.08, 0.02],
        };
        let mut p = base;
        match name {
            "Campfire crackle" => {}
            "Bonfire embers" => {
                p.spark_rate = 10.0;
                p.burst_count = 18;
                p.burst_interval = 2.5;
                p.ember_rate = 160.0;
                p.ember_lifetime = 4.5;
                p.ember_buoyancy = 0.5;
                p.ember_turbulence = 1.0;
                p.ember_size = 0.011;
            }
            "Grinder sparks" => {
                p.use_fire_emitter = false;
                p.emitter_pos = [-0.6, 0.55, 0.0];
                p.emitter_radius = 0.015;
                p.spark_rate = 900.0;
                p.burst_count = 0;
                p.spark_direction = [1.0, 0.15, 0.0];
                p.spark_spread = 14.0;
                p.spark_speed = 7.0;
                p.spark_speed_variation = 0.55;
                p.spark_lifetime = 0.9;
                p.spark_gravity = 9.81;
                p.spark_drag = 0.6;
                p.spark_bounce = 0.45;
                p.spark_follow_smoke = 0.0;
                p.spark_size = 0.0025;
                p.spark_streak = 0.018;
                p.spark_brightness = 3.5;
                p.spark_hot_color = [1.0, 0.85, 0.5];
                p.spark_cool_color = [1.0, 0.35, 0.04];
                p.embers = false;
            }
            "Forge" => {
                p.spark_rate = 60.0;
                p.burst_count = 120;
                p.burst_interval = 0.9;
                p.spark_spread = 55.0;
                p.spark_speed = 4.5;
                p.spark_gravity = 6.0;
                p.spark_bounce = 0.5;
                p.ember_rate = 80.0;
                p.spark_hot_color = [1.0, 0.9, 0.65];
            }
            "Firefly drift" => {
                p.sparks = false;
                p.ember_rate = 25.0;
                p.ember_lifetime = 6.0;
                p.ember_launch = 0.2;
                p.ember_buoyancy = 0.12;
                p.ember_follow_smoke = 0.3;
                p.ember_turbulence = 1.6;
                p.ember_flicker = 1.5;
                p.ember_size = 0.012;
                p.ember_brightness = 4.0;
            }
            _ => return None,
        }
        Some(p)
    }
}

/// A procedural open-ocean surface out to the horizon (no simulation).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OceanParams {
    pub enabled: bool,
    /// Height of the calm sea surface (m).
    pub level: f32,
    /// Significant wave height (m): the average height of the biggest third of
    /// the waves. Capped at wavelength / 7, the steepest a real sea gets.
    pub wave_height: f32,
    /// Peak wavelength of the wind sea (m).
    pub wavelength: f32,
    /// Direction the waves travel, in degrees.
    pub wind_direction: f32,
    /// How widely the waves spread around the wind direction (0 = long parallel swell).
    pub spread: f32,
    /// Sharp crests and wide troughs (horizontal "choppy" displacement, 0..1).
    pub choppiness: f32,
    /// Wave speed multiplier (1 = real deep-water speed).
    pub speed: f32,
    /// Extra energy in the short waves (0 = spectrum as is).
    pub chop: f32,
    /// Capillary ripples, 2-40 cm wrinkles on the surface (0..2).
    pub ripples: f32,
    /// Unused since the FFT ocean; kept so older scene files still load.
    pub ripple_scale: f32,
    /// Sunlight scattered inside the water (0 = none, 1 = clear sea water).
    pub scattering: f32,
    /// How far light gets through the water (1 = clear open ocean, lower = murkier).
    pub clarity: f32,
    /// Waves shadowing each other from the sun (0..1).
    pub shadows: f32,
    /// Brightness of the sunlight on and in the water.
    pub sun_intensity: f32,
    /// Foam on the steepest crests (0–1).
    pub whitecaps: f32,
    pub deep_color: [f32; 3],
    /// Colour of light glowing through thin crests.
    pub scatter_color: [f32; 3],
    pub foam_color: [f32; 3],
    /// Distance at which the sea fades into the hazy horizon (m).
    pub haze_distance: f32,
}

impl Default for OceanParams {
    fn default() -> Self {
        Self {
            enabled: false,
            level: 0.0,
            wave_height: 1.4,
            wavelength: 11.0,
            wind_direction: 75.0,
            spread: 1.0,
            choppiness: 1.0,
            speed: 1.0,
            chop: 0.1,
            ripples: 1.0,
            ripple_scale: 3.5,
            whitecaps: 0.2,
            deep_color: [0.008, 0.035, 0.065],
            scatter_color: [0.02, 0.085, 0.14],
            foam_color: [0.9, 0.94, 0.97],
            haze_distance: 900.0,
            scattering: 1.0,
            clarity: 1.0,
            shadows: 1.0,
            sun_intensity: 3.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderParams {
    pub exposure: f32,
    pub light_dir: [f32; 3],
    pub background: [f32; 3],
    pub transparent_background: bool,
    /// Draw a daytime sky behind everything (instead of the plain background colour).
    pub sky_background: bool,
    pub show_ground: bool,
    pub show_bounds: bool,
}

impl Default for RenderParams {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            light_dir: [0.5, 0.8, 0.35],
            background: [0.006, 0.007, 0.009],
            transparent_background: false,
            sky_background: false,
            show_ground: true,
            show_bounds: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraParams {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_deg: f32,
}

impl Default for CameraParams {
    fn default() -> Self {
        Self {
            target: [0.0, 0.9, 0.0],
            yaw: 0.6,
            pitch: 0.28,
            distance: 4.6,
            fov_deg: 40.0,
        }
    }
}

pub use tidefire_video::{Codec, ProResProfile, Settings as VideoSettings};

/// What an export writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ExportFormat {
    /// Numbered PNGs plus a `_sequence.json` (keeps alpha).
    #[default]
    PngSequence,
    /// One H.264 .mp4 (no alpha).
    Mp4,
    /// One Apple ProRes .mov (4444 profiles keep alpha).
    ProRes,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportParams {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub start_frame: u32,
    pub prefix: String,
    pub format: ExportFormat,
    /// H.264 quality, 0..=100.
    pub mp4_quality: u8,
    pub prores_profile: ProResProfile,
}

impl ExportParams {
    /// The video codec, or `None` for a PNG sequence.
    pub fn codec(&self) -> Option<Codec> {
        match self.format {
            ExportFormat::PngSequence => None,
            ExportFormat::Mp4 => Some(Codec::H264 {
                quality: self.mp4_quality.min(100),
            }),
            ExportFormat::ProRes => Some(Codec::ProRes {
                profile: self.prores_profile,
            }),
        }
    }

    /// Whether the exported file can hold a transparent background.
    pub fn keeps_alpha(&self) -> bool {
        self.codec().map(|c| c.keeps_alpha()).unwrap_or(true)
    }
}

impl Default for ExportParams {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            frames: 120,
            start_frame: 0,
            prefix: "tidefire".into(),
            format: ExportFormat::PngSequence,
            mp4_quality: 80,
            prores_profile: ProResProfile::Hq,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub name: String,
    pub fps: f32,
    pub substeps: u32,
    pub smoke: SmokeParams,
    pub water: WaterParams,
    pub sparks: SparkParams,
    pub ocean: OceanParams,
    pub render: RenderParams,
    pub camera: CameraParams,
    pub export: ExportParams,
}

impl Default for Scene {
    fn default() -> Self {
        preset("Campfire").unwrap()
    }
}

pub const PRESETS: &[&str] = &[
    "Campfire",
    "Bonfire",
    "Fireball",
    "Forge",
    "Grinder Sparks",
    "Dam Break",
    "Fountain",
    "Fire & Water",
    "Open Ocean",
];

pub fn preset(name: &str) -> Option<Scene> {
    let base = |name: &str| Scene {
        name: name.to_string(),
        fps: 30.0,
        substeps: 2,
        smoke: SmokeParams::default(),
        water: WaterParams::default(),
        sparks: SparkParams::default(),
        ocean: OceanParams::default(),
        render: RenderParams::default(),
        camera: CameraParams::default(),
        export: ExportParams::default(),
    };
    let mut s = base(name);
    match name {
        "Campfire" => {
            s.water.enabled = false;
            s.camera.target = [0.0, 0.75, 0.0];
            s.camera.distance = 3.3;
            s.camera.pitch = 0.18;
        }
        "Bonfire" => {
            s.water.enabled = false;
            s.smoke.res = [72, 112, 72];
            s.smoke.box_min = [-1.125, 0.0, -1.125];
            s.smoke.emitter_pos = [0.0, 0.14, 0.0];
            s.smoke.emitter_radius = 0.6;
            s.smoke.fuel_rate = 7.5;
            s.smoke.cooling = 2.6;
            s.smoke.smoke_from_burn = 0.08;
            s.smoke.smoke_absorption = 6.0;
            s.sparks = SparkParams::preset("Bonfire embers").unwrap();
            s.camera.target = [0.0, 1.05, 0.0];
            s.camera.distance = 4.4;
            s.camera.pitch = 0.16;
        }
        "Forge" => {
            s.water.enabled = false;
            s.smoke.emitter_pos = [0.0, 0.12, 0.0];
            s.smoke.emitter_radius = 0.42;
            s.smoke.fuel_rate = 9.0;
            s.smoke.cooling = 2.4;
            s.sparks = SparkParams::preset("Forge").unwrap();
            s.camera.target = [0.0, 0.7, 0.0];
            s.camera.distance = 3.6;
            s.camera.pitch = 0.2;
        }
        "Grinder Sparks" => {
            s.water.enabled = false;
            s.smoke.enabled = false;
            s.sparks = SparkParams::preset("Grinder sparks").unwrap();
            s.camera.target = [0.2, 0.35, 0.0];
            s.camera.distance = 3.0;
            s.camera.yaw = 0.35;
            s.camera.pitch = 0.22;
        }
        "Open Ocean" => {
            s.smoke.enabled = false;
            s.water.enabled = false;
            s.ocean.enabled = true;
            s.render.show_ground = false;
            s.render.show_bounds = false;
            s.render.sky_background = true;
            s.render.light_dir = [0.35, 0.5, -0.7];
            s.render.exposure = 1.5;
            s.camera.target = [0.0, 3.4, -12.0];
            s.camera.distance = 12.0;
            s.camera.pitch = 0.04;
            s.camera.yaw = 0.0;
            s.camera.fov_deg = 68.0;
        }
        "Fireball" => {
            s.water.enabled = false;
            s.fps = 30.0;
            s.substeps = 3;
            s.smoke.res = [96, 112, 96];
            s.smoke.cell_size = 0.035;
            s.smoke.box_min = [-1.68, 0.0, -1.68];
            s.smoke.emitter_pos = [0.0, 0.55, 0.0];
            s.smoke.emitter_radius = 0.45;
            s.smoke.emitter_duration = 0.45;
            s.smoke.emit_velocity = [0.0, 3.5, 0.0];
            s.smoke.fuel_rate = 60.0;
            s.smoke.temp_rate = 12.0;
            s.smoke.smoke_rate = 0.6;
            s.smoke.burn_rate = 5.0;
            s.smoke.heat_release = 2.4;
            s.smoke.smoke_from_burn = 0.9;
            s.smoke.fuel_dissipation = 0.4;
            s.smoke.turbulence = 3.5;
            s.smoke.noise_scale = 0.12;
            s.smoke.vorticity = 14.0;
            s.smoke.buoyancy = 4.0;
            s.smoke.cooling = 0.9;
            s.smoke.smoke_dissipation = 0.15;
            s.smoke.flame_intensity = 14.0;
            s.smoke.temperature_scale = 0.5;
            s.smoke.smoke_absorption = 14.0;
            s.smoke.smoke_color = [0.1, 0.1, 0.11];
            s.smoke.self_illumination = 0.12;
            s.smoke.emitter_patchiness = 0.3;
            s.smoke.flame_detail = 0.7;
            s.camera.target = [0.0, 1.9, 0.0];
            s.camera.distance = 7.4;
            s.camera.pitch = 0.12;
        }
        "Dam Break" => {
            s.smoke.enabled = false;
            s.water.blocks = vec![WaterBlock {
                min: [-1.2, 0.04, -1.2],
                max: [-0.35, 1.1, 1.2],
                velocity: [0.0; 3],
            }];
            s.camera.target = [0.0, 0.4, 0.0];
            s.camera.distance = 4.2;
        }
        "Fountain" => {
            s.smoke.enabled = false;
            s.water.blocks = vec![WaterBlock {
                min: [-1.2, 0.04, -1.2],
                max: [1.2, 0.22, 1.2],
                velocity: [0.0; 3],
            }];
            s.water.jets = vec![WaterJet {
                enabled: true,
                pos: [0.0, 0.3, 0.0],
                radius: 0.07,
                velocity: [0.0, 5.4, 0.0],
                duration: 0.0,
            }];
            s.water.res = [64, 56, 64];
            s.camera.target = [0.0, 0.7, 0.0];
            s.camera.distance = 4.6;
        }
        "Fire & Water" => {
            s.smoke.box_min = [-0.4, 0.0, -1.0];
            s.smoke.res = [48, 96, 64];
            s.smoke.emitter_pos = [0.35, 0.2, 0.0];
            s.smoke.emitter_radius = 0.3;
            s.smoke.fuel_rate = 9.0;
            s.smoke.cooling = 1.9;
            s.water.res = [64, 40, 64];
            s.water.box_min = [-1.28, 0.0, -1.28];
            s.water.blocks = vec![WaterBlock {
                min: [-1.2, 0.04, -1.2],
                max: [1.2, 0.18, 1.2],
                velocity: [0.0; 3],
            }];
            s.water.jets = vec![WaterJet {
                enabled: true,
                pos: [-0.9, 0.35, 0.0],
                radius: 0.06,
                velocity: [2.6, 3.4, 0.0],
                duration: 0.0,
            }];
            s.camera.target = [0.0, 0.8, 0.0];
            s.camera.distance = 5.2;
        }
        _ => return None,
    }
    Some(s)
}
