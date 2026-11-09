//! Native renderer smoke binary (`render-smoke`).
//!
//! Exercises the complete native shader catalog (all 1,046 variants across the
//! 6 shader families in `rust/bfe-render/shaders/*/manifest.json`) through
//! `bfe-render`'s own program builder (`Bridge::add_owned_program`) and draws
//! one representative scene per family (`world`, `actors`, `effects`,
//! `interface`, `post-hud`, `utility`) into an offscreen RGBA8 + depth/stencil
//! target under either software GL (`llvmpipe`) or hardware GL.
//!
//! Why this binary exists alongside `static-frame`:
//! - `static-frame` validates static map component admission (`sky`, `props`,
//!   `foliage`, `water`), which touches only the handful of shader variants
//!   referenced by the synthetic `room` fixture.
//! - `render-smoke` closes the remaining gap between the shader catalog smoke and the native CI gate by compiling and
//!   linking every declared shader variant on a cold `Bridge` before executing
//!   all 6 shader families' draw passes (`WorldPass` + `FoliagePass`,
//!   `InstancePass`, `WaterPass` + `EffectsPass`, `InterfacePass`,
//!   `PostPass` + `HudPass`, and `Lines`) against the same offscreen target.
//! - Structured JSONL progress events (`variant-start`, `variant-result`,
//!   `family-result`, `family-draw`, and the final `runs` summary) mirror the
//!   schema of `tools/renderer-smoke.mjs` so CI and `tools/renderer-smoke-native.py`
//!   can cross-check the native and manifest catalogs without launching Chromium.
//!
//! Contract with adjacent crates:
//! - `bfe_render::programs::specs()` owns the compiled-in shader table. Every
//!   key in `rust/bfe-render/shaders/*/manifest.json` must resolve in `specs()`
//!   and pass `shader::rewrite` + GLSL compilation + program linking via
//!   `Bridge::add_owned_program`.
//! - `bfe_client::scene::Scene` owns offscreen `Canvas` creation (`RGBA8` color
//!   texture + `DEPTH24_STENCIL8` renderbuffer) and static map admission from
//!   `content/maps/<map>/`.
//! - `bfe_client::render_bridge::Bridge` exposes the native transport banks and
//!   `frame_aux_frame` entry points used by `FramePlan` to drive each pass.

#![deny(warnings, clippy::all)]

use bfe_client::{
    render_bridge::Bridge,
    resources::Ranges,
    scene::{Scene, View},
};
use glow::HasContext;
use glutin::{
    config::{ConfigSurfaceTypes, ConfigTemplateBuilder},
    context::{ContextApi, ContextAttributesBuilder, Version},
    prelude::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CString,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::Instant,
};

/// Canonical family order matching `tools/renderer-smoke-manifests.mjs`.
/// Keeping this order deterministic ensures JSONL diffs and contact-sheet
/// panel coordinates stay stable across software and hardware GL runs.
const FAMILY_ORDER: [&str; 6] = [
    "world",
    "actors",
    "effects",
    "interface",
    "post-hud",
    "utility",
];

/// Expected total variant count across all six shader families.
const EXPECTED_TOTAL_VARIANTS: usize = 1046;

/// Expected variant count for each individual shader family.
/// If a family manifest gains or loses variants without updating the native
/// catalog, the per-family assertion fails explicitly rather than silently
/// reporting a smaller denominator.
const EXPECTED_FAMILY_COUNTS: [(&str, usize); 6] = [
    ("world", 536),
    ("actors", 310),
    ("effects", 149),
    ("interface", 15),
    ("post-hud", 12),
    ("utility", 24),
];

/// Pass names ordered by `bfe_render::core::PASS_*` indices (`0..15`).
/// `FramePlan` and `Core::stats` both report draw counts in this fixed slot order.
const PASS_NAMES: [&str; 15] = [
    "world",
    "instances",
    "far",
    "terrain",
    "transparent",
    "effects",
    "foliage",
    "view",
    "post",
    "hud",
    "water",
    "shadow",
    "motion",
    "canopies",
    "screen",
];

/// Identity 4x4 matrix in column-major `f64` order for transport publications.
/// Both `InterfacePass`, `HudPass`, and `Lines` consume `f64` transport slices
/// that `bfe-render` narrows to `f32` when uploading GL uniforms.
const IDENTITY_F64: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// Command-line options parsed by `render-smoke`.
/// All paths are explicit or rooted in caller-supplied directories so the
/// binary never writes into the repository tree during CI or farm runs.
struct Args {
    /// Root directory of the synthetic or prepared content tree (`maps/<map>/`).
    content: PathBuf,
    /// Mission identifier under `content/maps/` (defaults to `"room"`).
    map: String,
    /// Optional path to `frame.json` carrying the 200-float `BfeFrame` bank.
    frame_json: Option<PathBuf>,
    /// Offscreen render target width in pixels.
    width: u32,
    /// Offscreen render target height in pixels.
    height: u32,
    /// Backend label written into JSONL records (`"llvmpipe"`, `"hardware-gl"`, etc.).
    backend: String,
    /// Optional file path for streaming JSONL records alongside stdout.
    output: Option<PathBuf>,
    /// Optional output path for the 2x3 contact-sheet PNG across all 6 families.
    png: Option<PathBuf>,
    /// Optional directory path for per-family PNGs (`family-<name>.png`).
    png_dir: Option<PathBuf>,
}

/// On-disk shader manifest structure (`rust/bfe-render/shaders/*/manifest.json`).
/// Version 1 manifests list every Rust-owned shader key in compilation order.
#[derive(Deserialize)]
struct ManifestFile {
    version: u32,
    programs: Vec<ManifestProgram>,
}

/// Individual shader program entry inside a family's `manifest.json`.
#[derive(Deserialize)]
struct ManifestProgram {
    key: String,
}

/// Per-variant compilation and link record matching `tools/renderer-smoke.mjs`.
/// Recording `shaders`, `links`, and `cached` proves that `Bridge::add_owned_program`
/// actually compiled both stages (`VERTEX` + `FRAGMENT`) and linked a fresh GL
/// program rather than returning a pre-cached handle.
#[derive(Clone, Serialize)]
struct VariantRecord {
    /// Full manifest key (`<family>/<variant>`).
    key: String,
    /// Program slot index in `Core::programs` (`u32::MAX` if rejected).
    id: u32,
    /// Wall-clock milliseconds spent compiling and linking this variant.
    ms: f64,
    /// True when compilation, linking, `glGetError`, and `Core::fault` all succeeded.
    accepted: bool,
    /// Number of shader stages compiled (`2` when uncached, `0` when cached).
    shaders: u32,
    /// Number of program links performed (`1` when uncached, `0` when cached).
    links: u32,
    /// True if `Core::owned_programs` already held a program for `key`.
    cached: bool,
    /// Always `false` in native GL (retained for schema parity with `renderer-smoke.mjs`).
    fallback: bool,
    /// Driver info log or GL error description when `accepted` is `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Per-family compilation summary record included in the final run report.
/// `sub_families` groups variant keys by their `<family>/<sub-family>-*` prefix
/// so `tools/renderer-smoke-native.py` can cross-check sub-family granularity
/// against `tools/renderer-smoke-manifests.mjs`.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShaderFamilySummary {
    family: String,
    count: usize,
    compile_ms: f64,
    ms: f64,
    sub_families: BTreeMap<String, usize>,
    variants: Vec<VariantRecord>,
}

/// Readback and pass-draw evidence for one family's representative scene draw.
/// Captures both the renderer's internal pass counters (`pass_draws`) and the
/// readback pixel statistics (`non_clear_pixels`, `unique_colors`, `digest`)
/// so a pass that submits a draw without producing fragments is caught.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FamilyDrawSummary {
    /// Family name (`world`, `actors`, `effects`, `interface`, `post-hud`, `utility`).
    family: String,
    /// True when `draws > 0`, `non_clear_pixels > 0`, `unique_colors >= 2`, and no GL/renderer fault occurred.
    ok: bool,
    /// Elapsed milliseconds for the representative scene draw.
    ms: f64,
    /// Representative shader keys exercised by this family's draw scene.
    programs: Vec<String>,
    /// Total pass draws recorded across `PASS_NAMES`.
    draws: u64,
    /// Non-zero pass draw counts keyed by pass name.
    pass_draws: BTreeMap<String, u64>,
    /// Count of pixels whose RGB channels differ from the clear color by > 3 counts.
    non_clear_pixels: usize,
    /// Count of distinct 32-bit RGBA colors observed in the readback buffer.
    unique_colors: usize,
    /// Expected background clear color (`[10, 15, 23, 255]`).
    clear_pixel: [u8; 4],
    /// First non-clear foreground pixel observed in the readback buffer.
    sample_pixel: [u8; 4],
    /// Lowercase hex SHA-256 digest of the raw RGBA8 readback buffer.
    digest: String,
    /// OpenGL error code observed after readback (`0` for `GL_NO_ERROR`).
    gl_error: u32,
    /// Renderer fault string if `Core::fault` was set during the draw.
    #[serde(skip_serializing_if = "Option::is_none")]
    fault: Option<String>,
}

/// Individual named assertion in `runs[0].results`.
#[derive(Clone, Serialize)]
struct AssertionResult {
    name: String,
    ok: bool,
    details: Value,
}

/// Owns the optional JSONL output file alongside standard output.
/// Flushing each line as it is emitted ensures that if a driver crash or
/// timeout occurs mid-catalog, the last `variant-start` line identifies the
/// exact shader variant that triggered the failure.
struct JsonlSink {
    file: Option<BufWriter<File>>,
}

impl JsonlSink {
    /// Opens the optional output file for streaming JSONL records, creating
    /// parent directories when needed.
    fn new(path: Option<&Path>) -> Result<Self, String> {
        let file = match path {
            Some(p) => {
                if let Some(parent) = p.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                }
                let f = File::create(p).map_err(|e| format!("{}: {e}", p.display()))?;
                Some(BufWriter::new(f))
            }
            None => None,
        };
        Ok(Self { file })
    }

    /// Writes one serialized JSON record as a single line and flushes immediately.
    fn emit(&mut self, value: &Value) -> Result<(), String> {
        let line = serde_json::to_string(value).map_err(|e| e.to_string())?;
        println!("{line}");
        let _ = std::io::stdout().flush();
        if let Some(w) = &mut self.file {
            writeln!(w, "{line}").map_err(|e| e.to_string())?;
            w.flush().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("render-smoke failed: {err}");
        std::process::exit(1);
    }
}

/// Parses command-line flags for `render-smoke`.
/// Supports `--content` (or `BFE_CONTENT_DIR`), `--map`, `--frame-json`,
/// `--width`, `--height`, `--backend`, `--output`, `--png`, and `--png-dir`.
fn parse_args() -> Result<Args, String> {
    let mut content = std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from);
    let mut map = String::from("room");
    let mut frame_json = None;
    let mut width = 320u32;
    let mut height = 180u32;
    let mut backend = String::from("native-gl");
    let mut output = None;
    let mut png = None;
    let mut png_dir = None;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--content" => {
                content = Some(PathBuf::from(it.next().ok_or("missing --content value")?))
            }
            "--map" => map = it.next().ok_or("missing --map value")?,
            "--frame-json" => {
                frame_json = Some(PathBuf::from(
                    it.next().ok_or("missing --frame-json value")?,
                ))
            }
            "--width" => {
                width = it
                    .next()
                    .ok_or("missing --width value")?
                    .parse()
                    .map_err(|_| "invalid --width")?
            }
            "--height" => {
                height = it
                    .next()
                    .ok_or("missing --height value")?
                    .parse()
                    .map_err(|_| "invalid --height")?
            }
            "--backend" => backend = it.next().ok_or("missing --backend value")?,
            "--output" => output = Some(PathBuf::from(it.next().ok_or("missing --output value")?)),
            "--png" => png = Some(PathBuf::from(it.next().ok_or("missing --png value")?)),
            "--png-dir" => {
                png_dir = Some(PathBuf::from(it.next().ok_or("missing --png-dir value")?))
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let content = content.ok_or("missing --content <dir> (or BFE_CONTENT_DIR)")?;
    if !(1..=8192).contains(&width) || !(1..=8192).contains(&height) {
        return Err("dimensions must be in 1..=8192".into());
    }
    Ok(Args {
        content,
        map,
        frame_json,
        width,
        height,
        backend,
        output,
        png,
        png_dir,
    })
}

/// Rounds millisecond durations to microsecond precision for stable JSONL logs.
fn round_ms(ms: f64) -> f64 {
    (ms * 1000.0).round() / 1000.0
}

/// Extracts the sub-family prefix from a shader key (`<family>/<sub-family>-...`).
/// For single-token keys such as `post-hud/hud` or `interface/room`, the token
/// after `/` is itself the sub-family name.
fn sub_family_of(key: &str) -> String {
    let rest = key.split_once('/').map_or(key, |(_, r)| r);
    rest.split('-').next().unwrap_or(rest).to_string()
}

/// Maps `bfe_render::shader::Kind` to its numeric tag used by `renderer-smoke.mjs`:
/// `0 = World`, `1 = Skinned`, `2 = Rigid`, `3 = Plain`, `4 = Effect`, `9 = Motion`.
fn kind_code(kind: bfe_render::shader::Kind) -> u32 {
    use bfe_render::shader::Kind;
    match kind {
        Kind::World => 0,
        Kind::Skinned => 1,
        Kind::Rigid => 2,
        Kind::Plain => 3,
        Kind::Effect => 4,
        Kind::Motion => 9,
    }
}

/// Loads all 6 `manifest.json` files from `rust/bfe-render/shaders/` (or `BFE_SHADERS_DIR`)
/// and verifies that their keys match `bfe_render::programs::specs()` 1-to-1.
///
/// Checking both the on-disk manifests and the compiled-in `programs::specs()`
/// slice guarantees that no manifest entry is missing from `bfe-render`'s
/// generated table and no extra compiled-in spec is absent from the manifests.
fn load_shader_manifests() -> Result<Vec<(String, Vec<String>, BTreeMap<String, usize>)>, String> {
    let shaders_root = std::env::var_os("BFE_SHADERS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../bfe-render/shaders"));
    let mut on_disk_dirs = BTreeSet::new();
    for entry in
        fs::read_dir(&shaders_root).map_err(|e| format!("{}: {e}", shaders_root.display()))?
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() && path.join("manifest.json").is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                on_disk_dirs.insert(name.to_string());
            }
        }
    }
    let expected_dirs: BTreeSet<String> = FAMILY_ORDER.iter().map(|s| (*s).to_string()).collect();
    if on_disk_dirs != expected_dirs {
        return Err(format!(
            "shader family directory mismatch: on disk {:?} vs expected {:?}",
            on_disk_dirs, expected_dirs
        ));
    }

    let mut families = Vec::with_capacity(FAMILY_ORDER.len());
    let mut manifest_keys = Vec::with_capacity(EXPECTED_TOTAL_VARIANTS);
    for &family in &FAMILY_ORDER {
        let path = shaders_root.join(family).join("manifest.json");
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let parsed: ManifestFile =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if parsed.version != 1 {
            return Err(format!(
                "{}: unexpected manifest version {}",
                path.display(),
                parsed.version
            ));
        }
        let mut keys = Vec::with_capacity(parsed.programs.len());
        let mut sub_families = BTreeMap::new();
        for prog in parsed.programs {
            if !prog.key.starts_with(&format!("{family}/")) {
                return Err(format!("key {} does not match family {family}", prog.key));
            }
            let sub = sub_family_of(&prog.key);
            *sub_families.entry(sub).or_insert(0) += 1;
            manifest_keys.push(prog.key.clone());
            keys.push(prog.key);
        }
        families.push((family.to_string(), keys, sub_families));
    }

    // Cross-check the manifest keys against the compiled-in bfe-render catalog.
    let spec_keys: BTreeSet<&str> = bfe_render::programs::specs()
        .iter()
        .map(|s| s.key)
        .collect();
    let disk_keys: BTreeSet<&str> = manifest_keys.iter().map(String::as_str).collect();
    if spec_keys != disk_keys || manifest_keys.len() != bfe_render::programs::specs().len() {
        return Err(format!(
            "manifest key set ({}) does not match bfe_render::programs::specs() ({})",
            manifest_keys.len(),
            bfe_render::programs::specs().len()
        ));
    }
    Ok(families)
}

/// Builds a default `BfeFrame` uniform view looking along `-Z` from `[0.0, 2.0, 3.0]`.
///
/// The synthetic `room` fixture places its near world quad at `z = -12`, its
/// rigid prop at `(0, 2, -5)`, its foliage patches at `z = -5..-8`, and its
/// water patch at `y = -1.5, z = -2..-18`, so this camera sees every fixture.
fn default_view(width: u32, height: u32) -> View {
    use bfe_render::shader::{FRAME_FLOATS, FRAME_MEMBERS, FRAME_OFFSETS};
    let mut values = [0.0f64; FRAME_FLOATS];
    let mut put = |name: &str, data: &[f64]| {
        if let Some(idx) = FRAME_MEMBERS.iter().position(|(n, _)| *n == name) {
            let at = FRAME_OFFSETS[idx];
            values[at..at + data.len()].copy_from_slice(data);
        }
    };
    let aspect = (width as f64) / (height.max(1) as f64);
    let near = 0.3f64;
    let far = 20000.0f64;
    let top = near * (60.0f64.to_radians() * 0.5).tan();
    let bottom = -top;
    let right = top * aspect;
    let left = -right;
    let x = 2.0 * near / (right - left);
    let y = 2.0 * near / (top - bottom);
    let c = -(far + near) / (far - near);
    let d = -2.0 * far * near / (far - near);
    let proj = [
        x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, c, -1.0, 0.0, 0.0, d, 0.0,
    ];
    let eye = [0.0, 2.0, 3.0];
    let view = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -eye[0], -eye[1], -eye[2], 1.0,
    ];
    put("projectionMatrix", &proj);
    put("viewMatrix", &view);
    put("cameraPosition", &eye);
    put("time", &[0.25]);
    put("sunDirection", &[0.4, 0.8, 0.4]);
    put("bump", &[1.0]);
    put("sunColor", &[0.7, 0.7, 0.7]);
    put("shadowStrength", &[0.0]);
    put("ambient", &[0.6, 0.6, 0.6]);
    put("aaSamples", &[1.0]);
    put("ambientTop", &[0.6, 0.6, 0.6]);
    put("terrainShade", &[1.0]);
    put("ambientBottom", &[0.35, 0.35, 0.35]);
    put("farAaSamples", &[1.0]);
    put("characterAmbient", &[0.6, 0.6, 0.6]);
    put("fogColor", &[0.08, 0.12, 0.18]);
    put("fade", &[0.0, 20000.0]);
    put("resolution", &[width as f64, height as f64]);
    put("fogRange", &[19999.0, 20000.0]);
    View { values }
}

/// Runs the complete shader compile/link sweep and the 6 representative family draws.
fn run() -> Result<(), String> {
    let args = parse_args()?;
    let mut sink = JsonlSink::new(args.output.as_deref())?;
    let families = load_shader_manifests()?;

    // Open a surfaceless EGL context via `EGL_EXT_device_enumeration` + `EGL_EXT_platform_device`.
    // Under `LIBGL_ALWAYS_SOFTWARE=1`, Mesa enumerates the software `llvmpipe` device;
    // when hardware GL is requested on a GPU VM, EGL selects the NVIDIA/DRI device.
    let device = glutin::api::egl::device::Device::query_devices()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no EGL device")?;
    let display = unsafe { glutin::api::egl::display::Display::with_device(&device, None) }
        .map_err(|e| e.to_string())?;
    let template = ConfigTemplateBuilder::new()
        .with_surface_type(ConfigSurfaceTypes::empty())
        .build();
    let config = unsafe { display.find_configs(template) }
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no EGL config")?;
    let gles_attrs = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(Some(Version::new(3, 0))))
        .build(None);
    let gl_core_attrs = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
        .build(None);
    let not_current = unsafe {
        display
            .create_context(&config, &gles_attrs)
            .or_else(|_| display.create_context(&config, &gl_core_attrs))
            .map_err(|e| e.to_string())?
    };
    let _context = not_current
        .make_current_surfaceless()
        .map_err(|e| e.to_string())?;
    let mut load = || unsafe {
        glow::Context::from_loader_function(|name| {
            display
                .get_proc_address(&CString::new(name).unwrap())
                .cast()
        })
    };

    // Query the active OpenGL strings so the JSONL log records the exact
    // software (`llvmpipe`) or hardware (`NVIDIA ...`) driver under test.
    let gl_probe = load();
    let (gl_version, gl_vendor, gl_renderer) = unsafe {
        (
            gl_probe.get_parameter_string(glow::VERSION),
            gl_probe.get_parameter_string(glow::VENDOR),
            gl_probe.get_parameter_string(glow::RENDERER),
        )
    };
    let context_kind = "egl-surfaceless";

    sink.emit(&json!({
        "stage": "runtime",
        "backend": args.backend,
        "versions": {
            "rust": env!("CARGO_PKG_VERSION"),
            "manifest": 1
        }
    }))?;
    sink.emit(&json!({
        "stage": "window",
        "backend": args.backend,
        "context": context_kind,
        "glVersion": gl_version,
        "glVendor": gl_vendor,
        "glRenderer": gl_renderer
    }))?;

    // Phase 1: Compile and link every variant in every shader family through
    // bfe-render's own program builder (`Bridge::add_owned_program`) on a fresh
    // cold `Bridge` before `Scene::load_with_random` pre-admits any map programs.
    // Running on a cold `Bridge` guarantees `cached == false`, `shaders == 2`,
    // and `links == 1` for all 1,046 variants.
    let mut cold_bridge = Bridge::new_boxed(load(), Ranges::new(load()));
    sink.emit(&json!({
        "stage": "fixtures",
        "backend": args.backend,
        "catalogVariants": bfe_render::programs::specs().len(),
        "manifestVariants": families.iter().map(|(_, k, _)| k.len()).sum::<usize>(),
        "families": families.len()
    }))?;

    let mut shader_families = Vec::with_capacity(families.len());
    let mut program_kinds = BTreeSet::new();
    let mut total_variants = 0usize;
    let mut accepted_variants = 0usize;
    let mut uncached_variants = 0usize;
    let mut compiled_shaders = 0u32;
    let mut linked_programs = 0u32;
    let mut total_compile_ms = 0.0f64;

    for (family, keys, sub_families) in &families {
        let family_start = Instant::now();
        let mut family_compile_ms = 0.0f64;
        let mut variants = Vec::with_capacity(keys.len());

        for key in keys {
            // Emit `variant-start` before invoking `add_owned_program` so a hang
            // or crash in the driver compiler names the offending variant key.
            sink.emit(&json!({
                "stage": "variant-start",
                "backend": args.backend,
                "key": key,
                "fallback": false
            }))?;

            let cached = cold_bridge.r.core.has_owned_program(key);
            let t0 = Instant::now();
            let res = cold_bridge.add_owned_program(key);
            let ms = round_ms(t0.elapsed().as_secs_f64() * 1000.0);
            family_compile_ms += ms;

            // Check both the GL error state and `Core::fault` after every link.
            let gl_err = unsafe { cold_bridge.r.core.gl.get_error() };
            let fault = cold_bridge.r.core.fault.take();
            let spec = bfe_render::programs::spec(key)?;
            program_kinds.insert(kind_code(spec.kind));

            let (id, accepted, shaders, links, error) = match res {
                Ok(id) if gl_err == glow::NO_ERROR && fault.is_none() => {
                    let s = if cached { 0 } else { 2 };
                    let l = if cached { 0 } else { 1 };
                    (id, true, s, l, None)
                }
                Ok(id) => (
                    id,
                    false,
                    0,
                    0,
                    Some(fault.unwrap_or_else(|| format!("GL error 0x{gl_err:04x}"))),
                ),
                Err(err) => (u32::MAX, false, 0, 0, Some(err)),
            };

            total_variants += 1;
            if accepted {
                accepted_variants += 1;
            }
            if !cached {
                uncached_variants += 1;
            }
            compiled_shaders += shaders;
            linked_programs += links;

            let record = VariantRecord {
                key: key.clone(),
                id,
                ms,
                accepted,
                shaders,
                links,
                cached,
                fallback: false,
                error,
            };
            sink.emit(&json!({
                "stage": "variant-result",
                "backend": args.backend,
                "key": record.key,
                "id": record.id,
                "ms": record.ms,
                "accepted": record.accepted,
                "shaders": record.shaders,
                "links": record.links,
                "cached": record.cached,
                "fallback": record.fallback,
                "error": record.error
            }))?;
            variants.push(record);
        }

        let family_ms = round_ms(family_start.elapsed().as_secs_f64() * 1000.0);
        let compile_ms = round_ms(family_compile_ms);
        total_compile_ms += compile_ms;

        sink.emit(&json!({
            "stage": "family-result",
            "backend": args.backend,
            "family": family,
            "fallback": false,
            "compileMs": compile_ms,
            "ms": family_ms,
            "subFamilies": sub_families,
            "variants": variants
        }))?;

        shader_families.push(ShaderFamilySummary {
            family: family.clone(),
            count: keys.len(),
            compile_ms,
            ms: family_ms,
            sub_families: sub_families.clone(),
            variants,
        });
    }
    // Drop the cold compilation Bridge before creating the scene Bridge so peak
    // driver program memory does not hold two copies of the catalog at once.
    drop(cold_bridge);

    // Phase 2: Admit the map scene and draw one representative scene for each of
    // the 6 shader families into `scene.canvas.framebuffer`, reading back RGBA8 pixels.
    let (view, random) = if let Some(frame_path) = &args.frame_json {
        let input: Value =
            serde_json::from_slice(&fs::read(frame_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let values: Vec<f64> =
            serde_json::from_value(input["values"].clone()).map_err(|e| e.to_string())?;
        if values.len() != bfe_render::shader::FRAME_FLOATS || values.iter().any(|v| !v.is_finite())
        {
            return Err("invalid frame.json values".into());
        }
        let random: Option<Vec<f64>> =
            serde_json::from_value(input["foliageRandom"].clone()).map_err(|e| e.to_string())?;
        let mut v = View {
            values: [0.0; bfe_render::shader::FRAME_FLOATS],
        };
        v.values.copy_from_slice(&values);
        (v, random)
    } else {
        (default_view(args.width, args.height), None)
    };

    let size = [args.width as i32, args.height as i32];
    let (mut scene, _) =
        Scene::load_with_random(&args.content, &args.map, size, random.as_deref(), &mut load)?;
    let solid_core_tex = admit_solid_white_texture(&mut scene)?;

    let mut family_draws = Vec::with_capacity(FAMILY_ORDER.len());
    let mut family_panels: Vec<(String, Vec<u8>)> = Vec::with_capacity(FAMILY_ORDER.len());
    let mut cumulative_pass_draws = [0u64; 15];
    let mut total_draws = 0u64;
    let mut total_draw_ms = 0.0f64;

    for &family in &FAMILY_ORDER {
        // Reset per-frame statistics before each family's representative draw
        // so `pass_draws` reflects only the passes exercised by this family.
        scene.bridge.r.core.stats = Default::default();
        scene.bridge.r.frames.stats.fill(0.0);
        scene.draws = 0;

        let draw_start = Instant::now();
        let (programs, pass_counts) = match family {
            "world" => draw_world_family(&mut scene, &view)?,
            "actors" => draw_actors_family(&mut scene, &view)?,
            "effects" => draw_effects_family(&mut scene, &view, solid_core_tex)?,
            "interface" => draw_interface_family(&mut scene, solid_core_tex)?,
            "post-hud" => draw_post_hud_family(&mut scene, solid_core_tex)?,
            "utility" => draw_utility_family(&mut scene)?,
            _ => unreachable!(),
        };
        let ms = round_ms(draw_start.elapsed().as_secs_f64() * 1000.0);
        total_draw_ms += ms;

        for (i, &count) in pass_counts.iter().enumerate() {
            cumulative_pass_draws[i] += count;
        }
        let pass_sum: u64 = pass_counts.iter().sum();
        total_draws += pass_sum;

        // Read back the RGBA8 framebuffer via `scene.pixels()` and check both
        // `Core::fault` and `gl.get_error()`.
        let fault = scene.bridge.r.core.fault.take();
        let pixels = scene.pixels()?;
        let gl_error = unsafe { scene.bridge.r.core.gl.get_error() };
        let metrics = analyze_pixels(&pixels);

        let mut pass_draws = BTreeMap::new();
        for (i, &name) in PASS_NAMES.iter().enumerate() {
            if pass_counts[i] > 0 {
                pass_draws.insert(name.to_string(), pass_counts[i]);
            }
        }
        // Every family draw must record at least one pass draw, produce non-clear
        // pixels and at least two distinct RGBA colors, and leave zero GL/renderer errors.
        let ok = gl_error == glow::NO_ERROR
            && fault.is_none()
            && pass_sum > 0
            && metrics.non_clear_pixels > 0
            && metrics.unique_colors >= 2;

        if let Some(dir) = &args.png_dir {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = dir.join(format!("family-{family}.png"));
            write_png(&path, args.width, args.height, &pixels)?;
        }
        family_panels.push((family.to_string(), pixels));

        let summary = FamilyDrawSummary {
            family: family.to_string(),
            ok,
            ms,
            programs,
            draws: pass_sum,
            pass_draws,
            non_clear_pixels: metrics.non_clear_pixels,
            unique_colors: metrics.unique_colors,
            clear_pixel: metrics.clear_pixel,
            sample_pixel: metrics.sample_pixel,
            digest: metrics.digest,
            gl_error,
            fault,
        };
        sink.emit(&json!({
            "stage": "family-draw",
            "backend": args.backend,
            "family": summary.family,
            "ok": summary.ok,
            "ms": summary.ms,
            "programs": summary.programs,
            "draws": summary.draws,
            "passDraws": summary.pass_draws,
            "nonClearPixels": summary.non_clear_pixels,
            "uniqueColors": summary.unique_colors,
            "digest": summary.digest,
            "glError": summary.gl_error,
            "fault": summary.fault
        }))?;
        family_draws.push(summary);
    }

    if let Some(png_path) = &args.png {
        write_contact_sheet(png_path, args.width, args.height, &family_panels)?;
    }

    // Build structured assertion results matching renderer-smoke.mjs.
    let mut results = Vec::new();
    results.push(AssertionResult {
        name: "All 1,046 manifest shader variants compiled and linked uncached".into(),
        ok: total_variants == EXPECTED_TOTAL_VARIANTS
            && accepted_variants == EXPECTED_TOTAL_VARIANTS
            && uncached_variants == EXPECTED_TOTAL_VARIANTS
            && compiled_shaders == (EXPECTED_TOTAL_VARIANTS as u32) * 2
            && linked_programs == EXPECTED_TOTAL_VARIANTS as u32,
        details: json!({
            "totalVariants": total_variants,
            "acceptedVariants": accepted_variants,
            "uncachedVariants": uncached_variants,
            "compiledShaders": compiled_shaders,
            "linkedPrograms": linked_programs
        }),
    });

    for &(expected_family, expected_count) in &EXPECTED_FAMILY_COUNTS {
        let fam = shader_families.iter().find(|f| f.family == expected_family);
        let count = fam.map_or(0, |f| f.count);
        let accepted = fam.map_or(0, |f| f.variants.iter().filter(|v| v.accepted).count());
        results.push(AssertionResult {
            name: format!(
                "Shader family '{expected_family}' compiled and linked all {expected_count} variants"
            ),
            ok: count == expected_count && accepted == expected_count,
            details: json!({
                "family": expected_family,
                "expected": expected_count,
                "compiled": count,
                "accepted": accepted,
                "subFamilies": fam.map(|f| &f.sub_families)
            }),
        });
    }

    results.push(AssertionResult {
        name: "Every shader rewrite kind links".into(),
        ok: [0u32, 1, 2, 3, 4, 9]
            .iter()
            .all(|k| program_kinds.contains(k)),
        details: json!({ "programKinds": program_kinds }),
    });

    for draw in &family_draws {
        results.push(AssertionResult {
            name: format!(
                "Shader family '{}' drew non-empty scene output",
                draw.family
            ),
            ok: draw.ok,
            details: json!({
                "family": draw.family,
                "programs": draw.programs,
                "draws": draw.draws,
                "passDraws": draw.pass_draws,
                "nonClearPixels": draw.non_clear_pixels,
                "uniqueColors": draw.unique_colors,
                "digest": draw.digest,
                "glError": draw.gl_error,
                "fault": draw.fault
            }),
        });
    }

    let all_ok = results.iter().all(|r| r.ok);
    let families_drawn = family_draws.iter().filter(|d| d.ok).count();

    sink.emit(&json!({
        "versions": {
            "rust": env!("CARGO_PKG_VERSION"),
            "manifest": 1
        },
        "runs": [{
            "ok": all_ok,
            "backend": args.backend,
            "fallback": false,
            "multiAvailable": true,
            "multiDraws": 0,
            "renderer": gl_renderer,
            "vendor": gl_vendor,
            "version": gl_version,
            "context": context_kind,
            "compileMs": round_ms(total_compile_ms),
            "drawMs": round_ms(total_draw_ms),
            "totalVariants": total_variants,
            "acceptedVariants": accepted_variants,
            "compiledShaders": compiled_shaders,
            "linkedPrograms": linked_programs,
            "familiesDrawn": families_drawn,
            "draws": total_draws,
            "passDraws": cumulative_pass_draws,
            "programKinds": program_kinds,
            "results": results,
            "shaderFamilies": shader_families,
            "familyDraws": family_draws
        }]
    }))?;

    if !all_ok {
        let failed: Vec<&str> = results
            .iter()
            .filter(|r| !r.ok)
            .map(|r| r.name.as_str())
            .collect();
        return Err(format!("smoke assertions failed: {}", failed.join("; ")));
    }
    Ok(())
}

/// Sums the per-pass draw counters recorded in `scene.bridge.r.frames.stats`.
/// `FramePlan` stores one `stats::LEN` row per frame role (`R::NEAR`, `R::TRANSPARENT`,
/// `R::LINES2`, etc.), with the 15 pass counters starting at `stats::PASS_DRAWS`.
fn collect_frame_pass_draws(scene: &Scene) -> [u64; 15] {
    use bfe_render::frame_plan::layout::stats as T;
    let mut out = [0u64; 15];
    for row in scene.bridge.r.frames.stats.chunks_exact(T::LEN) {
        for (i, v) in row[T::PASS_DRAWS..T::PASS_DRAWS + 15].iter().enumerate() {
            out[i] += *v as u64;
        }
    }
    out
}

/// Uploads a 1x1 opaque white RGBA8 texture through `scene.resources` and
/// registers its GL handle in `scene.bridge.r.core.textures`.
///
/// Used as a neutral diffuse/mask sampler input for `effects/particle-cpu`,
/// `interface/room`, `interface/model`, and `post-hud/hud` so shader vertex
/// colors and uniform tints drive the visible pixel output directly.
fn admit_solid_white_texture(scene: &mut Scene) -> Result<u32, String> {
    let src = scene.resources.source(
        &json!({
            "width": 1,
            "height": 1,
            "depth": 1,
            "target": glow::TEXTURE_2D,
            "format": 1023,
            "dataType": 1009,
            "srgb": false
        })
        .to_string(),
    )?;
    scene.resources.source_pixels(src, &[255, 255, 255, 255])?;
    let sample = scene.resources.sample(
        src,
        r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#,
    )?;
    scene.resources.upload(sample)?;
    let gl_tex = scene.resources.texture(sample)?;
    Ok(scene.bridge.r.core.add_texture(gl_tex))
}

/// Draws the `world` family scene (`world/static-lit-s0-cf-0-0-0`,
/// `world/foliage-grass`, `world/foliage-leaves`) with sky, props, and water off.
///
/// Calling `scene.draw_component(view, "foliage")` first populates the foliage
/// instance inputs and disables sky, prop instances, and water. Enabling world
/// mesh 0 and re-running role `R::NEAR` onto a freshly cleared canvas then
/// exercises both `WorldPass` (`PASS_WORLD`) and `FoliagePass` (`PASS_FOLIAGE`)
/// in a single `world`-family frame.
fn draw_world_family(scene: &mut Scene, view: &View) -> Result<(Vec<String>, [u64; 15]), String> {
    use bfe_render::frame_plan::layout::role as R;
    // First run `foliage` to populate the foliage frame inputs and clear `records`, `sky`, and `water`.
    scene.draw_component(view, "foliage")?;
    // Now also enable the near static world mesh (`world/static-lit-s0-cf-0-0-0`) and
    // re-execute `R::NEAR` onto a freshly cleared canvas so both `WorldPass` and `FoliagePass` draw.
    scene.bridge.r.world.world_shown(0, true);
    scene.bridge.frame_publish_frame(R::NEAR as u32);
    scene.bridge.r.frames.stats.fill(0.0);
    clear_canvas(scene);
    if !scene
        .bridge
        .frame_aux_frame(&mut scene.resources, R::NEAR as u32)
    {
        scene.bridge.r.world.world_shown(0, false);
        return Err(scene
            .bridge
            .frame_error_frame()
            .unwrap_or_else(|| "world family frame_aux failed".into()));
    }
    scene.bridge.r.world.world_shown(0, false);
    Ok((
        vec![
            "world/static-lit-s0-cf-0-0-0".into(),
            "world/foliage-grass".into(),
            "world/foliage-leaves".into(),
        ],
        collect_frame_pass_draws(scene),
    ))
}

/// Draws the `actors` family scene (`actors/rigid-lit-s2-cf-0-0-0` via
/// `InstancePass`) on a cleared canvas with sky, static world, foliage, and water off.
///
/// The synthetic `room` fixture places a 1-bone rigid prop (`banner_panel`) at
/// `(0, 2, -5)` in front of the camera. `scene.draw_component(view, "props")`
/// publishes its transform to `InstancePass` (`PASS_INSTANCES`).
fn draw_actors_family(scene: &mut Scene, view: &View) -> Result<(Vec<String>, [u64; 15]), String> {
    scene.draw_component(view, "props")?;
    // Ignore the empty far-image screen copy in `R::IMAGE` so pass_draws reflects `InstancePass`.
    let mut counts = collect_frame_pass_draws(scene);
    counts[bfe_render::core::PASS_SCREEN] = 0;
    Ok((vec!["actors/rigid-lit-s2-cf-0-0-0".into()], counts))
}

/// Draws the `effects` family scene (`effects/water-textured-s2` via `WaterPass`
/// plus `effects/particle-cpu` via `EffectsPass`) on a cleared canvas.
///
/// How the two `effects` passes are combined in role `R::TRANSPARENT`:
/// - `WaterPass` (`PASS_WATER`) draws the subdivided water surface admitted from
///   `map.json` using `effects/water-textured-s2`.
/// - `EffectsPass` (`PASS_EFFECTS`) draws two billboarded particle quads from a
///   CPU particle batch (`effect_layout::CPU`, 21 `f32` words per quad: center,
///   RGBA tint, size, rotation, UV rect, velocity, and axis vectors) using
///   `effects/particle-cpu`.
/// - Setting `M::FX_WORDS = 42.0` on `R::TRANSPARENT` before `frame_publish_frame`
///   instructs `FramePlan` to upload the 42 `f32` words of particle vertex data
///   and execute both `WaterPass` and `EffectsPass` into `scene.canvas.framebuffer`.
fn draw_effects_family(
    scene: &mut Scene,
    view: &View,
    solid_core_tex: u32,
) -> Result<(Vec<String>, [u64; 15]), String> {
    use bfe_render::{
        effects as fx,
        frame_plan::layout::{meta as M, role as R},
    };
    let batch = scene.bridge.effect_batch(
        bfe_render::effect_layout::CPU as u32,
        0,
        false,
        solid_core_tex as i32,
        4,
        -1,
        &[],
    )?;
    scene.bridge.fx_reserve(64);
    // Two billboarded particle quads in front of the camera (`[0, 2, 3]` looking toward `-Z`).
    // Layout per quad (21 floats):
    //   [0..3] world center, [3..7] RGBA color, [7] size, [8] angle,
    //   [9..13] UV rect (u0, v0, u1, v1), [13..16] velocity, [16..21] axis/stretch.
    let q0: [f32; 21] = [
        -1.8, 2.2, -4.0, 0.95, 0.45, 0.15, 0.92, 2.2, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 1.0, 1.0,
    ];
    let q1: [f32; 21] = [
        1.8, 2.4, -4.0, 0.20, 0.80, 0.95, 0.92, 2.2, 0.3, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 1.0, 1.0,
    ];
    scene.bridge.r.effects.data[0..21].copy_from_slice(&q0);
    scene.bridge.r.effects.data[21..42].copy_from_slice(&q1);
    scene.bridge.r.effects.input[fx::IN_DRAWS] = 1.0;
    scene.bridge.r.effects.input[fx::IN_OWNERS] = 0.0;
    scene.bridge.r.effects.input[fx::IN_SOFT..fx::IN_SOFT + 6]
        .copy_from_slice(&[0.0, 1.0, 1.0, 0.3, 20000.0, -1.0]);
    {
        let row = &mut scene.bridge.r.effects.input[fx::DRAW_AT..fx::DRAW_AT + fx::DRAW_STRIDE];
        row.fill(0.0);
        row[..6].copy_from_slice(&[fx::KIND_BATCH as f64, batch as f64, 2.0, 0.0, 0.0, 4.0]);
        row[fx::DRAW_KEY + 1] = 10.0;
        row[fx::DRAW_KEY + 5] = batch as f64;
    }

    // Run `water` to prepare the water surface and camera matrices, then re-publish
    // `R::TRANSPARENT` with `FX_WORDS = 42` so both `WaterPass` and `EffectsPass` execute.
    scene.draw_component(view, "water")?;
    scene.bridge.r.frames.meta[R::TRANSPARENT * M::LEN + M::FX_WORDS] = 42.0;
    scene.bridge.frame_publish_frame(R::TRANSPARENT as u32);
    scene.bridge.r.frames.stats.fill(0.0);
    clear_canvas(scene);
    let ok = scene
        .bridge
        .frame_aux_frame(&mut scene.resources, R::TRANSPARENT as u32);
    scene.bridge.r.effects.input[fx::IN_DRAWS] = 0.0;
    if !ok {
        return Err(scene
            .bridge
            .frame_error_frame()
            .unwrap_or_else(|| "effects family frame_aux failed".into()));
    }

    Ok((
        vec![
            "effects/water-textured-s2".into(),
            "effects/particle-cpu".into(),
        ],
        collect_frame_pass_draws(scene),
    ))
}

/// Draws the `interface` family scene (`interface/room` and `interface/model`
/// via `InterfacePass`) into `scene.canvas.framebuffer`.
///
/// Admits two procedural 3D interface geometries through `Bridge::actor_source`:
/// a backdrop plane (`interface/room`) and a lit 3D sphere (`interface/model`),
/// registers their appearances via `Bridge::ui_appearance`, populates two
/// `interface_scene::layout` draw rows with identity transforms and the policy
/// defaults, and executes `InterfacePass::draw` (`PASS_VIEW`).
fn draw_interface_family(
    scene: &mut Scene,
    solid_core_tex: u32,
) -> Result<(Vec<String>, [u64; 15]), String> {
    use bfe_render::{
        interface_resource_layout::{kind as K, look as L, meta as M, raw as R},
        interface_scene::layout as D,
    };

    let plane_src = scene
        .bridge
        .actor_source(r#"{"kind":"plane","width":1.3,"height":1.1}"#, &[], &[])?
        .ok_or("failed to admit interface plane source")?;
    let sphere_src = scene
        .bridge
        .actor_source(
            r#"{"kind":"sphere","radius":0.48,"widthSegments":16,"heightSegments":12}"#,
            &[],
            &[],
        )?
        .ok_or("failed to admit interface sphere source")?;

    let room_meta = scene.bridge.ui_appearance(
        plane_src,
        r#"{"program":"room","side":2,"values":{"color":[0.18,0.36,0.62],"pool":[0.45,0.78,0.96],"shade":[0.08,0.14,0.26],"strength":[0.85]}}"#,
        false,
        &[solid_core_tex as i32, -1, -1, 0],
        &[-1, -1, -1, -1],
        1,
    )?;
    let model_meta = scene.bridge.ui_appearance(
        sphere_src,
        r#"{"program":"model","lit":true,"side":2,"values":{"color":[0.92,0.56,0.20],"rim":[0.30,0.85,0.98],"rimPower":[2.5]}}"#,
        false,
        &[solid_core_tex as i32, -1, -1, 0],
        &[-1, -1, -1, -1],
        2,
    )?;

    clear_canvas(scene);
    scene.bridge.r.core.stats = Default::default();

    // Populate one `interface_scene::layout` row per appearance descriptor.
    // `D::VALUES` carries the 5 `Mat4` blocks (`WORLD`, `VIEW`, `PROJECTION`,
    // `BIND`, `INVERSE`) followed by `R::LOOK` (pre-filled from `policy.defaults`).
    for (idx, app_id) in [
        room_meta[M::APPEARANCE] as usize,
        model_meta[M::APPEARANCE] as usize,
    ]
    .into_iter()
    .enumerate()
    {
        let desc = &scene.bridge.r.interface.resources.descriptors[app_id];
        let count = desc.count as f64;
        let defaults = desc.policy.defaults.clone();
        let row = &mut scene.bridge.r.interface.input[idx * D::STRIDE..(idx + 1) * D::STRIDE];
        row.fill(0.0);
        row[D::PROGRAM] = K::INTERFACE as f64;
        row[D::GEOMETRY] = app_id as f64;
        row[D::FIRST] = 0.0;
        row[D::COUNT] = count;
        row[D::RENDER_ORDER] = idx as f64;
        let raw = &mut row[D::VALUES..];
        raw[R::WORLD..R::WORLD + 16].copy_from_slice(&IDENTITY_F64);
        raw[R::VIEW..R::VIEW + 16].copy_from_slice(&IDENTITY_F64);
        raw[R::PROJECTION..R::PROJECTION + 16].copy_from_slice(&IDENTITY_F64);
        raw[R::BIND..R::BIND + 16].copy_from_slice(&IDENTITY_F64);
        raw[R::INVERSE..R::INVERSE + 16].copy_from_slice(&IDENTITY_F64);
        raw[R::LOOK..R::LOOK + L::LEN].copy_from_slice(&defaults);
        raw[R::PIXEL_RATIO] = 1.0;
        raw[R::HEIGHT] = scene.size[1] as f64;
    }

    scene.bridge.r.interface.draw(&mut scene.bridge.r.core, 2);
    Ok((
        vec!["interface/room".into(), "interface/model".into()],
        scene.bridge.r.core.stats.pass_draws.map(|v| v as u64),
    ))
}

/// Draws the `post-hud` family scene (`post-hud/copy` via `PostPass` and
/// `post-hud/hud` via `HudPass`) into `scene.canvas.framebuffer`.
///
/// Exercises both sub-passes of the `post-hud` family:
/// 1. `PostPass::draw` (`PASS_POST`) runs `post-hud/copy` with a 2x2 source
///    texture whose `(0, 0)` texel matches the clear color `[10, 15, 23, 255]`
///    and whose remaining quadrants carry distinct colors.
/// 2. `HudPass::draw` (`PASS_HUD`) uploads 8 vertices (2 quads) across the
///    position, UV, mask-UV, and RGBA tint streams and renders them with
///    `post-hud/hud` over the post-processed background.
fn draw_post_hud_family(
    scene: &mut Scene,
    solid_core_tex: u32,
) -> Result<(Vec<String>, [u64; 15]), String> {
    use bfe_render::{hud as H, post as P};

    // Create a 2x2 texture whose corner (0,0) matches the clear color `[10, 15, 23, 255]`
    // and whose other texels carry distinct colors for `post-hud/copy` to sample.
    let post_src = scene.resources.source(
        &json!({
            "width": 2,
            "height": 2,
            "depth": 1,
            "target": glow::TEXTURE_2D,
            "format": 1023,
            "dataType": 1009,
            "srgb": false
        })
        .to_string(),
    )?;
    let post_pixels: [u8; 16] = [
        10, 15, 23, 255, // (0,0) matches clear color so corner clear check holds
        32, 72, 128, 255, // (1,0) deep blue
        28, 96, 112, 255, // (0,1) teal
        48, 64, 140, 255, // (1,1) indigo
    ];
    scene.resources.source_pixels(post_src, &post_pixels)?;
    let post_sample = scene.resources.sample(
        post_src,
        r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#,
    )?;
    scene.resources.upload(post_sample)?;
    let post_gl_tex = scene.resources.texture(post_sample)?;
    let post_core_tex = scene.bridge.r.core.add_texture(post_gl_tex);

    let copy_prog = scene.bridge.add_owned_program("post-hud/copy")?;
    let hud_prog = scene.bridge.add_owned_program("post-hud/hud")?;
    scene.bridge.r.hud.prepare_program(
        hud_prog as usize,
        bfe_render::programs::spec("post-hud/hud")?.uniforms,
    );

    let indices: Vec<u32> = (0..H::QUADS as u32)
        .flat_map(|i| [i * 4, i * 4 + 1, i * 4 + 2, i * 4 + 2, i * 4 + 1, i * 4 + 3])
        .collect();
    scene.bridge.hud_init(&indices);

    clear_canvas(scene);
    scene.bridge.r.core.stats = Default::default();
    let [width, height] = scene.size;

    // Step 1: Execute `PostPass` (`post-hud/copy`) into the bound canvas framebuffer.
    {
        let step = &mut scene.bridge.r.post.input[0..P::STEP];
        step.fill(0.0);
        step[P::STEP_PROGRAM] = copy_prog as f64;
        step[P::STEP_TARGET] = P::TARGET_BOUND;
        step[P::STEP_WIDTH] = width as f64;
        step[P::STEP_HEIGHT] = height as f64;
        step[P::STEP_UNIFORMS] = 1.0;
        let u = &mut step[P::STEP_FIRST..P::STEP_FIRST + P::UNIFORM];
        u[0] = 0.0; // slot 0: `map`
        u[1] = P::TYPE_SAMPLER as f64;
        u[2] = post_core_tex as f64;
    }
    scene.bridge.r.post.draw(&mut scene.bridge.r.core, 1);

    // Step 2: Execute `HudPass` (`post-hud/hud`) to draw two tinted HUD quads over the canvas.
    let quad_positions: [f32; 24] = [
        -0.70, -0.45, 0.0, -0.70, 0.45, 0.0, -0.10, -0.45, 0.0, -0.10, 0.45, 0.0, 0.10, -0.45, 0.0,
        0.10, 0.45, 0.0, 0.70, -0.45, 0.0, 0.70, 0.45, 0.0,
    ];
    let quad_uvs: [f32; 16] = [
        0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0,
    ];
    let quad_tints: [f32; 32] = [
        0.95, 0.75, 0.18, 0.95, 0.95, 0.45, 0.12, 0.95, 0.85, 0.88, 0.25, 0.95, 0.92, 0.60, 0.15,
        0.95, 0.18, 0.85, 0.95, 0.95, 0.15, 0.62, 0.92, 0.95, 0.25, 0.95, 0.65, 0.95, 0.20, 0.78,
        0.88, 0.95,
    ];
    scene.bridge.r.hud.position[0..24].copy_from_slice(&quad_positions);
    scene.bridge.r.hud.uv[0..16].copy_from_slice(&quad_uvs);
    scene.bridge.r.hud.mask_uv[0..16].copy_from_slice(&quad_uvs);
    scene.bridge.r.hud.tint[0..32].copy_from_slice(&quad_tints);

    // Populate the `HudPass` header (`IN_GROUPS`, `IN_PROGRAM`, `IN_CAMERA`,
    // `IN_SLOTS` for `[map, maskMap, useMask]`, `IN_UPLOAD`, `IN_VIEWPORT`, and
    // the first group record `[start=0, count=12, map, maskMap, useMask=0]`).
    {
        let input = &mut scene.bridge.r.hud.input;
        input.fill(0.0);
        input[H::IN_GROUPS] = 1.0;
        input[H::IN_PROGRAM] = hud_prog as f64;
        for i in 0..2 {
            let at = H::IN_CAMERA + i * P::UNIFORM;
            input[at] = i as f64;
            input[at + 1] = P::TYPE_MAT4 as f64;
            input[at + 2..at + 18].copy_from_slice(&IDENTITY_F64);
        }
        input[H::IN_SLOTS..H::IN_SLOTS + 3].copy_from_slice(&[2.0, 3.0, 4.0]);
        input[H::IN_UPLOAD] = 0.0;
        input[H::IN_UPLOAD + 1] = 8.0;
        input[H::IN_VIEWPORT] = width as f64;
        input[H::IN_VIEWPORT + 1] = height as f64;
        let g = &mut input[H::IN_FIRST_GROUP..H::IN_FIRST_GROUP + H::GROUP];
        g.copy_from_slice(&[0.0, 12.0, solid_core_tex as f64, solid_core_tex as f64, 0.0]);
    }
    scene.bridge.r.hud.draw(&mut scene.bridge.r.core);

    Ok((
        vec!["post-hud/copy".into(), "post-hud/hud".into()],
        scene.bridge.r.core.stats.pass_draws.map(|v| v as u64),
    ))
}

/// Draws the `utility` family scene (`utility/line-plain-solid` and
/// `utility/line-3-solid` via `Lines` and `R::LINES2`) into `scene.canvas.framebuffer`.
///
/// Exercises two distinct `utility/line-*` variants in role `R::LINES2` (`PASS_VIEW`):
/// 1. `Bridge::line_source` + `Bridge::line_frame(2)` admits a `FLIGHT` line
///    strip and a `TOW` cable strip using `utility/line-plain-solid` and writes
///    their draw packets into `frames.lines` slots 0 and 1 of view 2.
/// 2. A 3-axis vertex-colored line geometry (`Descriptor::axes()`, with `position`
///    at attribute 0 and `color` at attribute 3) is admitted for
///    `utility/line-3-solid` and written into slot 2 of view 2 so both uniform-
///    colored and vertex-colored utility line shaders draw in the same pass.
fn draw_utility_family(scene: &mut Scene) -> Result<(Vec<String>, [u64; 15]), String> {
    use bfe_render::{
        frame_plan::layout::{binding as B, role as R},
        line_layout::{kind as K, limit as C, row as ROW, view as V},
    };

    let plain_prog = scene.bridge.add_owned_program("utility/line-plain-solid")?;
    let flight_src = scene
        .bridge
        .line_source(plain_prog, K::FLIGHT as u32, 6)?
        .ok_or("failed to admit flight line source")?;
    let tow_src = scene
        .bridge
        .line_source(plain_prog, K::TOW as u32, 6)?
        .ok_or("failed to admit tow line source")?;

    // Six 3D endpoints (3 line segments) for the flight path and tow cable in NDC range.
    let flight_pts: [f32; 18] = [
        -0.75, -0.55, 0.0, 0.75, 0.55, 0.0, -0.75, 0.55, 0.0, 0.75, -0.55, 0.0, -0.65, 0.0, 0.0,
        0.65, 0.0, 0.0,
    ];
    let tow_pts: [f32; 18] = [
        -0.70, -0.65, 0.0, -0.35, 0.45, 0.0, 0.0, -0.45, 0.0, 0.35, 0.45, 0.0, 0.70, -0.65, 0.0,
        0.0, 0.65, 0.0,
    ];
    // Write vertex positions into `line_data` at each source's `C::VERTICES * 3` stride.
    {
        let raw_data = scene.bridge.bank("line_data")?;
        let line_data: &mut [f32] = unsafe {
            std::slice::from_raw_parts_mut(raw_data.as_mut_ptr().cast(), raw_data.len() / 4)
        };
        let f_at = (flight_src as usize) * C::VERTICES * 3;
        line_data[f_at..f_at + 18].copy_from_slice(&flight_pts);
        let t_at = (tow_src as usize) * C::VERTICES * 3;
        line_data[t_at..t_at + 18].copy_from_slice(&tow_pts);
    }
    // Publish the view header (`PROJECTION`, `INVERSE`, `WORLD`, `ORTHOGRAPHIC`, `COUNT`)
    // and two `ROW::LEN` instance records into `line_input` before calling `line_frame(2)`.
    {
        let raw_input = scene.bridge.bank("line_input")?;
        let input: &mut [f64] = unsafe {
            std::slice::from_raw_parts_mut(raw_input.as_mut_ptr().cast(), raw_input.len() / 8)
        };
        input.fill(0.0);
        input[V::PROJECTION..V::PROJECTION + 16].copy_from_slice(&IDENTITY_F64);
        input[V::INVERSE..V::INVERSE + 16].copy_from_slice(&IDENTITY_F64);
        input[V::WORLD..V::WORLD + 16].copy_from_slice(&IDENTITY_F64);
        input[V::ORTHOGRAPHIC] = 0.0;
        input[V::COUNT] = 2.0;

        for (i, src) in [flight_src, tow_src].into_iter().enumerate() {
            let r = &mut input[V::LEN + i * ROW::LEN..V::LEN + (i + 1) * ROW::LEN];
            r[ROW::SOURCE] = src as f64;
            r[ROW::COUNT] = 6.0;
            r[ROW::SERIAL] = 1.0;
            r[ROW::WORLD..ROW::WORLD + 16].copy_from_slice(&IDENTITY_F64);
            r[ROW::OBJECT] = (i + 1) as f64;
        }
    }
    scene.bridge.line_frame(2);

    // Also admit a vertex-colored axis geometry for `utility/line-3-solid` with `color` at location 3.
    // `shader::rewrite` assigns attribute location 0 to `position` and location 3 to `color`
    // for plain shaders whose key starts with `utility/line-3-` (`vertexColors == true`).
    let axes_prog = scene.bridge.add_owned_program("utility/line-3-solid")?;
    let axes_geo = scene.bridge.add_geometry();
    let (axes_pos, axes_col) = bfe_render::line_resources::Descriptor::axes();
    let pos_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(axes_pos.as_ptr().cast(), std::mem::size_of_val(&axes_pos))
    };
    let col_bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(axes_col.as_ptr().cast(), std::mem::size_of_val(&axes_col))
    };
    let pos_buf = scene.bridge.geometry_buffer(axes_geo, pos_bytes);
    scene
        .bridge
        .geometry_pointer(axes_geo, pos_buf, 0, 3, glow::FLOAT, false, 0, 0);
    let col_buf = scene.bridge.geometry_buffer(axes_geo, col_bytes);
    scene
        .bridge
        .geometry_pointer(axes_geo, col_buf, 3, 3, glow::FLOAT, false, 0, 0);
    scene
        .bridge
        .geometry_primitive(axes_geo, glow::LINES, 0, false, 1.0);

    // Append the `utility/line-3-solid` draw step as line slot 2 in view 2.
    {
        use bfe_render::{
            frame_plan::layout::{limit as L, line as LINE},
            post as P,
        };
        let scale_mv: [f64; 16] = [
            0.06, 0.0, 0.0, 0.0, 0.0, 0.06, 0.0, 0.0, 0.0, 0.0, 0.06, 0.0, -0.1, -0.1, 0.0, 1.0,
        ];
        let at = (2 * L::LINES + 2) * LINE::LEN;
        let row = &mut scene.bridge.r.frames.lines[at..at + LINE::LEN];
        row.fill(0.0);
        let s = LINE::STEP;
        row[s + P::STEP_PROGRAM] = axes_prog as f64;
        row[s + P::STEP_TARGET] = P::TARGET_BOUND;
        row[s + P::STEP_UNIFORMS] = 4.0;
        // Uniform 0: modelViewMatrix, 1: projectionMatrix, 2: diffuse, 3: opacity.
        let u0 = s + P::STEP_FIRST;
        row[u0] = 0.0;
        row[u0 + 1] = P::TYPE_MAT4 as f64;
        row[u0 + 2..u0 + 18].copy_from_slice(&scale_mv);
        let u1 = u0 + P::UNIFORM;
        row[u1] = 1.0;
        row[u1 + 1] = P::TYPE_MAT4 as f64;
        row[u1 + 2..u1 + 18].copy_from_slice(&IDENTITY_F64);
        let u2 = u1 + P::UNIFORM;
        row[u2] = 2.0;
        row[u2 + 1] = 3.0;
        row[u2 + 2..u2 + 5].copy_from_slice(&[1.0, 1.0, 1.0]);
        let u3 = u2 + P::UNIFORM;
        row[u3] = 3.0;
        row[u3 + 1] = 1.0;
        row[u3 + 2] = 1.0;
        let st = LINE::STATE;
        row[st + P::STATE_FLAGS] =
            (P::STATE_OWN_GEOMETRY | P::STATE_DEPTH_WRITE | P::STATE_DEPTH_TEST) as f64;
        row[st + P::STATE_DEPTH_FUNC] = glow::LEQUAL as f64;
        row[st + P::STATE_GEOMETRY] = axes_geo as f64;
        row[st + P::STATE_GEOMETRY + 1] = 6.0;
        scene.bridge.r.frames.line_counts[2] = 3;
    }

    // Bind role `R::LINES2` directly to `scene.canvas.framebuffer` (`TARGET = -1.0`)
    // and execute all 3 line draw packets.
    clear_canvas(scene);
    scene.bridge.r.frames.stats.fill(0.0);
    {
        let row = &mut scene.bridge.r.frames.bindings[R::LINES2 * B::LEN..(R::LINES2 + 1) * B::LEN];
        row.fill(0.0);
        row[B::TARGET] = -1.0;
        row[B::SAMPLE] = -1.0;
        row[B::WIDTH] = scene.size[0] as f64;
        row[B::HEIGHT] = scene.size[1] as f64;
        row[B::SCISSOR + 2] = scene.size[0] as f64;
        row[B::SCISSOR + 3] = scene.size[1] as f64;
    }
    if !scene
        .bridge
        .frame_aux_frame(&mut scene.resources, R::LINES2 as u32)
    {
        return Err(scene
            .bridge
            .frame_error_frame()
            .unwrap_or_else(|| "utility family frame_aux failed".into()));
    }

    Ok((
        vec![
            "utility/line-plain-solid".into(),
            "utility/line-3-solid".into(),
        ],
        collect_frame_pass_draws(scene),
    ))
}

/// Binds `scene.canvas.framebuffer` and clears color, depth, and stencil to `[0.04, 0.06, 0.09, 1.0]`.
/// Also resets `Core::state` so the next pass re-applies its full pipeline state.
fn clear_canvas(scene: &mut Scene) {
    let gl = &scene.bridge.r.core.gl;
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(scene.canvas.framebuffer));
        gl.viewport(0, 0, scene.size[0], scene.size[1]);
        gl.disable(glow::SCISSOR_TEST);
        gl.color_mask(true, true, true, true);
        gl.depth_mask(true);
        gl.clear_color(0.04, 0.06, 0.09, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
    }
    scene.bridge.r.core.state.reset();
}

/// Summary metrics computed from an RGBA8 readback buffer.
struct PixelMetrics {
    clear_pixel: [u8; 4],
    sample_pixel: [u8; 4],
    non_clear_pixels: usize,
    unique_colors: usize,
    digest: String,
}

/// Computes `nonClearPixels`, `uniqueColors`, and SHA-256 digest for an RGBA8 frame.
/// The clear color `[0.04, 0.06, 0.09, 1.0]` quantizes to `[10, 15, 23, 255]`;
/// any pixel whose RGB channels differ by more than 3 counts is counted as foreground.
fn analyze_pixels(pixels: &[u8]) -> PixelMetrics {
    let clear: [u8; 4] = [10, 15, 23, 255];
    let mut sample = clear;
    let mut non_clear_pixels = 0usize;
    let mut unique = BTreeSet::new();
    for px in pixels.chunks_exact(4) {
        let rgba: [u8; 4] = [px[0], px[1], px[2], px[3]];
        unique.insert(u32::from_le_bytes(rgba));
        let diff = (rgba[0] as i32 - clear[0] as i32)
            .abs()
            .max((rgba[1] as i32 - clear[1] as i32).abs())
            .max((rgba[2] as i32 - clear[2] as i32).abs());
        if diff > 3 {
            non_clear_pixels += 1;
            if sample == clear {
                sample = rgba;
            }
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(pixels);
    let digest = format!("{:x}", hasher.finalize());
    PixelMetrics {
        clear_pixel: clear,
        sample_pixel: sample,
        non_clear_pixels,
        unique_colors: unique.len(),
        digest,
    }
}

/// Writes an RGBA8 buffer (with OpenGL bottom-up rows flipped to top-down) to a PNG file.
fn write_png(path: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", dir_or_path(parent)))?;
        }
    }
    let row_bytes = (width * 4) as usize;
    let mut flipped = Vec::with_capacity(pixels.len());
    for row in (0..height as usize).rev() {
        let start = row * row_bytes;
        flipped.extend_from_slice(&pixels[start..start + row_bytes]);
    }
    let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&flipped).map_err(|e| e.to_string())
}

fn dir_or_path(p: &Path) -> String {
    p.display().to_string()
}

/// Composes a 2x3 contact sheet of the 6 family renders into a single PNG image.
/// Panels are placed left-to-right, top-to-bottom in `FAMILY_ORDER`:
/// - Row 0: `world` (top-left), `actors` (top-right)
/// - Row 1: `effects` (middle-left), `interface` (middle-right)
/// - Row 2: `post-hud` (bottom-left), `utility` (bottom-right)
fn write_contact_sheet(
    path: &Path,
    panel_w: u32,
    panel_h: u32,
    panels: &[(String, Vec<u8>)],
) -> Result<(), String> {
    let cols = 2u32;
    let rows = 3u32;
    let out_w = panel_w * cols;
    let out_h = panel_h * rows;
    let mut canvas = vec![0u8; (out_w * out_h * 4) as usize];

    for (idx, (_, pixels)) in panels.iter().take((cols * rows) as usize).enumerate() {
        let col = (idx as u32) % cols;
        let row = (idx as u32) / cols;
        // Panels are stored bottom-up; place row 0 at the top of the contact sheet
        // in bottom-up coordinates so `write_png`'s vertical flip puts panel 0 top-left.
        let flipped_row = (rows - 1) - row;
        for y in 0..panel_h {
            let src_start = ((y * panel_w) * 4) as usize;
            let dst_y = flipped_row * panel_h + y;
            let dst_start = (((dst_y * out_w) + col * panel_w) * 4) as usize;
            let len = (panel_w * 4) as usize;
            canvas[dst_start..dst_start + len].copy_from_slice(&pixels[src_start..src_start + len]);
        }
    }
    write_png(path, out_w, out_h, &canvas)
}
