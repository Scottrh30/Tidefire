//! egui front-end: viewport, parameter panels, transport, scene files and export.

use crate::engine::{self, Engine};
use crate::gpu::wgpu;
use crate::params::{self, Scene, WaterBlock, WaterJet};
use crate::render::RenderTarget;
use eframe::egui::{self, Color32, DragValue, Slider};
use std::path::PathBuf;

struct Export {
    dir: PathBuf,
    target: RenderTarget,
    done: u32,
    total: u32,
    writers: Vec<std::thread::JoinHandle<()>>,
}

pub struct TidefireApp {
    engine: Engine,
    playing: bool,
    viewport: Option<(RenderTarget, egui::TextureId)>,
    export: Option<Export>,
    export_dir: Option<PathBuf>,
    reset_before_export: bool,
    show_export: bool,
    status: String,
    scene_path: Option<PathBuf>,
}

impl TidefireApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let rs = cc.wgpu_render_state.as_ref().expect("Tidefire needs the wgpu renderer");
        let mut style = (*cc.egui_ctx.global_style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(24, 25, 29);
        style.visuals.window_fill = Color32::from_rgb(30, 31, 36);
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        cc.egui_ctx.set_global_style(style);
        let engine = Engine::new(rs.device.clone(), rs.queue.clone(), Scene::default());
        Self {
            engine,
            playing: true,
            viewport: None,
            export: None,
            export_dir: None,
            reset_before_export: true,
            show_export: false,
            status: "Ready".into(),
            scene_path: None,
        }
    }

    fn load_preset(&mut self, name: &str) {
        if let Some(s) = params::preset(name) {
            self.engine.scene = s;
            self.engine.reset();
            self.status = format!("Loaded preset \"{name}\"");
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                ui.menu_button("New from preset", |ui| {
                    for p in params::PRESETS {
                        if ui.button(*p).clicked() {
                            self.load_preset(p);
                            ui.close();
                        }
                    }
                });
                if ui.button("Open scene...").clicked() {
                    ui.close();
                    if let Some(path) = rfd::FileDialog::new().add_filter("Tidefire scene", &["tidefire", "json"]).pick_file() {
                        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Scene>(&t).map_err(|e| e.to_string())) {
                            Ok(s) => {
                                self.engine.scene = s;
                                self.engine.reset();
                                self.status = format!("Opened {}", path.display());
                                self.scene_path = Some(path);
                            }
                            Err(e) => self.status = format!("Could not open scene: {e}"),
                        }
                    }
                }
                if ui.button("Save scene...").clicked() {
                    ui.close();
                    let dlg = rfd::FileDialog::new().add_filter("Tidefire scene", &["tidefire"]).set_file_name(format!("{}.tidefire", self.engine.scene.name));
                    if let Some(path) = dlg.save_file() {
                        let text = serde_json::to_string_pretty(&self.engine.scene).unwrap();
                        match std::fs::write(&path, text) {
                            Ok(_) => self.status = format!("Saved {}", path.display()),
                            Err(e) => self.status = format!("Save failed: {e}"),
                        }
                        self.scene_path = Some(path);
                    }
                }
                ui.separator();
                if ui.button("Export PNG sequence...").clicked() {
                    self.show_export = true;
                    ui.close();
                }
            });
            ui.separator();
            let label = if self.playing { "Pause" } else { "Play" };
            if ui.button(label).clicked() {
                self.playing = !self.playing;
            }
            if ui.button("Step").clicked() {
                self.playing = false;
                self.engine.step_frame();
            }
            if ui.button("Reset").clicked() {
                self.engine.reset();
            }
            ui.separator();
            ui.label(format!("Frame {}   {:.2}s", self.engine.frame, self.engine.time));
            ui.separator();
            ui.label(format!("sim {:.1} ms", self.engine.last_step_ms));
            if self.engine.water.is_some() {
                ui.separator();
                ui.label(format!("{} particles", self.engine.particle_count()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Export...").clicked() {
                    self.show_export = true;
                }
                ui.weak(&self.status);
            });
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.engine.scene;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading(s.name.clone());
            ui.add_space(4.0);
            egui::CollapsingHeader::new("Simulation").default_open(true).show(ui, |ui| {
                grid(ui, "sim", |ui| {
                    row(ui, "Frame rate", DragValue::new(&mut s.fps).range(1.0..=240.0).suffix(" fps"));
                    row(ui, "Substeps", DragValue::new(&mut s.substeps).range(1..=16));
                });
            });

            egui::CollapsingHeader::new("Fire & smoke").default_open(true).show(ui, |ui| {
                let p = &mut s.smoke;
                ui.checkbox(&mut p.enabled, "Enabled");
                ui.add_enabled_ui(p.enabled, |ui| {
                    ui.label(egui::RichText::new("Domain").strong());
                    grid(ui, "smoke_dom", |ui| {
                        ui.label("Resolution");
                        ui.horizontal(|ui| {
                            for r in p.res.iter_mut() {
                                ui.add(DragValue::new(r).range(8..=384));
                            }
                        });
                        ui.end_row();
                        row(ui, "Voxel size", DragValue::new(&mut p.cell_size).range(0.005..=0.2).speed(0.001).suffix(" m"));
                        vec3_row(ui, "Position", &mut p.box_min, 0.01);
                        ui.label("Open top");
                        ui.checkbox(&mut p.open_top, "");
                        ui.end_row();
                    });
                    ui.label(egui::RichText::new("Emitter").strong());
                    grid(ui, "smoke_emit", |ui| {
                        ui.label("Emitting");
                        ui.checkbox(&mut p.emitter_on, "");
                        ui.end_row();
                        vec3_row(ui, "Position", &mut p.emitter_pos, 0.01);
                        row(ui, "Radius", Slider::new(&mut p.emitter_radius, 0.02..=1.0));
                        row(ui, "Stop after", DragValue::new(&mut p.emitter_duration).range(0.0..=600.0).speed(0.01).suffix(" s"));
                        vec3_row(ui, "Velocity", &mut p.emit_velocity, 0.02);
                        row(ui, "Turbulence", Slider::new(&mut p.turbulence, 0.0..=5.0));
                        row(ui, "Noise scale", Slider::new(&mut p.noise_scale, 0.02..=1.0));
                        row(ui, "Flicker", Slider::new(&mut p.flicker_speed, 0.0..=10.0));
                        row(ui, "Fuel", Slider::new(&mut p.fuel_rate, 0.0..=60.0));
                        row(ui, "Heat", Slider::new(&mut p.temp_rate, 0.0..=20.0));
                        row(ui, "Smoke", Slider::new(&mut p.smoke_rate, 0.0..=10.0));
                    });
                    ui.label(egui::RichText::new("Combustion").strong());
                    grid(ui, "smoke_comb", |ui| {
                        row(ui, "Burn rate", Slider::new(&mut p.burn_rate, 0.0..=20.0));
                        row(ui, "Heat release", Slider::new(&mut p.heat_release, 0.0..=6.0));
                        row(ui, "Soot from burn", Slider::new(&mut p.smoke_from_burn, 0.0..=4.0));
                        row(ui, "Ignition temp", Slider::new(&mut p.ignition_temp, 0.0..=2.0));
                        row(ui, "Cooling", Slider::new(&mut p.cooling, 0.0..=6.0));
                        row(ui, "Smoke fade", Slider::new(&mut p.smoke_dissipation, 0.0..=3.0));
                        row(ui, "Fuel fade", Slider::new(&mut p.fuel_dissipation, 0.0..=6.0));
                    });
                    ui.label(egui::RichText::new("Forces").strong());
                    grid(ui, "smoke_forces", |ui| {
                        row(ui, "Buoyancy", Slider::new(&mut p.buoyancy, 0.0..=15.0));
                        row(ui, "Smoke weight", Slider::new(&mut p.smoke_weight, 0.0..=3.0));
                        row(ui, "Vorticity", Slider::new(&mut p.vorticity, 0.0..=30.0));
                        row(ui, "Damping", Slider::new(&mut p.velocity_damping, 0.0..=2.0));
                        vec3_row(ui, "Wind", &mut p.wind, 0.02);
                        row(ui, "Pressure iters", Slider::new(&mut p.pressure_iters, 4..=100));
                    });
                    ui.label(egui::RichText::new("Shading").strong());
                    grid(ui, "smoke_shade", |ui| {
                        row(ui, "Flame intensity", Slider::new(&mut p.flame_intensity, 0.0..=30.0));
                        row(ui, "Temperature scale", Slider::new(&mut p.temperature_scale, 0.05..=3.0));
                        row(ui, "Smoke density", Slider::new(&mut p.smoke_absorption, 0.0..=100.0));
                        ui.label("Smoke colour");
                        ui.color_edit_button_rgb(&mut p.smoke_color);
                        ui.end_row();
                        row(ui, "Fire lights smoke", Slider::new(&mut p.self_illumination, 0.0..=2.0));
                        row(ui, "Shadow density", Slider::new(&mut p.shadow_density, 0.0..=4.0));
                        row(ui, "Quality (steps)", Slider::new(&mut p.max_steps, 32..=512));
                    });
                });
            });

            egui::CollapsingHeader::new("Water").default_open(true).show(ui, |ui| {
                let p = &mut s.water;
                ui.checkbox(&mut p.enabled, "Enabled");
                ui.add_enabled_ui(p.enabled, |ui| {
                    ui.label(egui::RichText::new("Domain").strong());
                    grid(ui, "water_dom", |ui| {
                        ui.label("Resolution");
                        ui.horizontal(|ui| {
                            for r in p.res.iter_mut() {
                                ui.add(DragValue::new(r).range(8..=256));
                            }
                        });
                        ui.end_row();
                        row(ui, "Cell size", DragValue::new(&mut p.cell_size).range(0.005..=0.2).speed(0.001).suffix(" m"));
                        vec3_row(ui, "Position", &mut p.box_min, 0.01);
                        row(ui, "Max particles", DragValue::new(&mut p.max_particles).range(1024..=4_000_000).speed(1000));
                    });
                    ui.label(egui::RichText::new("Solver").strong());
                    grid(ui, "water_solver", |ui| {
                        ui.label("Transfer");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut p.transfer, params::Transfer::Apic, "APIC");
                            ui.selectable_value(&mut p.transfer, params::Transfer::Flip, "FLIP");
                        });
                        ui.end_row();
                        if p.transfer == params::Transfer::Flip {
                            row(ui, "FLIP / PIC", Slider::new(&mut p.flip_ratio, 0.0..=1.0));
                        }
                        row(ui, "Gravity", Slider::new(&mut p.gravity, 0.0..=30.0));
                        row(ui, "Pressure iters", Slider::new(&mut p.pressure_iters, 4..=150));
                        row(ui, "Volume keep", Slider::new(&mut p.density_correction, 0.0..=0.3));
                    });
                    ui.label(egui::RichText::new("Sources").strong());
                    let mut remove_block = None;
                    for (i, b) in p.blocks.iter_mut().enumerate() {
                        ui.push_id(("block", i), |ui| {
                            ui.horizontal(|ui| {
                                ui.label(format!("Block {}", i + 1));
                                if ui.small_button("x").clicked() {
                                    remove_block = Some(i);
                                }
                            });
                            grid(ui, "b", |ui| {
                                vec3_row(ui, "Min", &mut b.min, 0.01);
                                vec3_row(ui, "Max", &mut b.max, 0.01);
                                vec3_row(ui, "Velocity", &mut b.velocity, 0.05);
                            });
                        });
                    }
                    if let Some(i) = remove_block {
                        p.blocks.remove(i);
                    }
                    let mut remove_jet = None;
                    for (i, j) in p.jets.iter_mut().enumerate() {
                        ui.push_id(("jet", i), |ui| {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut j.enabled, format!("Jet {}", i + 1));
                                if ui.small_button("x").clicked() {
                                    remove_jet = Some(i);
                                }
                            });
                            grid(ui, "j", |ui| {
                                vec3_row(ui, "Position", &mut j.pos, 0.01);
                                row(ui, "Radius", Slider::new(&mut j.radius, 0.01..=0.4));
                                vec3_row(ui, "Velocity", &mut j.velocity, 0.05);
                                row(ui, "Stop after", DragValue::new(&mut j.duration).range(0.0..=600.0).suffix(" s"));
                            });
                        });
                    }
                    if let Some(i) = remove_jet {
                        p.jets.remove(i);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("+ Block").clicked() {
                            p.blocks.push(WaterBlock::default());
                        }
                        if ui.button("+ Jet").clicked() {
                            p.jets.push(WaterJet { enabled: true, ..Default::default() });
                        }
                    });
                    ui.weak("Blocks fill on Reset; jets emit every frame");
                    ui.label(egui::RichText::new("Whitewater").strong());
                    grid(ui, "water_ww", |ui| {
                        ui.label("Spray");
                        ui.checkbox(&mut p.spray_enabled, "");
                        ui.end_row();
                        row(ui, "Spray speed", Slider::new(&mut p.spray_threshold, 0.2..=10.0).suffix(" m/s"));
                        row(ui, "Spray amount", Slider::new(&mut p.spray_amount, 0.0..=0.5));
                        row(ui, "Air drag", Slider::new(&mut p.spray_drag, 0.0..=5.0));
                        row(ui, "Lifetime", Slider::new(&mut p.spray_lifetime, 0.1..=5.0).suffix(" s"));
                        row(ui, "Foam speed", Slider::new(&mut p.foam_threshold, 0.1..=8.0));
                        row(ui, "Foam decay", Slider::new(&mut p.foam_decay, 0.0..=8.0));
                    });
                    ui.label(egui::RichText::new("Shading").strong());
                    grid(ui, "water_shade", |ui| {
                        ui.label("Deep");
                        ui.color_edit_button_rgb(&mut p.deep_color);
                        ui.end_row();
                        ui.label("Shallow");
                        ui.color_edit_button_rgb(&mut p.shallow_color);
                        ui.end_row();
                        ui.label("Foam");
                        ui.color_edit_button_rgb(&mut p.foam_color);
                        ui.end_row();
                        row(ui, "Foam strength", Slider::new(&mut p.foam_strength, 0.0..=2.0));
                        row(ui, "Particle size", Slider::new(&mut p.particle_size, 0.3..=3.0));
                        row(ui, "Spray size", Slider::new(&mut p.spray_size, 0.1..=2.0));
                    });
                });
            });

            egui::CollapsingHeader::new("Render").default_open(false).show(ui, |ui| {
                let r = &mut s.render;
                grid(ui, "render", |ui| {
                    row(ui, "Exposure", Slider::new(&mut r.exposure, 0.1..=4.0));
                    vec3_row(ui, "Light dir", &mut r.light_dir, 0.02);
                    ui.label("Background");
                    ui.color_edit_button_rgb(&mut r.background);
                    ui.end_row();
                    ui.label("Transparent BG");
                    ui.checkbox(&mut r.transparent_background, "");
                    ui.end_row();
                    ui.label("Ground");
                    ui.checkbox(&mut r.show_ground, "");
                    ui.end_row();
                    ui.label("Domain boxes");
                    ui.checkbox(&mut r.show_bounds, "");
                    ui.end_row();
                });
            });
            egui::CollapsingHeader::new("Camera").default_open(false).show(ui, |ui| {
                let c = &mut s.camera;
                grid(ui, "cam", |ui| {
                    vec3_row(ui, "Target", &mut c.target, 0.01);
                    row(ui, "Distance", DragValue::new(&mut c.distance).range(0.2..=50.0).speed(0.02));
                    row(ui, "Field of view", Slider::new(&mut c.fov_deg, 10.0..=100.0).suffix(" deg"));
                });
                ui.weak("Viewport: drag = orbit, right/middle drag = pan, wheel = zoom");
            });
            ui.add_space(8.0);
            let gpu = self.engine.smoke.as_ref().map(|s| s.memory_bytes()).unwrap_or(0);
            ui.weak(format!("Smoke grid memory ~{:.0} MB", gpu as f64 / 1e6));
        });
    }

    fn viewport(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
        // camera controls
        let cam = &mut self.engine.scene.camera;
        if resp.dragged_by(egui::PointerButton::Primary) {
            let d = resp.drag_delta();
            cam.yaw -= d.x * 0.008;
            cam.pitch = (cam.pitch + d.y * 0.008).clamp(-1.45, 1.45);
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            let cm = crate::render::camera_math(cam, rect.aspect_ratio());
            let k = cam.distance * 0.0015;
            let t = glam::Vec3::from(cam.target) - cm.right * d.x * k + cm.up * d.y * k;
            cam.target = t.into();
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta().y);
            if scroll != 0.0 {
                cam.distance = (cam.distance * (1.0 - scroll * 0.0015)).clamp(0.2, 50.0);
            }
        }

        let ppp = ui.pixels_per_point();
        let w = (rect.width() * ppp).round().max(8.0) as u32;
        let h = (rect.height() * ppp).round().max(8.0) as u32;
        let Some(rs) = frame.wgpu_render_state() else { return };
        let needs_new = self.viewport.as_ref().map(|(t, _)| t.width != w || t.height != h).unwrap_or(true);
        if needs_new {
            let target = RenderTarget::new(&self.engine.device, w, h);
            let mut renderer = rs.renderer.write();
            let id = match &self.viewport {
                Some((_, id)) => {
                    renderer.update_egui_texture_from_wgpu_texture(&self.engine.device, &target.color_view, wgpu::FilterMode::Linear, *id);
                    *id
                }
                None => renderer.register_native_texture(&self.engine.device, &target.color_view, wgpu::FilterMode::Linear),
            };
            self.viewport = Some((target, id));
        }
        let (target, id) = self.viewport.as_ref().unwrap();
        self.engine.render(target, false);
        ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(12, 12, 14));
        ui.painter().image(*id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }

    fn export_window(&mut self, ui: &mut egui::Ui) {
        let mut open = self.show_export;
        egui::Window::new("Export PNG sequence").open(&mut open).resizable(false).show(ui.ctx(), |ui| {
            let e = &mut self.engine.scene.export;
            grid(ui, "export", |ui| {
                ui.label("Size");
                ui.horizontal(|ui| {
                    ui.add(DragValue::new(&mut e.width).range(16..=7680));
                    ui.label("x");
                    ui.add(DragValue::new(&mut e.height).range(16..=4320));
                });
                ui.end_row();
                ui.label("Presets");
                ui.horizontal(|ui| {
                    for (lbl, w, h) in [("720p", 1280, 720), ("1080p", 1920, 1080), ("4K", 3840, 2160), ("Square", 1080, 1080)] {
                        if ui.small_button(lbl).clicked() {
                            e.width = w;
                            e.height = h;
                        }
                    }
                });
                ui.end_row();
                row(ui, "Frames", DragValue::new(&mut e.frames).range(1..=100_000));
                ui.label("File prefix");
                ui.text_edit_singleline(&mut e.prefix);
                ui.end_row();
            });
            ui.checkbox(&mut self.reset_before_export, "Reset simulation before export");
            ui.checkbox(&mut self.engine.scene.render.transparent_background, "Transparent background (alpha for Reelforge compositing)");
            ui.horizontal(|ui| {
                if ui.button("Folder...").clicked() {
                    self.export_dir = rfd::FileDialog::new().pick_folder();
                }
                ui.label(self.export_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "no folder chosen".into()));
            });
            ui.separator();
            if let Some(ex) = &self.export {
                ui.add(egui::ProgressBar::new(ex.done as f32 / ex.total as f32).text(format!("{}/{}", ex.done, ex.total)));
                if ui.button("Cancel").clicked() {
                    self.export = None;
                    self.status = "Export cancelled".into();
                }
            } else if ui.add_enabled(self.export_dir.is_some(), egui::Button::new("Start export")).clicked() {
                let dir = self.export_dir.clone().unwrap();
                if self.reset_before_export {
                    self.engine.reset();
                }
                let e = &self.engine.scene.export;
                let target = RenderTarget::new(&self.engine.device, e.width, e.height);
                let _ = write_manifest(&dir, &self.engine.scene);
                self.export = Some(Export { dir, target, done: 0, total: e.frames, writers: vec![] });
                self.playing = false;
            }
        });
        self.show_export = open;
    }

    fn export_tick(&mut self) {
        let Some(ex) = &mut self.export else { return };
        self.engine.step_frame();
        self.engine.render(&ex.target, true);
        let img = engine::read_target(&self.engine.device, &self.engine.queue, &ex.target);
        let path = engine::frame_path(&ex.dir, &self.engine.scene.export.prefix, self.engine.scene.export.start_frame + ex.done);
        ex.writers.retain(|h| !h.is_finished());
        ex.writers.push(std::thread::spawn(move || {
            if let Err(err) = img.save(&path) {
                eprintln!("write {}: {err}", path.display());
            }
        }));
        ex.done += 1;
        if ex.done >= ex.total {
            let ex = self.export.take().unwrap();
            for h in ex.writers {
                let _ = h.join();
            }
            self.status = format!("Exported {} frames to {}", ex.total, ex.dir.display());
        }
    }
}

impl eframe::App for TidefireApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.export.is_some() {
            self.export_tick();
        } else if self.playing {
            self.engine.step_frame();
        }
        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        egui::Panel::left("params").resizable(true).default_size(330.0).show(ui, |ui| self.side_panel(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.viewport(ui, frame));
        self.export_window(ui);
        ui.ctx().request_repaint();
    }
}

pub fn write_manifest(dir: &std::path::Path, scene: &Scene) -> std::io::Result<()> {
    let e = &scene.export;
    let m = serde_json::json!({
        "generator": "Tidefire",
        "scene": scene.name,
        "fps": scene.fps,
        "frames": e.frames,
        "start_frame": e.start_frame,
        "width": e.width,
        "height": e.height,
        "alpha": scene.render.transparent_background,
        "pattern": format!("{}_%05d.png", e.prefix),
    });
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(format!("{}_sequence.json", e.prefix)), serde_json::to_string_pretty(&m).unwrap())
}

fn grid(ui: &mut egui::Ui, id: &str, f: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([10.0, 4.0]).show(ui, f);
}

fn row(ui: &mut egui::Ui, label: &str, w: impl egui::Widget) {
    ui.label(label);
    ui.add(w);
    ui.end_row();
}

fn vec3_row(ui: &mut egui::Ui, label: &str, v: &mut [f32; 3], speed: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        for (c, x) in v.iter_mut().enumerate() {
            let col = [Color32::from_rgb(220, 90, 90), Color32::from_rgb(110, 200, 110), Color32::from_rgb(100, 140, 230)][c];
            ui.add(DragValue::new(x).speed(speed).max_decimals(3).prefix(egui::RichText::new(["x ", "y ", "z "][c]).color(col).text()));
        }
    });
    ui.end_row();
}
