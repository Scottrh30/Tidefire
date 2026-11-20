// When TIDEFIRE_FFMPEG_EMBED points at an ffmpeg binary (the Windows CI build
// sets it to the small one from ci/build-ffmpeg.sh), it is embedded in the
// program and unpacked on first use, so video export works with no install.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(embedded_ffmpeg)");
    println!("cargo:rerun-if-env-changed=TIDEFIRE_FFMPEG_EMBED");
    if let Ok(path) = std::env::var("TIDEFIRE_FFMPEG_EMBED") {
        let p = std::path::Path::new(&path);
        if p.is_file() {
            println!("cargo:rerun-if-changed={path}");
            println!("cargo:rustc-cfg=embedded_ffmpeg");
            println!(
                "cargo:rustc-env=TIDEFIRE_FFMPEG_FILE={}",
                std::path::absolute(p).unwrap().display()
            );
        } else {
            println!(
                "cargo:warning=TIDEFIRE_FFMPEG_EMBED={path} is not a file; video export will look for ffmpeg at run time"
            );
        }
    }
}
