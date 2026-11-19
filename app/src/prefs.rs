//! Interface preferences kept between runs (in the window's local storage):
//! the colour theme and which panel dropdowns are open. They belong to the app,
//! not to a scene, so they are not saved in .tidefire files.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

pub fn load<T: for<'de> Deserialize<'de>>(key: &str) -> Option<T> {
    let text = storage()?.get_item(&format!("tidefire.{key}")).ok()??;
    serde_json::from_str(&text).ok()
}

pub fn save<T: Serialize>(key: &str, value: &T) {
    if let (Some(s), Ok(text)) = (storage(), serde_json::to_string(value)) {
        let _ = s.set_item(&format!("tidefire.{key}"), &text);
    }
}

/// The colours a user can change. Everything else (hover states, dimmed
/// labels, filled slider tracks) is derived from these in the stylesheet.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub text: String,
    pub slider: String,
    pub toggle: String,
    pub button: String,
    pub button_text: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self::ember()
    }
}

impl Theme {
    pub fn ember() -> Self {
        Self {
            text: "#d9dce2".into(),
            slider: "#ff8a3d".into(),
            toggle: "#ff8a3d".into(),
            button: "#222429".into(),
            button_text: "#cfd3da".into(),
        }
    }

    pub fn tide() -> Self {
        Self {
            text: "#d6e2ec".into(),
            slider: "#3d9bff".into(),
            toggle: "#2fbf9f".into(),
            button: "#1d2733".into(),
            button_text: "#c9d8e6".into(),
        }
    }

    pub fn mono() -> Self {
        Self {
            text: "#e4e4e4".into(),
            slider: "#bdbdbd".into(),
            toggle: "#9e9e9e".into(),
            button: "#262626".into(),
            button_text: "#e0e0e0".into(),
        }
    }

    pub const PRESETS: [(&'static str, fn() -> Theme); 3] = [
        ("Ember", Theme::ember),
        ("Tide", Theme::tide),
        ("Mono", Theme::mono),
    ];

    /// Pushes the colours into the stylesheet's variables.
    pub fn apply(&self) {
        let Some(root) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
        else {
            return;
        };
        let Ok(el) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(root) else {
            return;
        };
        let style = el.style();
        for (var, value) in [
            ("--text", &self.text),
            ("--slider", &self.slider),
            ("--toggle", &self.toggle),
            ("--button", &self.button),
            ("--button-text", &self.button_text),
        ] {
            let _ = style.set_property(var, value);
        }
    }
}

/// Open/closed state of every panel dropdown, by key ("water.sources"...).
#[derive(Clone, Copy)]
pub struct Folds(RwSignal<std::collections::HashMap<String, bool>>);

impl Folds {
    pub fn load() -> Self {
        Self(RwSignal::new(load("folds").unwrap_or_default()))
    }

    pub fn is_open(&self, key: &str, default: bool) -> bool {
        self.0.with(|m| m.get(key).copied().unwrap_or(default))
    }

    pub fn set(&self, key: &str, open: bool) {
        self.0.update(|m| {
            m.insert(key.to_owned(), open);
        });
        self.0.with_untracked(|m| save("folds", m));
    }
}
