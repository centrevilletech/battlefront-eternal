// Admission constructs the owning world before any devices are opened.
// Match the 8 MiB Linux main stack on MSVC rather than its 1 MiB default.
fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_include_paths(vec![std::path::PathBuf::from("../bfe-ui/ui")]);
    slint_build::compile_with_config("ui/overlay.slint", config)
        .expect("compile the native UI layer");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
