//! Tidefire's desktop shell. The interface and the whole fire/water engine run
//! in the window (WebAssembly + WebGPU); this side only does what a web page
//! can't: native file dialogs, writing scenes and exported frames to disk, and
//! feeding frames to ffmpeg for .mp4 / ProRes export.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::Engine as _;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri_plugin_dialog::DialogExt;
use tidefire_video::{Settings, VideoWriter};

/// Video exports in progress, by id.
#[derive(Default)]
struct Videos {
    next: Mutex<u32>,
    open: Mutex<HashMap<u32, Arc<Mutex<Option<VideoWriter>>>>>,
}

impl Videos {
    fn get(&self, id: u32) -> Result<Arc<Mutex<Option<VideoWriter>>>, String> {
        self.open
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no video export {id}"))
    }

    fn take(&self, id: u32) -> Result<VideoWriter, String> {
        let w = self
            .open
            .lock()
            .unwrap()
            .remove(&id)
            .ok_or_else(|| format!("no video export {id}"))?;
        let w = w
            .lock()
            .unwrap()
            .take()
            .ok_or("video export already closed")?;
        Ok(w)
    }
}

/// Save dialog for a video file (`ext` = "mp4" or "mov").
#[tauri::command]
async fn pick_video_file(
    app: tauri::AppHandle,
    suggested: String,
    ext: String,
) -> Result<Option<String>, String> {
    let label = if ext == "mov" {
        "QuickTime movie (ProRes)"
    } else {
        "MP4 video (H.264)"
    };
    let picked = app
        .dialog()
        .file()
        .set_title("Export video")
        .add_filter(label, &[ext.as_str()])
        .set_file_name(format!("{suggested}.{ext}"))
        .blocking_save_file();
    let Some(picked) = picked else {
        return Ok(None);
    };
    let mut p = picked.into_path().map_err(|e| e.to_string())?;
    if p.extension().map(|e| e != ext.as_str()).unwrap_or(true) {
        p.set_extension(&ext);
    }
    Ok(Some(p.display().to_string()))
}

/// Where ffmpeg is (unpacking the built-in copy if needed), or why there is none.
#[tauri::command]
async fn ffmpeg_status() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        tidefire_video::find_ffmpeg().map(|p| p.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Starts ffmpeg writing `path`; returns the export's id.
#[tauri::command]
async fn video_begin(
    videos: tauri::State<'_, Videos>,
    path: String,
    settings: Settings,
) -> Result<u32, String> {
    let w =
        tauri::async_runtime::spawn_blocking(move || VideoWriter::new(Path::new(&path), settings))
            .await
            .map_err(|e| e.to_string())??;
    let id = {
        let mut n = videos.next.lock().unwrap();
        *n += 1;
        *n
    };
    videos
        .open
        .lock()
        .unwrap()
        .insert(id, Arc::new(Mutex::new(Some(w))));
    Ok(id)
}

/// One frame of raw RGBA pixels, sent as the request body (no base64), with
/// the export id in the `x-video-id` header.
#[tauri::command]
async fn video_frame(
    videos: tauri::State<'_, Videos>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), String> {
    let id: u32 = request
        .headers()
        .get("x-video-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or("missing x-video-id header")?;
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else {
        return Err("expected raw frame bytes".into());
    };
    let data = data.clone();
    let w = videos.get(id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut g = w.lock().unwrap();
        g.as_mut().ok_or("video export already closed")?.push(data)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Finishes the file; returns the number of frames written.
#[tauri::command]
async fn video_finish(videos: tauri::State<'_, Videos>, id: u32) -> Result<u32, String> {
    let w = videos.take(id)?;
    tauri::async_runtime::spawn_blocking(move || w.finish().map(|(_, n)| n))
        .await
        .map_err(|e| e.to_string())?
}

/// Stops an export and deletes the partial file.
#[tauri::command]
async fn video_abort(videos: tauri::State<'_, Videos>, id: u32) -> Result<(), String> {
    if let Ok(w) = videos.take(id) {
        tauri::async_runtime::spawn_blocking(move || w.abort())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

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
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(Opened {
        path: path.display().to_string(),
        text,
    }))
}

/// Writes a scene to `path`, or asks where with the Save dialog when there is
/// none yet. Returns where it went (`None` = cancelled).
#[tauri::command]
async fn save_scene(
    app: tauri::AppHandle,
    text: String,
    path: Option<String>,
    suggested: String,
) -> Result<Option<String>, String> {
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
            let Some(picked) = picked else {
                return Ok(None);
            };
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
    let picked = app
        .dialog()
        .file()
        .set_title("Export frames to")
        .blocking_pick_folder();
    let Some(picked) = picked else {
        return Ok(None);
    };
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
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| e.to_string())?;
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
        .manage(Videos::default())
        .invoke_handler(tauri::generate_handler![
            open_scene,
            save_scene,
            pick_folder,
            write_file,
            write_text,
            pick_video_file,
            ffmpeg_status,
            video_begin,
            video_frame,
            video_finish,
            video_abort
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tidefire");
}
