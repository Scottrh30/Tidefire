//! Video export for Tidefire: rendered RGBA frames are piped into ffmpeg,
//! which writes H.264 in an .mp4 or Apple ProRes in a .mov.
//!
//! Frames are handed to a writer thread through a short queue, so the GPU can
//! render the next frame while ffmpeg encodes the last one.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;

/// The ProRes flavours, smallest to largest. Only the 4444 ones keep alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProResProfile {
    Proxy,
    Lt,
    Standard,
    #[default]
    Hq,
    P4444,
    P4444Xq,
}

impl ProResProfile {
    pub const ALL: [ProResProfile; 6] = [
        Self::Proxy,
        Self::Lt,
        Self::Standard,
        Self::Hq,
        Self::P4444,
        Self::P4444Xq,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Proxy => "422 Proxy",
            Self::Lt => "422 LT",
            Self::Standard => "422",
            Self::Hq => "422 HQ",
            Self::P4444 => "4444",
            Self::P4444Xq => "4444 XQ",
        }
    }

    pub fn has_alpha(self) -> bool {
        matches!(self, Self::P4444 | Self::P4444Xq)
    }

    fn ks_profile(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Codec {
    /// H.264 in .mp4. `quality` 0..=100 (higher = better, larger); no alpha.
    H264 { quality: u8 },
    /// Apple ProRes in .mov.
    ProRes { profile: ProResProfile },
}

impl Codec {
    pub fn extension(self) -> &'static str {
        match self {
            Codec::H264 { .. } => "mp4",
            Codec::ProRes { .. } => "mov",
        }
    }

    /// Whether the file keeps the alpha channel (transparent background).
    pub fn keeps_alpha(self) -> bool {
        matches!(self, Codec::ProRes { profile } if profile.has_alpha())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

/// The ffmpeg arguments for raw RGBA frames on stdin → `out`.
pub fn ffmpeg_args(s: &Settings, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-s",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(format!("{}x{}", s.width, s.height));
    a.push("-r".into());
    a.push(format!("{}", s.fps));
    // "pipe:0" rather than "-": the latter needs the fd protocol in newer ffmpeg
    a.extend(["-i", "pipe:0"].map(String::from));
    // Frames are sRGB; tag the output as Rec.709 so editors read the colours right.
    let tags = [
        "-colorspace",
        "bt709",
        "-color_primaries",
        "bt709",
        "-color_trc",
        "bt709",
        "-color_range",
        "tv",
    ];
    match s.codec {
        Codec::H264 { quality } => {
            // quality 100 -> crf 12 (near lossless), 0 -> crf 36
            let crf = 36.0 - quality.min(100) as f32 * 0.24;
            a.extend(
                [
                    "-vf",
                    // 4:2:0 needs even sizes
                    "pad=ceil(iw/2)*2:ceil(ih/2)*2,scale=out_color_matrix=bt709:out_range=tv,format=yuv420p",
                    "-c:v",
                    "libx264",
                    "-preset",
                    "slow",
                    "-crf",
                ]
                .map(String::from),
            );
            a.push(format!("{crf:.0}"));
            a.extend(["-movflags", "+faststart"].map(String::from));
        }
        Codec::ProRes { profile } => {
            let (fmt, extra): (&str, &[&str]) = if profile.has_alpha() {
                ("yuva444p10le", &["-alpha_bits", "16"])
            } else {
                ("yuv422p10le", &[])
            };
            a.push("-vf".into());
            a.push(format!(
                "scale=out_color_matrix=bt709:out_range=tv,format={fmt}"
            ));
            a.extend(["-c:v", "prores_ks", "-profile:v"].map(String::from));
            a.push(profile.ks_profile().to_string());
            a.extend(extra.iter().map(|s| s.to_string()));
            // Apple vendor tag, and a colr atom so the Rec.709 tags reach the file
            a.extend(["-vendor", "apl0", "-movflags", "+write_colr"].map(String::from));
        }
    }
    a.extend(tags.map(String::from));
    a.push(out.display().to_string());
    a
}

/// Where ffmpeg is: `TIDEFIRE_FFMPEG`, the copy built into this program,
/// one next to the program, or one on the PATH.
pub fn find_ffmpeg() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("TIDEFIRE_FFMPEG") {
        return Ok(PathBuf::from(p));
    }
    #[cfg(embedded_ffmpeg)]
    {
        return unpack_embedded();
    }
    #[allow(unreachable_code)]
    {
        let name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf))
        {
            let p = dir.join(name);
            if p.is_file() {
                return Ok(p);
            }
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path) {
            let p = dir.join(name);
            if p.is_file() {
                return Ok(p);
            }
        }
        Err("video export needs ffmpeg, and none was found (put ffmpeg on the PATH or next to Tidefire, or set TIDEFIRE_FFMPEG)".into())
    }
}

#[cfg(embedded_ffmpeg)]
static EMBEDDED: &[u8] = include_bytes!(env!("TIDEFIRE_FFMPEG_FILE"));

/// Writes the embedded ffmpeg to the user's local app data once (the name
/// carries the size and a checksum, so a new build unpacks its own copy).
#[cfg(embedded_ffmpeg)]
fn unpack_embedded() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Tidefire");
    let sum = EMBEDDED.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    });
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let path = base.join(format!("ffmpeg-{}-{sum:016x}{ext}", EMBEDDED.len()));
    if path
        .metadata()
        .map(|m| m.len() as usize == EMBEDDED.len())
        .unwrap_or(false)
    {
        return Ok(path);
    }
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, EMBEDDED).map_err(|e| format!("{}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// An ffmpeg process being fed frames.
pub struct VideoWriter {
    settings: Settings,
    tx: Option<mpsc::SyncSender<Vec<u8>>>,
    feeder: Option<JoinHandle<Result<(), String>>>,
    stderr: Option<JoinHandle<String>>,
    child: Child,
    path: PathBuf,
    frames: u32,
}

impl VideoWriter {
    pub fn new(path: &Path, settings: Settings) -> Result<Self, String> {
        if settings.width == 0 || settings.height == 0 {
            return Err("video size must not be zero".into());
        }
        if !(settings.fps > 0.0) {
            return Err("frame rate must be above zero".into());
        }
        let ffmpeg = find_ffmpeg()?;
        let mut cmd = Command::new(&ffmpeg);
        cmd.args(ffmpeg_args(&settings, path))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", ffmpeg.display()))?;
        let mut stdin = child.stdin.take().ok_or("ffmpeg has no stdin")?;
        let mut err = child.stderr.take().ok_or("ffmpeg has no stderr")?;
        let stderr = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            s
        });
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(2);
        let feeder = std::thread::spawn(move || {
            for frame in rx {
                stdin
                    .write_all(&frame)
                    .map_err(|e| format!("ffmpeg stopped taking frames ({e})"))?;
            }
            Ok(())
        });
        Ok(Self {
            settings,
            tx: Some(tx),
            feeder: Some(feeder),
            stderr: Some(stderr),
            child,
            path: path.to_path_buf(),
            frames: 0,
        })
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// One frame: `width * height` RGBA pixels, top row first.
    pub fn push(&mut self, rgba: Vec<u8>) -> Result<(), String> {
        let want = self.settings.width as usize * self.settings.height as usize * 4;
        if rgba.len() != want {
            return Err(format!("frame is {} bytes, expected {want}", rgba.len()));
        }
        let sent = self
            .tx
            .as_ref()
            .map(|tx| tx.send(rgba).is_ok())
            .unwrap_or(false);
        if !sent {
            // the feeder gave up: finish to collect ffmpeg's message
            return Err(self.collect_error());
        }
        self.frames += 1;
        Ok(())
    }

    fn collect_error(&mut self) -> String {
        self.tx = None;
        let fed = self.feeder.take().map(|h| h.join().unwrap_or(Ok(())));
        let _ = self.child.wait();
        let log = self
            .stderr
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        let log = log.trim();
        match fed {
            Some(Err(e)) if log.is_empty() => e,
            _ if !log.is_empty() => format!("ffmpeg: {}", last_lines(log, 4)),
            _ => "ffmpeg stopped unexpectedly".into(),
        }
    }

    /// Closes the stream and waits for ffmpeg to finish the file.
    pub fn finish(mut self) -> Result<(PathBuf, u32), String> {
        self.tx = None;
        let fed = self
            .feeder
            .take()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("writer thread panicked".into()))
            })
            .unwrap_or(Ok(()));
        let status = self.child.wait().map_err(|e| e.to_string())?;
        let log = self
            .stderr
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        if !status.success() || fed.is_err() {
            let log = log.trim();
            return Err(if log.is_empty() {
                fed.err()
                    .unwrap_or_else(|| format!("ffmpeg failed ({status})"))
            } else {
                format!("ffmpeg: {}", last_lines(log, 4))
            });
        }
        Ok((self.path.clone(), self.frames))
    }

    /// Stops early and deletes the partial file.
    pub fn abort(mut self) {
        self.tx = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(h) = self.feeder.take() {
            let _ = h.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for VideoWriter {
    fn drop(&mut self) {
        if self.tx.is_some() {
            // dropped without finish/abort: don't leave ffmpeg running
            self.tx = None;
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join(" / ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32, t: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                v.extend([
                    ((x + t) % 256) as u8,
                    (y % 256) as u8,
                    128,
                    ((x * 255) / w) as u8,
                ]);
            }
        }
        v
    }

    fn encode(codec: Codec, name: &str, w: u32, h: u32) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        let mut vw = VideoWriter::new(
            &path,
            Settings {
                codec,
                width: w,
                height: h,
                fps: 30.0,
            },
        )
        .unwrap();
        for t in 0..12 {
            vw.push(gradient(w, h, t * 4)).unwrap();
        }
        let (p, n) = vw.finish().unwrap();
        assert_eq!(n, 12);
        assert!(std::fs::metadata(&p).unwrap().len() > 1000);
        p
    }

    #[test]
    #[ignore = "needs ffmpeg"]
    fn writes_mp4_with_odd_size() {
        encode(Codec::H264 { quality: 70 }, "tidefire_test.mp4", 161, 91);
    }

    #[test]
    #[ignore = "needs ffmpeg"]
    fn writes_prores_4444() {
        encode(
            Codec::ProRes {
                profile: ProResProfile::P4444,
            },
            "tidefire_test.mov",
            160,
            90,
        );
    }

    #[test]
    #[ignore = "needs ffmpeg"]
    fn wrong_frame_size_is_an_error() {
        let path = std::env::temp_dir().join("tidefire_bad.mp4");
        let mut vw = VideoWriter::new(
            &path,
            Settings {
                codec: Codec::H264 { quality: 50 },
                width: 64,
                height: 64,
                fps: 24.0,
            },
        )
        .unwrap();
        assert!(vw.push(vec![0; 10]).is_err());
        vw.abort();
    }
}
