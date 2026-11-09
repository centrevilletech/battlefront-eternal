// Admission constructs the owning world before any devices are opened.
// Match the 8 MiB Linux main stack on MSVC rather than its 1 MiB default.
fn main() {
    slint_build::compile("ui/overlay.slint").expect("compile the native UI layer");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
