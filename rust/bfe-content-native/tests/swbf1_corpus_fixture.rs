//! Synthetic 2004 odd-input corpus regression test against a golden fixture
//! captured directly from the released 2004 build.
//!
//! Uses 100% invented map, side, unit, actor, geometry, and animation data
//! (zero game files) and checks every 2004 admission stage:
//! `fingerprint::hash`, `Loader::load` (world, models, actors, units, BVH,
//! collision, prop bodies, clips), `Defs::build`, `placed::{admit, admit_live,
//! admit_turrets, admit_look, admit_trap_pad}`, and `Context::world_admit`
//! stepped for 60 ticks, plus equivalence with `hash_for_family(Swbf1)` and
//! `load_for_family(Swbf1)`.

use bfe_content::files::DirFiles;
use bfe_content_native::{
    fingerprint,
    loader::{authored_json, Loader, Mesh},
    placed,
    setup::{
        admission::{CLOCK_START_MS, CLOCK_UNIX_MS},
        TickRows,
    },
    // SWBF1_FAMILY_IMPORT_START
    GameFamily,
    // SWBF1_FAMILY_IMPORT_END
};
use bfe_sim::abi::context::Context;
use bfe_sim::abi::context_step::Publication;
use bfe_sim::gen::{WirePublicationLink, WireWorldFacts};
use bfe_sim::world::admit::{Bots, Options};
use bfe_wide::{Fnv, Wide};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bfe-swbf1-corpus-{tag}-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn write(&self, rel: &str, bytes: &[u8]) {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }
    fn remove(&self, rel: &str) {
        let _ = std::fs::remove_file(self.0.join(rel));
    }
    fn rewrite_json(&self, rel: &str, f: impl FnOnce(&mut Value)) {
        let path = self.0.join(rel);
        let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        f(&mut v);
        std::fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct H(Fnv);
impl H {
    fn new() -> Self {
        H(Fnv::new())
    }
    fn b(&mut self, bytes: &[u8]) {
        self.0.bytes(bytes);
    }
    fn s(&mut self, s: &str) {
        self.b(s.as_bytes());
        self.b(&[0xff]);
    }
    fn f(&mut self, v: f64) {
        self.b(&v.to_bits().to_le_bytes());
    }
    fn f32s(&mut self, v: &[f32]) {
        for x in v {
            self.b(&x.to_bits().to_le_bytes());
        }
    }
    fn u32s(&mut self, v: &[u32]) {
        for x in v {
            self.b(&x.to_le_bytes());
        }
    }
    /// A count or an index, as the eight little-endian bytes of its word.
    fn u(&mut self, v: usize) {
        self.0.small(u32::try_from(v).unwrap_or(u32::MAX));
    }
    /// A signed number, sign-extended to the eight bytes of its word.
    fn signed(&mut self, v: i32) {
        self.0.word(Wide::from_i32(v));
    }
    fn hex(&self) -> String {
        format!("{:016x}", self.0.finish())
    }
}

fn f32_bytes(xs: &[f32]) -> Vec<u8> {
    xs.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn u32_bytes(xs: &[u32]) -> Vec<u8> {
    xs.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn identity_mat() -> Value {
    json!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1])
}

fn translate_mat(x: f64, y: f64, z: f64) -> Value {
    json!([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1])
}

fn make_rig_model(
    v_offset_bytes: usize,
    i_offset_bytes: usize,
    box_lo: [f64; 3],
    box_hi: [f64; 3],
    bones: Value,
) -> Value {
    json!({
        "parts": [{
            "vertexOffset": v_offset_bytes,
            "vertexCount": 3,
            "indexOffset": i_offset_bytes,
            "indexCount": 3,
            "texture": "",
            "bone": "root_bone"
        }],
        "bones": bones,
        "box": [box_lo, box_hi],
        "sphere": [0.0, 1.0, 0.0, 2.0]
    })
}

/// Populates a 100%-invented synthetic 2004 content tree (`synth1`, sides
/// `alpha` and `beta`) with zero game files.
#[allow(clippy::approx_constant)]
fn build_invented_base_2004(dir: &TempDir) {
    dir.write(
        "common/common.json",
        br#"{"models":{},"textures":{},"materials":{}}"#,
    );
    dir.write("common/effects.json", br#"{"effects":{}}"#);
    dir.write("common/fxmodels.bin", &[]);
    dir.write("common/fxmodels_index.bin", &[]);

    // 4 synthetic clips, 2 frames each, 2 bone tracks (tx,ty,tz, qx,qy,qz,qw).
    let mut anim_floats: Vec<f32> = Vec::new();
    let mut clips_map = Map::new();
    let clip_names = [
        "human_rifle_stand_idle_emote",
        "human_rifle_stand_idle_emote_alert",
        "human_rifle_stand_walkforward",
        "human_rifle_stand_runforward",
    ];
    for (ci, name) in clip_names.iter().enumerate() {
        let offset = anim_floats.len();
        for frame in 0..2 {
            for bone in 0..2 {
                let shift = (ci as f32) * 0.05 + (frame as f32) * 0.1 + (bone as f32) * 0.25;
                anim_floats.extend_from_slice(&[shift, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
            }
        }
        clips_map.insert(
            (*name).to_string(),
            json!({
                "frames": 2,
                "offset": offset,
                "tracks": [0, 1],
                "flags": 0,
                "travel": [0.0, 0.0, if ci >= 2 { 0.4 } else { 0.0 }]
            }),
        );
    }
    dir.write(
        "common/anims.json",
        &serde_json::to_vec(&json!({
            "fps": 30,
            "bones": ["DummyRoot", "torso_bone"],
            "clips": clips_map
        }))
        .unwrap(),
    );
    dir.write("common/anims.bin", &f32_bytes(&anim_floats));

    // World geometry: 2 triangles (6 vertices * 8 floats: pos3, norm3, uv2).
    let world_verts: [f32; 48] = [
        -30.0, 0.0, -30.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        30.0, 0.0, -30.0, 0.0, 1.0, 0.0, 1.0, 0.0,
        30.0, 0.0, 30.0, 0.0, 1.0, 0.0, 1.0, 1.0,
        -30.0, 0.0, -30.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        30.0, 0.0, 30.0, 0.0, 1.0, 0.0, 1.0, 1.0,
        -30.0, 0.0, 30.0, 0.0, 1.0, 0.0, 0.0, 1.0,
    ];
    dir.write("maps/synth1/geom.bin", &f32_bytes(&world_verts));
    dir.write("maps/synth1/index.bin", &u32_bytes(&[0, 1, 2, 0, 1, 2]));

    // World static collision (2 floor triangles) + terrain collision (2 floor triangles).
    let coll_verts: [f32; 12] = [
        -30.0, 0.0, -30.0,
        30.0, 0.0, -30.0,
        30.0, 0.0, 30.0,
        -30.0, 0.0, 30.0,
    ];
    dir.write("maps/synth1/collision.bin", &f32_bytes(&coll_verts));
    dir.write("maps/synth1/collision_index.bin", &u32_bytes(&[0, 1, 2, 0, 2, 3]));
    dir.write("maps/synth1/collision_poly.bin", &u32_bytes(&[0, 1]));
    dir.write("maps/synth1/terrain_collision.bin", &f32_bytes(&coll_verts));
    dir.write("maps/synth1/terrain_collision_index.bin", &u32_bytes(&[0, 1, 2, 0, 2, 3]));

    // Props geometry (3 triangles: crate, post, wreck/unbuilt share buffers), collision, anims.
    let prop_verts: [f32; 48] = [
        -1.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        1.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0, 0.0,
        0.0, 2.0, 1.0, 0.0, 1.0, 0.0, 0.5, 1.0,
        -0.5, 0.0, -0.5, 0.0, 1.0, 0.0, 0.0, 0.0,
        0.5, 0.0, -0.5, 0.0, 1.0, 0.0, 1.0, 0.0,
        0.0, 3.0, 0.5, 0.0, 1.0, 0.0, 0.5, 1.0,
    ];
    dir.write("maps/synth1/props.bin", &f32_bytes(&prop_verts));
    dir.write("maps/synth1/props_index.bin", &u32_bytes(&[0, 1, 2, 3, 4, 5]));
    let pcoll_verts: [f32; 18] = [
        4.0, 0.0, 4.0, 6.0, 0.0, 4.0, 5.0, 2.0, 6.0,
        -6.0, 0.0, 4.0, -4.0, 0.0, 4.0, -5.0, 2.0, 6.0,
    ];
    dir.write("maps/synth1/props_collision.bin", &f32_bytes(&pcoll_verts));
    dir.write("maps/synth1/props_collision_index.bin", &u32_bytes(&[0, 1, 2, 3, 4, 5]));
    dir.write("maps/synth1/props_collision_poly.bin", &u32_bytes(&[0, 1]));
    dir.write("maps/synth1/props_anims.bin", &[]);

    let single_bone = json!([{"name": "root_bone", "parent": -1, "local": identity_mat()}]);
    let turret_bones = json!([
        {"name": "root_bone", "parent": -1, "local": identity_mat()},
        {"name": "aim_bone", "parent": 0, "local": translate_mat(0.0, 1.5, 0.0)}
    ]);
    let props_json = json!({
        "models": {
            "synth_crate": make_rig_model(0, 0, [-1.0, 0.0, -1.0], [1.0, 2.0, 1.0], single_bone.clone()),
            "synth_post": make_rig_model(96, 12, [-0.5, 0.0, -0.5], [0.5, 3.0, 0.5], turret_bones),
            "synth_wreck": make_rig_model(0, 0, [-1.0, 0.0, -1.0], [1.0, 1.0, 1.0], single_bone.clone()),
            "synth_unbuilt": make_rig_model(0, 0, [-1.0, 0.0, -1.0], [1.0, 0.5, 1.0], single_bone)
        },
        "instances": [
            {
                "name": "station_one",
                "base": "powerupstation",
                "class": "synth_station",
                "model": "synth_crate",
                "matrix": translate_mat(5.0, 0.0, 5.0),
                "collision": [0, 1],
                "team": 1,
                "radius": 2.0,
                "label": "SynthStation",
                "settings": {"MaxHealth": 400},
                "destroyed": {"model": "synth_wreck"}
            },
            {
                "name": "post_one",
                "base": "prop",
                "class": "synth_cp",
                "model": "synth_post",
                "matrix": translate_mat(0.0, 0.0, 0.0),
                "team": 1,
                "radius": 3.0,
                "label": "PostAlpha",
                "settings": {}
            },
            {
                "name": "turret_one",
                "base": "prop",
                "class": "synth_turret",
                "model": "synth_post",
                "matrix": translate_mat(-5.0, 0.0, 5.0),
                "collision": [1, 1],
                "team": 1,
                "radius": 2.5,
                "label": "SynthTurret",
                "settings": {"TurretNodeName": "aim_bone"},
                "unbuilt": {"model": "synth_unbuilt"}
            }
        ]
    });
    dir.write("maps/synth1/props.json", &serde_json::to_vec(&props_json).unwrap());

    let map_json = json!({
        "name": "synth1",
        "stride": 8,
        "meshes": [
            {
                "name": "floor_a",
                "vertexOffset": 0,
                "vertexCount": 3,
                "indexOffset": 0,
                "indexCount": 3,
                "stride": 8,
                "attributes": ["position", "normal", "uv"],
                "texture": "",
                "flags": 0,
                "doubleSided": false
            },
            {
                "name": "floor_b",
                "vertexOffset": 96,
                "vertexCount": 3,
                "indexOffset": 12,
                "indexCount": 3,
                "stride": 8,
                "attributes": ["position", "normal", "uv"],
                "texture": "",
                "flags": 0,
                "doubleSided": false
            }
        ],
        "textures": {},
        "spawns": {
            "sp_alpha": [[0.0, 0.5, -8.0, 0.0], [2.0, 0.5, -8.0, 0.0]],
            "sp_beta": [[0.0, 0.5, 8.0, 3.14], [-2.0, 0.5, 8.0, 3.14]]
        },
        "commandPosts": [
            {
                "name": "cp_alpha",
                "team": 1,
                "spawnPath": "sp_alpha",
                "matrix": translate_mat(0.0, 0.0, -8.0),
                "capture": {"center": [0.0, 0.0, -8.0], "radius": 6.0}
            },
            {
                "name": "cp_beta",
                "team": 2,
                "spawnPath": "sp_beta",
                "matrix": translate_mat(0.0, 0.0, 8.0),
                "capture": {"center": [0.0, 0.0, 8.0], "radius": 6.0}
            }
        ],
        "missions": {
            "synth1a": {
                "mission": {
                    "name": "synth1a",
                    "era": "cw",
                    "sides": ["alpha", "beta"],
                    "teams": {
                        "1": {
                            "name": "alpha",
                            "units": 2,
                            "reinforcements": 25,
                            "classes": [["alpha_trooper", 2]]
                        },
                        "2": {
                            "name": "beta",
                            "units": 2,
                            "reinforcements": 25,
                            "classes": [["beta_trooper", 2]]
                        }
                    }
                }
            }
        }
    });
    dir.write("maps/synth1/map.json", &serde_json::to_vec(&map_json).unwrap());

    dir.write("maps/synth1/ai-nav.json", br#"{"hubs":[],"arcs":[]}"#);
    dir.write("maps/synth1/ai-nav.bin", &[]);
    dir.write("maps/synth1/ai-mission.json", br#"{"goals":[]}"#);

    // Two invented sides: alpha and beta.
    let actor_verts: [f32; 27] = [
        -0.3, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        0.3, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0,
        0.0, 1.7, 0.0, 0.0, 0.0, 1.0, 0.5, 1.0, 0.0,
    ];
    for (side, unit_name, actor_name, weap_name) in [
        ("alpha", "alpha_trooper", "alpha_body", "alpha_carbine"),
        ("beta", "beta_trooper", "beta_body", "beta_carbine"),
    ] {
        let units = json!({
            "units": {
                unit_name: {
                    "class": unit_name,
                    "base": "soldier",
                    "model": actor_name,
                    "actor": format!("actors/{actor_name}.json"),
                    "MaxHealth": 200,
                    "MaxSpeed": 6.5,
                    "MaxStrafeSpeed": 4.5,
                    "MaxTurnSpeed": 4.0,
                    "weapons": [{
                        "class": weap_name,
                        "base": "cannon",
                        "ShotDelay": 0.25,
                        "RoundsPerClip": 20,
                        "ReloadTime": 1.5,
                        "ordnance": {
                            "class": format!("{weap_name}_bolt"),
                            "base": "bolt",
                            "Velocity": 80.0,
                            "LifeSpan": 1.5,
                            "Damage": 35.0
                        }
                    }]
                }
            }
        });
        dir.write(&format!("sides/{side}/units.json"), &serde_json::to_vec(&units).unwrap());
        dir.write(
            &format!("sides/{side}/weapons.json"),
            br#"{"models":{},"firePoints":{}}"#,
        );
        dir.write(&format!("sides/{side}/weapons.bin"), &[]);
        dir.write(&format!("sides/{side}/weapons_index.bin"), &[]);
        dir.write(&format!("sides/{side}/ai.json"), br#"{}"#);
        let actor_json = json!({
            "bones": [
                {"name": "DummyRoot", "parent": -1, "local": identity_mat()},
                {"name": "torso_bone", "parent": 0, "local": translate_mat(0.0, 0.9, 0.0)}
            ],
            "parts": [{
                "vertexOffset": 0,
                "vertexCount": 3,
                "indexOffset": 0,
                "indexCount": 3,
                "texture": ""
            }]
        });
        dir.write(
            &format!("sides/{side}/actors/{actor_name}.json"),
            &serde_json::to_vec(&actor_json).unwrap(),
        );
        dir.write(
            &format!("sides/{side}/actors/{actor_name}.bin"),
            &f32_bytes(&actor_verts),
        );
        dir.write(
            &format!("sides/{side}/actors/{actor_name}_index.bin"),
            &u32_bytes(&[0, 1, 2]),
        );
    }
}

fn hash_mesh(h: &mut H, m: &Mesh) {
    h.s(&m.name);
    let g = &m.geometry;
    for buf in &g.buffers {
        h.b(buf);
    }
    h.s(&format!("{:?}", g.attributes));
    h.u32s(&g.indices);
    h.u(g.vertices);
    h.s(&format!("{:?}", g.groups));
    let mat = &m.material;
    h.s(&format!(
        "{:?} {:?} {} {} {} {} {:?} {:?}",
        mat.kind, mat.blend, mat.cutout, mat.double_sided, mat.flags, mat.far, mat.texture, mat.bump
    ));
    h.s(&mat.record.to_string());
}

fn hash_parts(h: &mut H, p: &placed::Placements) {
    h.u(p.deferred);
    for part in &p.parts {
        h.u(part.instance);
        h.s(&format!("{:?}", part.attachment));
        h.s(&part.model);
        h.u(part.source);
        h.s(&part.geometry_key);
        for v in part.matrix {
            h.f(v);
        }
        h.f32s(&part.positions);
        h.f32s(&part.normals);
        h.f32s(&part.uv);
        h.u32s(&part.index);
        for j in &part.joints {
            h.b(&j.to_le_bytes());
        }
        h.f32s(&part.weights);
        h.f32s(&part.bones);
        for v in part.bind_inverse {
            h.f(v);
        }
        for v in part.center {
            h.f(v);
        }
        h.f(part.radius);
    }
}

fn mix(mut hash: Fnv, bytes: &[u8]) -> Fnv {
    hash.bytes(bytes);
    hash
}

fn measure_variant(root: &Path) -> Value {
    let map = "synth1";
    let side = "alpha";
    let query = format!("?map={map}&side={side}");
    let files = DirFiles::new(root, bfe_family::GameFamily::Swbf1);
    let mut rec = Map::new();

    match fingerprint::hash(&files, &query) {
        Ok(f) => rec.insert("fingerprint".into(), Value::String(f)),
        Err(e) => rec.insert("fingerprint".into(), Value::String(format!("ERR:{e}"))),
    };

    match Loader::detect(root).load(map, &query) {
        Err(e) => {
            rec.insert("load".into(), Value::String(format!("ERR:{e}")));
        }
        Ok(bc) => {
            rec.insert("load".into(), Value::String("OK".into()));
            let mut h = H::new();
            for m in &bc.world {
                hash_mesh(&mut h, m);
            }
            rec.insert("world".into(), json!({"n": bc.world.len(), "h": h.hex()}));

            let mut h = H::new();
            for m in &bc.models {
                hash_mesh(&mut h, m);
            }
            let model_bumps = bc.models.iter().filter(|m| m.material.bump.is_some()).count();
            rec.insert(
                "models".into(),
                json!({"n": bc.models.len(), "bumps": model_bumps, "h": h.hex()}),
            );
            rec.insert(
                "world0".into(),
                json!({
                    "blend": format!("{:?}", bc.world.first().map(|m| m.material.blend)),
                    "ds": bc.world.first().is_some_and(|m| m.material.double_sided),
                    "cut": bc.world.first().is_some_and(|m| m.material.cutout),
                    "flags": bc.world.first().map_or(0, |m| m.material.flags)
                }),
            );

            let mut h = H::new();
            for a in &bc.actors {
                h.s(&a.name);
                for buf in &a.geometry.buffers {
                    h.b(buf);
                }
                h.u32s(&a.geometry.indices);
                h.s(&format!("{:?}", a.textures));
            }
            h.s(&format!("{:?}", bc.skins));
            h.s(&format!("{:?}", bc.texture_paths));
            rec.insert(
                "actors".into(),
                json!({"n": bc.actors.len(), "skins": bc.skins.len(), "h": h.hex()}),
            );

            let mut h = H::new();
            for (name, input) in &bc.units {
                h.s(name);
                for b in &input.bones {
                    h.s(&b.name);
                    h.signed(b.parent);
                    for v in b.pose {
                        h.f(v);
                    }
                }
                for c in &input.clips {
                    h.u(c.frames);
                    h.f(c.fps);
                    h.f(c.duration);
                    h.f(c.speed);
                    h.b(&[u8::from(c.root_motion), u8::from(c.idle)]);
                    for t in &c.tracks {
                        h.b(&t.to_le_bytes());
                    }
                    h.f32s(&c.data);
                }
                let t = &input.tables;
                h.s(&format!(
                    "{:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
                    t.sets, t.alert, t.transitions, t.jump, t.fall, t.land_soft, t.land_hard, t.dive, t.jet, t.thrown, t.upper
                ));
            }
            rec.insert("units".into(), json!({"n": bc.units.len(), "h": h.hex()}));

            let mut h = H::new();
            h.s(&format!("{:?} {:?}", bc.buildings_bvh, bc.terrain_bvh));
            rec.insert("bvh".into(), Value::String(h.hex()));

            let c = &bc.content;
            let mut h = H::new();
            h.f32s(&c.collision.positions);
            h.u32s(&c.collision.indices);
            h.u32s(&c.collision.polygons);
            h.f32s(&c.collision.terrain_positions);
            h.u32s(&c.collision.terrain_indices);
            h.f32s(&c.collision.tri_bounds);
            h.f(c.collision.kill_y);
            rec.insert(
                "collision".into(),
                json!({"tris": c.collision.indices.len() / 3, "h": h.hex()}),
            );

            let mut h = H::new();
            for pb in &c.prop_bodies {
                h.u(pb.instance);
                for look in [&pb.built, &pb.destroyed, &pb.unbuilt] {
                    match look {
                        None => h.s("none"),
                        Some(t) => {
                            h.f32s(&t.positions);
                            h.u32s(&t.indices);
                            h.u32s(&t.polygons);
                        }
                    }
                }
            }
            rec.insert("prop_bodies".into(), json!({"n": c.prop_bodies.len(), "h": h.hex()}));

            let mut h = H::new();
            for (k, clips) in &c.body_clips {
                h.s(k);
                for cl in &clips.list {
                    h.s(&cl.name);
                    h.f(cl.fps);
                    h.s(&format!("{:?}", cl.frames));
                    for t in &cl.tracks {
                        h.f(*t);
                    }
                    h.u(cl.start);
                    h.u(cl.end);
                    h.f(cl.duration);
                    h.f(cl.stride);
                    h.f(cl.speed);
                    h.b(&[u8::from(cl.root_motion)]);
                }
            }
            for (k, clips) in &c.prop_clips {
                h.s(k);
                h.u(clips.as_ref().map_or(0, |cl| cl.list.len()));
            }
            rec.insert(
                "clips".into(),
                json!({"bodies": c.body_clips.len(), "props": c.prop_clips.len(), "h": h.hex()}),
            );

            match bc.defs() {
                Err(e) => {
                    rec.insert("defs".into(), Value::String(format!("ERR:{e}")));
                }
                Ok(d) => {
                    let mut h = H::new();
                    let j = |v: &bfe_content::Value| authored_json(v).to_string();
                    for cls in &d.classes {
                        h.s(&format!(
                            "{:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
                            cls.name, cls.side, cls.base, cls.body, cls.clips, cls.walker_clips,
                            cls.weapons, cls.drops, cls.getup_time.to_bits(), cls.dive_time.to_bits(),
                            cls.droideka_guns, cls.registry_id
                        ));
                        h.s(&j(&cls.record));
                    }
                    for w in &d.weapons {
                        h.s(&format!(
                            "{} {} {:?} {:?} {:?} {:?} {:?} {:?}",
                            w.owner, w.slot, w.class, w.base, w.geometry, w.muzzle, w.item, w.registry_id
                        ));
                        h.s(&j(&w.record));
                    }
                    for pd in &d.props {
                        h.s(&format!(
                            "{} {:?} {:?} {:?} {:?} {:?}",
                            pd.index, pd.class, pd.base, pd.model, pd.bodies, pd.weapon_id
                        ));
                        h.s(&j(&pd.record));
                    }
                    rec.insert("defs".into(), json!({"classes": d.classes.len(), "h": h.hex()}));
                }
            }

            for (name, r) in [
                ("placed_admit", placed::admit(&bc)),
                ("placed_live", placed::admit_live(&bc)),
                ("placed_turrets", placed::admit_turrets(&bc)),
            ] {
                match r {
                    Err(e) => {
                        rec.insert(name.into(), Value::String(format!("ERR:{e}")));
                    }
                    Ok(p) => {
                        let mut h = H::new();
                        hash_parts(&mut h, &p);
                        rec.insert(
                            name.into(),
                            json!({"parts": p.parts.len(), "deferred": p.deferred, "h": h.hex()}),
                        );
                    }
                }
            }

            let n = c
                .battle
                .props
                .as_ref()
                .and_then(|p| p.get("instances"))
                .and_then(bfe_content::Value::as_array)
                .map_or(0, |a| a.len());
            let mut h = H::new();
            let mut errs = 0usize;
            for i in 0..n {
                for look in 0..3 {
                    match placed::admit_look(&bc, i, look) {
                        Ok(p) => hash_parts(&mut h, &p),
                        Err(e) => {
                            errs += 1;
                            h.s(&e);
                        }
                    }
                }
                match placed::admit_trap_pad(&bc, i) {
                    Ok(p) => hash_parts(&mut h, &p),
                    Err(e) => {
                        errs += 1;
                        h.s(&e);
                    }
                }
            }
            rec.insert("looks".into(), json!({"instances": n, "errors": errs, "h": h.hex()}));
        }
    }

    let options = Options {
        family: GameFamily::Swbf1,
        mode: bfe_family::DEFAULT_MODE.into(),
        map: map.into(),
        side: side.into(),
        enhanced: false,
        seed: 1,
        heroes: false,
        unit: None,
        name: None,
        bots: Bots::default(),
        ..Default::default()
    };
    let context = Context::new();
    match context.world_admit(&files, &options) {
        Err(e) => {
            rec.insert("sim".into(), Value::String(format!("ERR:{e}")));
        }
        Ok(admitted) => {
            context.world_step_init(admitted.setup.clone()).unwrap();
            let mut hash = mix(Fnv::new(), &admitted.publication);
            let mut clock = TickRows::new();
            let mut pub_buf = Publication::default();
            let mut facts_words = [0.0f64; WireWorldFacts::WORDS];
            let mut heap = Vec::new();
            let mut step_err: Option<String> = None;
            let mut ticks_done = 0u32;
            for tick in 1..=60u32 {
                let units = match context.world_prepare_units() {
                    Ok(u) => u,
                    Err(e) => {
                        step_err = Some(e.to_string());
                        break;
                    }
                };
                let at_ms = CLOCK_START_MS + f64::from(tick) * (1000.0 / 60.0);
                let input = clock.input(units, true, true, 0, true, 0, at_ms, 1.0);
                let output = match context.world_tick(input) {
                    Ok(o) => o,
                    Err(e) => {
                        step_err = Some(e.to_string());
                        break;
                    }
                };
                clock.complete(&output);
                let unix_ms = CLOCK_UNIX_MS + f64::from(tick) * (1000.0 / 60.0);
                let facts = WireWorldFacts {
                    tick: f64::from(tick),
                    at: at_ms,
                    unix_ms,
                    dropped_ticks: 0.0,
                    links: vec![WirePublicationLink {
                        id: admitted.handles.client,
                        discarded_actions: 0.0,
                        rtt: f64::NAN,
                        ..Default::default()
                    }],
                };
                heap.clear();
                facts.put(&mut facts_words, &mut heap);
                let mut combined = Vec::with_capacity(facts_words.len() + heap.len());
                combined.extend_from_slice(&facts_words);
                combined.extend_from_slice(&heap);
                context
                    .world_publish_into(admitted.handles.publication, &combined, &mut pub_buf)
                    .unwrap();
                hash = mix(hash, &pub_buf.bytes);
                for word in &pub_buf.block.words {
                    hash = mix(hash, &word.to_le_bytes());
                }
                ticks_done = tick;
            }
            let living = context.world_living_units().unwrap();
            context.dispose().unwrap();
            rec.insert(
                "sim".into(),
                json!({
                    "h": format!("{:016x}", hash.finish()),
                    "ticks": ticks_done,
                    "living": living,
                    "step_err": step_err
                }),
            );
        }
    }

    Value::Object(rec)
}

type VariantEdit = Box<dyn Fn(&TempDir)>;

fn build_variants() -> Vec<(&'static str, VariantEdit)> {
    let mat_2005 = json!({
        "blendMode": "alpha",
        "alphaCutout": 0.5,
        "doubleSided": true,
        "scroll": [0.1, 0.2],
        "textureMode": "none",
        "sourceTexture": "synth_src"
    });
    let mat_2005_a = mat_2005.clone();
    vec![
        ("c00_control", Box::new(|_| {})),
        (
            "c01_odd_flags_bump_empty",
            Box::new(|d| {
                d.rewrite_json("maps/synth1/map.json", |v| {
                    v["meshes"][0]["doubleSided"] = json!(true);
                    v["meshes"][0]["flags"] = json!(33);
                });
                d.rewrite_json("maps/synth1/props.json", |v| {
                    v["models"]["synth_crate"]["parts"][0]["bump"] = json!("synth_bump_tex");
                    v["instances"][0]["model"] = json!("");
                });
            }),
        ),
        (
            "c02_mat2005_ignored",
            Box::new(move |d| {
                let m = mat_2005_a.clone();
                d.rewrite_json("maps/synth1/map.json", |v| {
                    for mesh in v["meshes"].as_array_mut().unwrap() {
                        mesh["material"] = m.clone();
                        mesh["blendMode"] = json!("additive");
                        mesh["alphaCutout"] = json!(0.5);
                        mesh["textureMode"] = json!("missing");
                    }
                });
                let m2 = mat_2005_a.clone();
                d.rewrite_json("maps/synth1/props.json", |v| {
                    for model in v["models"].as_object_mut().unwrap().values_mut() {
                        for p in model["parts"].as_array_mut().unwrap() {
                            p["material"] = m2.clone();
                            p["bump"] = json!("synth_bump_tex");
                        }
                    }
                });
            }),
        ),
        (
            "c03_rig2005_keys_ignored",
            Box::new(|d| {
                d.rewrite_json("maps/synth1/props.json", |v| {
                    v["family"] = json!("swbf2");
                    v["attributesFile"] = json!("props_attributes.bin");
                    let mut skels = Map::new();
                    let mut colls = Map::new();
                    for (k, m) in v["models"].as_object_mut().unwrap().iter_mut() {
                        skels.insert(k.clone(), m["bones"].clone());
                        colls.insert(k.clone(), json!([0, 1]));
                        m["rigid"] = json!(true);
                        for p in m["parts"].as_array_mut().unwrap() {
                            p["bindBone"] = json!(0);
                            p["vertexStreams"] = json!({"tint": 0});
                            p["influencesPerVertex"] = json!(1);
                        }
                    }
                    v["skeletons"] = Value::Object(skels);
                    v["collision"] = Value::Object(colls);
                });
            }),
        ),
        (
            "c04_map2005_keys_ignored",
            Box::new(|d| {
                d.rewrite_json("maps/synth1/map.json", |v| {
                    v["family"] = json!("swbf2");
                    v["layers"] = json!({
                        "synth_extra": {
                            "hash": "01234567",
                            "name": "synth_extra",
                            "meshes": [],
                            "placedObjects": [],
                            "collision": "layers/none/collision.bin"
                        }
                    });
                    v["layerIndex"] = json!({"01234567": "synth_extra"});
                    v["bounds"] = json!({"min": [-1, -1, -1], "max": [1, 1, 1]});
                    v["worldAnimations"] = json!({
                        "clips": {"spin": {"duration": 1, "loop": true, "positions": [], "rotations": []}},
                        "groups": {},
                        "hierarchies": []
                    });
                    v["lights"] = json!([{"name": "l", "type": "omni", "position": [0, 0, 0]}]);
                    v["missions"]["synth1a"]["layers"] = json!(["synth_extra"]);
                });
            }),
        ),
        (
            "c05_flat_parts_array",
            Box::new(|d| {
                d.rewrite_json("maps/synth1/props.json", |v| {
                    let parts = v["models"]["synth_crate"]["parts"].clone();
                    v["models"]["synth_crate"] = parts;
                });
            }),
        ),
        (
            "c06_missing_optional_fields",
            Box::new(|d| {
                d.rewrite_json("maps/synth1/props.json", |v| {
                    for m in v["models"].as_object_mut().unwrap().values_mut() {
                        if let Some(obj) = m.as_object_mut() {
                            obj.remove("box");
                            obj.remove("sphere");
                            obj.remove("bones");
                        }
                    }
                    for inst in v["instances"].as_array_mut().unwrap() {
                        if let Some(obj) = inst.as_object_mut() {
                            obj.remove("collision");
                            obj.remove("settings");
                            obj.remove("radius");
                            obj.remove("label");
                        }
                    }
                });
            }),
        ),
        (
            "c07_empty_looks",
            Box::new(|d| {
                d.rewrite_json("maps/synth1/props.json", |v| {
                    for inst in v["instances"].as_array_mut().unwrap() {
                        inst["destroyed"] = json!({"model": ""});
                        inst["unbuilt"] = json!({"model": ""});
                    }
                });
            }),
        ),
        (
            "c08_full_only_clips",
            Box::new(|d| {
                d.rewrite_json("common/anims.json", |v| {
                    let clips = v["clips"].as_object_mut().unwrap();
                    let entry = clips.remove("human_rifle_stand_idle_emote").unwrap();
                    clips.insert("human_rifle_stand_idle_emote_full".into(), entry);
                });
            }),
        ),
        (
            "c09_older_no_nav_no_anims",
            Box::new(|d| {
                d.remove("maps/synth1/props_anims.bin");
                d.remove("maps/synth1/ai-nav.json");
                d.remove("maps/synth1/ai-nav.bin");
                d.remove("maps/synth1/ai-mission.json");
            }),
        ),
        (
            "c10_case_changed_model",
            Box::new(|d| {
                d.rewrite_json("maps/synth1/props.json", |v| {
                    for inst in v["instances"].as_array_mut().unwrap() {
                        if let Some(s) = inst["model"].as_str() {
                            inst["model"] = Value::String(s.to_ascii_uppercase());
                        }
                    }
                });
            }),
        ),
        (
            "c11_lowercase_dummyroot",
            Box::new(|d| {
                d.rewrite_json("sides/alpha/actors/alpha_body.json", |v| {
                    v["bones"][0]["name"] = json!("dummyroot");
                });
            }),
        ),
        (
            "c12_2005_files_beside_2004",
            Box::new(|d| {
                for rel in [
                    "maps/synth1/collision_mask.bin",
                    "maps/synth1/props_collision_mask.bin",
                    "maps/synth1/props_attributes.bin",
                    "maps/synth1/anim_banks.bin",
                ] {
                    d.write(rel, &[0u8; 64]);
                }
                d.write("maps/synth1/classes.json", br#"{"entc": {}}"#);
                d.write(
                    "maps/synth1/anim_banks.json",
                    br#"{"fps": 30, "banks": {"zz": {"clips": [{"crc": "x", "frames": 1e30}]}}}"#,
                );
                d.write("common/anim_banks.json", br#"{"fps": 0, "banks": {}}"#);
                d.write("sides/alpha/anim_banks.json", b"not json");
            }),
        ),
        (
            "c13_string_matrix_and_team",
            Box::new(|d| {
                d.rewrite_json("maps/synth1/props.json", |v| {
                    for inst in v["instances"].as_array_mut().unwrap() {
                        let mat_strs: Vec<Value> = inst["matrix"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|x| Value::String(x.to_string()))
                            .collect();
                        inst["matrix"] = Value::Array(mat_strs);
                        inst["team"] = json!("1");
                    }
                });
            }),
        ),
    ]
}

#[test]
fn swbf1_odd_corpus_matches_released_base_golden() {
    let mut actual: BTreeMap<String, Value> = BTreeMap::new();
    for (name, edit) in build_variants() {
        let dir = TempDir::new(name);
        build_invented_base_2004(&dir);
        edit(&dir);
        let measured = measure_variant(&dir.0);

        // SWBF1_FAMILY_CHECK_START
        let files = DirFiles::new(&dir.0, bfe_family::GameFamily::Swbf1);
        let query = "?map=synth1&side=alpha";
        let fp_direct = fingerprint::hash(&files, query);
        let fp_family = fingerprint::hash_for_family(&files, GameFamily::Swbf1, query);
        assert_eq!(
            fp_direct, fp_family,
            "{name}: hash_for_family(Swbf1) diverged from fingerprint::hash"
        );
        let load_direct = Loader::detect(&dir.0).load("synth1", query);
        let load_family = Loader::detect(&dir.0).load_for_family(GameFamily::Swbf1, "synth1", query);
        assert_eq!(
            load_direct.is_ok(),
            load_family.is_ok(),
            "{name}: load_for_family(Swbf1) ok/err status diverged from Loader::load"
        );
        if let (Ok(a), Ok(b)) = (load_direct, load_family) {
            assert_eq!(a.world.len(), b.world.len(), "{name}: world count diverged");
            assert_eq!(a.models.len(), b.models.len(), "{name}: models count diverged");
            assert_eq!(a.actors.len(), b.actors.len(), "{name}: actors count diverged");
            assert_eq!(a.units.len(), b.units.len(), "{name}: units count diverged");
        }
        // SWBF1_FAMILY_CHECK_END

        actual.insert(name.to_string(), measured);
    }

    if let Ok(out_path) = std::env::var("BFE_WRITE_SWBF1_GOLDEN") {
        let text = serde_json::to_string_pretty(&actual).unwrap();
        std::fs::write(&out_path, format!("{text}\n")).unwrap();
        return;
    }

    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/swbf1_base_corpus_golden.json");
    let golden_text = std::fs::read_to_string(&golden_path).unwrap_or_else(|e| {
        panic!("missing released 2004 golden fixture {}: {e}", golden_path.display())
    });
    let expected: BTreeMap<String, Value> = serde_json::from_str(&golden_text).unwrap();
    assert_eq!(
        actual.len(),
        expected.len(),
        "synthetic 2004 corpus variant count mismatch"
    );
    for (name, exp_val) in &expected {
        let act_val = actual
            .get(name)
            .unwrap_or_else(|| panic!("missing variant {name}"));
        assert_eq!(
            act_val, exp_val,
            "2004 corpus variant '{name}' diverged from released 2004 golden"
        );
    }
}
