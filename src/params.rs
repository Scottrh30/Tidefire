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
}

impl Default for SmokeParams {
    fn default() -> Self {
        Self {
            enabled: true,
            res: [64, 96, 64],
            cell_size: 0.03125,
            box_min: [-1.0, 0.0, -1.0],
            emitter_on: true,
            emitter_pos: [0.0, 0.25, 0.0],
            emitter_radius: 0.28,
            emitter_duration: 0.0,
            emit_velocity: [0.0, 0.8, 0.0],
            turbulence: 1.0,
            noise_scale: 0.18,
            flicker_speed: 2.5,
            fuel_rate: 6.0,
            temp_rate: 2.0,
            smoke_rate: 0.08,
            burn_rate: 3.0,
            heat_release: 1.6,
            smoke_from_burn: 0.3,
            ignition_temp: 0.3,
            cooling: 1.0,
            smoke_dissipation: 0.4,
            fuel_dissipation: 1.0,
            velocity_damping: 0.05,
            buoyancy: 3.5,
            smoke_weight: 0.3,
            vorticity: 6.0,
            wind: [0.0, 0.0, 0.0],
            open_top: true,
            pressure_iters: 24,
            flame_intensity: 9.0,
            temperature_scale: 0.45,
            smoke_color: [0.16, 0.16, 0.17],
            smoke_absorption: 14.0,
            self_illumination: 0.25,
            shadow_density: 1.0,
            fuel_glow: 0.0,
            step_scale: 1.0,
            max_steps: 160,
        }
    }
}

impl SmokeParams {
    pub fn box_size(&self) -> [f32; 3] {
        let r = self.res;
        [r[0] as f32 * self.cell_size, r[1] as f32 * self.cell_size, r[2] as f32 * self.cell_size]
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
        Self { min: [-1.2, 0.05, -1.2], max: [-0.4, 1.0, 1.2], velocity: [0.0; 3] }
    }
}

impl Default for WaterJet {
    fn default() -> Self {
        Self { enabled: false, pos: [0.0, 0.2, 0.0], radius: 0.08, velocity: [0.0, 5.0, 0.0], duration: 0.0 }
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
            deep_color: [0.008, 0.06, 0.15],
            shallow_color: [0.03, 0.26, 0.44],
            foam_color: [0.9, 0.95, 1.0],
            foam_strength: 0.85,
            particle_size: 1.55,
            spray_size: 0.55,
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
        Self { target: [0.0, 0.9, 0.0], yaw: 0.6, pitch: 0.28, distance: 4.6, fov_deg: 40.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportParams {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub start_frame: u32,
    pub prefix: String,
}

impl Default for ExportParams {
    fn default() -> Self {
        Self { width: 1920, height: 1080, frames: 120, start_frame: 0, prefix: "tidefire".into() }
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
    pub render: RenderParams,
    pub camera: CameraParams,
    pub export: ExportParams,
}

impl Default for Scene {
    fn default() -> Self {
        preset("Campfire").unwrap()
    }
}

pub const PRESETS: &[&str] = &["Campfire", "Fireball", "Dam Break", "Fountain", "Fire & Water"];

pub fn preset(name: &str) -> Option<Scene> {
    let base = |name: &str| Scene {
        name: name.to_string(),
        fps: 30.0,
        substeps: 2,
        smoke: SmokeParams::default(),
        water: WaterParams::default(),
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
            s.smoke.flame_intensity = 6.5;
            s.camera.target = [0.0, 1.9, 0.0];
            s.camera.distance = 7.4;
            s.camera.pitch = 0.12;
        }
        "Dam Break" => {
            s.smoke.enabled = false;
            s.water.blocks = vec![WaterBlock { min: [-1.2, 0.04, -1.2], max: [-0.35, 1.1, 1.2], velocity: [0.0; 3] }];
            s.camera.target = [0.0, 0.4, 0.0];
            s.camera.distance = 4.2;
        }
        "Fountain" => {
            s.smoke.enabled = false;
            s.water.blocks = vec![WaterBlock { min: [-1.2, 0.04, -1.2], max: [1.2, 0.22, 1.2], velocity: [0.0; 3] }];
            s.water.jets = vec![WaterJet { enabled: true, pos: [0.0, 0.3, 0.0], radius: 0.07, velocity: [0.0, 5.4, 0.0], duration: 0.0 }];
            s.water.res = [64, 56, 64];
            s.camera.target = [0.0, 0.7, 0.0];
            s.camera.distance = 4.6;
        }
        "Fire & Water" => {
            s.smoke.box_min = [-0.4, 0.0, -1.0];
            s.smoke.res = [48, 96, 64];
            s.smoke.emitter_pos = [0.35, 0.25, 0.0];
            s.smoke.emitter_radius = 0.25;
            s.water.res = [64, 40, 64];
            s.water.box_min = [-1.28, 0.0, -1.28];
            s.water.blocks = vec![WaterBlock { min: [-1.2, 0.04, -1.2], max: [1.2, 0.18, 1.2], velocity: [0.0; 3] }];
            s.water.jets = vec![WaterJet { enabled: true, pos: [-0.9, 0.35, 0.0], radius: 0.06, velocity: [2.6, 3.4, 0.0], duration: 0.0 }];
            s.camera.target = [0.0, 0.8, 0.0];
            s.camera.distance = 5.2;
        }
        _ => return None,
    }
    Some(s)
}
