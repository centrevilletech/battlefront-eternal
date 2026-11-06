// Compile only the upstream transcoder and Zstandard decompressor. The small
// C ABI borrows Rust buffers; neither an encoder nor an Emscripten JS runtime
// is linked. All original GPU formats remain enabled, including PVRTC.
fn main() {
    let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap() == "wasm32";
    let mut cpp = cc::Build::new();
    cpp.cpp(true).std("c++17").warnings(false).define("NDEBUG",None)
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fno-strict-aliasing")
        .files(["src/bridge.cpp", "vendor/basis-universal/transcoder/basisu_transcoder.cpp"]);
    let mut c = cc::Build::new();
    c.warnings(false).define("NDEBUG",None).file("vendor/basis-universal/zstd/zstddeclib.c");
    if wasm {
        // Bare WebAssembly has no platform libc or libc++. Header order is
        // significant: libc++ wrappers must find their own C headers first.
        cpp.includes(paths("DEP_BASISU_WASM_LIBCXX_INCLUDE_LIBCXX"))
            .includes(paths("DEP_BASISU_WASM_LIBCXX_INCLUDE_LIBC"))
            .cpp_link_stdlib(None);
        c.includes(paths("DEP_BASISU_WASM_LIBCXX_INCLUDE_LIBC"));
    }
    cpp.compile("bfe_basis_transcode");
    c.compile("bfe_basis_zstd");
    // Changes to lookup tables must invalidate the compiled archive too.
    println!("cargo:rerun-if-changed=src/bridge.cpp");
    println!("cargo:rerun-if-changed=vendor");
}

fn paths(name: &str) -> Vec<std::path::PathBuf> {
    // The support crate publishes the same headers used to build its ABI.
    // A missing path is a configuration error, never a system-header guess.
    std::env::split_paths(&std::env::var_os(name).expect("Missing WebAssembly libc++ headers")).collect()
}
