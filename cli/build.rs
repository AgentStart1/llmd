fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "android" {
        return;
    }

    let library_dir = std::env::var("DEP_LITERT_LM_LIB_DIR")
        .expect("litertlm-sys did not report its native library directory");
    let library_name = std::env::var("DEP_LITERT_LM_LIB_FILENAME")
        .expect("litertlm-sys did not report its native library filename");
    let source = std::path::Path::new(&library_dir).join(&library_name);
    let out_dir = std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").expect("Cargo did not provide OUT_DIR"),
    );
    let binary_dir = out_dir
        .ancestors()
        .nth(3)
        .expect("unexpected Cargo OUT_DIR layout");
    let destination = binary_dir.join(&library_name);
    let already_staged = std::fs::read(&destination)
        .ok()
        .zip(std::fs::read(&source).ok())
        .is_some_and(|(destination_bytes, source_bytes)| destination_bytes == source_bytes);
    if !already_staged {
        std::fs::copy(&source, &destination)
            .expect("failed to copy LiteRT-LM native library beside the CLI binary");
    }

    match target_os.as_str() {
        "linux" => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN"),
        "macos" => println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path"),
        _ => {}
    }
}
