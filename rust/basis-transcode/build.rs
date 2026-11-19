// Compile only the upstream transcoder and Zstandard decompressor. The small
// C ABI borrows Rust buffers; no encoder is linked. All original GPU formats remain enabled, including PVRTC.
fn main() {
    let mut cpp = cc::Build::new();
    cpp.cpp(true).std("c++17").warnings(false).define("NDEBUG",None)
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fno-strict-aliasing")
        // The transcoder's block choices come out of floating-point sums. A compiler that fuses a
        // multiply and an add into one instruction (Apple's on arm64 does by default) rounds them
        // differently and writes other BC7 blocks than every other build of this crate.
        .flag_if_supported("-ffp-contract=off")
        .files(["src/bridge.cpp", "vendor/basis-universal/transcoder/basisu_transcoder.cpp"]);
    let mut c = cc::Build::new();
    c.warnings(false).define("NDEBUG",None).file("vendor/basis-universal/zstd/zstddeclib.c");
    cpp.compile("bfe_basis_transcode");
    c.compile("bfe_basis_zstd");
    // Changes to lookup tables must invalidate the compiled archive too.
    println!("cargo:rerun-if-changed=src/bridge.cpp");
    println!("cargo:rerun-if-changed=vendor");
}
