// Compile only the upstream transcoder and Zstandard decompressor. The small
// C ABI borrows Rust buffers; no encoder is linked. All original GPU formats remain enabled, including PVRTC.
fn main() {
    let mut cpp = cc::Build::new();
    cpp.cpp(true).std("c++17").warnings(false).define("NDEBUG",None)
        .flag_if_supported("-fno-exceptions")
        .flag_if_supported("-fno-strict-aliasing")
        // Clang fuses a multiply and an add into one rounding step unless told not to (GCC does
        // not under -std=c++17), and the BC7 blocks then differ from every other build's. Off:
        // the same texture bytes from every compiler and processor.
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
