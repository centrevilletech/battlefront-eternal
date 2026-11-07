//! effects.js::EffectResources and presentation/content.js::effectContent.
//! Resolve names, shared definitions and child groups once. Runtime particles
//! consume numeric handles and never walk the authored JSON library.
use bfe_content::{Object, Value};
use serde_json::Value as Json;
use std::rc::Rc;

const ANALYTIC_KEYS: usize = 8;
const ANALYTIC_DEFINITIONS: usize = 1024;

pub struct Effects {
    pub descriptor: Json,
    pub handles: Vec<(String, usize)>,
    pub gpu_tracks: Vec<f32>,
    pub gpu_size: [u32; 2],
    pub gpu_indices:Vec<Option<usize>>,
}
impl Effects {
    pub fn compile(library: &Value) -> Result<Self, String> {
        let roots = library
            .as_object()
            .ok_or("effect library must be an object")?
            .entries()
            .map(|(name, defs)| {
                let filtered = defs
                    .as_array()
                    .ok_or("effect group must be an array")?
                    .iter()
                    .filter(|d| {
                        !d.get("lightning").is_some_and(Value::truthy)
                            && !d.get("shield").is_some_and(Value::truthy)
                    })
                    .cloned()
                    .collect();
                // filter() creates a distinct root array even for equal/empty
                // groups. Definition objects and nested emitter arrays keep identity.
                Ok((name.to_string(), Value::Array(Rc::new(filtered))))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut gpu = Vec::new();
        for (_, root) in &roots {
            visit_gpu(root, &mut gpu)?;
        }
        let mut compiler = Compiler {
            gpu,
            definitions: Vec::new(),
            source: Vec::new(),
            groups: vec![Vec::new()],
            arrays: Vec::new(),
        };
        let mut handles = Vec::new();
        for (name, root) in roots {
            handles.push((name, compiler.group(&root)?));
        }
        let gpu_tracks = tracks(&compiler.gpu);
        let gpu_size = [
            TRACK_WIDTH as u32,
            compiler.gpu.len().max(1).next_power_of_two() as u32,
        ];
        let gpu_indices=compiler.source.iter().map(|source|compiler.gpu.iter().position(|v|v.same(source))).collect();
        Ok(Self {
            gpu_indices,
            gpu_tracks,
            gpu_size,
            descriptor: serde_json::json!({"definitions":compiler.definitions,"groups":compiler.groups}),
            handles,
        })
    }
}
fn visit_gpu(defs: &Value, gpu: &mut Vec<Value>) -> Result<(), String> {
    for def in defs.as_array().ok_or("emitter group must be an array")? {
        let geometry = def.get("geometry");
        let transformer = def.get("transformer");
        if let Some(children) = geometry
            .and_then(|g| g.get("emitters"))
            .filter(|v| v.truthy())
        {
            visit_gpu(children, gpu)?;
        }
        // effects-gpu.js visits children first, admits at most 1024 authored
        // definitions and only eight keys per analytic track. Excess definitions
        // stay CPU-presented; their groups and birth order remain unchanged.
        let Some(g) = geometry else {
            continue;
        };
        let Some(t) = transformer else {
            continue;
        };
        if g.get("blend").and_then(Value::as_str) == Some("blur")
            || !g.get("texture").is_some_and(Value::truthy)
            || !matches!(
                g.get("type").and_then(Value::as_str),
                Some("particle" | "animated" | "spark" | "streak" | "billboard")
            )
        {
            continue;
        }
        if ["position", "size", "color"].iter().any(|name| {
            t.get(name)
                .and_then(Value::as_array)
                .is_some_and(|a| a.len() > ANALYTIC_KEYS)
        }) {
            continue;
        }
        if gpu.len() < ANALYTIC_DEFINITIONS && !gpu.iter().any(|old| old.same(def)) {
            gpu.push(def.clone());
        }
    }
    Ok(())
}
struct Compiler {
    gpu: Vec<Value>,
    source: Vec<Value>,
    definitions: Vec<Json>,
    groups: Vec<Vec<usize>>,
    arrays: Vec<(Value, usize)>,
}
impl Compiler {
    fn group(&mut self, defs: &Value) -> Result<usize, String> {
        if let Some((_, id)) = self.arrays.iter().find(|(old, _)| old.same(defs)) {
            return Ok(*id);
        }
        let id = self.groups.len();
        self.groups.push(Vec::new());
        self.arrays.push((defs.clone(), id));
        for def in defs.as_array().ok_or("emitter group must be an array")? {
            let index = if let Some(i) = self.source.iter().position(|old| old.same(def)) {
                i
            } else {
                let index = self.source.len();
                self.source.push(def.clone());
                self.definitions.push(Json::Null);
                // Reserve the parent handle before descending, as Map.set does.
                // Child groups can therefore refer to an already known emitter.
                let children = def.get("geometry").and_then(|g| g.get("emitters"));
                let child = if let Some(children) =
                    children.filter(|v| v.as_array().is_some_and(|a| !a.is_empty()))
                {
                    self.group(children)?
                } else {
                    0
                };
                let mut descriptor = crate::loader::authored_json(def);
                let object = descriptor
                    .as_object_mut()
                    .ok_or("emitter definition must be an object")?;
                object.insert("child".into(), child.into());
                object.insert(
                    "gpu".into(),
                    self.gpu.iter().any(|old| old.same(def)).into(),
                );
                self.definitions[index] = descriptor;
                index
            };
            self.groups[id].push(index);
        }
        Ok(id)
    }
}
/// Object spread keeps the first property's order while replacing its value.
/// Keep Rc-backed authored objects shared so identical references keep one
/// definition handle; equal JSON values in different objects remain distinct.
pub fn merge<'a>(libraries: impl IntoIterator<Item = Option<&'a Value>>) -> Value {
    let mut written = Vec::new();
    for library in libraries {
        if let Some(object) = library.and_then(Value::as_object) {
            for (name, value) in object.entries() {
                written.push((Rc::from(name), value.clone()));
            }
        }
    }
    Value::Object(Rc::new(Object::from_written(written)))
}

// effects-gpu.js stores analytic tracks in a 64-texel row, two texels per
// key. These are the existing particle-analytic shader's columns, not frame
// transport offsets. Capture admission checks the resulting bytes by digest.
const TRACK_WIDTH: usize = 64;
const POSITION_TRACK: usize = 4;
const SIZE_TRACK: usize = 20;
const COLOR_TRACK: usize = 36;
fn tracks(defs: &[Value]) -> Vec<f32> {
    let height = defs.len().max(1).next_power_of_two();
    let mut data = vec![0.0; TRACK_WIDTH * height * 4];
    for (row, def) in defs.iter().enumerate() {
        let g = def.get("geometry").unwrap();
        let t = def.get("transformer").unwrap();
        let number = |v: Option<&Value>, fallback: f64| {
            v.and_then(Value::as_f64)
                .filter(|v| *v != 0.0)
                .unwrap_or(fallback)
        };
        let mut put = |column: usize, values: &[f64]| {
            for (i, v) in values.iter().take(4).enumerate() {
                data[(row * TRACK_WIDTH + column) * 4 + i] = *v as f32;
            }
        };
        let length = |key: &str| t.get(key).and_then(Value::as_array).map_or(0, |v| v.len()) as f64;
        put(
            0,
            &[length("position"), length("size"), length("color"), 0.0],
        );
        let kind = g.get("type").and_then(Value::as_str).unwrap_or("");
        put(
            1,
            &[
                if kind == "billboard" {
                    2.0
                } else if matches!(kind, "spark" | "streak") {
                    1.0
                } else {
                    0.0
                },
                if def.get("registerStep").is_some_and(Value::truthy) {
                    0.0
                } else {
                    1.0
                },
                number(g.get("sparkLength"), 0.1),
                0.0,
            ],
        );
        put(
            2,
            &[
                if kind == "animated" {
                    number(g.get("timePerFrame"), 0.0)
                } else {
                    0.0
                },
                number(g.get("frameSize"), 1.0).round().max(1.0),
                number(g.get("totalFrames"), 1.0).round().max(1.0),
                0.0,
            ],
        );
        for (kind, column, ops) in [
            (
                "position",
                POSITION_TRACK,
                &["accelerate", "reach", "scale"][..],
            ),
            ("size", SIZE_TRACK, &["scale", "move", "reach"][..]),
            ("color", COLOR_TRACK, &["reach", "move"][..]),
        ] {
            for (i, key) in t
                .get(kind)
                .and_then(Value::as_array)
                .unwrap_or(&[])
                .iter()
                .take(ANALYTIC_KEYS)
                .enumerate()
            {
                let operation = ops.iter().position(|name| key.get(name).is_some());
                let value = operation.and_then(|op| key.get(ops[op]));
                put(
                    column + i * 2,
                    &[
                        bfe_content::js::to_number(key.get("lifetime")),
                        operation.map_or(0.0, |op| op as f64 + 1.0),
                        0.0,
                        0.0,
                    ],
                );
                let mut channels = [0.0; 4];
                let count = if let Some(values) = value.and_then(Value::as_array) {
                    for (i, v) in values.iter().take(4).enumerate() {
                        channels[i] = bfe_content::js::to_number(Some(v));
                    }
                    values.len().min(4)
                } else {
                    channels[0] = number(value, 0.0);
                    1
                };
                if kind == "color" {
                    for v in &mut channels {
                        *v /= 255.0;
                    }
                }
                put(column + i * 2 + 1, &channels[..count]);
            }
        }
    }
    data
}
