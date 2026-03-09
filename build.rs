fn main() {
    #[cfg(windows)]
    winres::WindowsResource::new()
        .set_icon("assets/logo/bloom.ico")
        .compile()
        .expect("failed to compile Windows resource");

    // Copy FFmpeg DLLs next to the output binary so the app can find them at runtime.
    #[cfg(windows)]
    copy_ffmpeg_dlls();
}

#[cfg(windows)]
fn copy_ffmpeg_dlls() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    // OUT_DIR is .../target/<profile>/build/<crate>/out — go up 3 levels to get target/<profile>
    let target_dir = std::path::Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();

    let ffmpeg_bin = std::path::Path::new("vendor/ffmpeg/bin");
    if !ffmpeg_bin.exists() {
        return;
    }

    for entry in std::fs::read_dir(ffmpeg_bin).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("dll") {
            let dest = target_dir.join(path.file_name().unwrap());
            if !dest.exists() {
                std::fs::copy(&path, &dest).unwrap();
            }
        }
    }

    // Tell Cargo to re-run this if the vendor DLLs change.
    println!("cargo:rerun-if-changed=vendor/ffmpeg/bin");
}
