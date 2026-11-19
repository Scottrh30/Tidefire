//! Tidefire's desktop shell. The interface and the whole fire/water engine run
//! in the window (WebAssembly + WebGPU); this side only does what a web page
//! can't: native file dialogs and writing scenes and exported frames to disk.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine as _;
use std::path::{Path, PathBuf};
use tauri_plugin_dialog::DialogExt;

#[derive(serde::Serialize)]
struct Opened {
    path: String,
    text: String,
}

/// Shows the Open dialog and reads the chosen scene.
#[tauri::command]
async fn open_scene(app: tauri::AppHandle) -> Result<Option<Opened>, String> {
    let picked = app
        .dialog()
        .file()
        .set_title("Open a Tidefire scene")
        .add_filter("Tidefire scene", &["tidefire", "json"])
        .add_filter("All files", &["*"])
        .blocking_pick_file();
    let Some(picked) = picked else { return Ok(None) };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(Opened { path: path.display().to_string(), text }))
}

/// Writes a scene to `path`, or asks where with the Save dialog when there is
/// none yet. Returns where it went (`None` = cancelled).
#[tauri::command]
async fn save_scene(app: tauri::AppHandle, text: String, path: Option<String>, suggested: String) -> Result<Option<String>, String> {
    let path = match path {
        Some(p) => PathBuf::from(p),
        None => {
            let picked = app
                .dialog()
                .file()
                .set_title("Save the Tidefire scene")
                .add_filter("Tidefire scene", &["tidefire"])
                .set_file_name(&suggested)
                .blocking_save_file();
            let Some(picked) = picked else { return Ok(None) };
            let mut p = picked.into_path().map_err(|e| e.to_string())?;
            if p.extension().is_none() {
                p.set_extension("tidefire");
            }
            p
        }
    };
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

/// Folder picker for PNG-sequence exports.
#[tauri::command]
async fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let picked = app.dialog().file().set_title("Export frames to").blocking_pick_folder();
    let Some(picked) = picked else { return Ok(None) };
    let p = picked.into_path().map_err(|e| e.to_string())?;
    Ok(Some(p.display().to_string()))
}

/// Only plain file names are written (no folders or `..`), into the chosen folder.
fn target(dir: &str, name: &str) -> Result<PathBuf, String> {
    let n = Path::new(name);
    if n.components().count() != 1 || n.file_name().is_none() {
        return Err(format!("bad file name {name:?}"));
    }
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir.join(n))
}

/// Writes one exported frame (PNG bytes, base64 from the interface).
#[tauri::command]
async fn write_file(dir: String, name: String, data: String) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| e.to_string())?;
    let path = target(&dir, &name)?;
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes a text file (the export's `_sequence.json`).
#[tauri::command]
async fn write_text(dir: String, name: String, text: String) -> Result<(), String> {
    let path = target(&dir, &name)?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .invoke_handler(tauri::generate_handler![open_scene, save_scene, pick_folder, write_file, write_text])
        .run(tauri::generate_context!())
        .expect("error while running Tidefire");
}
