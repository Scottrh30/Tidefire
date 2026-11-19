//! The interface's state. The scene being edited lives here as a signal; the
//! viewport copies it into the GPU engine whenever `rev` changes.

use crate::prefs::{self, Folds, Theme};
use leptos::prelude::*;
use tidefire_core::params::{self, Scene};

#[derive(Clone, Copy)]
pub struct Ui {
    pub scene: RwSignal<Scene>,
    /// Bumped on every edit, so the viewport knows to copy the scene into the engine.
    pub rev: RwSignal<u64>,
    /// Bumped to ask the viewport to restart the simulation from frame 0.
    pub reset_rev: RwSignal<u64>,
    /// Bumped to advance exactly one frame while paused.
    pub step_rev: RwSignal<u64>,
    pub playing: RwSignal<bool>,
    pub stats: RwSignal<Stats>,
    pub status: RwSignal<String>,
    /// Why the viewport can't run (no WebGPU, device lost...), if it can't.
    pub gpu_error: RwSignal<Option<String>>,
    pub scene_path: RwSignal<Option<String>>,
    pub export: Export,
    /// Interface colours (kept between runs, not part of the scene).
    pub theme: RwSignal<Theme>,
    pub appearance_open: RwSignal<bool>,
    /// Which panel dropdowns are open (kept between runs).
    pub folds: Folds,
}

#[derive(Clone, Copy)]
pub struct Export {
    pub open: RwSignal<bool>,
    pub folder: RwSignal<Option<String>>,
    pub reset_first: RwSignal<bool>,
    pub running: RwSignal<bool>,
    pub cancel: RwSignal<bool>,
    pub done: RwSignal<u32>,
    pub total: RwSignal<u32>,
}

#[derive(Clone, Default, PartialEq)]
pub struct Stats {
    pub frame: u32,
    pub time: f32,
    /// Simulated frames per second the viewport is achieving (0 while paused).
    pub fps: f32,
    pub particles: u32,
    pub adapter: String,
}

impl Ui {
    pub fn new() -> Self {
        let theme: Theme = prefs::load("theme").unwrap_or_default();
        theme.apply();
        Self {
            scene: RwSignal::new(Scene::default()),
            rev: RwSignal::new(1),
            reset_rev: RwSignal::new(0),
            step_rev: RwSignal::new(0),
            playing: RwSignal::new(true),
            stats: RwSignal::new(Stats::default()),
            status: RwSignal::new("Ready".into()),
            gpu_error: RwSignal::new(None),
            scene_path: RwSignal::new(None),
            export: Export {
                open: RwSignal::new(false),
                folder: RwSignal::new(None),
                reset_first: RwSignal::new(true),
                running: RwSignal::new(false),
                cancel: RwSignal::new(false),
                done: RwSignal::new(0),
                total: RwSignal::new(0),
            },
            theme: RwSignal::new(theme),
            appearance_open: RwSignal::new(false),
            folds: Folds::load(),
        }
    }

    /// Changes the interface colours, applies them right away and remembers them.
    pub fn set_theme(&self, f: impl FnOnce(&mut Theme)) {
        self.theme.update(f);
        self.theme.with_untracked(|t| {
            t.apply();
            prefs::save("theme", t);
        });
    }

    /// Changes the scene (from a field, the camera, a file...).
    pub fn edit(&self, f: impl FnOnce(&mut Scene)) {
        self.scene.update(f);
        self.rev.update(|r| *r += 1);
    }

    /// Replaces the whole scene and restarts the simulation.
    pub fn load(&self, scene: Scene) {
        self.scene.set(scene);
        self.rev.update(|r| *r += 1);
        self.reset();
    }

    pub fn reset(&self) {
        self.reset_rev.update(|r| *r += 1);
    }

    pub fn load_preset(&self, name: &str) {
        if let Some(s) = params::preset(name) {
            self.load(s);
            self.scene_path.set(None);
            self.status
                .set(format!("Loaded preset \u{201c}{name}\u{201d}"));
        }
    }
}
