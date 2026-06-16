fn main() {
    #[cfg(windows)]
    winres::WindowsResource::new()
        .set_icon("assets/logo/bloom.ico")
        .compile()
        .expect("failed to compile Windows resource");

    #[cfg(all(windows, feature = "av"))]
    copy_ffmpeg_dlls();
}

#[cfg(all(windows, feature = "av"))]
fn copy_ffmpeg_dlls() {
    use std::path::PathBuf;

    let ffmpeg_dir = match std::env::var("FFMPEG_DIR") {
        Ok(d) => d,
        Err(_) => {
            println!("cargo:warning=FFMPEG_DIR not set; skipping FFmpeg DLL copy");
            return;
        }
    };
    let bin = PathBuf::from(&ffmpeg_dir).join("bin");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let target_dir = match out_dir.ancestors().nth(3) {
        Some(d) => d.to_path_buf(),
        None => {
            println!(
                "cargo:warning=could not resolve target dir from OUT_DIR; skipping FFmpeg DLL copy"
            );
            return;
        }
    };

    println!("cargo:rerun-if-changed={}", bin.display());
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");

    let entries = match std::fs::read_dir(&bin) {
        Ok(e) => e,
        Err(_) => {
            println!(
                "cargo:warning=FFmpeg bin dir not found at {}; run scripts/setup-av.ps1",
                bin.display()
            );
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("dll") {
            let dest = target_dir.join(path.file_name().unwrap());
            let _ = std::fs::copy(&path, &dest);
        }
    }
}
