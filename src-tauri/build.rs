fn main() {
    println!("cargo:rerun-if-changed=../native/vision-ocr.swift");
    println!("cargo:rerun-if-changed=../native/vision-ocr-bounds.swift");
    println!("cargo:rerun-if-changed=../scripts/prepare-ocr-runtime.sh");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-env-changed=SDKROOT");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let status = std::process::Command::new("bash")
            .arg(manifest.join("../scripts/prepare-ocr-runtime.sh"))
            .env(
                "PINEFETCH_OCR_TARGET_ARCH",
                std::env::var("CARGO_CFG_TARGET_ARCH").unwrap(),
            )
            .status()
            .expect("Could not run the build-time Apple Vision helper compiler");
        assert!(status.success(), "Apple Vision helper compilation failed");
    }
    // Tauri overlays development resources and follows existing destination
    // symlinks. Remove this generated copy while Cargo owns the build lock, so
    // a former venv cannot retain pyvenv.cfg or links into a host installation.
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let profile_dir = out_dir
        .ancestors()
        .find(|path| path.file_name() == Some(std::ffi::OsStr::new("build")))
        .and_then(std::path::Path::parent)
        .expect("Could not resolve Cargo's profile resource directory");
    let whisper_copy = profile_dir.join("resources/whisper-runtime");
    match std::fs::remove_dir_all(&whisper_copy) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("Could not reset generated Whisper resources: {error}"),
    }
    tauri_build::build()
}
