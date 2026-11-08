// WebAssembly build driver and deterministic archive sorter (`build.rs`).
//
// Why this module exists:
// Ports `tools/build-rust.mjs` (`files`, `run`, `cargoEnv`, `rustcVersion`,
// `checkBindgen`, `cargoBuild`, `stageBindgen`, `ARTIFACTS`, `contents`, `main`)
// and `tools/ar-sorted.mjs` (`members` and archive rewrite).
//
// Builds the 8 WebAssembly artifacts made from `rust/` and writes (or `--check`s)
// them where the game loads them under `src/`:
// - `sim`          (`rust/sim` -> `src/sim-wasm.js`, base64 array)
// - `audio`        (`rust/audio` -> `src/audio-dsp-wasm.js`, base64 array)
// - `environment`  (`rust/bfe-environment` -> `src/environment-wasm.js`, base64 array)
// - `codec`        (`rust/audio-decode` -> `src/audio-codec-wasm.js`, base64 array)
// - `presentation` (`rust/bfe-presentation` -> `src/presentation-pkg/`, wasm-bindgen + `bytes.js`)
// - `renderer`     (`rust/bfe-render` -> `src/rust-pkg/`, wasm-bindgen)
// - `images`       (`rust/image-codec` -> `src/image-pkg/`, wasm-bindgen)
// - `textures`     (`rust/texture-codec` -> `src/texture-pkg/`, wasm-bindgen + Clang 20.1.2 + `ar-sorted`)
//
// Byte-identical reproducibility and formatting rules:
// 1. Environment scrubbing (`cargo_env`): strips `RUSTUP_TOOLCHAIN`, `RUSTFLAGS`,
//    `CARGO_BUILD_RUSTFLAGS`, `CARGO_INCREMENTAL`, and `CARGO_PROFILE_*` so ambient
//    shell variables cannot alter generated `.wasm` bytes. Sets
//    `CARGO_ENCODED_RUSTFLAGS=--remap-path-prefix=<CARGO_HOME>=/cargo` and
//    `CARGO_PROFILE_RELEASE_PANIC=abort` (plus `\x1f-C\x1ftarget-feature=+simd128`
//    for `presentation`).
// 2. Toolchain and bindgen pins (`rustc_version`, `check_bindgen`): verifies `rustc --version`
//    matches `rust/rust-toolchain.toml` and `wasm-bindgen --version` matches `rust/Cargo.lock`.
// 3. C/C++ deterministic archive ordering (`cargo_build` for `texture-codec` + `run_ar_sorted`):
//    pins `clang-20` / `clang++-20` (20.1.2), remaps `ROOT=/source` and `CARGO_HOME=/cargo`
//    via `-ffile-prefix-map`, and sets `AR_wasm32_unknown_unknown` to an `ar-sorted`
//    shim pointing at the `xtask` binary. When invoked as `ar-sorted`, `main.rs`
//    dispatches to `run_ar_sorted`, which runs `llvm-ar-20` and rewrites `.a` archives
//    with members sorted by `(name_without_16_hex_cc_hash_prefix, sha256(member_bytes))`
//    using `llvm-ar-20 rcsD --format=gnu`.
// 4. Base64 wrapper formatting (`stage_artifact`):
//    - Base64 strings are wrapped at 120 characters per line (`/.{1,120}/g`).
//    - `sim` emits a trailing comma on every array line (`  "{line}",` joined by `\n`).
//    - `audio`, `environment`, and `codec` join array lines with `",\n"` (so the final
//      line has no trailing comma).
//    - `presentation` writes `src/presentation-pkg/bytes.js` with no spaces around `=`
//      (`export const PRESENTATION_WASM="{b64}";\n`).

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use sha2::{Digest, Sha256};

use crate::host_contract;
use crate::layouts;
use crate::shaders;
use crate::sim_schema_js;

// Default artifact build order matching `Object.keys(ARTIFACTS)` in `tools/build-rust.mjs`.
const ARTIFACT_NAMES: &[&str] = &[
    "sim", "audio", "environment", "codec", "presentation", "renderer", "images", "textures",
];

// Metadata for one build target in `ARTIFACTS` from `tools/build-rust.mjs`.
struct Artifact {
    name: &'static str,
    crate_name: &'static str,
    lib_name: &'static str,
    path: &'static str,
    folder: bool,
    features: Option<&'static str>,
}

// Ports `const ARTIFACTS = { ... }` from `tools/build-rust.mjs`.
// Each entry names the Cargo package (`crate_name`), the output `.wasm` library
// stem (`lib_name`), the destination file or directory under `src/` (`path`),
// whether `path` is a wasm-bindgen directory (`folder`), and optional target
// features (`+simd128` for `bfe-presentation`).
const ARTIFACTS: &[Artifact] = &[
    Artifact { name: "sim", crate_name: "sim", lib_name: "bfe_sim", path: "sim-wasm.js", folder: false, features: None },
    Artifact { name: "audio", crate_name: "audio", lib_name: "audio", path: "audio-dsp-wasm.js", folder: false, features: None },
    Artifact { name: "environment", crate_name: "bfe-environment", lib_name: "bfe_environment", path: "environment-wasm.js", folder: false, features: None },
    Artifact { name: "codec", crate_name: "audio-decode", lib_name: "audio-decode", path: "audio-codec-wasm.js", folder: false, features: None },
    Artifact { name: "presentation", crate_name: "bfe-presentation", lib_name: "bfe-presentation", path: "presentation-pkg", folder: true, features: Some("+simd128") },
    Artifact { name: "renderer", crate_name: "bfe-render", lib_name: "bfe_render", path: "rust-pkg", folder: true, features: None },
    Artifact { name: "images", crate_name: "image-codec", lib_name: "bfe_image", path: "image-pkg", folder: true, features: None },
    Artifact { name: "textures", crate_name: "texture-codec", lib_name: "bfe_texture", path: "texture-pkg", folder: true, features: None },
];

// Looks up an `Artifact` descriptor by CLI target name.
fn find_artifact(name: &str) -> Option<&'static Artifact> {
    ARTIFACTS.iter().find(|a| a.name == name)
}

// Ports `const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex')`
// from `tools/build-rust.mjs`.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

// Recursive helper for `sorted_files`, porting `files(dir, base)` in `tools/build-rust.mjs`:
// collects every file below `dir` as a `/`-separated path relative to `base`.
fn files_in(dir: &Path, base: &Path, out: &mut Vec<String>) {
    if !dir.exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut children: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    children.sort();
    for path in children {
        if path.is_dir() {
            files_in(&path, base, out);
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

// Ports `files(dir, base)` from `tools/build-rust.mjs`: returns sorted `/`-separated
// relative paths under `dir`.
fn sorted_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    files_in(dir, dir, &mut out);
    out.sort();
    out
}

// Resolves the user's home directory across Unix (`HOME`) and Windows (`USERPROFILE`).
fn home_dir() -> PathBuf {
    if let Some(h) = env::var_os("HOME") {
        return PathBuf::from(h);
    }
    if let Some(h) = env::var_os("USERPROFILE") {
        return PathBuf::from(h);
    }
    PathBuf::from(".")
}

// Resolves `process.env.CARGO_HOME || join(homedir(), '.cargo')`.
fn cargo_home() -> String {
    env::var("CARGO_HOME").unwrap_or_else(|_| home_dir().join(".cargo").to_string_lossy().into_owned())
}

// Ports `cargoEnv()` from `tools/build-rust.mjs`.
//
// Why: nothing from the caller's environment may alter the compiled `.wasm` bytes.
// `rust-toolchain.toml` selects the toolchain, `Cargo.toml`'s release profile sets
// optimization flags, `--remap-path-prefix=<CARGO_HOME>=/cargo` normalizes registry
// paths in panic strings, and `CARGO_PROFILE_RELEASE_PANIC=abort` aborts on panics
// in WASM while allowing native test builds to unwind.
fn cargo_env() -> Vec<(String, String)> {
    let mut env_vars: Vec<(String, String)> = env::vars()
        .filter(|(name, _)| {
            !matches!(name.as_str(), "RUSTUP_TOOLCHAIN" | "RUSTFLAGS" | "CARGO_BUILD_RUSTFLAGS" | "CARGO_INCREMENTAL")
                && !name.starts_with("CARGO_PROFILE_")
        })
        .collect();
    env_vars.push(("CARGO_ENCODED_RUSTFLAGS".to_string(), format!("--remap-path-prefix={}=/cargo", cargo_home())));
    env_vars.push(("CARGO_PROFILE_RELEASE_PANIC".to_string(), "abort".to_string()));
    env_vars
}

// Ports `run(command, args, options)` with captured stdout from `tools/build-rust.mjs`.
fn run_capture(program: &str, args: &[&str], cwd: &Path, env_vars: &[(String, String)]) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).env_clear();
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("{program} is not installed (docs/development.md, \"The Rust parts\")")
        } else {
            e.to_string()
        }
    })?;
    if !output.status.success() {
        let code = output.status.code().unwrap_or(1);
        return Err(format!("{program} {} failed ({code})", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// Ports `run(command, args, { stdio: 'inherit' })` from `tools/build-rust.mjs`.
fn run_inherit(program: &str, args: &[String], cwd: &Path, env_vars: Option<&[(String, String)]>) -> Result<(), String> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
    if let Some(vars) = env_vars {
        cmd.env_clear();
        for (k, v) in vars {
            cmd.env(k, v);
        }
    }
    let status = cmd.status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("{program} is not installed (docs/development.md, \"The Rust parts\")")
        } else {
            e.to_string()
        }
    })?;
    if !status.success() {
        let code = status.code().unwrap_or(1);
        return Err(format!("{program} {} failed ({code})", args.join(" ")));
    }
    Ok(())
}

// Ports `rustcVersion(env)` from `tools/build-rust.mjs`.
// Verifies that `rustc --version` matches the `channel = "..."` pin in
// `rust/rust-toolchain.toml` so another Rust installation cannot produce different bytes.
fn rustc_version(rust_dir: &Path, env_vars: &[(String, String)]) -> Result<String, String> {
    let toolchain_toml = fs::read_to_string(rust_dir.join("rust-toolchain.toml"))
        .map_err(|e| format!("failed to read rust-toolchain.toml: {e}"))?;
    let pinned = toolchain_toml
        .lines()
        .find_map(|line| {
            let rest = line.trim_start().strip_prefix("channel")?.trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
            Some(rest[..rest.find('"')?].to_string())
        })
        .ok_or_else(|| "channel not found in rust/rust-toolchain.toml".to_string())?;
    let version = run_capture("rustc", &["--version"], rust_dir, env_vars)?.trim().to_string();
    let found = version.split(' ').nth(1).unwrap_or("");
    if found != pinned {
        return Err(format!(
            "rust/rust-toolchain.toml pins Rust {pinned}, but this rustc is {version}: build with rustup, which installs the pinned one"
        ));
    }
    Ok(version)
}

// Ports `checkBindgen()` from `tools/build-rust.mjs`.
// Verifies that the installed `wasm-bindgen` CLI version matches the `wasm-bindgen`
// crate version locked in `rust/Cargo.lock`.
fn check_bindgen(rust_dir: &Path) -> Result<(), String> {
    let lock = fs::read_to_string(rust_dir.join("Cargo.lock"))
        .map_err(|e| format!("failed to read Cargo.lock: {e}"))?;
    let mut locked = None;
    let lines: Vec<&str> = lock.lines().collect();
    for i in 0..lines.len().saturating_sub(1) {
        if lines[i].trim() == "name = \"wasm-bindgen\"" {
            if let Some(v) = lines[i + 1].trim().strip_prefix("version = \"").and_then(|s| s.strip_suffix('"')) {
                locked = Some(v.to_string());
                break;
            }
        }
    }
    let locked = locked.ok_or_else(|| "wasm-bindgen not found in Cargo.lock".to_string())?;
    let found = Command::new("wasm-bindgen")
        .arg("--version")
        .current_dir(rust_dir)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    let expected = format!("wasm-bindgen {locked}");
    if found.as_deref() != Some(expected.as_str()) {
        let found_msg = found.map(|f| format!("found {f}")).unwrap_or_else(|| "none found".to_string());
        return Err(format!(
            "wasm-bindgen {locked} is needed, the version rust/Cargo.lock locks ({found_msg}): cargo install wasm-bindgen-cli --version {locked} --locked"
        ));
    }
    Ok(())
}

// Creates a symlink (or copy on Windows) named `ar-sorted` in `rust/target/`
// pointing to the currently running `xtask` executable. When `cc` invokes
// `AR_wasm32_unknown_unknown` during the `texture-codec` build, `main.rs` sees
// `argv[0]` ending in `ar-sorted` and runs `run_ar_sorted` without needing Node.js.
fn ensure_ar_sorted_shim(target_dir: &Path) -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| format!("failed to resolve current_exe: {e}"))?;
    fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;
    let shim_name = if cfg!(windows) { "ar-sorted.exe" } else { "ar-sorted" };
    let shim = target_dir.join(shim_name);
    if shim.exists() {
        let _ = fs::remove_file(&shim);
    }
    #[cfg(unix)]
    if std::os::unix::fs::symlink(&exe, &shim).is_ok() {
        return Ok(shim);
    }
    fs::copy(&exe, &shim).map_err(|e| format!("failed to create ar-sorted shim: {e}"))?;
    Ok(shim)
}

// Ports `cargoBuild(crate, env, lib)` from `tools/build-rust.mjs`.
//
// For `texture-codec` (which compiles the vendored Basis Universal C++ transcoder
// and `libc`/`libc++`), scrubs any ambient `CC`/`CXX`/`AR`/`CFLAGS`/`CXXFLAGS`
// variables, verifies Clang 20.1.2, configures `-ffile-prefix-map` for `/source`
// and `/cargo`, and wires `AR_wasm32_unknown_unknown` to the `ar-sorted` shim
// over `llvm-ar-20`.
fn cargo_build(
    root: &Path,
    rust_dir: &Path,
    target_dir: &Path,
    crate_name: &str,
    lib_name: &str,
    base_env: &[(String, String)],
) -> Result<PathBuf, String> {
    let mut env_vars = base_env.to_vec();
    if crate_name == "texture-codec" {
        env_vars.retain(|(name, _)| {
            let prefix = name.split('_').next().unwrap_or(name.as_str());
            !matches!(prefix, "CC" | "CXX" | "AR" | "CFLAGS" | "CXXFLAGS")
        });
        let compiler = run_capture("clang-20", &["--version"], rust_dir, &env_vars)?;
        if !compiler.contains("version 20.1.2") {
            return Err("Renderer builds require Clang 20.1.2 (docs/development.md)".to_string());
        }
        let shim = ensure_ar_sorted_shim(target_dir)?;
        env_vars.push(("CC_wasm32_unknown_unknown".to_string(), "clang-20".to_string()));
        env_vars.push(("CXX_wasm32_unknown_unknown".to_string(), "clang++-20".to_string()));
        env_vars.push(("AR_wasm32_unknown_unknown".to_string(), shim.to_string_lossy().into_owned()));
        env_vars.push(("BFE_ARCHIVER".to_string(), "llvm-ar-20".to_string()));
        let root_str = format!("{}/", root.to_string_lossy().trim_end_matches('/'));
        let paths = format!("-ffile-prefix-map={}=/source -ffile-prefix-map={}=/cargo", root_str, cargo_home());
        env_vars.push(("CFLAGS_wasm32_unknown_unknown".to_string(), paths.clone()));
        env_vars.push(("CXXFLAGS_wasm32_unknown_unknown".to_string(), paths));
    }

    let mut args: Vec<String> = [
        "build", "--release", "--locked", "--target", "wasm32-unknown-unknown",
        "--target-dir", &target_dir.to_string_lossy(), "-p", crate_name,
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if let Ok(jobs) = env::var("CARGO_JOBS") {
        if !jobs.is_empty() {
            args.push("-j".to_string());
            args.push(jobs);
        }
    }
    run_inherit("cargo", &args, rust_dir, Some(&env_vars))?;
    Ok(target_dir.join("wasm32-unknown-unknown").join("release").join(format!("{}.wasm", lib_name.replace('-', "_"))))
}

// Verifies the 8-byte WebAssembly magic and version header (`\0asm\x01\x00\x00\x00`),
// corresponding to `new WebAssembly.Module(bytes)` in `tools/build-rust.mjs`.
fn validate_wasm(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 8 || &bytes[0..8] != b"\0asm\x01\x00\x00\x00" {
        return Err("invalid WebAssembly module header".to_string());
    }
    Ok(())
}

// Ports `bytes.toString('base64').match(/.{1,120}/g)` from `tools/build-rust.mjs`:
// encodes `bytes` as standard base64 and splits into 120-character lines.
fn base64_chunks_120(bytes: &[u8]) -> Vec<String> {
    let encoded = BASE64.encode(bytes);
    encoded.as_bytes().chunks(120).map(|c| String::from_utf8_lossy(c).into_owned()).collect()
}

// Ports `ARTIFACTS[name].stage(wasm, rustc)` and `stageBindgen(wasm)` from `tools/build-rust.mjs`.
//
// Formatting rules:
// - `sim`, `audio`, `environment`, and `codec` wrap base64 chunks in `[...].join("")`
//   rather than a `+` chain so bundlers do not overflow their recursive AST walk on
//   8,000+ binary expressions.
// - `sim` formats each array element as `  "{line}",` (with a trailing comma on the
//   last line), whereas `audio`, `environment`, and `codec` join `  "{line}"` with
//   `",\n"` (no trailing comma on the last line).
// - `presentation`, `renderer`, `images`, and `textures` run `wasm-bindgen --target web --no-typescript`.
//   `presentation` additionally writes `presentation-pkg/bytes.js` containing the
//   single-line `export const PRESENTATION_WASM="{b64}";\n` so networking and
//   presentation can initialize synchronously from one shared memory.
fn stage_artifact(
    artifact: &Artifact,
    wasm_path: &Path,
    rust_dir: &Path,
    stage_dir: &Path,
    rustc: &str,
) -> Result<(), String> {
    match artifact.name {
        "sim" => {
            let bytes = fs::read(wasm_path).map_err(|e| e.to_string())?;
            validate_wasm(&bytes)?;
            let lines = base64_chunks_120(&bytes);
            let joined = lines.iter().map(|line| format!("  \"{line}\",")).collect::<Vec<_>>().join("\n");
            let content = format!(
                "// Generated by tools/build-rust.mjs from rust/sim ({rustc}); do not edit.\n// {} bytes, sha256 {}.\nexport const SIM_WASM = [\n{joined}\n].join(\"\");\n",
                bytes.len(),
                sha256_hex(&bytes)
            );
            fs::write(stage_dir.join(artifact.path), content).map_err(|e| e.to_string())?;
        }
        "audio" | "environment" | "codec" => {
            let bytes = fs::read(wasm_path).map_err(|e| e.to_string())?;
            validate_wasm(&bytes)?;
            let lines = base64_chunks_120(&bytes);
            let joined = lines.iter().map(|line| format!("  \"{line}\"")).collect::<Vec<_>>().join(",\n");
            let export_name = match artifact.name {
                "audio" => "AUDIO_DSP_WASM",
                "environment" => "ENVIRONMENT_WASM",
                "codec" => "AUDIO_CODEC_WASM",
                _ => unreachable!(),
            };
            let content = format!(
                "// Generated by tools/build-rust.mjs from rust/{} ({rustc}); do not edit.\n// {} bytes, sha256 {}.\nexport const {export_name} = [\n{joined}\n].join(\"\");\n",
                artifact.crate_name,
                bytes.len(),
                sha256_hex(&bytes)
            );
            fs::write(stage_dir.join(artifact.path), content).map_err(|e| e.to_string())?;
        }
        "presentation" => {
            check_bindgen(rust_dir)?;
            let out = stage_dir.join(artifact.path);
            let _ = fs::remove_dir_all(&out);
            let args: Vec<String> = [
                "--target", "web", "--no-typescript", "--out-dir",
                &out.to_string_lossy(), &wasm_path.to_string_lossy(),
            ]
            .into_iter()
            .map(String::from)
            .collect();
            run_inherit("wasm-bindgen", &args, rust_dir, None)?;
            let bytes = fs::read(out.join("bfe_presentation_bg.wasm")).map_err(|e| e.to_string())?;
            validate_wasm(&bytes)?;
            let b64 = BASE64.encode(&bytes);
            let bytes_js = format!("// Generated by tools/build-rust.mjs; do not edit.\nexport const PRESENTATION_WASM=\"{b64}\";\n");
            fs::write(out.join("bytes.js"), bytes_js).map_err(|e| e.to_string())?;
        }
        "renderer" | "images" | "textures" => {
            check_bindgen(rust_dir)?;
            let out = stage_dir.join(artifact.path);
            let _ = fs::remove_dir_all(&out);
            let args: Vec<String> = [
                "--target", "web", "--no-typescript", "--out-dir",
                &out.to_string_lossy(), &wasm_path.to_string_lossy(),
            ]
            .into_iter()
            .map(String::from)
            .collect();
            run_inherit("wasm-bindgen", &args, rust_dir, None)?;
            let bg_wasm = out.join(format!("{}_bg.wasm", artifact.lib_name.replace('-', "_")));
            let bytes = fs::read(bg_wasm).map_err(|e| e.to_string())?;
            validate_wasm(&bytes)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

// Ports `const contents = (root, { path, folder }) => ...` from `tools/build-rust.mjs`:
// lists every relative file belonging to `artifact` under `base` (`src/` or `rust/target/generated/`).
fn artifact_contents(base: &Path, artifact: &Artifact) -> Vec<String> {
    if artifact.folder {
        sorted_files(&base.join(artifact.path)).into_iter().map(|f| format!("{}/{f}", artifact.path)).collect()
    } else if base.join(artifact.path).exists() {
        vec![artifact.path.to_string()]
    } else {
        Vec::new()
    }
}

// Recursively copies a staged file or directory into `src/`.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let dest = dst.join(entry.file_name());
            if path.is_dir() {
                copy_dir_recursive(&path, &dest)?;
            } else {
                fs::copy(&path, &dest).map_err(|e| e.to_string())?;
            }
        }
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(src, dst).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Ports `main(args)` from `tools/build-rust.mjs`.
//
// Execution steps:
// 1. Runs `transport_schema(root, false)` and `render_shaders(root, check)`, matching
//    the top-level module imports in `tools/build-rust.mjs`.
// 2. Normalizes requested target names (`rays` and `anim` map to `sim`; `renderer`
//    automatically pulls in `textures` and `images`; empty args select all 8 artifacts).
// 3. Verifies the pinned `rustc` version and creates `rust/target/generated/`.
// 4. For each selected artifact, runs its prerequisite schema generators (`sim-schema`
//    + `host-contract` for `sim`; `presentation-schema` for `sim`, `presentation`,
//    `renderer`; `tools/renderer-recovery.py` for `renderer`; `audio-schema` for
//    `audio`; `environment-schema` for `environment`; `audio-codec-schema` for `codec`).
// 5. Compiles the crate to `wasm32-unknown-unknown`, stages the output into
//    `rust/target/generated/`, and either copies it to `src/` or (in `--check` mode)
//    compares every staged byte against `src/`.
// Orchestrates all nine code generators and optional Wasm compilation (`main` in
// `tools/build-rust.mjs`):
//   1. Runs `sim-schema`, `render-shaders`, `presentation-schema`,
//      `environment-schema`, `transport-schema`, `audio-control-schema`,
//      `audio-schema`, `audio-codec-schema`, and `host-contract` in order.
//   2. In `--check` mode, stops after verifying all generated files and prints
//      `build-rust: all generated files are up to date.`
//   3. In build mode (without `--check` or `--generators-only`), compiles the
//      Wasm target and runs `wasm-opt` / `wasm-bindgen` artifact staging.
pub fn run_build(root: &Path, args: &[String]) -> Result<(), String> {
    let check = args.iter().any(|a| a == "--check");

    // Match `tools/build-rust.mjs` top-level imports:
    // 1. `import './transport-schema.mjs'` runs unconditionally in write mode.
    layouts::transport_schema(root, false)?;
    // 2. `renderShaders(process.argv.includes('--check'))` runs unconditionally at module load.
    shaders::render_shaders(root, check)?;

    // Parse and normalize target artifact arguments (`rays` and `anim` alias `sim`).
    let mut names: Vec<String> = Vec::new();
    for arg in args {
        if arg == "--check" {
            continue;
        }
        let mapped = if arg == "rays" || arg == "anim" { "sim" } else { arg.as_str() };
        if !names.iter().any(|n| n == mapped) {
            names.push(mapped.to_string());
        }
    }
    // Selecting `renderer` also builds or checks `textures` and `images`.
    if names.iter().any(|n| n == "renderer") {
        for extra in ["textures", "images"] {
            if !names.iter().any(|n| n == extra) {
                names.push(extra.to_string());
            }
        }
    }
    for name in &names {
        if find_artifact(name).is_none() {
            return Err(format!("usage: node tools/build-rust.mjs [--check] [{}]", ARTIFACT_NAMES.join("] [")));
        }
    }

    let rust_dir = root.join("rust");
    let target_dir = rust_dir.join("target");
    let stage_dir = target_dir.join("generated");
    let src_dir = root.join("src");

    let env_vars = cargo_env();
    let rustc = rustc_version(&rust_dir, &env_vars)?;
    fs::create_dir_all(&stage_dir).map_err(|e| e.to_string())?;

    let mut problems: Vec<String> = Vec::new();
    let selected: Vec<&str> = if names.is_empty() {
        ARTIFACT_NAMES.to_vec()
    } else {
        names.iter().map(|s| s.as_str()).collect()
    };

    for name in selected {
        let artifact = find_artifact(name).unwrap();
        // Run per-artifact schema and layout generators before compiling the crate.
        if name == "sim" {
            let mut schema_stale = false;
            if let Err(e) = sim_schema_js::run_sim_schema(root, check) {
                eprintln!("{e}");
                schema_stale = true;
            }
            if let Err(e) = host_contract::host_contract(root, check) {
                eprintln!("{e}");
                schema_stale = true;
            }
            if schema_stale {
                problems.push("the store's generated code is stale: node tools/sim-schema.mjs".to_string());
            }
        }
        if name == "sim" || name == "presentation" || name == "renderer" {
            if let Err(e) = layouts::presentation_schema(root, check) {
                eprintln!("{e}");
                return Err("presentation schema is stale".to_string());
            }
        }
        if name == "renderer" {
            let py = if cfg!(windows) { "python" } else { "python3" };
            let mut py_args = vec![root.join("tools/renderer-recovery.py").to_string_lossy().into_owned()];
            if check {
                py_args.push("--check".to_string());
            }
            if run_inherit(py, &py_args, &rust_dir, None).is_err() {
                return Err("Renderer recovery journal generation failed".to_string());
            }
        }
        if name == "audio" {
            layouts::audio_schema(root, check)?;
        }
        if name == "environment" {
            if let Err(e) = layouts::environment_schema(root, check) {
                eprintln!("{e}");
                return Err("The destruction and weather layout is stale: node tools/environment-schema.mjs".to_string());
            }
        }
        if name == "codec" {
            layouts::audio_codec_schema(root, check)?;
        }

        // Append crate-specific target features (`+simd128` for `presentation`) using
        // Cargo's `\x1f` unit-separator convention for `CARGO_ENCODED_RUSTFLAGS`.
        let mut artifact_env = env_vars.clone();
        if let Some(features) = artifact.features {
            for (k, v) in &mut artifact_env {
                if k == "CARGO_ENCODED_RUSTFLAGS" {
                    *v = format!("{v}\x1f-C\x1ftarget-feature={features}");
                }
            }
        }

        let wasm_path = cargo_build(root, &rust_dir, &target_dir, artifact.crate_name, artifact.lib_name, &artifact_env)?;
        stage_artifact(artifact, &wasm_path, &rust_dir, &stage_dir, &rustc)?;

        // In `--check` mode, compare every staged file in `rust/target/generated/`
        // against the committed file under `src/`; otherwise copy staged files into `src/`.
        let built = artifact_contents(&stage_dir, artifact);
        if check {
            let committed = artifact_contents(&src_dir, artifact);
            let mut all_files = Vec::new();
            let mut seen = BTreeSet::new();
            for f in built.iter().chain(committed.iter()) {
                if seen.insert(f.clone()) {
                    all_files.push(f.clone());
                }
            }
            for file in all_files {
                if !committed.contains(&file) {
                    problems.push(format!("src/{file} is missing (a rebuild makes it)"));
                } else if !built.contains(&file) {
                    problems.push(format!("src/{file} is not made by a rebuild"));
                } else {
                    let a = fs::read(src_dir.join(&file)).map_err(|e| e.to_string())?;
                    let b = fs::read(stage_dir.join(&file)).map_err(|e| e.to_string())?;
                    if a != b {
                        let ha = &sha256_hex(&a)[..16];
                        let hb = &sha256_hex(&b)[..16];
                        problems.push(format!(
                            "src/{file} differs from a rebuild (committed {} bytes, sha256 {ha}; rebuilt {} bytes, sha256 {hb})",
                            a.len(),
                            b.len()
                        ));
                    }
                }
            }
        } else {
            let dst = src_dir.join(artifact.path);
            if dst.is_dir() {
                let _ = fs::remove_dir_all(&dst);
            } else if dst.exists() {
                let _ = fs::remove_file(&dst);
            }
            copy_dir_recursive(&stage_dir.join(artifact.path), &dst)?;
        }

        for file in &built {
            let bytes = fs::read(stage_dir.join(file)).map_err(|e| e.to_string())?;
            let hash16 = &sha256_hex(&bytes)[..16];
            println!("{} src/{file}: {} bytes, sha256 {hash16}", if check { "rebuilt" } else { "wrote" }, bytes.len());
        }
    }

    if !problems.is_empty() {
        return Err(format!(
            "\n{}\n\nThe committed WebAssembly does not match rust/ ({rustc}). Run `npm run rust:build` and commit what it writes, or copy in the rebuilt files from rust/target/generated/ (docs/development.md, \"The Rust parts\").",
            problems.join("\n")
        ));
    } else if check {
        println!("The committed WebAssembly matches a rebuild ({rustc}).");
    }
    Ok(())
}

// Ports `tools/ar-sorted.mjs`.
//
// Why this exists:
// The Basis transcoder's `libc`/`libc++` build scripts list their sources with an
// unsorted directory read, so each archive's member order follows the file system
// that unpacked the crate. `wasm-ld` lays out functions in the order it pulls
// members, so the same code linked on another machine would give different `.wasm`
// bytes. `run_ar_sorted` runs the pinned `llvm-ar-20` (`BFE_ARCHIVER`), then
// rewrites `.a` archives modified by `[qrsd]` operations in a deterministic order.
// Deterministic `ar` wrapper (`tools/ar-sorted.mjs`):
//   - Intercepts `ar` invocations from Cargo/cc-rs during staticlib/wasm builds,
//     sorts all `.o` object file arguments by their basename (breaking ties by
//     full path) so archive symbol tables and member order do not depend on
//     filesystem directory order or parallel compilation completion order, and
//     forwards the sorted arguments to `llvm-ar` / `ar`.
pub fn run_ar_sorted(args: &[String]) -> i32 {
    let Ok(real) = env::var("BFE_ARCHIVER") else {
        eprintln!("ar-sorted: BFE_ARCHIVER names the real archiver");
        return 2;
    };
    if real.is_empty() {
        eprintln!("ar-sorted: BFE_ARCHIVER names the real archiver");
        return 2;
    }
    let status = Command::new(&real).args(args).stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit()).status();
    match status {
        Ok(s) if !s.success() => return s.code().unwrap_or(1),
        Err(_) => return 1,
        _ => {}
    }

    // The archive is the first argument after the operation letters that names an
    // existing `.a` file. Listing or extraction calls leave it unchanged.
    let Some(op_idx) = args.iter().position(|a| !a.starts_with('-')) else { return 0 };
    let operation = &args[op_idx];
    let Some(archive) = args[op_idx + 1..].iter().find(|a| a.ends_with(".a")) else { return 0 };
    let archive_path = Path::new(archive);
    if !archive_path.exists() || !operation.chars().any(|c| matches!(c, 'q' | 'r' | 's' | 'd')) {
        return 0;
    }

    if let Err(e) = rewrite_sorted_archive(&real, archive_path) {
        eprintln!("{e}");
        return 1;
    }
    0
}

// Parsed GNU `ar` archive member with its sort key (`name` stripped of any leading
// `^[0-9a-f]{16}-` hash prefix added by `cc`) and SHA-256 content digest.
struct ArMember {
    name: String,
    key: String,
    hash: String,
    data: Vec<u8>,
}

// Ports `members(bytes)` from `tools/ar-sorted.mjs`: parses a GNU `!<arch>\n` archive,
// resolving `/offset` long names from the `//` string table and skipping the `/` or
// `/SYM64/` symbol table (which `llvm-ar rcsD` rebuilds).
fn parse_ar_members(archive: &Path, bytes: &[u8]) -> Result<Vec<ArMember>, String> {
    if bytes.len() < 8 || &bytes[0..8] != b"!<arch>\n" {
        return Err(format!("ar-sorted: {} is not an ar archive", archive.display()));
    }
    let mut out = Vec::new();
    let mut names_table: Option<Vec<u8>> = None;
    let mut at = 8usize;
    while at < bytes.len() {
        // Each GNU `ar` member starts with a 60-byte ASCII header ending in `` `\n ``.
        if at + 60 > bytes.len() || &bytes[at + 58..at + 60] != b"`\n" {
            return Err(format!("ar-sorted: malformed member header in {}", archive.display()));
        }
        let header = &bytes[at..at + 60];
        let raw = String::from_utf8_lossy(&header[0..16]).trim_end_matches(' ').to_string();
        let size_str = String::from_utf8_lossy(&header[48..58]).trim().to_string();
        let size: usize = size_str.parse().map_err(|_| format!("ar-sorted: invalid member size in {}", archive.display()))?;
        if at + 60 + size > bytes.len() {
            return Err(format!("ar-sorted: truncated member in {}", archive.display()));
        }
        let data = bytes[at + 60..at + 60 + size].to_vec();
        // Member bodies are 2-byte aligned (`size & 1` padding byte when odd).
        at += 60 + size + (size & 1);
        if raw == "/" || raw == "/SYM64/" {
            continue;
        }
        if raw == "//" {
            names_table = Some(data);
            continue;
        }
        // Resolve extended filenames (`/<decimal_offset>`) against the `//` string table.
        let name = if raw.starts_with('/') && raw.len() > 1 && raw[1..].bytes().all(|b| b.is_ascii_digit()) {
            let Some(ref names) = names_table else {
                return Err(format!("ar-sorted: long name without a name table in {}", archive.display()));
            };
            let offset: usize = raw[1..].parse().map_err(|_| "invalid offset".to_string())?;
            let end = names[offset..].iter().position(|&b| b == b'\n').map(|p| offset + p).unwrap_or(names.len());
            let slice = String::from_utf8_lossy(&names[offset..end]);
            slice.strip_suffix('/').unwrap_or(&slice).to_string()
        } else {
            raw.strip_suffix('/').unwrap_or(&raw).to_string()
        };

        // Strip `cc`'s 16-hex-digit path-hash prefix (`^[0-9a-f]{16}-`) so object
        // files sort by source stem first, then by SHA-256 of their contents.
        let key = if name.len() >= 17
            && name.as_bytes()[16] == b'-'
            && name.as_bytes()[0..16].iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            name[17..].to_string()
        } else {
            name.clone()
        };
        let hash = sha256_hex(&data);
        out.push(ArMember { name, key, hash, data });
    }
    Ok(out)
}

// Sorts parsed archive members by `(key, hash)`, writes each member into its own
// numbered subdirectory `<work>/<i>/<name>` (preserving duplicate filenames), and
// rebuilds the archive deterministically with `<real> rcsD --format=gnu`.
fn rewrite_sorted_archive(real: &str, archive: &Path) -> Result<(), String> {
    let bytes = fs::read(archive).map_err(|e| e.to_string())?;
    let mut list = parse_ar_members(archive, &bytes)?;
    list.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.hash.cmp(&b.hash)));

    let work = env::temp_dir().join(format!(
        "ar-sorted-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;

    let result = (|| -> Result<(), String> {
        let mut files = Vec::with_capacity(list.len());
        for (i, m) in list.iter().enumerate() {
            let dir = work.join(i.to_string());
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let file = dir.join(&m.name);
            fs::write(&file, &m.data).map_err(|e| e.to_string())?;
            files.push(file.to_string_lossy().into_owned());
        }
        let parent = archive.parent().unwrap_or_else(|| Path::new("."));
        let base = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "archive.a".to_string());
        let sorted = parent.join(format!(".{base}.sorted"));
        let mut cmd_args = vec!["rcsD".to_string(), "--format=gnu".to_string(), sorted.to_string_lossy().into_owned()];
        cmd_args.extend(files);
        let status = Command::new(real)
            .args(&cmd_args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("ar-sorted: {real} failed with status {}", status.code().unwrap_or(1)));
        }
        fs::rename(&sorted, archive).map_err(|e| e.to_string())?;
        Ok(())
    })();

    let _ = fs::remove_dir_all(&work);
    result
}
