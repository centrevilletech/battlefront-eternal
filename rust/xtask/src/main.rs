// Command-line entry point for `cargo xtask`:
//   - `cargo xtask schema`
//   - `cargo xtask shaders`
//   - `cargo xtask presentation-schema`
//   - `cargo xtask environment-schema`
//   - `cargo xtask transport-schema`
//   - `cargo xtask audio-control-schema`
//   - `cargo xtask audio-schema`
//   - `cargo xtask audio-codec-schema`
//   - `cargo xtask matrix`
//   - `cargo xtask build` / `check`
//
// Passing `--check` to any generator subcommand (or invoking `cargo xtask check`)
// verifies that every generated Rust and JSON artifact on disk is
// byte-for-byte identical to what the generator produces from the current
// schema and shader sources.

mod build;
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

fn main() {
    let args: Vec<String> = env::args().collect();
    let root = repo_root();
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let rest = if args.len() > 2 { &args[2..] } else { &[] };
    let check = rest.iter().any(|a| a == "--check");

    let res = match sub {
        "schema" | "sim-schema" => sim_schema_js::run_sim_schema(&root, check),
        "shaders" | "render-shaders" => shaders::render_shaders(&root, check),
        "presentation-schema" => layouts::presentation_schema(&root, check),
        "environment-schema" => layouts::environment_schema(&root, check),
        "transport-schema" => layouts::transport_schema(&root, check),
        "audio-control-schema" => layouts::audio_control_schema(&root, check),
        "audio-schema" => layouts::audio_schema(&root, check),
        "audio-codec-schema" => layouts::audio_codec_schema(&root, check),
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
        _ => Err(
            "usage: cargo xtask <schema|shaders|presentation-schema|environment-schema|transport-schema|audio-schema|audio-control-schema|audio-codec-schema|matrix|build|check> [--check]"
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
    use crate::schema_eval::{load_json_schema, load_schemas, LoadedSchema};
    use crate::sim_schema::{build_model, parse_type, rust_file, TARGETS};
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

    // Verifies that `build_model` rejects colliding Rust type names, reserved
    // helper names, invalid map-key unions, and unknown target kinds.
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

    // Loads `.json` schema files and compares the resulting `JsonValue` trees
    // against expected JSON fixtures, including `$layoutOnly` extraction.
    #[test]
    fn json_schema_loader_matches_expected_json_for_real_schemas() {
        let root = repo_root();
        let load_one = |stem: &str| -> (JsonValue, bool) {
            let json = root.join(format!("rust/sim/schema/{stem}.json"));
            load_json_schema(&fs::read_to_string(&json).unwrap(), &format!("{stem}.json")).unwrap()
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

        let main_schemas = load_schemas(&root.join("rust/sim/schema"), true).unwrap();
        assert_eq!(main_schemas.len(), 42);
        let test_schemas = load_schemas(&root.join("rust/sim/schema-test"), true).unwrap();
        assert_eq!(test_schemas.len(), 1);
    }

    // Compiles a small synthetic schema containing a constant, an enum, a tagged
    // union, a record, and a singleton kind, and snapshots the generated Rust
    // module (`rust_file`).
    #[test]
    fn codegen_snapshot_matches_rust_module() {
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

        assert_eq!(rust_code, expected_rust);
    }

    // Verifies that `JsonValue::parse`, `to_json_compact`, and `to_json_pretty`
    // match V8's `JSON.stringify` property ordering, number formatting (`-0.0`,
    // integer-valued floats, fractions), and 2-space pretty-printing rules.
    #[test]
    fn json_serializer_preserves_v8_order_and_number_formatting() {
        let parsed = JsonValue::parse(
            r#"{"z": 1.0, "10": "ten", "a": -0.0, "2": [true, false, null], "0": {}, "z": 2.5, "empty_arr": []}"#,
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
    #[test]
    fn check_mode_detects_deliberately_stale_generated_file() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp = env::temp_dir().join(format!("xtask-check-test-{}-{nanos}", process::id()));

        fs::create_dir_all(tmp.join("rust/audio-decode/src")).unwrap();

        layouts::audio_codec_schema(&tmp, false).unwrap();
        layouts::audio_codec_schema(&tmp, true).unwrap();

        let generated_rs = tmp.join("rust/audio-decode/src/layout.rs");
        fs::write(&generated_rs, "// stale file content\n").unwrap();

        let err = layouts::audio_codec_schema(&tmp, true)
            .expect_err("check mode must fail when a generated file is stale");
        assert!(
            err.contains("rust/audio-decode/src/layout.rs"),
            "unexpected error message: {err}"
        );

        layouts::audio_codec_schema(&tmp, false).unwrap();
        layouts::audio_codec_schema(&tmp, true).unwrap();

        let _ = fs::remove_dir_all(&tmp);
    }

    // Runs all eight generators in `--check` mode against the committed repository
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
    }
}
