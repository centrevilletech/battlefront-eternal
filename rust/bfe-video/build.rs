use std::{env, path::PathBuf, process::Command};
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    // One portable C configuration avoids assembler/toolchain dependencies on
    // MSVC and ARM. The decoder still runs independently of the render thread.
    let status = Command::new(if cfg!(windows) { "python" } else { "python3" })
        .arg(root.join("../../build/native/vpx-source.py"))
        .arg(&out)
        .status()
        .expect("prepare libvpx source");
    assert!(status.success(), "libvpx source preparation failed");
    let source = out.join("libvpx-1.17.0");
    let build = out.join("vpx-build");
    let files = std::fs::read_to_string(build.join("libvpx_srcs.txt")).unwrap();
    let mut cc = cc::Build::new();
    cc.std("c11")
        .include(&build)
        .include(&source)
        .warnings(false)
        .define("NDEBUG", None);
    if env::var("CARGO_CFG_TARGET_OS").unwrap() != "windows" {
        cc.define("_POSIX_C_SOURCE", "200809L");
    }
    for file in files.lines().filter(|s| s.ends_with(".c")) {
        let path = PathBuf::from(file);
        cc.file(if path.is_absolute() {
            path
        } else if source.join(&path).exists() {
            source.join(path)
        } else {
            build.join(path)
        });
    }
    cc.file("src/bridge.c").compile("bfe_vpx");
    println!("cargo:rerun-if-changed=src/bridge.c");
    println!("cargo:rerun-if-changed=../../build/native/vpx-source.py");
}
