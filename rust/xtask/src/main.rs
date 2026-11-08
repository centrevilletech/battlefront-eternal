// Command-line entry point for `cargo xtask`, replacing the legacy Node.js
// build and code-generation scripts under `tools/`:
//   - `cargo xtask schema`              (`tools/sim-schema.mjs`)
//   - `cargo xtask schema --export-json DIR` (converts `.mjs` schemas to `.json`)
//   - `cargo xtask shaders`             (`tools/render-shaders.mjs`)
//   - `cargo xtask presentation-schema` (`tools/presentation-schema.mjs`)
//   - `cargo xtask environment-schema`  (`tools/environment-schema.mjs`)
//   - `cargo xtask transport-schema`    (`tools/transport-schema.mjs`)
//   - `cargo xtask audio-control-schema`(`tools/audio-control-schema.mjs`)
//   - `cargo xtask audio-schema`        (`tools/audio-schema.mjs`)
//   - `cargo xtask audio-codec-schema`  (`tools/audio-codec-schema.mjs`)
//   - `cargo xtask host-contract`       (`tools/host-contract.mjs`)
//   - `cargo xtask matrix`              (`tools/world-hash-matrix.mjs`)
//   - `cargo xtask build` / `check`     (`tools/build-rust.mjs`)
//   - `ar-sorted`                       (`tools/ar-sorted.mjs`)
//
// Passing `--check` to any generator subcommand (or invoking `cargo xtask check`)
// verifies that every generated Rust, JavaScript, and JSON artifact on disk is
// byte-for-byte identical to what the generator produces from the current
// schema and shader sources.

mod build;
mod host_contract;
mod json;
mod layouts;
mod matrix;
mod schema_eval;
mod shaders;
mod sim_schema;
mod sim_schema_js;

use std::env;
use std::path::{Path, PathBuf};
use std::process;

// Resolves the repository root directory from `CARGO_MANIFEST_DIR`
// (`<repo>/rust/xtask` -> `<repo>`).
pub fn repo_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("rust/xtask must be two levels below repo root")
        .to_path_buf()
}

// Extracts the directory argument from `--export-json <DIR>` or `--export-json=<DIR>`
// when passed to `cargo xtask schema`.
fn parse_export_json_arg(args: &[String]) -> Result<Option<String>, String> {
    for (i, arg) in args.iter().enumerate() {
        if arg == "--export-json" {
            let dir = args
                .get(i + 1)
                .filter(|s| !s.starts_with('-'))
                .ok_or_else(|| "usage: cargo xtask schema --export-json <DIR>".to_string())?;
            return Ok(Some(dir.clone()));
        }
        if let Some(dir) = arg.strip_prefix("--export-json=") {
            if dir.is_empty() {
                return Err("usage: cargo xtask schema --export-json <DIR>".to_string());
            }
            return Ok(Some(dir.to_string()));
        }
    }
    Ok(None)
}

// Dispatches `cargo xtask <subcommand>` or acts as the deterministic `ar-sorted`
// archiver wrapper when invoked via a symlink/copy named `ar-sorted`.
fn main() {
    let args: Vec<String> = env::args().collect();
    if let Some(arg0) = args.first() {
        let exe_name = Path::new(arg0)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if exe_name == "ar-sorted" || exe_name == "ar-sorted.exe" {
            process::exit(build::run_ar_sorted(&args[1..]));
        }
    }

    let root = repo_root();
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let rest = if args.len() > 2 { &args[2..] } else { &[] };
    let check = rest.iter().any(|a| a == "--check");

    // Dispatch the requested xtask subcommand:
    //   - `schema` / `sim-schema`: simulation store Rust & JS codegen (`sim_schema_js::run_sim_schema`),
    //     or `.mjs` -> `.json` schema export when `--export-json <DIR>` is passed
    //   - `shaders` / `render-shaders`: GLSL shader table codegen (`shaders::render_shaders`)
    //   - `presentation-schema`: presentation frame/command layout codegen (`layouts::presentation_schema`)
    //   - `environment-schema`: environment config/state layout codegen (`layouts::environment_schema`)
    //   - `transport-schema`: network transport packet/state layout codegen (`layouts::transport_schema`)
    //   - `audio-control-schema`: audio control command/state layout codegen (`layouts::audio_control_schema`)
    //   - `audio-schema`: audio mixer frame layout codegen (`layouts::audio_schema`)
    //   - `audio-codec-schema`: ADPCM/PCM codec header layout codegen (`layouts::audio_codec_schema`)
    //   - `host-contract`: boundary wire contract hash & table codegen (`host_contract::host_contract`)
    //   - `matrix`: native world-hash matrix runner (`matrix::run_matrix`)
    //   - `build`: full generator + Wasm build pipeline (`build::run_build`)
    //   - `check`: runs all nine generators in `--check` verification mode
    //   - `ar-sorted`: deterministic `ar` wrapper sorting archive members by basename
    let res = match sub {
        "schema" | "sim-schema" => match parse_export_json_arg(rest) {
            Ok(Some(dir)) => schema_eval::export_schemas_json(&root, &dir),
            Ok(None) => sim_schema_js::run_sim_schema(&root, check),
            Err(e) => Err(e),
        },
        "shaders" | "render-shaders" => shaders::render_shaders(&root, check),
        "presentation-schema" => layouts::presentation_schema(&root, check),
        "environment-schema" => layouts::environment_schema(&root, check),
        "transport-schema" => layouts::transport_schema(&root, check),
        "audio-control-schema" => layouts::audio_control_schema(&root, check),
        "audio-schema" => layouts::audio_schema(&root, check),
        "audio-codec-schema" => layouts::audio_codec_schema(&root, check),
        "host-contract" => host_contract::host_contract(&root, check),
        "matrix" => matrix::run_matrix(&root, rest),
        "build" => build::run_build(&root, rest),
        "check" => {
            let mut check_args = vec!["--check".to_string()];
            for a in rest {
                if a != "--check" {
                    check_args.push(a.clone());
                }
            }
            build::run_build(&root, &check_args)
        }
        "ar-sorted" => process::exit(build::run_ar_sorted(rest)),
        _ => Err(
            "usage: cargo xtask <schema|shaders|presentation-schema|environment-schema|host-contract|transport-schema|audio-schema|audio-control-schema|audio-codec-schema|matrix|build|check> [--check|--export-json DIR]"
                .to_string(),
        ),
    };

    if let Err(err) = res {
        eprintln!("{err}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::JsonValue;
    use crate::schema_eval::{eval_schema_module, load_json_schema, load_schemas, LoadedSchema};
    use crate::sim_schema::{build_model, parse_type, rust_file, TARGETS};
    use crate::sim_schema_js::js_file;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Helper that parses a JSON schema string and runs `build_model` against the
    // production simulation target (`TARGETS[0]`).
    fn model_from_json(file: &str, json: &str) -> Result<crate::sim_schema::Model, String> {
        let val = JsonValue::parse(json)?;
        build_model(
            &[LoadedSchema {
                file: file.to_string(),
                schema: val,
                layout_only: false,
            }],
            TARGETS[0].clone(),
        )
    }

    // Verifies that `parse_type` accepts valid scalar, nullable, list, map, obj,
    // and typed-array expressions and rejects malformed or nested-container types.
    // Tests `parse_type` across the full grammar of schema type expressions:
    //   - `map:<V>`, `map:num:<V>`, and `map:ref:<K>:<V>` key/value combinations
    //   - parenthesized nullable inner items (`list:(ref:character|null)`) vs
    //     outer nullable lists (`list:ref:character|null`)
    //   - `obj:<V>` plain-object dictionaries and `<Typed>Array(N)` fixed views
    //   - rejection of double-null (`|null|null`), nested containers (`map:map:f64`,
    //     `list:(list:f64|null)`), unknown scalars (`vec4`), and invalid modifiers.
    #[test]
    fn schema_types_are_checked_as_they_are_read() {
        assert_eq!(
            parse_type("map:list:thing|null", "x")
                .unwrap()
                .value
                .unwrap()
                .of
                .as_deref(),
            Some("thing")
        );
        assert!(parse_type("map:f64|null|null", "x")
            .unwrap_err()
            .contains("twice"));
        assert!(parse_type("map:map:f64", "x")
            .unwrap_err()
            .contains("no map"));
        assert!(parse_type("vec4", "x")
            .unwrap_err()
            .contains("unknown type"));
        assert!(parse_type("f64|maybe", "x")
            .unwrap_err()
            .contains("null or undefined"));

        let ring = parse_type("list:(ref:character|null)", "x").unwrap();
        let rv = ring.value.unwrap();
        assert_eq!(
            (
                rv.base.as_str(),
                rv.of.as_deref(),
                rv.nullable.null,
                ring.nullable.null
            ),
            ("ref", Some("character"), true, false)
        );
        assert!(
            parse_type("list:(f64|undefined)|null", "x")
                .unwrap()
                .nullable
                .null
        );
        assert!(
            parse_type("list:ref:character|null", "x")
                .unwrap()
                .nullable
                .null
        );
        assert!(parse_type("list:(f64)", "x")
            .unwrap_err()
            .contains("can be null or undefined"));
        assert!(parse_type("list:(list:f64|null)", "x")
            .unwrap_err()
            .contains("no list"));

        let num_key = parse_type("map:num:list:thing", "x").unwrap().key.unwrap();
        assert_eq!(
            (num_key.base.as_str(), num_key.of.as_deref()),
            ("num", None)
        );
        let ref_key = parse_type("map:ref:Who:list:thing", "x")
            .unwrap()
            .key
            .unwrap();
        assert_eq!(
            (ref_key.base.as_str(), ref_key.of.as_deref()),
            ("ref", Some("Who"))
        );
        assert!(parse_type("map:ref:person", "x").unwrap().key.is_none());
        assert!(
            parse_type("obj:(rec:Spot|null)", "x")
                .unwrap()
                .value
                .unwrap()
                .nullable
                .null
        );
        assert!(parse_type("obj:map:f64", "x")
            .unwrap_err()
            .contains("no map"));

        let ring2 = parse_type("Int32Array(100)|null", "x").unwrap();
        assert_eq!(
            (
                ring2.base.as_str(),
                ring2.elem.as_deref(),
                ring2.n,
                ring2.class.as_deref()
            ),
            ("typed", Some("i32"), 100, Some("Int32Array"))
        );
        assert!(parse_type("list:Float64Array(3)", "x")
            .unwrap_err()
            .contains("no object or typed array"));
    }

    // Verifies that `build_model` rejects colliding Rust type names, names that
    // shadow `runtime.js` helper functions, invalid map-key unions, and unknown
    // target kinds.
    // Verifies that `build_model` enforces global identifier invariants:
    //   - A union and a kind cannot map to the same PascalCase Rust type name (`Body` vs `body`).
    //   - A kind cannot use a name (`each`) whose generated `sameEach` function
    //     would shadow the `sameEach` runtime helper imported from `src/sim/runtime.js`.
    //   - A union used as a `map:ref:<Union>:<V>` key must consist exclusively of
    //     `ref:<kind>` variants (no `null` or primitive branches).
    //   - Every `ref:<target>` in a map key must resolve to a declared kind or union.
    //   - Fixed `<Typed>Array(N)` views cannot be used directly as union variants.
    #[test]
    fn duplicate_and_reserved_names_are_refused() {
        assert!(model_from_json(
            "x",
            r#"{"unions":{"Body":["null","ref:body"]},"kinds":{"body":{}}}"#
        )
        .err()
        .unwrap()
        .contains("Body is also"));
        assert!(model_from_json(
            "x",
            r#"{"kinds":{"each":{"singleton":true,"fields":{"n":"f64"}}}}"#
        )
        .err()
        .unwrap()
        .contains("sameEach"));
        assert!(model_from_json(
            "x",
            r#"{"unions":{"Key":["null","ref:body"]},"kinds":{"body":{},"shelf":{"singleton":true,"fields":{"m":"map:ref:Key:f64"}}}}"#
        )
        .err()
        .unwrap()
        .contains("each of its variants is a kind"));
        assert!(model_from_json(
            "x",
            r#"{"kinds":{"shelf":{"singleton":true,"fields":{"m":"map:ref:nobody:f64"}}}}"#
        )
        .err()
        .unwrap()
        .contains("no kind or union nobody"));
        assert!(
            model_from_json("x", r#"{"unions":{"U":["null","Float64Array(2)"]}}"#)
                .err()
                .unwrap()
                .contains("cannot be a union's variant")
        );
    }

    // Verifies identity-kind rules, `$publish` field lists, and `$observe`
    // metadata extraction in `build_model`.
    // Verifies store entity lifecycle and publication metadata rules:
    //   - Kinds with `identity: true` are stored in the Rust arena (`stored: true`)
    //     without requiring a numeric `$id` key or a singleton home list, and
    //     cannot simultaneously declare a `key` property.
    //   - Ordinary slotted kinds without `singleton: true` or `identity: true`
    //     are rejected if no singleton `list:<kind>` field claims them via `home`.
    //   - `$publish: [...]` records the ordered subset of fields exported to the
    //     double-buffered presentation publication snapshot.
    //   - `$observe: [...]` records the subset of fields tracked by `syncObserved*`
    //     when a record is referenced by a `$defer` kind field.
    #[test]
    fn identity_publication_and_observed_metadata_rules() {
        let model = model_from_json(
            "x",
            r#"{"kinds":{"snapshot":{"identity":true,"fields":{"life":"f64"}}}}"#,
        )
        .unwrap();
        let kind = model.kinds.get("snapshot").unwrap();
        assert!(kind.stored);
        assert_eq!(kind.key, None);
        assert_eq!(kind.home_kind_name, None);

        assert!(
            model_from_json("x", r#"{"kinds":{"snapshot":{"fields":{"life":"f64"}}}}"#)
                .err()
                .unwrap()
                .contains("needs a home list")
        );
        assert!(model_from_json(
            "x",
            r#"{"kinds":{"snapshot":{"identity":true,"key":"id","fields":{"life":"f64"}}}}"#
        )
        .err()
        .unwrap()
        .contains("identity fields have no key"));

        let pub_model = model_from_json(
            "publication",
            r#"{"records":{"Pose":{"$layout":true,"$publish":["position","tick"],"position":"vec3","tick":"f64","fixed":"f64"}}}"#,
        )
        .unwrap();
        let pose = pub_model.records.get("Pose").unwrap();
        assert_eq!(
            pose.publish,
            vec!["position".to_string(), "tick".to_string()]
        );
        assert_eq!(pose.fields.len(), 3);

        let obs_model = model_from_json(
            "observed",
            r#"{"records":{"Detail":{"$observe":["time"],"time":"f64","entries":"list:f64"}},"kinds":{"item":{"singleton":true,"delta":true,"defer":["detail"],"fields":{"detail":"rec:Detail|null"}}}}"#,
        )
        .unwrap();
        assert_eq!(
            obs_model.records.get("Detail").unwrap().observe,
            vec!["time".to_string()]
        );
        assert_eq!(obs_model.records.get("Detail").unwrap().fields.len(), 2);
    }

    // Evaluates declarative `.mjs` schema files (or `.json` equivalents after
    // conversion) and compares the resulting `JsonValue` trees against expected
    // JSON fixtures. Also verifies `layoutOnly` extraction and rejection of
    // unsupported JS statements.
    #[test]
    fn evaluator_matches_expected_json_for_real_schemas() {
        let root = repo_root();
        let load_one = |stem: &str| -> (JsonValue, bool) {
            let mjs = root.join(format!("rust/sim/schema/{stem}.mjs"));
            if mjs.exists() {
                eval_schema_module(&fs::read_to_string(&mjs).unwrap(), &format!("{stem}.mjs"))
                    .unwrap()
            } else {
                let json = root.join(format!("rust/sim/schema/{stem}.json"));
                load_json_schema(&fs::read_to_string(&json).unwrap(), &format!("{stem}.json"))
                    .unwrap()
            }
        };

        let (phase_val, phase_layout_only) = load_one("phase");
        assert!(!phase_layout_only);
        let expected_phase = r#"{"enums":{"BpQueryOp":["members","buildingAttacker","vehicleAttacker","victims","draw"],"BpActionOp":["post","mobile","kill","effect","over"]},"records":{"BpInput":{"dt":"f64","spawning":"bool"},"BpMember":{"unit":"ref:unit","soldier":"bool","alive":"bool","removed":"bool","seated":"bool","held":"bool","captures":"bool","playerMounted":"bool","team":"f64","position":"f64x3","character":"ref:character|null"},"BpHull":{"vehicle":"ref:vehicle","alive":"bool","position":"f64x3"},"BpQueryArgs":{"op":"enum:BpQueryOp","building":"ref:building|null","vehicle":"ref:vehicle|null"},"BpQueryResult":{"members":"list:rec:BpMember","hulls":"list:rec:BpHull","character":"ref:character|null","number":"f64"},"BpActionArgs":{"op":"enum:BpActionOp","post":"rec:PostEvent|null","mobile":"rec:PsMobileEvent|null","unit":"ref:unit|null","vehicle":"ref:vehicle|null","name":"str|null","position":"f64x3","number":"f64"}}}"#;
        assert_eq!(phase_val.to_json_compact(), expected_phase);

        let (flight_val, flight_layout_only) = load_one("flight");
        assert!(!flight_layout_only);
        let expected_flight = r#"{"enums":{"FlightState":["Landed","TakingOff","Flying","Landing","Destroyed"],"FlightClass":["fighter","scout","bomber","transport"]},"kinds":{"vehicle":{"fields":{"flightState":"enum:FlightState|null","grounded":"bool","altitude":"f64","throttle":"f64","airspeed":"f64","stateTime":"f64","floorY":"f64","bank":"f64","gearFoot":{"type":"f64|null","mirror":false},"radius":{"type":"f64","mirror":false},"team":"f64|undefined","flightClass":{"type":"enum:FlightClass|undefined","mirror":false},"minSpeed":{"type":"f64|undefined","mirror":false},"midSpeed":{"type":"f64|undefined","mirror":false},"boostSpeed":{"type":"f64|undefined","mirror":false},"power":"f64|undefined","stickX":"f64|undefined","stickY":"f64|undefined","flightRollRate":"f64|undefined","rollLock":"bool|undefined","boosting":"bool|undefined","trick":"str|undefined","trickTime":"f64|undefined","trickCooldown":"f64|undefined","trickPending":"f64|undefined","trickDirection":"f64|undefined","gear":"f64|undefined","landingMessage":"str|undefined","landingMessageTime":"f64|undefined","flightTrickRight":"vec3|undefined","flightClock":"f64|undefined","boostExhausted":"bool|undefined","landingReason":"str|undefined","flightWarning":"str|undefined","terrainWarning":"bool|undefined","flightAssistId":"f64|undefined","flightLockId":"f64|undefined","flightLockGeneration":"f64|undefined","flightLockTimer":"f64|undefined","flightLostLOS":"f64|undefined","flightLockState":"str|undefined","flightStart":"vec3|undefined","flightEnd":"vec3|undefined","flightStartQ":"quat|undefined","flightTrickLoopQ":"quat|undefined","flightTrickEndQ":"quat|undefined","flightPreviousQ":"quat|undefined","flightTickQ":"quat|undefined","flightActionHeld":"bool|undefined","trickHeld":"bool|undefined","immelmannHeld":"bool|undefined","flightFrameQ":"quat|undefined"},"skip":{"flightConfig":"its class's tuning (src/flight/config.js flightConfig): the class definition's speeds and handling under the live FLIGHT_CLASSES entry","flightLeanQ":"drawFlyer's scratch, set before every read"}}}}"#;
        assert_eq!(flight_val.to_json_compact(), expected_flight);

        let (_hud_val, hud_layout_only) = load_one("hud");
        assert!(hud_layout_only);
    }

    // Verifies that `cargo xtask schema --export-json DIR` exports every `.mjs`
    // schema to `.json` and that `load_schemas` + `sim_schema_js::generate_all`
    // produce byte-for-byte identical generated files from `.json` sources.
    #[test]
    fn export_json_roundtrips_and_regenerates_byte_identically() {
        let root = repo_root();
        if !root.join("rust/sim/schema/client.mjs").exists() {
            return;
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp = env::temp_dir().join(format!("xtask-export-json-{}-{nanos}", process::id()));
        let tmp_schema = tmp.join("rust/sim/schema");
        let tmp_schema_test = tmp.join("rust/sim/schema-test");
        fs::create_dir_all(&tmp_schema).unwrap();
        fs::create_dir_all(&tmp_schema_test).unwrap();

        // Copy `rust/sim/schema-test/shelf.mjs` into `tmp_schema_test`, export both
        // directories to `.json`, and delete the `.mjs` files so `load_schemas` reads
        // exclusively from the exported `.json` files.
        for entry in fs::read_dir(root.join("rust/sim/schema-test")).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("mjs") {
                fs::copy(&path, tmp_schema_test.join(entry.file_name())).unwrap();
            }
        }
        schema_eval::export_schemas_json(&root, &tmp_schema.to_string_lossy()).unwrap();
        schema_eval::export_schemas_json(&root, &tmp_schema_test.to_string_lossy()).unwrap();
        let _ = fs::remove_file(tmp_schema_test.join("shelf.mjs"));

        // Verify that `load_schemas` over the JSON-only directories produces the exact
        // same `LoadedSchema` list (file stem, V8-ordered `JsonValue`, and `layout_only`)
        // as `load_schemas` over the `.mjs` directories.
        let mjs_main = load_schemas(&root.join("rust/sim/schema"), true).unwrap();
        let json_main = load_schemas(&tmp_schema, true).unwrap();
        assert_eq!(mjs_main.len(), json_main.len());
        for (a, b) in mjs_main.iter().zip(json_main.iter()) {
            assert_eq!(a.file, b.file);
            assert_eq!(a.layout_only, b.layout_only);
            assert_eq!(a.schema, b.schema, "schema mismatch for {}", a.file);
        }

        // Verify that `generate_all` against the JSON-only temp workspace produces
        // byte-for-byte identical contents for all 94 generated Rust and JS files.
        let from_mjs = sim_schema_js::generate_all(&root).unwrap();
        let from_json = sim_schema_js::generate_all(&tmp).unwrap();
        assert_eq!(from_mjs.len(), from_json.len());
        for ((p_mjs, c_mjs), (p_json, c_json)) in from_mjs.iter().zip(from_json.iter()) {
            let rel_mjs = p_mjs.strip_prefix(&root).unwrap();
            let rel_json = p_json.strip_prefix(&tmp).unwrap();
            assert_eq!(rel_mjs, rel_json);
            assert_eq!(
                c_mjs,
                c_json,
                "generated file differs for {}",
                rel_mjs.display()
            );
        }

        let _ = fs::remove_dir_all(&tmp);
    }

    // Compiles a small synthetic schema containing a constant, an enum, a tagged
    // union, a record, and a singleton kind, and snapshots both the generated
    // Rust module (`rust_file`) and JavaScript mirror module (`js_file`).
    // Snapshot test for both `sim_schema::rust_file` and `sim_schema_js::js_file`
    // over a synthetic schema module (`synth.mjs`) covering:
    //   - a shared integer constant (`MAX_STEPS`)
    //   - a 2-variant string enum (`Mode`: `idle`, `active`)
    //   - a 3-variant tagged union (`Choice`: `null`, `f64`, `enum:Mode`)
    //   - a 2-field flat record (`Pair`: `i32`, `enum:Mode`)
    //   - a singleton store kind (`panel`) combining mirrored (`pair`, `choice`)
    //     and import-only (`notes`, `mirror: false`) fields so `EXPORT_WORDS` (6)
    //     and `IMPORT_WORDS` / `FULL_WORDS` (7) differ.
    #[test]
    fn codegen_snapshot_matches_rust_and_js_mirrors() {
        let model = model_from_json(
            "synth",
            r#"{
                "constants": { "MAX_STEPS": 4 },
                "enums": { "Mode": ["idle", "active"] },
                "unions": { "Choice": ["null", "f64", "enum:Mode"] },
                "records": {
                    "Pair": { "count": "i32", "mode": "enum:Mode" }
                },
                "kinds": {
                    "panel": {
                        "singleton": true,
                        "fields": {
                            "pair": "rec:Pair",
                            "choice": "union:Choice",
                            "notes": { "type": "f64", "mirror": false }
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let rust_code = rust_file(&model, "synth");
        let js_code = js_file(&model, "synth");

        let expected_rust = concat!(
            "// Generated by tools/sim-schema.mjs from rust/sim/schema/synth.mjs; do not edit.\n", "#![allow(clippy::all, unused_braces, unused_parens, unused_variables)]\n", "\n",
            "use super::*;\n", "\n", "pub const MAX_STEPS:u32=4;\n",
            "#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]\n", "#[repr(u8)]\n", "pub enum Mode {\n",
            "    #[default] Idle = 0,\n", "    Active = 1,\n", "}\n",
            "\n", "impl Mode {\n", "    pub const NAMES: [&'static str; 2] = [\"idle\", \"active\"];\n",
            "\n", "    pub fn from_u8(v: u8) -> Option<Self> {\n", "        match v {\n",
            "            0 => Some(Mode::Idle),\n", "            1 => Some(Mode::Active),\n", "            _ => None,\n",
            "        }\n", "    }\n", "}\n",
            "\n", "#[derive(Clone, Copy, Debug, PartialEq)]\n", "pub enum Choice {\n",
            "    Null,\n", "    F64(f64),\n", "    Mode(Mode),\n",
            "}\n", "\n", "impl Default for Choice {\n",
            "    fn default() -> Self { Choice::Null }\n", "}\n", "\n",
            "impl Choice {\n", "    pub const WORDS: usize = 2;\n", "\n",
            "    /// Its row words: the variant, then its value (a reference's slot and generation).\n", "    pub fn put(&self, row: &mut [f64], heap: &mut Vec<f64>) {\n", "        match self {\n",
            "            Choice::Null => { row[0] = 0.0; row[1..2].fill(0.0);  },\n", "            Choice::F64(v) => { row[0] = 1.0; row[1] = (*v); }\n", "            Choice::Mode(v) => { row[0] = 2.0; row[1] = (*v) as u8 as f64; }\n",
            "        }\n", "    }\n", "\n",
            "    pub fn take(row: &[f64], heap: &[f64]) -> Self {\n", "        match word_u32(row[0]) {\n", "            0 => Choice::Null,\n",
            "            1 => Choice::F64(row[1]),\n", "            2 => Choice::Mode(Mode::from_u8(word_u32(row[1]) as u8).unwrap_or_default()),\n", "            _ => Choice::default(),\n",
            "        }\n", "    }\n", "}\n",
            "\n", "#[derive(Clone, Debug, Default, PartialEq)]\n", "pub struct Pair {\n",
            "    pub count: i32,\n", "    pub mode: Mode,\n", "}\n",
            "\n", "impl Pair {\n", "    pub const WORDS: usize = 2;\n",
            "\n", "    pub fn put(&self, row: &mut [f64], heap: &mut Vec<f64>) {\n", "        let row: &mut [f64; 2] = (&mut row[..2]).try_into().unwrap();\n",
            "        row[0] = self.count as f64;\n", "        row[1] = self.mode as u8 as f64;\n", "    }\n",
            "\n", "    pub fn take(row: &[f64], heap: &[f64]) -> Self {\n", "        let row: &[f64; 2] = (&row[..2]).try_into().unwrap();\n",
            "        Self {\n", "            count: word_i32(row[0]),\n", "            mode: Mode::from_u8(word_u32(row[1]) as u8).unwrap_or_default(),\n",
            "        }\n", "    }\n", "\n",
            "}\n", "\n", "/// A panel (one a battle), from rust/sim/schema/synth.mjs.\n",
            "#[derive(Clone, Debug, Default, PartialEq)]\n", "pub struct Panel {\n", "    pub pair: Pair,\n",
            "    pub choice: Choice,\n", "    pub notes: f64,\n", "}\n",
            "\n", "impl Panel {\n", "    /// Words a row takes: slot, generation, then the fields mirrored (export),\n",
            "    /// read by a Rust phase while JavaScript owns the kind (import), or all (full).\n", "    pub const EXPORT_STRIDE: usize = 6;\n", "    pub const IMPORT_STRIDE: usize = 7;\n",
            "    pub const FULL_STRIDE: usize = 7;\n", "\n", "    pub fn put_full(&self, row: &mut [f64], heap: &mut Vec<f64>) {\n",
            "        let row: &mut [f64; 7] = (&mut row[..7]).try_into().unwrap();\n", "        self.pair.put(&mut row[2..], heap);\n", "        self.choice.put(&mut row[4..4 + 2], heap);\n",
            "        row[6] = self.notes;\n", "    }\n", "\n",
            "    pub fn take_full(&mut self, row: &[f64], heap: &[f64]) {\n", "        let row: &[f64; 7] = (&row[..7]).try_into().unwrap();\n", "        self.pair = Pair::take(&row[2..], heap);\n",
            "        self.choice = Choice::take(&row[4..4 + 2], heap);\n", "        self.notes = row[6];\n", "    }\n",
            "\n", "    pub fn put_export(&self, row: &mut [f64], heap: &mut Vec<f64>) {\n", "        let row: &mut [f64; 6] = (&mut row[..6]).try_into().unwrap();\n",
            "        self.pair.put(&mut row[2..], heap);\n", "        self.choice.put(&mut row[4..4 + 2], heap);\n", "    }\n",
            "\n", "    pub fn take_export(&mut self, row: &[f64], heap: &[f64]) {\n", "        let row: &[f64; 6] = (&row[..6]).try_into().unwrap();\n",
            "        self.pair = Pair::take(&row[2..], heap);\n", "        self.choice = Choice::take(&row[4..4 + 2], heap);\n", "    }\n",
            "\n", "    #[inline(always)]\n", "    pub fn put_import(&self, row: &mut [f64], heap: &mut Vec<f64>) { self.put_full(row, heap) }\n",
            "\n", "    #[inline(always)]\n", "    pub fn take_import(&mut self, row: &[f64], heap: &[f64]) { self.take_full(row, heap) }\n",
            "\n", "}\n", "",
        );

        let expected_js = concat!(
            "// Generated by tools/sim-schema.mjs from rust/sim/schema/synth.mjs; do not edit.\n", "import * as THREE from \"three\";\n", "import { sameArray, sameEach, sameList, sameMap, sameQuat, sameVec } from \"../compare.js\";\n",
            "\n", "export const MAX_STEPS = 4;\n", "export const Mode_NAMES = [\"idle\",\"active\"];\n",
            "export const Mode_INDEX = new Map(Mode_NAMES.map((name, i) => [name, i]));\n", "export const Mode_VALUE = Object.freeze({\"idle\":0,\"active\":1});\n", "\n",
            "export function variantOfChoice(b, x) {\n", "  if (x === null) return 0;\n", "  if (typeof x === \"number\") return 1;\n",
            "  if (Mode_INDEX.has(x)) return 2;\n", "  return -1;\n", "}\n",
            "\n", "// Which of Choice's variants a value is.\n", "export function whichChoice(b, x) {\n",
            "  const i = variantOfChoice(b, x);\n", "  if (i < 0) throw Error(`Choice: ${x?.constructor?.name ?? x} is none of its variants`);\n", "  return i;\n",
            "}\n", "\n", "// An export row's union (from `at`) as a value: a vector or record the field held, if it is of the variant, read into in place.\n",
            "export function syncChoice(b, f, at, h, current) {\n", "  switch (f[at]) {\n", "    case 0: return null;\n",
            "    case 1: { let v = variantOfChoice(b, current) === 1 ? current : undefined; v = f[at + 1]; return v; }\n", "    case 2: { let v = variantOfChoice(b, current) === 2 ? current : undefined; v = Mode_NAMES[f[at + 1]]; return v; }\n", "    default: throw Error('Choice: no such variant');\n",
            "  }\n", "}\n", "\n",
            "export function putChoice(b, x, f, at, heap) {\n", "  const i = whichChoice(b, x);\n", "  f[at + 1] = 0;\n",
            "  f[at] = i;\n", "  switch (i) {\n", "    case 1: { f[at + 1] = x; break; }\n",
            "    case 2: { f[at + 1] = Mode_INDEX.get(x); break; }\n", "  }\n", "}\n",
            "\n", "export function sameChoice(x, y) {\n", "  if (Object.is(x, y)) return true;\n",
            "  if (x === null || y === null || x === undefined || y === undefined) return false;\n", "  return false;\n", "}\n",
            "\n", "export function exportChoice(x, cx) {\n", "  switch (whichChoice(cx, x)) {\n",
            "    case 0: return null;\n", "    case 1: return x;\n", "    case 2: return x;\n",
            "  }\n", "}\n", "\n",
            "export class PairMirror {\n", "  constructor() {\n", "    this.count = 0;\n",
            "    this.mode = \"idle\";\n", "  }\n", "}\n",
            "\n", "export function syncPair(b, f, at, h, m) {\n", "  m.count = f[at + 0] | 0;\n",
            "  m.mode = Mode_NAMES[f[at + 1]];\n", "  return m;\n", "}\n",
            "\n", "export function putPair(b, o, f, at, heap) {\n", "  f[at + 0] = o.count;\n",
            "  f[at + 1] = Mode_INDEX.get(o.mode);\n", "}\n", "\n",
            "export function samePair(a, b) {\n", "  if (!Object.is(a.count, b.count)) return \".count\";\n", "  if (!Object.is(a.mode, b.mode)) return \".mode\";\n",
            "  return null;\n", "}\n", "\n",
            "export function exportPair(o, cx) {\n", "  let v = cx.seen.get(o);\n", "  if (v) return v;\n",
            "  v = {};\n", "  cx.seen.set(o, v);\n", "  v.count = o.count;\n",
            "  v.mode = o.mode;\n", "  return v;\n", "}\n",
            "\n", "// panel: from rust/sim/schema/synth.mjs.\n", "export class PanelMirror {\n",
            "  constructor() {\n", "    this.pair = new PairMirror();\n", "    this.choice = null;\n",
            "  }\n", "}\n", "\n",
            "export const PANEL_KEYS = new Set([\"pair\", \"choice\", \"notes\"]);\n", "export const PANEL_EXPORT_STRIDE = 6, PANEL_IMPORT_STRIDE = 7, PANEL_FULL_STRIDE = 7;\n", "\n",
            "// An export row (from `at`; the heap from `h`) into mirror `m`.\n", "export function syncPanel(b, f, at, h, m) {\n", "  m.pair = syncPair(b, f, at + 2, h, m.pair ?? new PairMirror());\n",
            "  m.choice = syncChoice(b, f, at + 4, h, m.choice);\n", "  return m;\n", "}\n",
            "\n", "// A full row into object `m`.\n", "export function syncFullPanel(b, f, at, h, m) {\n",
            "  m.pair = syncPair(b, f, at + 2, h, m.pair ?? new PairMirror());\n", "  m.choice = syncChoice(b, f, at + 4, h, m.choice);\n", "  m.notes = f[at + 6];\n",
            "  return m;\n", "}\n", "\n",
            "// Object `o`'s import fields into a row (lists to `heap`).\n", "export function putPanel(b, o, f, at, heap) {\n", "  putPair(b, o.pair, f, at + 2, heap);\n",
            "  putChoice(b, o.choice, f, at + 4, heap);\n", "  f[at + 6] = o.notes;\n", "}\n",
            "\n", "// Every field of object `o` into a row.\n", "export function putFullPanel(b, o, f, at, heap) {\n",
            "  putPair(b, o.pair, f, at + 2, heap);\n", "  putChoice(b, o.choice, f, at + 4, heap);\n", "  f[at + 6] = o.notes;\n",
            "}\n", "\n", "// The first mirrored field of `m` that is not what the export row says (a\n",
            "// write JavaScript made to Rust's state without a command), or null.\n", "export function checkPanel(b, f, at, h, m) {\n", "  const want = syncPanel(b, f, at, h, new PanelMirror());\n",
            "  { const d = samePair(m.pair, want.pair); if (d) return \".pair\" + d; }\n", "  if (!sameChoice(m.choice, want.choice)) return \".choice\";\n", "  return null;\n",
            "}\n", "\n", "export function samePanel(a, b) {\n",
            "  { const d = samePair(a.pair, b.pair); if (d) return \".pair\" + d; }\n", "  if (!sameChoice(a.choice, b.choice)) return \".choice\";\n", "  if (!Object.is(a.notes, b.notes)) return \".notes\";\n",
            "  return null;\n", "}\n", "\n",
            "export function exportPanel(o, cx) {\n", "  let v = cx.seen.get(o);\n", "  if (v) return v;\n",
            "  v = {};\n", "  cx.seen.set(o, v);\n", "  v.pair = exportPair(o.pair, cx);\n",
            "  v.choice = exportChoice(o.choice, cx);\n", "  v.notes = o.notes;\n", "  return v;\n",
            "}\n", "",
        );

        assert_eq!(rust_code, expected_rust);
        assert_eq!(js_code, expected_js);
    }

    // Verifies that `JsonValue::parse`, `to_json_compact`, and `to_json_pretty`
    // match V8's `JSON.stringify` property ordering (ascending array-index keys
    // followed by string keys in insertion order, with duplicate keys updating
    // in place), number formatting (`-0.0`, integer-valued floats, fractions),
    // and 2-space pretty-printing rules.
    // Tests the custom `JsonValue` parser and serializer (`json.rs`) against
    // V8 / ECMAScript `JSON.stringify` rules:
    //   - Array-index keys (`"0"`, `"2"`, `"10"`) are sorted in ascending numeric
    //     order ahead of ordinary string keys (`"z"`, `"a"`, `"empty_arr"`).
    //   - Duplicate keys (`"z": 1.0` followed by `"z": 2.5`) update the existing
    //     slot in place without moving `"z"` after `"a"`.
    //   - `-0.0` formats as `"0"`, integer-valued floats (`1.0`) format without
    //     a trailing `.0`, and fractional floats (`2.5`) preserve their digits.
    //   - `to_json_pretty` uses 2-space indentation per nesting level while keeping
    //     empty `{}` and `[]` containers on a single line.
    #[test]
    fn json_serializer_preserves_v8_order_and_number_formatting() {
        let (parsed, _) = eval_schema_module(
            r#"export default {"z": 1.0, "10": "ten", "a": -0.0, "2": [true, false, null], "0": {}, "z": 2.5, "empty_arr": []};"#,
            "v8_order.mjs",
        )
        .unwrap();

        assert_eq!(
            parsed.to_json_compact(),
            r#"{"z":2.5,"10":"ten","a":0,"2":[true,false,null],"0":{},"empty_arr":[]}"#
        );

        let expected_pretty = concat!(
            "{\n",
            "  \"z\": 2.5,\n",
            "  \"10\": \"ten\",\n",
            "  \"a\": 0,\n",
            "  \"2\": [\n",
            "    true,\n",
            "    false,\n",
            "    null\n",
            "  ],\n",
            "  \"0\": {},\n",
            "  \"empty_arr\": []\n",
            "}"
        );
        assert_eq!(parsed.to_json_pretty(), expected_pretty);
    }

    // Verifies that generator `--check` mode (`check = true`) detects a
    // deliberately stale generated file and returns an `Err` naming the stale
    // path, while write mode (`check = false`) restores the file so a subsequent
    // `--check` passes.
    // End-to-end test of generator `--check` verification (`write_or_check`):
    //   1. Sets up an isolated temporary workspace root with `rust/audio-codec/schema.json`.
    //   2. Runs `layouts::audio_codec_schema(&tmp, false)` to write the generated
    //      Rust (`rust/audio-codec/src/layout.rs`) and JS (`src/audio/codec-layout.js`)
    //      artifacts and confirms `check = true` succeeds.
    //   3. Corrupts `src/audio/codec-layout.js` with stale content and asserts
    //      that `check = true` fails with an error message naming the stale file.
    //   4. Re-runs with `check = false` to regenerate the file and confirms that
    //      `check = true` passes again.
    #[test]
    fn check_mode_detects_deliberately_stale_generated_file() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp = env::temp_dir().join(format!("xtask-check-test-{}-{nanos}", process::id()));

        fs::create_dir_all(tmp.join("src/audio")).unwrap();
        fs::create_dir_all(tmp.join("rust/audio-decode/src")).unwrap();

        // Generate the initial Rust and JS layout files into the temp root.
        layouts::audio_codec_schema(&tmp, false).unwrap();
        layouts::audio_codec_schema(&tmp, true).unwrap();

        // Deliberately corrupt one generated output file so it becomes stale.
        let generated_js = tmp.join("src/audio/codec-layout.js");
        fs::write(&generated_js, "// stale file content\n").unwrap();

        let err = layouts::audio_codec_schema(&tmp, true)
            .expect_err("check mode must fail when a generated file is stale");
        assert!(
            err.contains("src/audio/codec-layout.js"),
            "unexpected error message: {err}"
        );

        // Re-running with `check = false` overwrites the stale file and restores check pass.
        layouts::audio_codec_schema(&tmp, false).unwrap();
        layouts::audio_codec_schema(&tmp, true).unwrap();

        let _ = fs::remove_dir_all(&tmp);
    }

    // Runs all nine generators in `--check` mode against the committed repository
    // tree to guarantee byte-for-byte parity across every generated file.
    #[test]
    fn all_generators_match_committed_files() {
        let root = repo_root();
        sim_schema_js::run_sim_schema(&root, true).unwrap();
        shaders::render_shaders(&root, true).unwrap();
        layouts::presentation_schema(&root, true).unwrap();
        layouts::environment_schema(&root, true).unwrap();
        layouts::transport_schema(&root, true).unwrap();
        layouts::audio_control_schema(&root, true).unwrap();
        layouts::audio_schema(&root, true).unwrap();
        layouts::audio_codec_schema(&root, true).unwrap();
        host_contract::host_contract(&root, true).unwrap();
    }
}
