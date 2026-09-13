fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "android" {
        let library_dir = std::env::var("DEP_LITERT_LM_LIB_DIR")
            .expect("litertlm-sys did not report its native library directory");
        let library_name = std::env::var("DEP_LITERT_LM_LIB_FILENAME")
            .expect("litertlm-sys did not report its native library filename");
        let source = std::path::Path::new(&library_dir).join(&library_name);
        let native_dir = std::path::Path::new("native");
        std::fs::create_dir_all(native_dir).expect("failed to create native library directory");
        std::fs::copy(&source, native_dir.join(&library_name))
            .expect("failed to stage LiteRT-LM native library for bundling");
    }
    if target_os == "linux" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/llmd");
    }
    tauri_build::build();
}
