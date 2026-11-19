//! Talks to the Tauri shell: native file dialogs and writing files. In a plain
//! browser (the CI preview) these report that they need the desktop app.

use base64::Engine as _;
use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

fn tauri() -> Option<JsValue> {
    let w = web_sys::window()?;
    let t = Reflect::get(&w, &"__TAURI__".into()).ok()?;
    (!t.is_undefined() && !t.is_null()).then_some(t)
}

pub fn in_tauri() -> bool {
    tauri().is_some()
}

fn get(obj: &JsValue, key: &str) -> Result<JsValue, String> {
    Reflect::get(obj, &key.into()).map_err(|_| format!("missing {key}"))
}

fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::JSON::stringify(&e).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "unknown error".to_owned())
}

async fn invoke(cmd: &str, args: &serde_json::Value) -> Result<JsValue, String> {
    let t = tauri().ok_or("this needs the Tidefire desktop app")?;
    let core = get(&t, "core")?;
    let f: Function = get(&core, "invoke")?
        .dyn_into()
        .map_err(|_| "invoke is not a function")?;
    let args = serde_wasm_bindgen::to_value(args).map_err(|e| e.to_string())?;
    let p: Promise = f
        .call2(&core, &cmd.into(), &args)
        .map_err(js_err)?
        .dyn_into()
        .map_err(|_| "invoke did not return a promise")?;
    JsFuture::from(p).await.map_err(js_err)
}

fn opt_string(v: JsValue) -> Option<String> {
    if v.is_null() || v.is_undefined() {
        None
    } else {
        v.as_string()
    }
}

#[derive(serde::Deserialize)]
pub struct Opened {
    pub path: String,
    pub text: String,
}

/// Shows the Open dialog and reads the chosen scene. `None` when cancelled.
pub async fn open_scene() -> Result<Option<Opened>, String> {
    let v = invoke("open_scene", &serde_json::json!({})).await?;
    if v.is_null() || v.is_undefined() {
        return Ok(None);
    }
    serde_wasm_bindgen::from_value(v)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Saves a scene: to `path`, or (if `None`) wherever the Save dialog says.
/// Returns where it was saved, or `None` when cancelled.
pub async fn save_scene(
    text: String,
    path: Option<String>,
    suggested: String,
) -> Result<Option<String>, String> {
    let v = invoke(
        "save_scene",
        &serde_json::json!({ "text": text, "path": path, "suggested": suggested }),
    )
    .await?;
    Ok(opt_string(v))
}

/// Folder picker for exports.
pub async fn pick_folder() -> Result<Option<String>, String> {
    Ok(opt_string(
        invoke("pick_folder", &serde_json::json!({})).await?,
    ))
}

pub async fn write_file(dir: &str, name: &str, bytes: &[u8]) -> Result<(), String> {
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    invoke(
        "write_file",
        &serde_json::json!({ "dir": dir, "name": name, "data": data }),
    )
    .await
    .map(|_| ())
}

pub async fn write_text(dir: &str, name: &str, text: &str) -> Result<(), String> {
    invoke(
        "write_text",
        &serde_json::json!({ "dir": dir, "name": name, "text": text }),
    )
    .await
    .map(|_| ())
}
