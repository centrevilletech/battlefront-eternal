//! Admission adapters for the map inspector. These build resource recipes
//! once; every frame copies into the existing renderer's bounded banks.
use crate::render_bridge::Bridge;
use bfe_content_native::{
    geometry::{Attribute, Geometry, Scalar},
    loader::Blend,
    BattleContent,
};
use bfe_render::{far as F, instances as I, programs, shader, water as W};
use serde_json::{json, Value};
use three::Matrix4;
// FarPass.registerItem retains a numeric admission slot for each sky layer.
// Matrix and frame sources are resolved once, before the first render.
// Keeping only offsets here avoids uniform-name searches in the frame loop.
pub struct Sky {
    pub slot: usize,
    pub follow: Option<[f64; 2]>,
    uniforms: Vec<(usize, usize, usize)>,
}
// WaterPass.write has one camera bank and fixed material values. Sun direction
// is negated/normalized the way SurfaceHost publishes it; the authored ray
// direction in the shared lighting bank has the opposite sign.
pub struct Water {
    pub(super) key: &'static str,
    sun: Option<usize>,
    sun_from: usize,
    ocean: Option<Box<(bfe_environment::surfaces::Phase, usize, f64)>>,
    uniforms: Vec<(usize, usize, usize)>,
}
#[inline(never)]
fn new_ocean_phase(
    model: usize,
    height: f64,
) -> Box<(bfe_environment::surfaces::Phase, usize, f64)> {
    let mut phase = bfe_environment::surfaces::Phase::new(false);
    phase.frame[bfe_environment::layout::surface_frame::OCEAN] = 1.;
    Box::new((phase, model, height))
}
// These are cold bind-pose templates, independent of the visible command
// prefix. Selection owns two reserved index lists, so changing camera views
// neither allocates nor destroys an object that may be visible later.
pub struct Admission {
    pub records: Vec<[f64; I::REC_STRIDE]>,
    pub bones: Vec<f32>,
    pub skies: Vec<Sky>,
    pub water: Option<Water>,
    pub props: usize,
    pub foliage: usize,
    pub deferred: usize,
    eye: usize,
    selected: Vec<usize>,
}
// Resolve the shader-owned std140 layout at admission. Padding between vec3
// members is not a host guess; FRAME_OFFSETS remains the single authority.
// Copy lengths describe values rather than the following member's padding.
fn frame(name: &str) -> Option<(usize, usize)> {
    shader::FRAME_MEMBERS
        .iter()
        .position(|(n, _)| *n == name)
        .map(|i| {
            (
                shader::FRAME_OFFSETS[i],
                match shader::FRAME_MEMBERS[i].1 {
                    "mat4" => 16,
                    "vec4" => 4,
                    "vec3" => 3,
                    "vec2" => 2,
                    _ => 1,
                },
            )
        })
}
// modelPack uses separate typed buffers after native rig batching. Preserve
// their values and index order; repacking or welding changes raster edges.
// Geometry.register resolves locations against the selected owned program.
fn geometry(positions: &[f32], normals: &[f32], uv: &[f32], index: &[u32]) -> Geometry {
    let attributes = [
        ("position", 3, positions),
        ("normal", 3, normals),
        ("uv", 2, uv),
    ]
    .into_iter()
    .filter(|(_, _, data)| !data.is_empty())
    .enumerate()
    .map(|(buffer, (name, size, _))| Attribute {
        name: name.into(),
        buffer,
        size,
        scalar: Scalar::Float,
        normalized: false,
        stride: 0,
        offset: 0,
    })
    .collect();
    let buffers = [positions, normals, uv]
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.iter().flat_map(|v| v.to_le_bytes()).collect())
        .collect();
    Geometry {
        buffers,
        attributes,
        indices: index.to_vec(),
        vertices: positions.len() / 3,
        groups: vec![],
    }
}
pub(crate) fn actor_key(material: &bfe_content_native::loader::Material, cascades: usize) -> Result<String, String> {
    // actorProgram's closed identity comes from the same source manifest.
    // The caller supplies cascades for world actors; view models use none.
    let mut defines = vec![
        (
            if material.flags & 1 == 0 {
                "UNLIT"
            } else {
                "DYNAMIC"
            },
            "",
        ),
        ("ZERO_LIGHTS", "16"),
    ];
    if material.cutout {
        defines.push(("ALPHA_TEST", ""))
    }
    if material.blend != Blend::Opaque {
        defines.push(("BLENDED", ""))
    }
    if material.record["scroll"].is_array() {
        defines.push(("SCROLL", ""))
    }
    defines.sort();
    // The selector includes draw kind as well as material defines. A rigid
    // look and a skinned bind-pose look are different programs even when
    // they cover the same triangles at admission.
    let wanted = json!({"defines":defines,"skin":true,"cascades":cascades,"mode":"skinned","lamps":false});
    static MANIFEST: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    let manifest = MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../bfe-render/shaders/actors/manifest.json"
        ))
        .expect("authored actor manifest")
    });
    manifest["programs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["selector"] == wanted)
        .and_then(|p| p["key"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("unowned static prop material {wanted}"))
}
pub fn props(
    bridge: &mut Bridge,
    content: &BattleContent,
    textures: &[u32],
    placed: bfe_content_native::placed::Placements,
    cascades: usize,
) -> Result<Admission, String> {
    let mut a = Admission {
        records: vec![],
        bones: vec![],
        skies: vec![],
        water: None,
        props: 0,
        foliage: 0,
        deferred: placed.deferred,
        eye: frame("cameraPosition").unwrap().0,
        selected: Vec::with_capacity(I::MAX_RECORDS),
    };
    // Copies of one model share GPU geometry. Their skeletons and placements
    // remain independent, so moving or replacing a later host-owned look
    // cannot mutate another instance through a shared scene object.
    let mut geometries = std::collections::BTreeMap::new();
    for part in placed.parts {
        if a.records.len() >= 65536 || part.bones.len() > I::BONES_LEN {
            return Err("placed prop admission capacity".into());
        }
        let mesh = content
            .models
            .iter()
            .find(|m| m.name == format!("{}/{}", part.model, part.source))
            .ok_or("missing placed model resource")?;
        let material = &mesh.material;
        let key = actor_key(material, cascades)?;
        let program = bridge.add_owned_program(&key)?;
        // The batch builder's representative source part fixes its material.
        // Texture names alone are not enough to share batches: flags, scroll
        // values and bind-frame transforms can differ on a single model.
        let resource = part.geometry_key.clone();
        let g = if let Some(g) = geometries.get(&resource) {
            *g
        } else {
            let mut geometry = geometry(&part.positions, &part.normals, &part.uv, &part.index);
            for (name, size, scalar, bytes) in [
                (
                    "skinIndex",
                    4,
                    Scalar::U16,
                    part.joints
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                ),
                (
                    "skinWeight",
                    4,
                    Scalar::Float,
                    part.weights.iter().flat_map(|v| v.to_le_bytes()).collect(),
                ),
            ] {
                let buffer = geometry.buffers.len();
                geometry.buffers.push(bytes);
                geometry.attributes.push(Attribute {
                    name: name.into(),
                    buffer,
                    size,
                    scalar,
                    normalized: false,
                    stride: 0,
                    offset: 0,
                });
            }
            let g = geometry.register(&mut bridge.r, &key)?;
            geometries.insert(resource, g);
            g
        };
        // InstancePass reads the existing generated record layout. Matrices
        // and bones use attached-mode skinning exactly as the browser rigs do,
        // including its recomposed bind inverses and final Float32 bone rows.
        let mut row = [0.; I::REC_STRIDE];
        row[I::REC_KIND] = 1.;
        row[I::REC_BONES] = (part.bones.len() / 16) as f64;
        row[I::REC_BONE_AT] = (a.bones.len() / 16) as f64;
        a.bones.extend_from_slice(&part.bones);
        row[I::REC_BIND..I::REC_BIND + 16].copy_from_slice(&Matrix4::default().elements);
        row[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE + 16].copy_from_slice(&part.bind_inverse);
        row[I::REC_PROGRAM] = program as f64;
        row[I::REC_GEOMETRY] = g as f64;
        row[I::REC_TEXTURES] = material.texture.map_or(-1., |t| textures[t] as f64);
        row[I::REC_WORLD..I::REC_WORLD + 16].copy_from_slice(&part.matrix);
        row[I::REC_SPHERE..I::REC_SPHERE + 3].copy_from_slice(&part.center);
        row[I::REC_SPHERE + 3] = part.radius;
        // The object-space box feeds the rigid light-mask path; the sphere
        // feeds visibility for both draw kinds. Keep source bounds rather
        // than expanding them from an already placed world-space mesh.
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in part.positions.chunks_exact(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k] as f64);
                hi[k] = hi[k].max(p[k] as f64);
            }
        }
        row[I::REC_BOX..I::REC_BOX + 3].copy_from_slice(&lo);
        row[I::REC_BOX + 3..I::REC_BOX + 6].copy_from_slice(&hi);
        row[I::REC_LOOK] = 1.;
        row[I::REC_LOOK + 3] = 1.;
        row[I::REC_FLAGS] = (if material.double_sided { 2 } else { 0 }
            | if material.cutout { 4 } else { 0 }
            | 8) as f64;
        // Scroll is the first two values of the instance extra bank, matching
        // shader.rs's declared scroll row. No uniform-name work occurs in draw.
        if let Some(scroll) = material.record["scroll"].as_array() {
            for (i, v) in scroll.iter().take(2).enumerate() {
                row[I::REC_EXTRA + i] = v.as_f64().unwrap_or(0.);
            }
        }
        // Transparent rigs keep their sort identity and declared blend, then
        // join the renderer's common transparent list. No host draw splits
        // opaque and transparent content into an independent GL ordering.
        if material.blend != Blend::Opaque {
            let additive = material.blend == Blend::Additive;
            let blend = [
                1,
                glow::FUNC_ADD,
                glow::FUNC_ADD,
                glow::SRC_ALPHA,
                if additive {
                    glow::ONE
                } else {
                    glow::ONE_MINUS_SRC_ALPHA
                },
                glow::ONE,
                if additive {
                    glow::ONE
                } else {
                    glow::ONE_MINUS_SRC_ALPHA
                },
            ];
            let id = bridge.inst_add_blend(
                &blend,
                1,
                glow::LEQUAL,
                &[program, if material.double_sided { 2 } else { 0 }],
            );
            row[I::REC_KIND] = I::KIND_BLENDED;
            row[I::REC_BLEND] = id as f64;
            row[I::REC_BLEND + 3] = a.records.len() as f64;
        }
        a.records.push(row);
        a.props += 1;
    }
    Ok(a)
}
// A sky keeps its own Plain camera uniforms. FarPass owns matrix formation,
// sorting, sampler units and raster state; this host supplies numeric sources.
pub fn sky(
    bridge: &mut Bridge,
    mesh: &bfe_content_native::loader::Mesh,
    textures: &[u32],
    slot: usize,
    empty: u32,
) -> Result<Sky, String> {
    let blended = mesh.material.blend != Blend::Opaque;
    let key = if blended {
        "effects/sky-zero-lights-16-sky-no-point-blended-s2-plain-chunks"
    } else {
        "effects/sky-zero-lights-16-sky-no-point-s0-plain-chunks"
    };
    let spec = programs::spec(key)?;
    let p = bridge.add_owned_program(key)?;
    let g = mesh.geometry.register(&mut bridge.r, key)?;
    let types = spec
        .uniforms
        .iter()
        .map(|(_, t)| gl_type(*t))
        .collect::<Vec<_>>();
    let names = spec
        .uniforms
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join(",");
    let table = bridge
        .far_table(p, &names, &types, &vec![1; types.len()])
        .ok_or("invalid sky table")?;
    // Sky layers draw before far terrain without testing or writing depth.
    // A blended layer still stays in sky order; it is not moved behind the
    // far scene merely because its texture carries alpha.
    let flags = F::F_PLACED | F::F_COLOR_WRITE | if blended { F::F_BLEND | 2 } else { 0 };
    let dst = if mesh.material.blend == Blend::Additive {
        glow::ONE
    } else {
        glow::ONE_MINUS_SRC_ALPHA
    };
    let blend = [
        glow::FUNC_ADD,
        glow::FUNC_ADD,
        glow::SRC_ALPHA,
        dst,
        glow::SRC_ALPHA,
        dst,
    ];
    let layer = mesh.material.record["skyLayer"].as_f64().unwrap_or(0.);
    let id = bridge.far_item(
        p,
        table,
        g,
        flags,
        glow::LEQUAL,
        &blend,
        &[0., -100000. + layer, layer, layer],
        &[0.; 4],
        slot as u32,
    );
    // FarPass forms object matrices from the retained placement. Non-camera
    // lighting values refer to preassigned numeric slots, refreshed from the
    // same frame bank that feeds near rendering.
    let mut at = slot + 17;
    let mut uniforms = Vec::new();
    for (i, (name, ty)) in spec.uniforms.iter().enumerate() {
        let source = match *name {
            "projectionMatrix" => Some(F::SRC_PROJECTION),
            "viewMatrix" => Some(F::SRC_VIEW),
            "cameraPosition" => Some(F::SRC_EYE),
            "modelMatrix" => Some(F::SRC_MODEL),
            "modelViewMatrix" => Some(F::SRC_MODEL_VIEW),
            "normalMatrix" => Some(F::SRC_NORMAL),
            _ => None,
        };
        if let Some(source) = source {
            bridge.far_source(id, i as u32, source, &[], 0)
        } else if *name == "map" {
            bridge.far_source(
                id,
                i as u32,
                F::SRC_TEXTURE,
                &[],
                mesh.material.texture.map_or(empty, |t| textures[t]),
            );
        } else if *name == "sunScale" {
            bridge.far_source(id, i as u32, F::SRC_FLOATS, &[1.], 0)
        } else if let Some((from, len)) = frame(name) {
            if at + len > F::FAR_INPUT_LEN {
                return Err("native sky uniform capacity".into());
            }
            bridge.far_source(id, i as u32, F::SRC_INPUT, &[], at as u32);
            uniforms.push((at, from, len));
            at += len;
        } else {
            return Err(format!("unknown native sky uniform {name}:{ty}"));
        }
    }
    bridge.r.far.input[slot] = 1.;
    bridge.r.far.input[slot + 1..slot + 17].copy_from_slice(&Matrix4::default().elements);
    Ok(Sky {
        slot,
        follow: serde_json::from_value(mesh.material.record["skyFollow"].clone()).ok(),
        uniforms,
    })
}
pub fn sky_end(s: &Sky) -> usize {
    s.uniforms
        .last()
        .map_or(s.slot + 17, |(at, _, len)| at + len)
}
fn gl_type(t: u32) -> u32 {
    match t {
        1 => glow::FLOAT,
        2 => glow::FLOAT_VEC2,
        3 => glow::FLOAT_VEC3,
        4 => glow::FLOAT_VEC4,
        5 => glow::INT,
        9 => glow::FLOAT_MAT3,
        16 => glow::FLOAT_MAT4,
        32 => glow::SAMPLER_2D,
        _ => 0,
    }
}
// graphics.js's sky-follow rule changes placement, not authored vertices.
// A horizon model can retain some of its fixed height while following the
// camera in X/Z. All resource identities remain stable across those writes.
pub fn update(a: &mut Admission, bridge: &mut Bridge, view: &[f64]) {
    let eye = a.eye;
    for s in &a.skies {
        let y = s
            .follow
            .map_or(view[eye + 1], |f| f[0] * view[eye + 1] + (1. - f[0]) * f[1]);
        bridge.r.far.input[s.slot + 13..s.slot + 16].copy_from_slice(&[
            view[eye],
            y,
            view[eye + 2],
        ]);
        for &(at, from, len) in &s.uniforms {
            bridge.r.far.input[at..at + len].copy_from_slice(&view[from..from + len]);
        }
    }
    if let Some(w) = &mut a.water {
        for &(at, from, len) in &w.uniforms {
            bridge.r.water.input[at..at + len].copy_from_slice(&view[from..from + len]);
        }
        let from = w.sun_from;
        let mut sun = three::Vector3::new(-view[from], -view[from + 1], -view[from + 2]);
        sun.normalize();
        if let Some(at) = w.sun {
            bridge.r.water.input[at..at + 3].copy_from_slice(&[sun.x, sun.y, sun.z]);
        }
        if let Some(ocean) = &mut w.ocean {
            let (phase, model, height) = &mut **ocean;
            use bfe_environment::layout::{surface_frame as F, surface_result as R};
            phase.frame[F::EYE..F::EYE + 3].copy_from_slice(&view[eye..eye + 3]);
            phase.update::<bfe_environment::surface_math::V8, _>(&mut NoShore);
            let mut matrix = Matrix4::default();
            matrix.elements[12] = phase.result[R::OCEAN_X];
            matrix.elements[13] = *height;
            matrix.elements[14] = phase.result[R::OCEAN_Z];
            bridge.r.water.input[*model..*model + 16].copy_from_slice(&matrix.elements);
            bridge.r.water.input[W::S_WORLD..W::S_WORLD + 16].copy_from_slice(&matrix.elements);
        }
    }
}
pub fn water(
    bridge: &mut Bridge,
    data: &Value,
    main: u32,
    mask: u32,
    empty: u32,
    foam: u32,
    ocean_mask: u32,
) -> Result<Option<Water>, String> {
    if data.is_null() {
        return Ok(None);
    }
    let classic_ocean = data["look"]["OceanEnable"][0].as_f64().unwrap_or(0.) > 0.;
    let key = if classic_ocean {
        "effects/ocean-classic-s2"
    } else {
        "effects/water-textured-s2"
    };
    let spec = programs::spec(key)?;
    let p = bridge.add_owned_program(key)?;
    bridge.r.water.prepare_program(p as usize,spec.uniforms);
    let look = &data["look"];
    let scale = data["scale"].as_f64().unwrap_or(1.);
    let divisions = look["PatchDivisions"][0].as_f64().unwrap_or(16.);
    let rows = look["PatchDivisions"][1].as_f64().unwrap_or(divisions);
    // Water.isSea selects the coarse surface whenever cells touch all four
    // grid edges, even when OceanEnable is absent. This changes subdivision
    // and must agree before geometry is uploaded.
    let sea = data["grid"]
        .as_u64()
        .filter(|n| *n > 0)
        .is_some_and(|grid| {
            let Some(cells) = data["cells"].as_array() else {
                return false;
            };
            [(0, 0), (0, grid - 1), (1, 0), (1, grid - 1)]
                .iter()
                .all(|&(axis, edge)| cells.iter().any(|c| c[axis].as_u64() == Some(edge)))
        });
    let ocean = look["OceanEnable"][0].as_f64().unwrap_or(0.) > 0. || sea;
    // Water.surface rounds PatchDivisions after applying map scale. A flat
    // sea uses one quad per cell; clipped shore patches keep their authored
    // triangles regardless of the regular grid's subdivision.
    let surface = if classic_ocean {
        bfe_content_native::water::ocean_geometry()
    } else {
        bfe_content_native::water::surface(
            data,
            if ocean {
                1
            } else {
                (divisions * scale).round().max(1.) as usize
            },
            if ocean {
                1
            } else {
                (rows * scale).round().max(1.) as usize
            },
        )?
    };
    let mut positions = Vec::new();
    let mut uv = Vec::new();
    for row in surface.vertices.chunks_exact(5) {
        positions.extend_from_slice(&row[..3]);
        uv.extend_from_slice(&row[3..]);
    }
    let g = geometry(&positions, &[], &uv, &surface.index).register(&mut bridge.r, key)?;
    let blend = [
        if classic_ocean { 0 } else { 1 },
        glow::FUNC_ADD,
        glow::FUNC_ADD,
        glow::SRC_ALPHA,
        glow::ONE_MINUS_SRC_ALPHA,
        glow::ONE,
        glow::ONE_MINUS_SRC_ALPHA,
    ];
    // DrawState requires colour write explicitly as well as depth test and
    // depth write. Omitting it submits triangles that only alter the depth
    // buffer, so the isolated water view also qualifies visible colour.
    let id = bridge.water_surface(g, &blend, glow::LEQUAL, 1 | 2 | 4, 2) as usize;
    let base = id * W::SURFACE;
    let v = &mut bridge.r.water.input[base..base + W::SURFACE];
    v[W::S_SHOWN] = 1.;
    v[W::S_PROGRAM] = p as f64;
    v[W::S_KEY + 1] = -20.;
    v[W::S_WORLD..W::S_WORLD + 16].copy_from_slice(&Matrix4::default().elements);
    v[W::S_RANGE + 1] = surface.index.len() as f64;
    v[W::S_UNIFORMS] = spec.uniforms.len() as f64;
    v[W::S_ARRAY] = -1.;
    // The browser's pair helper repeats the first component when the second
    // is absent. Texture tiling scales up with the map; scrolling velocity
    // scales down, preserving the prepared water look across map scales.
    let pair = |key: &str, default: f64, mult: f64| {
        let a = look[key][0].as_f64().unwrap_or(default);
        vec![a * mult, look[key][1].as_f64().unwrap_or(a) * mult]
    };
    let mut uniforms = Vec::new();
    // Resolve each catalog uniform into the existing WaterPass row format.
    // Camera sources vary; texture ids, tint, mask frame and zero animation
    // time stay fixed for this static inspection boundary.
    let mut sun = None;
    let mut model = base;
    let mut wind = three::Vector2::new(
        look["WindDirection"][0].as_f64().unwrap_or(1.),
        look["WindDirection"][1].as_f64().unwrap_or(0.),
    );
    wind.normalize();
    if wind.length_sq() == 0. {
        wind.set(1., 0.);
    }
    for (i, (name, ty)) in spec.uniforms.iter().enumerate() {
        let at = W::S_FIRST + i * bfe_render::post::UNIFORM;
        v[at] = i as f64;
        v[at + 1] = *ty as f64;
        let values = match *name {
            "mainMap" => vec![main as f64],
            "maskA" | "maskB" => vec![if mask == u32::MAX { empty } else { mask } as f64],
            "maskMix" | "time" => vec![0.],
            "modelMatrix" => {
                model = base + at + 2;
                Matrix4::default().elements.to_vec()
            }
            "waterMap" => vec![main as f64],
            "foamMap" => vec![foam as f64],
            "mask" => vec![ocean_mask as f64],
            "wind" => vec![wind.x, wind.y],
            "amplitude" => {
                vec![(look["WindSpeed"][0].as_f64().unwrap_or(0.) * 0.05).clamp(0.15, 2.)]
            }
            "foamTile" => pair("FoamTile", 1., 1.),
            "cell" => vec![data["cell"].as_f64().ok_or("water cell missing")?],
            "gridWidth" => vec![
                data["cell"].as_f64().unwrap()
                    * data["grid"].as_f64().ok_or("water grid missing")?,
            ],
            "gridOffset" => serde_json::from_value::<Vec<f64>>(data["offset"].clone())
                .map_err(|e| e.to_string())?,
            "tint" => (0..4)
                .map(|k| look["RefractionColor"][k].as_f64().unwrap_or(255.) / 255.)
                .collect(),
            "tile" => pair("Tile", 1., if classic_ocean { 1. } else { scale }),
            "velocity" => pair("Velocity", 0., if classic_ocean { 1. } else { 1. / scale }),
            "maskTile" => pair("SpecularMaskTile", 1., scale),
            "maskVelocity" => pair("SpecularMaskScrollSpeed", 0., 1. / scale),
            "cameraRange" => vec![0.3, 20000.],
            "toSun" => {
                sun = Some(base + at + 2);
                let (from, len) = frame("sunDirection").unwrap();
                uniforms.push((base + at + 2, from, len));
                vec![0.; len]
            }
            _ => {
                if let Some((from, len)) = frame(name) {
                    uniforms.push((base + at + 2, from, len));
                    vec![0.; len]
                } else {
                    return Err(format!("unknown native water uniform {name}"));
                }
            }
        };
        v[at + 2..at + 2 + values.len()].copy_from_slice(&values);
    }
    Ok(Some(Water {
        key,
        sun,
        sun_from: frame("sunDirection").unwrap().0,
        ocean: if classic_ocean {
            Some(new_ocean_phase(
                model,
                data["height"].as_f64().ok_or("ocean height missing")?,
            ))
        } else {
            None
        },
        uniforms,
    }))
}

impl Admission {
    // The browser InstancePass culls before filling its bounded command bank.
    // Retain every cold prop; a camera moving elsewhere must not discover that
    // admission permanently discarded the last objects after a capacity limit.
    // Online actors share the same near bank. Reserving their prefix budget
    // keeps a prop-dense map from starving every remote body of a draw record.
    pub fn select_limit(&mut self,bridge:&mut Bridge,maximum:usize)->usize{
        let camera = bridge.r.core.camera();
        select_rows(&self.records, &camera.frustum, &mut self.selected);
        self.selected.truncate(maximum.min(I::MAX_RECORDS));
        // Frame packets restore shared instance and bone banks, including
        // the first-person pass at index zero. Refill the static prefix even
        // when culling selects the same props as the preceding frame.
        pack_rows(&self.records, &self.bones, &self.selected,
            &mut bridge.r.core.input, &mut bridge.r.core.bones);
        self.selected.len()
    }
}

fn pack_rows(records: &[[f64; I::REC_STRIDE]], bones: &[f32], selected: &[usize],
    input: &mut [f64], output_bones: &mut [f32]) {
    let mut bone = 0;
    for (to, &from) in selected.iter().enumerate() {
        let row = &records[from];
        let at = I::IN_RECORDS + to * I::REC_STRIDE;
        input[at..at + I::REC_STRIDE].copy_from_slice(row);
        input[at + I::REC_BONE_AT] = (bone / 16) as f64;
        let first = row[I::REC_BONE_AT] as usize * 16;
        let count = row[I::REC_BONES] as usize * 16;
        output_bones[bone..bone + count].copy_from_slice(&bones[first..first + count]);
        bone += count;
    }
}

// Shared sphere/frustum arithmetic is also used by InstancePass.draw. This
// selection only packs the visible prefix; it does not own camera policy.
fn select_rows(
    records: &[[f64; I::REC_STRIDE]],
    frustum: &bfe_render::math::Frustum,
    next: &mut Vec<usize>,
) {
    next.clear();
    let mut bones = 0;
    for (i, row) in records.iter().enumerate() {
        let mut matrix = [0.; 16];
        matrix.copy_from_slice(&row[I::REC_WORLD..I::REC_WORLD + 16]);
        let (center, radius) = bfe_render::math::sphere_to_world(
            &matrix,
            &[
                row[I::REC_SPHERE],
                row[I::REC_SPHERE + 1],
                row[I::REC_SPHERE + 2],
            ],
            row[I::REC_SPHERE + 3],
        );
        if !frustum.sphere(&center, radius) {
            continue;
        }
        let count = row[I::REC_BONES] as usize * 16;
        if next.len() == I::MAX_RECORDS || bones + count > I::BONES_LEN {
            continue;
        }
        bones += count;
        next.push(i);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_prefix_survives_a_view_packet_restoring_the_shared_bank() {
        let mut row = [0.; I::REC_STRIDE];
        row[I::REC_KIND] = 1.;
        row[I::REC_BONES] = 2.;
        row[I::REC_BONE_AT] = 1.;
        row[I::REC_WORLD..I::REC_WORLD + 16].copy_from_slice(&Matrix4::default().elements);
        let source_bones = vec![0.75; 48];
        let mut input = vec![0.; I::IN_RECORDS + I::REC_STRIDE];
        let mut bones = vec![0.; 32];
        pack_rows(&[row], &source_bones, &[0], &mut input, &mut bones);
        let expected = input.clone();
        // The view pass has a different skeleton at the same bank offsets.
        input[I::IN_RECORDS + I::REC_BONES] = I::MAX_BONES as f64;
        bones.fill(-1.);
        pack_rows(&[row], &source_bones, &[0], &mut input, &mut bones);
        assert_eq!(input, expected);
        assert_eq!(input[I::IN_RECORDS + I::REC_BONE_AT], 0.);
        assert_eq!(bones, [0.75; 32]);
    }
    #[test]
    fn camera_can_reach_late_props_and_bounded_bone_storage() {
        let mut row = [0.; I::REC_STRIDE];
        row[I::REC_WORLD..I::REC_WORLD + 16].copy_from_slice(&Matrix4::default().elements);
        row[I::REC_SPHERE + 3] = 0.1;
        row[I::REC_BONES] = 4.;
        let mut records = vec![row; I::MAX_RECORDS * 3 + 1];
        for row in records.iter_mut().take(I::MAX_RECORDS * 2) {
            row[I::REC_WORLD + 12] = 10.;
        }
        let frustum = bfe_render::math::Frustum::from_matrix(&Matrix4::default().elements);
        let mut selected = Vec::with_capacity(I::MAX_RECORDS);
        let capacity = selected.capacity();
        select_rows(&records, &frustum, &mut selected);
        assert_eq!(selected.len(), I::MAX_RECORDS);
        assert_eq!(selected[0], I::MAX_RECORDS * 2);
        assert_eq!(selected.capacity(), capacity);
        records[I::MAX_RECORDS * 2][I::REC_BONES] = I::MAX_BONES as f64;
        select_rows(&records, &frustum, &mut selected);
        assert_eq!(selected, [I::MAX_RECORDS * 2]);
        assert_eq!(selected.capacity(), capacity);
    }
}

// Static ocean admission leaves shoreline quality at zero. Its shared surface
// owner computes camera-grid placement with zero elapsed time and no contacts.
struct NoShore;
impl bfe_environment::shore::Queries for NoShore {
    fn triangles(&mut self, _: f64, _: f64, _: f64, _: &mut [f64]) -> usize {
        0
    }
    fn exposed(&mut self, _: f64, _: f64, _: f64) -> bool {
        false
    }
}
