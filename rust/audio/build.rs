// The WebAssembly build imports the host's math functions (src/lib.rs,
// host): leave them undefined at link time so they stay imports the host
// fills, rather than failing the link.
fn main() {
    if std::env::var("TARGET").is_ok_and(|t| t == "wasm32-unknown-unknown") {
        println!("cargo:rustc-link-arg=--allow-undefined");
    }
}
