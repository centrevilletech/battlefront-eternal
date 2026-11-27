//! Admission adapters for the map inspector. These build resource recipes
//! once; every frame copies into the existing renderer's bounded banks.
use crate::render_bridge::Bridge;
use bfe_content_native::{
    geometry::{Attribute, Geometry, Scalar},
    loader::Blend,
    BattleContent,
};
use bfe_render::{far as F, instances as I, programs, shader};
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
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkyMode {
    Opaque,
    Blended,
    AlphaTest,
    Stars,
}
// These are cold bind-pose templates, independent of the visible command
// prefix. Selection owns two reserved index lists, so changing camera views
// neither allocates nor destroys an object that may be visible later.
pub struct Admission {
    pub records: Vec<[f64; I::REC_STRIDE]>,
    pub record_names: Vec<Option<String>>,
    pub bones: Vec<f32>,
    pub skies: Vec<Sky>,
    pub props: usize,
    pub foliage: usize,
    pub deferred: usize,
    pub animations: crate::world_anim::WorldAnimations,
    pub portals: crate::portals::PortalCuller,
    pub shadow_distance: f64,
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
pub(crate) fn actor_key(material: &bfe_content_native::loader::Material, cascades: usize, skinned: bool) -> Result<String, String> {
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
    let wanted = if skinned {
        json!({"defines":defines,"skin":true,"cascades":cascades,"mode":"skinned","lamps":false})
    } else {
        json!({"defines":defines,"skin":false,"cascades":cascades,"mode":"rigid","lamps":false})
    };
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
pub(crate) fn actor_key_extended(
    material: &bfe_content_native::loader::Material,
    cascades: usize,
    skinned: bool,
    has_bump: bool,
    baked: bool,
) -> Result<String, String> {
    // Placed and pack models carry their resolved surface. A character part
    // keeps the glow, scroll and specular fields of its material record.
    let (glow, scroll, specular) = match material.extended.as_ref() {
        Some(surface) => (
            surface.glow,
            surface.scroll.is_some(),
            surface.specular.is_some_and(|s| s[1..].iter().any(|v| *v > 0.)),
        ),
        None => {
            let record = &material.record;
            let spec = record["specular"].as_array().or_else(|| record["material"]["specular"].as_array());
            (
                record["glow"].as_bool().unwrap_or(false) || record["material"]["glow"].as_bool().unwrap_or(false),
                record["scroll"].as_array().or_else(|| record["material"]["scroll"].as_array()).is_some(),
                spec.is_some_and(|a| a.len() >= 4 && a[1..4].iter().any(|v| v.as_f64().unwrap_or(0.0) > 0.0)),
            )
        }
    };
    let lit = !glow && material.flags & 1 != 0;
    let has_spec = lit && material.flags & 0x20 != 0 && specular;
    let mut defines: Vec<(String, String)> = vec![
        ("EXTENDED_LIGHT".into(), String::new()),
        ("ZERO_LIGHTS".into(), "16".into()),
    ];
    if baked {
        defines.push(("BAKED".into(), String::new()));
        if !lit { defines.push(("NO_POINT".into(), String::new())); }
        if glow { defines.push(("GLOW".into(), String::new())); }
    } else if glow {
        defines.push(("GLOW".into(), String::new()));
        defines.push(("UNLIT".into(), String::new()));
    } else if lit {
        defines.push(("DYNAMIC".into(), String::new()));
    } else {
        defines.push(("UNLIT".into(), String::new()));
    }
    if material.cutout {
        defines.push(("ALPHA_TEST".into(), String::new()));
    }
    if material.blend != Blend::Opaque {
        defines.push(("BLENDED".into(), String::new()));
    }
    if scroll {
        defines.push(("SCROLL".into(), String::new()));
    }
    if has_spec {
        defines.push(("SPECULAR".into(), String::new()));
    }
    if lit && has_bump {
        defines.push(("BUMP".into(), String::new()));
    }
    let cascades = if baked { 0 } else { cascades };
    defines.sort();
    let mode = if skinned { "skinned" } else { "rigid" };
    static SELECTORS: std::sync::OnceLock<Vec<bfe_render::actor_resources::Selector>> =
        std::sync::OnceLock::new();
    let selectors = SELECTORS.get_or_init(|| {
        serde_json::from_str(include_str!("../../bfe-render/src/actor-selectors.json"))
            .expect("authored actor selectors")
    });
    selectors
        .iter()
        .find(|s| {
            s.defines == defines
                && s.skin == skinned
                && s.cascades == cascades as u32
                && s.mode == mode
                && !s.lamps
        })
        .map(|s| s.key.clone())
        .ok_or_else(|| format!("unowned extended prop material {defines:?} skin={skinned} cascades={cascades}"))
}
// What a part uploads, hashed: two parts with one key share geometry only when their uploads are the same (a
// baked part's posed positions and normals; a skinned part's joints and weights as well).
fn contents(part: &bfe_content_native::placed::Part, positions: &[f32], normals: &[f32], bake: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let skin: &[f32] = if bake { &[] } else { &part.weights };
    for stream in [positions, normals, &part.uv, skin] {
        stream.len().hash(&mut h);
        for v in stream.iter() { v.to_bits().hash(&mut h); }
    }
    part.index.hash(&mut h);
    if !bake { part.joints.hash(&mut h); }
    h.finish()
}
// A placed prop's pose never changes after admission, so its parts are skinned once here, as the four-bone
// shader would skin them every vertex of every frame (bind matrix identity: transformed = bindInverse *
// sum(weight * bone) * position, the normal through the same matrix), and drawn as rigid meshes: no bone
// rows, no skin attributes, the rigid program. Returns the baked positions and normals.
fn bake_pose(part: &bfe_content_native::placed::Part) -> (Vec<f32>, Vec<f32>) {
    let bone = |i: usize| -> [f64; 16] { std::array::from_fn(|k| part.bones.get(i * 16 + k).copied().unwrap_or(0.) as f64) };
    let bind_inverse: [f64; 16] = std::array::from_fn(|k| part.bind_inverse[k] as f32 as f64);
    let mut positions = Vec::with_capacity(part.positions.len());
    let mut normals = Vec::with_capacity(part.normals.len());
    for (v, p) in part.positions.chunks_exact(3).enumerate() {
        let mut skin = [0.; 16];
        for j in 0..4 {
            let w = part.weights.get(v * 4 + j).copied().unwrap_or(0.) as f64;
            if w != 0. {
                let b = bone(part.joints.get(v * 4 + j).copied().unwrap_or(0) as usize);
                for k in 0..16 {
                    skin[k] += w * b[k];
                }
            }
        }
        let m = mul(&bind_inverse, &skin);
        let at = |x: f64, y: f64, z: f64, w: f64, r: usize| m[r] * x + m[4 + r] * y + m[8 + r] * z + m[12 + r] * w;
        let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
        positions.extend((0..3).map(|r| at(x, y, z, 1., r) as f32));
        let n = part.normals.get(v * 3..v * 3 + 3).map_or([0., 1., 0.], |n| [n[0] as f64, n[1] as f64, n[2] as f64]);
        normals.extend((0..3).map(|r| at(n[0], n[1], n[2], 0., r) as f32));
    }
    (positions, normals)
}
// Column-major 4x4 product a * b.
fn mul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    std::array::from_fn(|i| {
        let (c, r) = (i / 4, i % 4);
        (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum()
    })
}
pub fn props(
    bridge: &mut Bridge,
    content: &BattleContent,
    textures: &[u32],
    placed: bfe_content_native::placed::Placements,
    cascades: usize,
    bake: bool,
) -> Result<Admission, String> {
    let mut a = Admission {
        records: vec![],
        record_names: vec![],
        bones: vec![],
        skies: vec![],
        props: 0,
        foliage: 0,
        deferred: placed.deferred,
        animations: crate::world_anim::WorldAnimations::default(),
        portals: crate::portals::PortalCuller::default(),
        shadow_distance: 70.,
        eye: frame("cameraPosition").unwrap().0,
        selected: Vec::with_capacity(I::MAX_RECORDS),
    };
    // Copies of one model share GPU geometry, across calls too (every building, door and trap is admitted on its
    // own). Their skeletons and placements remain independent, so moving or replacing a later host-owned look
    // cannot mutate another instance through a shared scene object.
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
        // Only the map's own props are baked: hosts that admit their models here (vehicles, turrets, animated
        // props) pose the bones afterwards, every frame.
        let key = actor_key(material, cascades, !bake)?;
        let program = bridge.add_owned_program(&key)?;
        let (positions, normals) = if bake { bake_pose(&part) } else { (part.positions.clone(), part.normals.clone()) };
        // The batch builder's representative source part fixes its material.
        // Texture names alone are not enough to share batches: flags, scroll
        // values and bind-frame transforms can differ on a single model.
        // Copies share one mesh only when what they upload is the same (a baked copy's pose too).
        let resource = (part.geometry_key.clone(), key.clone(), contents(&part, &positions, &normals, bake));
        let g = if let Some(g) = bridge.prop_geometries.get(&resource) {
            *g
        } else {
            let mut geometry = geometry(&positions, &normals, &part.uv, &part.index);
            if !bake {
                for (name, size, scalar, bytes) in [
                    ("skinIndex", 4, Scalar::U16, part.joints.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()),
                    ("skinWeight", 4, Scalar::Float, part.weights.iter().flat_map(|v| v.to_le_bytes()).collect()),
                ] {
                    let buffer = geometry.buffers.len();
                    geometry.buffers.push(bytes);
                    geometry.attributes.push(Attribute { name: name.into(), buffer, size, scalar, normalized: false, stride: 0, offset: 0 });
                }
            }
            let g = geometry.register(&mut bridge.r, &key)?;
            bridge.prop_geometries.insert(resource, g);
            g
        };
        // InstancePass reads the existing generated record layout. Matrices
        // and bones use attached-mode skinning exactly as the client rigs do,
        // including its recomposed bind inverses and final Float32 bone rows.
        let mut row = [0.; I::REC_STRIDE];
        row[I::REC_BIND..I::REC_BIND + 16].copy_from_slice(&Matrix4::default().elements);
        if bake {
            row[I::REC_KIND] = 2.;
            row[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE + 16].copy_from_slice(&Matrix4::default().elements);
        } else {
            row[I::REC_KIND] = 1.;
            row[I::REC_BONES] = (part.bones.len() / 16) as f64;
            row[I::REC_BONE_AT] = (a.bones.len() / 16) as f64;
            a.bones.extend_from_slice(&part.bones);
            row[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE + 16].copy_from_slice(&part.bind_inverse);
        }
        row[I::REC_PROGRAM] = program as f64;
        row[I::REC_GEOMETRY] = g as f64;
        row[I::REC_TEXTURES] = material.texture.map_or(-1., |t| textures[t] as f64);
        row[I::REC_WORLD..I::REC_WORLD + 16].copy_from_slice(&part.matrix);

        // The object-space box feeds the rigid light-mask path; the sphere
        // feeds visibility for both draw kinds. Keep source bounds rather
        // than expanding them from an already placed world-space mesh.
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in positions.chunks_exact(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k] as f64);
                hi[k] = hi[k].max(p[k] as f64);
            }
        }
        row[I::REC_BOX..I::REC_BOX + 3].copy_from_slice(&lo);
        row[I::REC_BOX + 3..I::REC_BOX + 6].copy_from_slice(&hi);
        if bake {
            // The sphere bounds the baked mesh (the visibility test of both draw kinds).
            let center: [f64; 3] = std::array::from_fn(|k| if lo[k] <= hi[k] { (lo[k] + hi[k]) / 2. } else { 0. });
            let radius = positions.chunks_exact(3).map(|p| (0..3).map(|k| (p[k] as f64 - center[k]).powi(2)).sum::<f64>().sqrt()).fold(0., f64::max);
            row[I::REC_SPHERE..I::REC_SPHERE + 3].copy_from_slice(&center);
            row[I::REC_SPHERE + 3] = radius;
        } else {
            row[I::REC_SPHERE..I::REC_SPHERE + 3].copy_from_slice(&part.center);
            row[I::REC_SPHERE + 3] = part.radius;
        }
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
            row[I::REC_KIND] = if bake { I::KIND_BLENDED_RIGID } else { I::KIND_BLENDED };
            row[I::REC_BLEND] = id as f64;
            row[I::REC_BLEND + 3] = a.records.len() as f64;
        }
        a.records.push(row);
        a.props += 1;
    }
    Ok(a)
}

/// Every part of a 2005 moving prop, building or turret takes the 2005 drawing
/// with its own material (glow, bump, specular, the 2005 lights), whether or not
/// it carries baked vertex light; a 2004 one keeps the released drawing.
fn swbf2_moving_parts(family: bfe_content_native::GameFamily) -> bool {
    family == bfe_content_native::GameFamily::Swbf2
}

pub(crate) fn moving_props(
    bridge: &mut Bridge, content: &BattleContent, textures: &[u32],
    placed: bfe_content_native::placed::Placements, cascades: usize,
) -> Result<Admission, String> {
    if swbf2_moving_parts(content.family) {
        props_swbf2(bridge, content, textures, placed, cascades, false, &Default::default())
    } else {
        props(bridge, content, textures, placed, cascades, false)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn props_swbf2(
    bridge: &mut Bridge,
    content: &BattleContent,
    textures: &[u32],
    placed: bfe_content_native::placed::Placements,
    cascades: usize,
    bake: bool,
    bump_lookup: &std::collections::HashMap<String, u32>,
) -> Result<Admission, String> {
    let mut a = Admission {
        records: vec![],
        record_names: vec![],
        bones: vec![],
        skies: vec![],
        props: 0,
        foliage: 0,
        deferred: placed.deferred,
        animations: crate::world_anim::WorldAnimations::default(),
        portals: crate::portals::PortalCuller::default(),
        shadow_distance: 70.,
        eye: frame("cameraPosition").unwrap().0,
        selected: Vec::with_capacity(I::MAX_RECORDS),
    };
    let prop_instances = content
        .content
        .battle
        .props
        .as_ref()
        .and_then(|p| p.get("instances"))
        .and_then(bfe_content::Value::as_array);
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
        let surface = material.extended.as_ref().ok_or("missing extended model surface")?;
        let bump_tex = surface.bump.as_ref().and_then(|name| bump_lookup.get(&name.to_ascii_lowercase()).copied());
        let colours = part.colours.as_deref();
        let baked = colours.is_some_and(|c| !c.light.is_empty());
        let key = actor_key_extended(material, cascades, !bake, bump_tex.is_some(), baked)?;
        let program = bridge.add_owned_program(&key)?;
        let (positions, normals) = if bake { bake_pose(&part) } else { (part.positions.clone(), part.normals.clone()) };
        let mut fingerprint = contents(&part, &positions, &normals, bake);
        if let Some(colours) = colours {
            use std::hash::{Hash, Hasher};
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            fingerprint.hash(&mut hash);
            colours.hash(&mut hash);
            fingerprint = hash.finish();
        }
        let resource = (part.geometry_key.clone(), key.clone(), fingerprint);
        let g = if let Some(g) = bridge.prop_geometries.get(&resource) {
            *g
        } else {
            let mut geometry = geometry(&positions, &normals, &part.uv, &part.index);
            if !bake {
                for (name, size, scalar, bytes) in [
                    ("skinIndex", 4, Scalar::U16, part.joints.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()),
                    ("skinWeight", 4, Scalar::Float, part.weights.iter().flat_map(|v| v.to_le_bytes()).collect()),
                ] {
                    let buffer = geometry.buffers.len();
                    geometry.buffers.push(bytes);
                    geometry.attributes.push(Attribute { name: name.into(), buffer, size, scalar, normalized: false, stride: 0, offset: 0 });
                }
            }
            let mut attribute = |name: &str, scalar, normalized, bytes| {
                let buffer = geometry.buffers.len();
                geometry.buffers.push(bytes);
                geometry.attributes.push(Attribute { name: name.into(), buffer, size: 4,
                    scalar, normalized, stride: 0, offset: 0 });
            };
            if let Some(tint) = colours.map(|c| &c.tint).filter(|s| !s.is_empty()) {
                attribute("vertexTint", Scalar::Float, false, tint.clone());
            } else {
                // Keep the tint multiplier exactly one, including after another VAO
                // enabled this location. Disabled generic attribute values are global.
                attribute("vertexTint", Scalar::U8, true, vec![255; positions.len() / 3 * 4]);
            }
            if let Some(light) = colours.map(|c| &c.light).filter(|s| !s.is_empty()) {
                attribute("bakedLight", Scalar::Float, false, light.clone());
            }
            let g = geometry.register(&mut bridge.r, &key)?;
            bridge.prop_geometries.insert(resource, g);
            g
        };
        let mut row = [0.; I::REC_STRIDE];
        row[I::REC_BIND..I::REC_BIND + 16].copy_from_slice(&Matrix4::default().elements);
        if bake {
            row[I::REC_KIND] = 2.;
            row[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE + 16].copy_from_slice(&Matrix4::default().elements);
        } else {
            row[I::REC_KIND] = 1.;
            row[I::REC_BONES] = (part.bones.len() / 16) as f64;
            row[I::REC_BONE_AT] = (a.bones.len() / 16) as f64;
            a.bones.extend_from_slice(&part.bones);
            row[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE + 16].copy_from_slice(&part.bind_inverse);
        }
        row[I::REC_PROGRAM] = program as f64;
        row[I::REC_GEOMETRY] = g as f64;
        row[I::REC_TEXTURES] = material.texture.map_or(-1., |t| textures[t] as f64);
        row[I::REC_WORLD..I::REC_WORLD + 16].copy_from_slice(&part.matrix);

        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in positions.chunks_exact(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k] as f64);
                hi[k] = hi[k].max(p[k] as f64);
            }
        }
        row[I::REC_BOX..I::REC_BOX + 3].copy_from_slice(&lo);
        row[I::REC_BOX + 3..I::REC_BOX + 6].copy_from_slice(&hi);
        if bake {
            let center: [f64; 3] = std::array::from_fn(|k| if lo[k] <= hi[k] { (lo[k] + hi[k]) / 2. } else { 0. });
            let radius = positions.chunks_exact(3).map(|p| (0..3).map(|k| (p[k] as f64 - center[k]).powi(2)).sum::<f64>().sqrt()).fold(0., f64::max);
            row[I::REC_SPHERE..I::REC_SPHERE + 3].copy_from_slice(&center);
            row[I::REC_SPHERE + 3] = radius;
        } else {
            row[I::REC_SPHERE..I::REC_SPHERE + 3].copy_from_slice(&part.center);
            row[I::REC_SPHERE + 3] = part.radius;
        }
        row[I::REC_LOOK] = 1.;
        row[I::REC_LOOK + 3] = 1.;
        row[I::REC_FLAGS] = (if material.double_sided { 2 } else { 0 }
            | if material.cutout { 4 } else { 0 }
            | 8) as f64;
        if let Some(scroll) = surface.scroll {
            for (i, speed) in scroll.into_iter().enumerate() {
                row[I::REC_EXTRA + i] = if !speed.is_finite() || speed.abs() > 1000. { 0. } else { speed };
            }
        }
        let lit = !surface.glow && material.flags & 1 != 0;
        if lit && !baked && cascades > 0 {
            row[I::REC_RECEIVE_SHADOW] = 1.;
        }
        if lit {
            if let Some(tex) = bump_tex {
                row[I::REC_BUMP] = (tex + 1) as f64;
            }
            if material.flags & 0x20 != 0 {
                if let Some(spec) = surface.specular {
                    row[I::REC_SPECULAR] = spec[0].clamp(10., 100.);
                    row[I::REC_SPECULAR+1..I::REC_SPECULAR+4].copy_from_slice(&spec[1..]);
                }
            }
        }
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
            let passes: &[u32] = if material.double_sided {
                &[program, 1, program, 0]
            } else {
                &[program, 0]
            };
            let id = bridge.inst_add_blend(&blend, 1, glow::LEQUAL, passes);
            row[I::REC_KIND] = if bake { I::KIND_BLENDED_RIGID } else { I::KIND_BLENDED };
            row[I::REC_BLEND] = id as f64;
            row[I::REC_BLEND + 3] = a.records.len() as f64;
        }
        let instance_name = prop_instances
            .and_then(|list| list.get(part.instance))
            .and_then(|ins| ins.get("name"))
            .and_then(bfe_content::Value::as_str)
            .map(str::to_owned);
        a.records.push(row);
        a.record_names.push(instance_name);
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

// 2005 sky layers support alpha-tested domes, scrolling blended domes,
// a procedural starfield backdrop, and space-scale model scaling.
pub(crate) fn sky_swbf2(
    bridge: &mut Bridge,
    mesh: &bfe_content_native::loader::Mesh,
    textures: &[u32],
    slot: usize,
    empty: u32,
    mode: SkyMode,
    scale: f64,
) -> Result<Sky, String> {
    let scroll = mesh.material.record["scroll"]
        .as_array()
        .or_else(|| mesh.material.record["material"]["scroll"].as_array())
        .and_then(|a| {
            let s = [a.first()?.as_f64()?, a.get(1)?.as_f64()?];
            (s[0].is_finite() && s[1].is_finite() && (s[0] != 0. || s[1] != 0.)).then_some(s)
        });
    let (key, flags) = match mode {
        SkyMode::Stars => ("effects/sky-stars", F::F_PLACED | F::F_COLOR_WRITE | 2),
        SkyMode::AlphaTest => ("effects/sky-alpha-test-s2", F::F_PLACED | F::F_COLOR_WRITE | 2),
        SkyMode::Blended => (
            if scroll.is_some() {
                "effects/sky-blended-scroll-s2"
            } else {
                "effects/sky-zero-lights-16-sky-no-point-blended-s2-plain-chunks"
            },
            F::F_PLACED | F::F_COLOR_WRITE | F::F_BLEND | 2,
        ),
        SkyMode::Opaque => (
            "effects/sky-zero-lights-16-sky-no-point-s0-plain-chunks",
            F::F_PLACED | F::F_COLOR_WRITE,
        ),
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
        } else if *name == "scroll" {
            let s = scroll.unwrap_or([0.; 2]);
            bridge.far_source(id, i as u32, F::SRC_FLOATS, &[s[0] as f32, s[1] as f32], 0)
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
    let mut mat = Matrix4::default().elements;
    mat[0] = scale;
    mat[5] = scale;
    mat[10] = scale;
    bridge.r.far.input[slot + 1..slot + 17].copy_from_slice(&mat);
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
// Where each sky layer stands for a view: on the eye, or (the sky-follow
// rule) partly following its height. A water mirror stands every layer at
// its reflected eye instead (`follow` false), the way the client's mirror did.
pub fn place_skies(a: &Admission, bridge: &mut Bridge, view: &[f64], follow: bool) {
    let eye = a.eye;
    for s in &a.skies {
        let y = s
            .follow
            .filter(|_| follow)
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
}
impl Admission {
    pub fn update_animations(&mut self, time: f64) {
        self.animations.apply(time, &mut self.records);
    }

    pub fn update_animations_sim(&mut self, tick: u32, alpha: f64) {
        self.animations.apply_sim(tick, alpha, &mut self.records);
    }

    // The earlier build InstancePass culls before filling its bounded command bank.
    // Retain every cold prop; a camera moving elsewhere must not discover that
    // admission permanently discarded the last objects after a capacity limit.
    // Online actors share the same near bank. Reserving their prefix budget
    // keeps a prop-dense map from starving every remote body of a draw record.
    pub fn select_limit(&mut self, bridge: &mut Bridge, maximum: usize) -> usize {
        self.select_range(bridge, maximum, 0., f64::INFINITY, true, self.portals.is_active())
    }

    /// Select visible props within an optional distance band from the camera eye.
    /// `min_dist_max`: include records whose farthest point (`dist + radius`) is `>= min_dist_max`.
    /// `max_dist_min`: include records whose nearest point (`dist - radius`) is `<= max_dist_min`.
    /// Blended records split cleanly at the band midpoint to avoid double-blending across overlap.
    pub fn select_range(
        &mut self,
        bridge: &mut Bridge,
        maximum: usize,
        min_dist_max: f64,
        max_dist_min: f64,
        receive_shadows: bool,
        apply_portals: bool,
    ) -> usize {
        let camera = bridge.r.core.camera();
        let eye = [
            bridge.r.core.input[self.eye],
            bridge.r.core.input[self.eye + 1],
            bridge.r.core.input[self.eye + 2],
        ];
        let (dropped_records, dropped_bones) = if min_dist_max <= 0. && !max_dist_min.is_finite() && !apply_portals {
            select_rows(&self.records, &camera.frustum, I::MAX_RECORDS, &mut self.selected)
        } else {
            let portals = apply_portals.then_some(&self.portals);
            select_rows_filtered(
                &self.records,
                &camera.frustum,
                eye,
                min_dist_max,
                max_dist_min,
                portals,
                I::MAX_RECORDS,
                &mut self.selected,
            )
        };
        let keep = maximum.min(I::MAX_RECORDS);
        let truncated = self.selected.len().saturating_sub(keep);
        self.selected.truncate(keep);
        if dropped_records + truncated > 0 {
            bridge.r.instances.overflow.note_record_drop(dropped_records + truncated);
        }
        if dropped_bones > 0 {
            bridge.r.instances.overflow.note_bone_drop(dropped_bones);
        }
        // Frame packets restore shared instance and bone banks, including
        // the first-person pass at index zero. Refill the static prefix even
        // when culling selects the same props as the preceding frame.
        pack_rows(
            &self.records,
            &self.bones,
            &self.selected,
            &mut bridge.r.core.input,
            &mut bridge.r.core.bones,
        );
        if !receive_shadows {
            for to in 0..self.selected.len() {
                bridge.r.core.input[I::IN_RECORDS + to * I::REC_STRIDE + I::REC_RECEIVE_SHADOW] = 0.;
            }
        } else {
            crate::shadows::limit_distance(
                &mut bridge.r.core.input,
                0,
                self.selected.len(),
                self.shadow_distance,
            );
        }
        self.selected.len()
    }
}

fn select_rows_filtered(
    records: &[[f64; I::REC_STRIDE]],
    frustum: &bfe_render::math::Frustum,
    eye: [f64; 3],
    min_dist_max: f64,
    max_dist_min: f64,
    portals: Option<&crate::portals::PortalCuller>,
    max_records: usize,
    next: &mut Vec<usize>,
) -> (usize, usize) {
    const BLEND_SPLIT: f64 = 530.;
    let band_active = min_dist_max > 0. || max_dist_min.is_finite();
    next.clear();
    let mut bones = 0;
    let mut dropped_records = 0;
    let mut dropped_bones = 0;
    for (i, row) in records.iter().enumerate() {
        if let Some(p) = portals {
            if !p.record_visible(i) {
                continue;
            }
        }
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
        if band_active {
            let dx = center[0] - eye[0];
            let dy = center[1] - eye[1];
            let dz = center[2] - eye[2];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            let blended = row[I::REC_KIND] == I::KIND_BLENDED || row[I::REC_KIND] == I::KIND_BLENDED_RIGID;
            if blended {
                if min_dist_max > 0. && dist < BLEND_SPLIT {
                    continue;
                }
                if max_dist_min.is_finite() && dist >= BLEND_SPLIT {
                    continue;
                }
            } else {
                if dist + radius < min_dist_max {
                    continue;
                }
                if dist - radius > max_dist_min {
                    continue;
                }
            }
        }
        if !frustum.sphere(&center, radius) {
            continue;
        }
        let count = row[I::REC_BONES] as usize * 16;
        if next.len() == max_records {
            dropped_records += 1;
            continue;
        }
        if bones + count > I::BONES_LEN {
            dropped_bones += 1;
            continue;
        }
        bones += count;
        next.push(i);
    }
    (dropped_records, dropped_bones)
}

fn pack_rows(records: &[[f64; I::REC_STRIDE]], bones: &[f32], selected: &[usize],
    input: &mut [f64], output_bones: &mut [f32]) {
    let mut bone = 0;
    for (to, &from) in selected.iter().enumerate() {
        let row = &records[from];
        let at = I::IN_RECORDS + to * I::REC_STRIDE;
        input[at..at + I::REC_STRIDE].copy_from_slice(row);
        input[at + I::REC_BONE_AT] = (bone / 16) as f64;
        input[at+I::REC_MOTION_ID]=4294967296.+from as f64;
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
    max_records: usize,
    next: &mut Vec<usize>,
) -> (usize, usize) {
    next.clear();
    let mut bones = 0;
    let mut dropped_records = 0;
    let mut dropped_bones = 0;
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
        if next.len() == max_records {
            dropped_records += 1;
            continue;
        }
        if bones + count > I::BONES_LEN {
            dropped_bones += 1;
            continue;
        }
        bones += count;
        next.push(i);
    }
    (dropped_records, dropped_bones)
}
#[cfg(test)]
mod tests {
    #[test]
    fn a_2005_moving_part_without_baked_light_takes_the_2005_drawing() {
        use bfe_content_native::GameFamily;
        // A turret hands its parts over one at a time: an unbaked glowing part
        // alone must still reach the 2005 drawing; a 2004 part never does.
        assert!(super::swbf2_moving_parts(GameFamily::Swbf2));
        assert!(!super::swbf2_moving_parts(GameFamily::Swbf1));
    }
    use super::*;
    #[test]
    fn water_depth_uses_the_drawn_camera_clipping_planes() {
        for (near, far) in [(0.3, 20000.), (0.3, 200.), (0.5, 200.), (0.5, 220.)] {
            let mut projection = Matrix4::default();
            projection.make_perspective(-1., 1., 1., -1., near, far, three::CoordinateSystem::WebGL);
            let p = projection.elements;
            let [n, f] = crate::water_host::camera_range(&p);
            for depth in [near, 1., 10., far] {
                let opaque = ((-p[10] * depth + p[14]) / depth + 1.) * 0.5;
                let water = (f - n * f / depth) / (f - n);
                assert!((opaque - water).abs() < 1e-12);
            }
        }
    }
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
        let (dropped_rec, dropped_bones) = select_rows(&records, &frustum, I::MAX_RECORDS, &mut selected);
        assert_eq!(selected.len(), I::MAX_RECORDS);
        assert_eq!(dropped_rec, 1);
        assert_eq!(dropped_bones, 0);
        assert_eq!(selected[0], I::MAX_RECORDS * 2);
        assert_eq!(selected.capacity(), capacity);
        records[I::MAX_RECORDS * 2][I::REC_BONES] = I::MAX_BONES as f64;
        let (dropped_rec, dropped_bones) = select_rows(&records, &frustum, I::MAX_RECORDS, &mut selected);
        assert_eq!(selected, [I::MAX_RECORDS * 2]);
        assert_eq!(dropped_rec, 0);
        assert_eq!(dropped_bones, I::MAX_RECORDS);
        assert_eq!(selected.capacity(), capacity);
    }
}

// Static ocean admission leaves shoreline quality at zero. Its shared surface
// owner computes camera-grid placement with zero elapsed time and no contacts.

#[cfg(test)]
mod bake_tests {
    use super::*;
    #[test]
    fn a_baked_prop_matches_what_the_skinning_shader_draws() {
        // Two bones: a translation by (1, 2, 3) and a quarter turn about y; the bind inverse moves by -1 in x.
        let mut translate = Matrix4::default().elements.map(|v| v as f32);
        translate[12] = 1.;
        translate[13] = 2.;
        translate[14] = 3.;
        let turn: [f32; 16] = [0., 0., -1., 0., 0., 1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 1.];
        let mut bind_inverse = Matrix4::default().elements;
        bind_inverse[12] = -1.;
        let part = bfe_content_native::placed::Part {
            instance: 0,
            attachment: None,
            model: String::new(),
            source: 0,
            colours: None,
            geometry_key: String::new(),
            matrix: Matrix4::default().elements,
            positions: vec![1., 0., 0.],
            normals: vec![1., 0., 0.],
            uv: vec![0., 0.],
            index: vec![0, 0, 0],
            joints: vec![0, 1, 0, 0],
            weights: vec![0.5, 0.5, 0., 0.],
            bones: translate.iter().chain(turn.iter()).copied().collect(),
            bind_inverse,
            center: [0.; 3],
            radius: 1.,
        };
        let (p, n) = bake_pose(&part);
        // 0.5 * (2, 2, 3) + 0.5 * (0, 0, -1) = (1, 1, 1), then x - 1.
        assert_eq!(p, vec![0., 1., 1.]);
        // Normal: 0.5 * (1, 0, 0) + 0.5 * (0, 0, -1); the bind inverse's translation does not move it.
        assert_eq!(n, vec![0.5, 0., -0.5]);
    }
}
