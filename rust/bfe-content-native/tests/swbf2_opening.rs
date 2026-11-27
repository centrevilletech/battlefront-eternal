//! Opening 2005 targets integration proof (`BFE_SWBF2_CONTENT`).
//!
//! Exercises the shared typed map admission (`Loader::admit_map`), full native
//! visual/content admission (`Loader::load_for_family`), placed prop rigs,
//! skeletons, materials, drawn-vs-raw vertex alignment on every rigged placed
//! model, world animations (`placed::admit`, `placed::admit_at`,
//! `placed::admit_look`), world-space prop collision body placement and
//! exact-slot ray queries (capital ship hull, turret, door, orbiting moving
//! frigate, hangar floor), Grievous and saber hero animation slot resolution,
//! ordnance explosions in simulation, family-aware content fingerprinting,
//! equal stand-in teams, and 1,800-tick headless simulation determinism with
//! living, moving bots plus mutation failure checks for both opening 2005
//! targets:
//! - Mos Eisley, Hero Assault (`tat2`, mission `tat2g_eli`, layer `tat2_tdm`)
//! - Space Mygeeto, Space Assault (`spa6`, mission `spa6c_ass`, layer `spa6_obj`)
//!
//! Run on prepared 2005 content with:
//! `BFE_SWBF2_CONTENT=... cargo test -p bfe-content-native --test swbf2_opening -- --ignored --nocapture`

use bfe_content::battle::SimulationSupport;
use bfe_content::files::{DirFiles, Files};
use bfe_content_native::{
    fingerprint,
    loader::{Blend, Loader},
    placed,
    setup::{
        admission::{CLOCK_START_MS, CLOCK_UNIX_MS},
        TickRows,
    },
    GameFamily,
};
use bfe_sim::abi::context::{CollisionQuery, Context};
use bfe_sim::abi::context_step::Publication;
use bfe_sim::collision::{Ray, MISS, STATIC};
use bfe_sim::gen::{WirePublicationLink, WireWorldFacts};
use bfe_sim::host::census::Census;
use bfe_sim::host::content_tables_files_for_family;
use bfe_sim::world::admit::{Bots, Options};
use bfe_wide::{Fnv, Wide};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;

fn mix(mut hash: Fnv, bytes: &[u8]) -> Fnv {
    hash.bytes(bytes);
    hash
}

fn mix_f64(hash: Fnv, v: f64) -> Fnv {
    mix(hash, &v.to_le_bytes())
}

/// A count as the eight little-endian bytes of its word.
fn mix_count(mut hash: Fnv, n: usize) -> Fnv {
    hash.small(u32::try_from(n).unwrap_or(u32::MAX));
    hash
}

/// A byte count that stops at the largest 32-bit number (a battle's publications are far below it).
fn count_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Overlay `Files` wrapper used by mutation and fingerprint checks to alter
/// files in memory without touching the prepared content directory on disk.
struct OverlayFiles<'a> {
    base: &'a DirFiles,
    overrides: HashMap<String, Rc<[u8]>>,
}

impl Files for OverlayFiles<'_> {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        let key = path.trim_start_matches('/');
        if let Some(bytes) = self.overrides.get(key) {
            return Some(Rc::clone(bytes));
        }
        self.base.read(path)
    }
}

struct RunSummary {
    digest: Wide,
    pub_bytes: u32,
    peak_living: usize,
    total_distance_moved: f64,
    spawned_classes: BTreeSet<String>,
    census_lines: Vec<String>,
}

fn step_headless_world(
    files: &dyn Files,
    map: &str,
    mission: &str,
    side: &str,
    ticks: u32,
) -> Result<RunSummary, String> {
    let options = Options {
        family: GameFamily::Swbf2,
        mode: mission.rsplit_once('_').unwrap().1.into(),
        practice_target: None,
        map: map.into(),
        side: side.into(),
        enhanced: false,
        seed: 1,
        heroes: true,
        unit: None,
        name: None,
        bots: Bots::default(),
        ..Default::default()
    };
    let context = Context::new();
    let admitted = context
        .world_admit_for_family(
            files,
            GameFamily::Swbf2,
            &options,
            Some(mission),
            true, // spawn screen: no human player spawned
        )
        .map_err(|e| format!("admit {map}/{mission}: {e}"))?;
    context
        .world_step_init(admitted.setup.clone())
        .map_err(|e| format!("step_init {map}/{mission}: {e}"))?;

    let mut hash = Fnv::new();
    hash = mix(hash, &admitted.publication);

    let mut clock = TickRows::new();
    let mut pub_buf = Publication::default();
    let mut facts_words = [0.0f64; WireWorldFacts::WORDS];
    let mut heap = Vec::new();
    let mut total_pub_bytes = count_u32(admitted.publication.len());
    let mut census = Census::default();
    let mut census_lines = Vec::new();
    let mut peak_living = 0usize;
    let mut total_distance_moved = 0.0f64;
    let mut last_pos: HashMap<u32, [f64; 3]> = HashMap::new();
    let mut spawned_classes: BTreeSet<String> = BTreeSet::new();

    for tick in 1..=ticks {
        let units = context
            .world_prepare_units()
            .map_err(|e| format!("prepare_units {map}/{mission} tick {tick}: {e}"))?;
        let at_ms = CLOCK_START_MS + f64::from(tick) * (1000.0 / 60.0);
        let input = clock.input(units, true, true, 0, true, 0, at_ms, 1.0);
        let output = context
            .world_tick(input)
            .map_err(|e| format!("world_tick {map}/{mission} tick {tick}: {e}"))?;
        if !output.accepted {
            return Err(format!("world_tick refused at {map}/{mission} tick {tick}"));
        }
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
            .map_err(|e| format!("publish {map}/{mission} tick {tick}: {e}"))?;
        hash = mix(hash, &pub_buf.bytes);
        for word in &pub_buf.block.words {
            hash = mix_f64(hash, *word);
        }
        total_pub_bytes = total_pub_bytes.saturating_add(count_u32(pub_buf.bytes.len()));

        if let Ok(Some(line)) = census.sample(&context, u64::from(tick), 60) {
            hash = mix(hash, line.as_bytes());
            census_lines.push(format!("t{tick}: {line}"));
        }

        // Digest authoritative world state (living units, positions, health,
        // classes, and prop targets) every 30 ticks so parked-client wire
        // silence cannot mask simulation divergence.
        if tick % 30 == 0 {
            let living = context
                .world_living_units()
                .map_err(|e| format!("world_living_units: {e}"))?;
            peak_living = peak_living.max(living[0] + living[1] + living[2]);
            for c in living {
                hash = mix_count(hash, c);
            }
            let states = context
                .world_unit_states()
                .map_err(|e| format!("world_unit_states: {e}"))?;
            for (slot, team, alive, pos, health, class) in states {
                hash = mix(hash, &slot.to_le_bytes());
                hash = mix_f64(hash, team);
                hash = mix(hash, &[u8::from(alive)]);
                if alive {
                    if !class.is_empty() {
                        spawned_classes.insert(class.clone());
                    }
                    hash = mix(hash, class.as_bytes());
                    for coord in pos {
                        hash = mix_f64(hash, coord);
                    }
                    hash = mix_f64(hash, health);
                    if let Some(prev) = last_pos.insert(slot, pos) {
                        let dx = pos[0] - prev[0];
                        let dy = pos[1] - prev[1];
                        let dz = pos[2] - prev[2];
                        let step = (dx * dx + dy * dy + dz * dz).sqrt();
                        if step.is_finite() && step < 40.0 {
                            total_distance_moved += step;
                        }
                    }
                } else {
                    last_pos.remove(&slot);
                }
            }
            let props = context
                .prop_targets()
                .map_err(|e| format!("prop_targets: {e}"))?;
            for p in props {
                hash = mix_f64(hash, p.id);
                hash = mix(hash, &[u8::from(p.alive)]);
                hash = mix_f64(hash, p.team);
                hash = mix_f64(hash, p.life);
            }
        }
    }

    context.dispose().map_err(|e| format!("dispose: {e}"))?;
    Ok(RunSummary {
        digest: hash.finish(),
        pub_bytes: total_pub_bytes,
        peak_living,
        total_distance_moved,
        spawned_classes,
        census_lines,
    })
}

fn mul4(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let mut out = [0.0f64; 16];
    for col in 0..4 {
        for row in 0..4 {
            out[col * 4 + row] = a[row] * b[col * 4]
                + a[4 + row] * b[col * 4 + 1]
                + a[8 + row] * b[col * 4 + 2]
                + a[12 + row] * b[col * 4 + 3];
        }
    }
    out
}

fn transform_point(m: &[f64; 16], p: [f64; 3]) -> [f64; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

/// Evaluates a single skinned vertex the same way the native renderer's
/// `bake_pose` does (`part.matrix * part.bind_inverse * skin * v`).
fn eval_skinned_world_vertex(part: &placed::Part, vi: usize) -> [f64; 3] {
    let p = [
        f64::from(part.positions[vi * 3]),
        f64::from(part.positions[vi * 3 + 1]),
        f64::from(part.positions[vi * 3 + 2]),
    ];
    let mut skin = [0.0f64; 16];
    for weight_idx in 0..4 {
        let w = f64::from(part.weights[vi * 4 + weight_idx]);
        if w == 0.0 {
            continue;
        }
        let j = usize::from(part.joints[vi * 4 + weight_idx]) * 16;
        for (k, slot) in skin.iter_mut().enumerate() {
            *slot += w * f64::from(part.bones[j + k]);
        }
    }
    let bind_inverse: [f64; 16] = std::array::from_fn(|k| f64::from(part.bind_inverse[k] as f32));
    let combined = mul4(&part.matrix, &mul4(&bind_inverse, &skin));
    transform_point(&combined, p)
}

/// Verifies that every rigged placed model (models with a non-empty skeleton)
/// preserves its model-space vertices through `placed::admit_map`, evaluates
/// to the exact same rest-pose world AABB as raw `props.bin` * `placed.matrix`,
/// binds per-vertex `vertexStreams.joints` from `props_attributes.bin`, and
/// preserves aimer bone attachment batches on `com_bldg_chaingun_roof`.
fn verify_drawn_rigged_alignment(
    exp: &TargetExpectations,
    admitted_map: &bfe_content_native::map::AdmittedMap,
    map_placed: &placed::Placements,
) {
    let map = exp.map;
    let props = admitted_map.admission.props.as_ref().expect("props.json");
    let models = props
        .get("models")
        .and_then(bfe_content::Value::as_object)
        .expect("props.models");
    let pack = admitted_map.props_pack.as_ref().expect("props_pack");
    let attrs = admitted_map.admission.props_attributes_buf.as_deref();

    let mut parts_by_instance: HashMap<usize, Vec<&placed::Part>> = HashMap::new();
    for part in &map_placed.parts {
        parts_by_instance.entry(part.instance).or_default().push(part);
    }

    let mut checked_rigged = 0usize;
    let mut chaingun_roof_checked = 0usize;
    let mut skinned_stream_instances = 0usize;
    let mut max_local_err = 0.0f64;
    let mut max_world_err = 0.0f64;

    for (inst_idx, placed_obj) in admitted_map.admission.placed.iter().enumerate() {
        let active_model = placed_obj
            .unbuilt_model
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| placed_obj.model.as_deref().filter(|s| !s.is_empty()));
        let Some(model_name) = active_model else { continue };
        let Some((resolved_name, rig)) = models
            .get(model_name)
            .map(|r| (model_name, r))
            .or_else(|| models.entries().find(|(k, _)| k.eq_ignore_ascii_case(model_name)))
        else {
            continue;
        };
        let bone_count = rig
            .get("bones")
            .and_then(bfe_content::Value::as_array)
            .map_or(0, <[bfe_content::Value]>::len);
        if bone_count == 0 {
            continue;
        }
        let raw_parts = pack
            .get(resolved_name)
            .or_else(|| {
                pack.iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(resolved_name))
                    .map(|(_, v)| v)
            })
            .unwrap_or_else(|| panic!("{map}: missing raw pack parts for rigged model '{resolved_name}'"));

        let mut raw_lmin = [f64::INFINITY; 3];
        let mut raw_lmax = [f64::NEG_INFINITY; 3];
        let mut raw_wmin = [f64::INFINITY; 3];
        let mut raw_wmax = [f64::NEG_INFINITY; 3];
        let mut raw_vert_count = 0usize;
        for rp in raw_parts.iter() {
            for v in rp.data.as_chunks::<8>().0 {
                let lp = [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])];
                let wp = transform_point(&placed_obj.matrix, lp);
                for k in 0..3 {
                    raw_lmin[k] = raw_lmin[k].min(lp[k]);
                    raw_lmax[k] = raw_lmax[k].max(lp[k]);
                    raw_wmin[k] = raw_wmin[k].min(wp[k]);
                    raw_wmax[k] = raw_wmax[k].max(wp[k]);
                }
                raw_vert_count += 1;
            }
        }
        if raw_vert_count == 0 {
            continue;
        }

        let drawn = parts_by_instance.get(&inst_idx).unwrap_or_else(|| {
            panic!(
                "{map}: rigged placed instance {inst_idx} ('{}' model '{resolved_name}') produced 0 drawn parts",
                placed_obj.name
            )
        });
        let mut drawn_lmin = [f64::INFINITY; 3];
        let mut drawn_lmax = [f64::NEG_INFINITY; 3];
        let mut drawn_wmin = [f64::INFINITY; 3];
        let mut drawn_wmax = [f64::NEG_INFINITY; 3];
        for part in drawn {
            let vcount = part.positions.len() / 3;
            for vi in 0..vcount {
                let lp = [
                    f64::from(part.positions[vi * 3]),
                    f64::from(part.positions[vi * 3 + 1]),
                    f64::from(part.positions[vi * 3 + 2]),
                ];
                let wp = eval_skinned_world_vertex(part, vi);
                for k in 0..3 {
                    drawn_lmin[k] = drawn_lmin[k].min(lp[k]);
                    drawn_lmax[k] = drawn_lmax[k].max(lp[k]);
                    drawn_wmin[k] = drawn_wmin[k].min(wp[k]);
                    drawn_wmax[k] = drawn_wmax[k].max(wp[k]);
                }
            }
        }

        for k in 0..3 {
            let local_err = (drawn_lmin[k] - raw_lmin[k])
                .abs()
                .max((drawn_lmax[k] - raw_lmax[k]).abs());
            let world_err = (drawn_wmin[k] - raw_wmin[k])
                .abs()
                .max((drawn_wmax[k] - raw_wmax[k]).abs());
            max_local_err = max_local_err.max(local_err);
            max_world_err = max_world_err.max(world_err);
            assert!(
                local_err < 1e-3,
                "{map}: rigged instance {inst_idx} ('{}' model '{resolved_name}') local box axis {k} displaced by {local_err:.4}m (drawn={drawn_lmin:?}..{drawn_lmax:?} vs raw={raw_lmin:?}..{raw_lmax:?})",
                placed_obj.name
            );
            assert!(
                world_err < 0.05,
                "{map}: rigged instance {inst_idx} ('{}' model '{resolved_name}') rest-pose world box axis {k} displaced by {world_err:.4}m (drawn={drawn_wmin:?}..{drawn_wmax:?} vs raw={raw_wmin:?}..{raw_wmax:?})",
                placed_obj.name
            );
        }
        let mut expected_stream_bones: BTreeSet<u16> = BTreeSet::new();
        let mut streamed_vertices = 0usize;
        for rp in raw_parts.iter() {
            if let Some(off_f) = rp
                .record
                .get("vertexStreams")
                .and_then(bfe_content::Value::as_object)
                .and_then(|vs| vs.get("joints"))
                .and_then(bfe_content::Value::as_f64)
            {
                let attr_bytes = attrs.unwrap_or_else(|| {
                    panic!("{map}: missing props_attributes.bin for streamed model '{resolved_name}'")
                });
                let vcount = rp.data.len() / 8;
                let infl = rp
                    .record
                    .get("influencesPerVertex")
                    .and_then(bfe_content::Value::as_f64)
                    .unwrap_or(1.0) as usize;
                let off = off_f as usize;
                let end = off + vcount * infl * 4;
                for chunk in attr_bytes[off..end].chunks_exact(4 * infl) {
                    let j = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    expected_stream_bones.insert(j as u16);
                }
                streamed_vertices += vcount;
            }
        }
        if !expected_stream_bones.is_empty() {
            let mut actual_joints: BTreeSet<u16> = BTreeSet::new();
            let mut bound_vertices = 0usize;
            for part in drawn {
                let vcount = part.positions.len() / 3;
                for vi in 0..vcount {
                    let j0 = part.joints[vi * 4];
                    actual_joints.insert(j0);
                    if usize::from(j0) < bone_count {
                        bound_vertices += 1;
                    }
                }
            }
            assert!(
                bound_vertices >= streamed_vertices,
                "{map}: skinned instance {inst_idx} ('{}' model '{resolved_name}') bound {bound_vertices}/{streamed_vertices} vertices (actual_joints={actual_joints:?}, expected={expected_stream_bones:?})",
                placed_obj.name
            );
            for b in &expected_stream_bones {
                assert!(
                    actual_joints.contains(b),
                    "{map}: skinned instance {inst_idx} ('{}' model '{resolved_name}') missing streamed bone {b} (actual={actual_joints:?}, expected={expected_stream_bones:?})",
                    placed_obj.name
                );
            }
            skinned_stream_instances += 1;
        }
        if resolved_name.eq_ignore_ascii_case("com_bldg_chaingun_roof") {
            let has_aimer_x = drawn
                .iter()
                .any(|p| p.attachment.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("aimer_x")));
            let has_aimer_y = drawn
                .iter()
                .any(|p| p.attachment.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("aimer_y")));
            assert!(
                has_aimer_x && has_aimer_y,
                "{map}: com_bldg_chaingun_roof instance {inst_idx} ('{}') missing aimer attachment batches (aimer_x={has_aimer_x}, aimer_y={has_aimer_y})",
                placed_obj.name
            );
            chaingun_roof_checked += 1;
        }
        checked_rigged += 1;
    }

    assert!(checked_rigged > 0, "{map}: expected rigged placed models to verify");
    assert_eq!(
        skinned_stream_instances, exp.expected_skinned_stream_instances,
        "{map}: skinned vertexStreams.joints placed instance count mismatch"
    );
    if map == "spa6" {
        assert_eq!(
            chaingun_roof_checked, 10,
            "spa6: expected all 10 com_bldg_chaingun_roof turret instances aligned"
        );
    }
    println!(
        "rigged_alignment {map}: checked={checked_rigged} skinned_stream={skinned_stream_instances} chaingun_roof={chaingun_roof_checked} max_local_err={max_local_err:.6}m max_world_err={max_world_err:.6}m (0 displaced)"
    );
}

/// Casts exterior triangle-normal rays against `pb.built` until an unoccluded
/// triangle produces an exact hit on `expected_slot`. Never accepts `MISS`.
fn cast_prop_surface_ray(
    context: &Context,
    flags: &[u8],
    pb: &bfe_content::content::PropBodies,
    placed_obj: &bfe_content::map::PlacedObject,
    expected_slot: u32,
    label: &str,
) -> (i32, f64, [f64; 3]) {
    let shape = pb
        .built
        .as_ref()
        .unwrap_or_else(|| panic!("{label}: prop body has no built collision shape"));
    let verts = shape.positions.as_chunks::<3>().0;
    let tris = shape.indices.as_chunks::<3>().0;
    assert!(!tris.is_empty(), "{label}: built collision shape has 0 triangles");

    let world_v = |idx: u32| -> [f64; 3] {
        let v = verts[idx as usize];
        let p = [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])];
        if pb.world_animated {
            transform_point(&placed_obj.matrix, p)
        } else {
            p
        }
    };

    let mut last_code = MISS;
    for &[i0, i1, i2] in tris {
        let a = world_v(i0);
        let b = world_v(i1);
        let c = world_v(i2);
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let nx = ab[1] * ac[2] - ab[2] * ac[1];
        let ny = ab[2] * ac[0] - ab[0] * ac[2];
        let nz = ab[0] * ac[1] - ab[1] * ac[0];
        let nlen = (nx * nx + ny * ny + nz * nz).sqrt();
        if !nlen.is_finite() || nlen < 1e-3 {
            continue;
        }
        let n = [nx / nlen, ny / nlen, nz / nlen];
        let center = [
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
            (a[2] + b[2] + c[2]) / 3.0,
        ];
        for offset in [2.0f64, 0.5f64] {
            for sign in [1.0f64, -1.0f64] {
                let ray = Ray {
                    ox: center[0] + sign * n[0] * offset,
                    oy: center[1] + sign * n[1] * offset,
                    oz: center[2] + sign * n[2] * offset,
                    dx: -sign * n[0],
                    dy: -sign * n[1],
                    dz: -sign * n[2],
                };
                let res = context
                    .collision_query(
                        CollisionQuery::Cast {
                            world: 0,
                            ray,
                            far: offset + 1.0,
                            hypot_xz: 0.0,
                            ignore: -1,
                            static_only: false,
                            moving_only: false,
                        },
                        flags,
                    )
                    .unwrap();
                last_code = res.code;
                if res.code == expected_slot as i32 {
                    let dist = res.io[10];
                    if dist.is_finite() && (dist - offset).abs() < 0.05 {
                        return (res.code, dist, [res.io[11], res.io[12], res.io[13]]);
                    }
                }
            }
        }
    }
    panic!(
        "{label} (inst {}, model {:?}, expected slot {expected_slot}): no exterior triangle-normal ray hit exact body slot (last_code={last_code}, MISS={MISS})",
        pb.instance, placed_obj.model
    );
}

fn verify_world_space_prop_collision(
    root: &Path,
    exp: &TargetExpectations,
    battle: &bfe_content_native::BattleContent,
    admitted_map: &bfe_content_native::map::AdmittedMap,
) {
    let map = exp.map;
    let mission = exp.mission;
    let side = exp.side;
    let files = DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf2);
    let options = Options {
        family: GameFamily::Swbf2,
        mode: mission.rsplit_once('_').unwrap().1.into(),
        practice_target: None,
        map: map.into(),
        side: side.into(),
        enhanced: false,
        seed: 1,
        heroes: true,
        unit: None,
        name: None,
        bots: Bots::default(),
        ..Default::default()
    };
    let context = Context::new();
    let admitted = context
        .world_admit_for_family(&files, GameFamily::Swbf2, &options, Some(mission), true)
        .unwrap_or_else(|e| panic!("admit for collision check {map}/{mission}: {e}"));

    // Verify exact 2005 ordnance explosion counts reach simulation with positive damage and radius.
    let explosions = context.world_explosion_defs().unwrap();
    let positive_explosions = explosions
        .iter()
        .filter(|&&(dmg, _, outer)| dmg > 0.0 && outer > 0.0)
        .count();
    assert_eq!(
        explosions.len(),
        exp.expected_explosions_total,
        "{map}: exact total explosion count mismatch in simulation"
    );
    assert_eq!(
        positive_explosions,
        exp.expected_explosions_positive,
        "{map}: exact positive-damage explosion count mismatch in simulation"
    );
    let (host_explosions, _host_content) = content_tables_files_for_family(
        &context,
        &files,
        GameFamily::Swbf2,
        &options,
        Some(mission),
        admitted.handles.store_source,
    )
    .unwrap_or_else(|e| panic!("{map}: content_tables_files_for_family failed: {e}"));
    // Every admitted explosion needs a combat definition in the host table: a
    // blast without one cannot be recorded for the wire.
    let unmapped = bfe_sim::host::unmapped_explosions(&context, admitted.handles.store_source, &host_explosions)
        .unwrap_or_else(|e| panic!("{map}: unmapped_explosions failed: {e}"));
    assert!(
        unmapped.is_empty(),
        "{map}: admitted explosions without a host table entry: {unmapped:?}"
    );
    assert_eq!(
        host_explosions.len(),
        exp.expected_explosions_total,
        "{map}: exact host content table explosion count mismatch"
    );
    println!(
        "sim_explosions {map}: total={} positive_damage_and_radius={} host_table_entries={}",
        explosions.len(),
        positive_explosions,
        host_explosions.len()
    );

    // Verify family-aware dedicated network admission succeeds for 2005.
    let net_ctx = Context::new();
    let _net_admitted = net_ctx
        .world_network_admit_for_family(&files, GameFamily::Swbf2, &options, Some(mission))
        .unwrap_or_else(|e| panic!("{map}: world_network_admit_for_family failed: {e}"));
    net_ctx.dispose().unwrap();

    let prop_kernels = context.prop_body_kernels().unwrap();
    let body_bounds = context.collision_body_bounds().unwrap();
    let fighter_bodies = context.owned(|world| {
        world.space().into_iter().flat_map(|space| {
            space.fighters.iter().filter_map(|fighter| fighter.kernel_body)
        }).collect::<BTreeSet<_>>()
    }).unwrap();
    let bounds_by_slot: HashMap<u32, (bool, [f64; 6])> = body_bounds
        .iter()
        .filter(|(slot, _, _)| !fighter_bodies.contains(slot))
        .map(|&(slot, moving, b)| (slot, (moving, b)))
        .collect();
    assert_eq!(bounds_by_slot.len() + fighter_bodies.len(), body_bounds.len());
    assert_eq!(
        bounds_by_slot.len(),
        exp.kernel_prop_bodies,
        "{map}: exact kernel_prop_bodies count mismatch"
    );

    // Map every `(instance, look)` to its exact collision kernel slot in the
    // exact allocation order of `construct_swbf2_prop_bodies`.
    let mut slot_by_inst_look: HashMap<(usize, u8), u32> = HashMap::new();
    let mut flags = vec![0u8; body_bounds.len()];
    let mut moving_slots_by_inst: HashMap<usize, Vec<bool>> = HashMap::new();
    let mut next_slot = 0u32;
    let mut built_by_base: BTreeMap<String, usize> = BTreeMap::new();
    for pb in &battle.content.prop_bodies {
        let placed_obj = &admitted_map.admission.placed[pb.instance];
        let has_unbuilt = placed_obj.unbuilt_model.as_deref().is_some_and(|s| !s.is_empty());
        let first_look: u8 = if has_unbuilt { 2 } else { 0 };
        if pb.built.is_some() {
            *built_by_base.entry(placed_obj.base.clone()).or_default() += 1;
        }
        for (look_idx, look) in [&pb.built, &pb.destroyed, &pb.unbuilt].into_iter().enumerate() {
            if look.is_some() {
                slot_by_inst_look.insert((pb.instance, look_idx as u8), next_slot);
                let &(is_moving, _) = bounds_by_slot.get(&next_slot).unwrap_or_else(|| {
                    panic!("{map}: missing collision body bounds for slot {next_slot}")
                });
                assert_eq!(
                    is_moving, pb.world_animated,
                    "{map}: instance {} ('{}') moving flag {is_moving} != world_animated {}",
                    pb.instance, placed_obj.name, pb.world_animated
                );
                moving_slots_by_inst.entry(pb.instance).or_default().push(is_moving);
                if look_idx as u8 == first_look {
                    flags[next_slot as usize] = 1;
                }
                next_slot += 1;
            }
        }
    }
    assert_eq!(
        next_slot as usize,
        bounds_by_slot.len(),
        "{map}: each placed look must match one prop collision body"
    );
    // Verify no instance has both a static baked body and a moving body.
    for (inst, modes) in &moving_slots_by_inst {
        let has_static = modes.iter().any(|m| !m);
        let has_moving = modes.iter().any(|m| *m);
        assert!(
            !(has_static && has_moving),
            "{map}: instance {inst} ('{}') has both baked static and moving collision bodies",
            admitted_map.admission.placed[*inst].name
        );
    }
    let moving_body_count = bounds_by_slot.values().filter(|(moving, _)| *moving).count();
    assert!(
        moving_body_count >= exp.expected_moving_min && moving_body_count <= exp.expected_moving_max,
        "{map}: moving kernel body count {moving_body_count} outside expected {}..={}",
        exp.expected_moving_min,
        exp.expected_moving_max
    );
    assert_eq!(
        built_by_base.get("door").copied().unwrap_or(0),
        exp.expected_doors,
        "{map}: door built collision body count mismatch"
    );
    assert_eq!(
        built_by_base.get("armedbuilding").copied().unwrap_or(0),
        exp.expected_turrets,
        "{map}: turret (armedbuilding) built collision body count mismatch"
    );

    let props_models = battle
        .content
        .battle
        .props
        .as_ref()
        .and_then(|p| p.get("models"))
        .and_then(bfe_content::Value::as_object)
        .expect("props.models");

    let live_sources = bfe_sim::world::admit::rows::props::live_sources(&battle.content.battle);

    // Verify every building look reported by `prop_body_kernels` matches the
    // exact slot allocated from `Content::prop_bodies`.
    for &(building_idx, looks) in &prop_kernels {
        let inst_idx = live_sources[building_idx];
        for (look_idx, slot_opt) in looks.iter().enumerate() {
            if let Some(slot) = *slot_opt {
                assert_eq!(
                    slot_by_inst_look.get(&(inst_idx, look_idx as u8)).copied(),
                    Some(slot),
                    "{map}: building {building_idx} (inst {inst_idx}) look {look_idx} slot mismatch"
                );
            }
        }
    }

    let drawn_world_aabb = |inst_idx: usize| -> ([f64; 3], [f64; 3], &str) {
        let placed_obj = &admitted_map.admission.placed[inst_idx];
        let model_name = placed_obj.model.as_deref().unwrap_or("");
        let rig = props_models.get(model_name).expect("rig for placed prop");
        let model_box = rig.get("box").and_then(bfe_content::Value::as_array).expect("rig box");
        let lo = [
            model_box[0].at(0).and_then(bfe_content::Value::as_f64).unwrap(),
            model_box[0].at(1).and_then(bfe_content::Value::as_f64).unwrap(),
            model_box[0].at(2).and_then(bfe_content::Value::as_f64).unwrap(),
        ];
        let hi = [
            model_box[1].at(0).and_then(bfe_content::Value::as_f64).unwrap(),
            model_box[1].at(1).and_then(bfe_content::Value::as_f64).unwrap(),
            model_box[1].at(2).and_then(bfe_content::Value::as_f64).unwrap(),
        ];
        let mut draw_min = [f64::INFINITY; 3];
        let mut draw_max = [f64::NEG_INFINITY; 3];
        for sx in [lo[0], hi[0]] {
            for sy in [lo[1], hi[1]] {
                for sz in [lo[2], hi[2]] {
                    let wp = transform_point(&placed_obj.matrix, [sx, sy, sz]);
                    for k in 0..3 {
                        draw_min[k] = draw_min[k].min(wp[k]);
                        draw_max[k] = draw_max[k].max(wp[k]);
                    }
                }
            }
        }
        (draw_min, draw_max, model_name)
    };

    // Check every admitted kernel prop body AABB against its drawn world AABB.
    let mut checked_built_bodies = 0usize;
    let mut origin_piled = 0usize;
    for pb in &battle.content.prop_bodies {
        let Some(built_slot) = slot_by_inst_look.get(&(pb.instance, 0)).copied() else {
            continue;
        };
        let &(_, bb) = bounds_by_slot.get(&built_slot).unwrap();
        let placed_obj = &admitted_map.admission.placed[pb.instance];
        let (draw_min, draw_max, model_name) = drawn_world_aabb(pb.instance);
        let pad = 8.0;
        let overlaps = (0..3).all(|k| bb[k] <= draw_max[k] + pad && bb[k + 3] >= draw_min[k] - pad);
        assert!(
            overlaps,
            "{map} prop instance {} ('{}' model '{model_name}' slot {built_slot} moving={} at {:?}): kernel collision AABB {:?} does not match drawn world AABB {:?}..{:?}",
            pb.instance, placed_obj.name, pb.world_animated, placed_obj.position, bb, draw_min, draw_max
        );
        let pos_dist = (placed_obj.position[0].powi(2)
            + placed_obj.position[1].powi(2)
            + placed_obj.position[2].powi(2))
        .sqrt();
        let body_center = [
            0.5 * (bb[0] + bb[3]),
            0.5 * (bb[1] + bb[4]),
            0.5 * (bb[2] + bb[5]),
        ];
        let center_dist =
            (body_center[0].powi(2) + body_center[1].powi(2) + body_center[2].powi(2)).sqrt();
        if pos_dist > 50.0 && center_dist < 10.0 {
            origin_piled += 1;
        }
        checked_built_bodies += 1;
    }
    assert!(checked_built_bodies > 0, "{map}: expected built prop collision bodies");
    assert_eq!(origin_piled, 0, "{map}: found prop collision bodies piled at world origin");
    println!(
        "prop_collision_bounds {map}: verified {checked_built_bodies} built bodies (placed_slots={next_slot}, total_kernel_slots={}, moving_props={moving_body_count}, by_base={built_by_base:?}, 0 at origin)",
        body_bounds.len()
    );

    // Install collision order covering all kernel bodies and run exact-slot ray proofs.
    let order: Vec<u32> = (0..body_bounds.len() as u32).collect();
    context.collision_order(0, &order).unwrap();

    if map == "tat2" {
        let station_pb = battle
            .content
            .prop_bodies
            .iter()
            .find(|pb| {
                pb.built.is_some() && admitted_map.admission.placed[pb.instance].base == "powerupstation"
            })
            .expect("tat2: expected powerupstation prop body");
        let station_slot = slot_by_inst_look[&(station_pb.instance, 0)];
        let (code, dist, pt) = cast_prop_surface_ray(
            &context,
            &flags,
            station_pb,
            &admitted_map.admission.placed[station_pb.instance],
            station_slot,
            "tat2 powerupstation",
        );
        assert_eq!(code, station_slot as i32);
        println!(
            "ray_proof tat2 powerupstation (inst {}, slot {station_slot}): code={code} dist={dist:.2} hit=[{:.1},{:.1},{:.1}]",
            station_pb.instance, pt[0], pt[1], pt[2]
        );
    }

    if map == "spa6" {
        // 1. Capital ship hull ray: largest destructablebuilding body (extent > 400 m).
        let (hull_pb, hull_slot, hull_extent) = battle
            .content
            .prop_bodies
            .iter()
            .filter_map(|pb| {
                pb.built.as_ref()?;
                let placed_obj = &admitted_map.admission.placed[pb.instance];
                if placed_obj.base != "destructablebuilding" || pb.world_animated {
                    return None;
                }
                let slot = slot_by_inst_look.get(&(pb.instance, 0)).copied()?;
                let bb = bounds_by_slot.get(&slot)?.1;
                let ext = ((bb[3] - bb[0]).powi(2) + (bb[4] - bb[1]).powi(2) + (bb[5] - bb[2]).powi(2)).sqrt();
                Some((pb, slot, ext))
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .expect("spa6: expected capital ship hull destructablebuilding body");
        assert!(
            hull_extent > 400.0,
            "spa6: expected largest capital ship hull body to exceed 400 m, got {hull_extent:.1} m"
        );
        let (code, dist, pt) = cast_prop_surface_ray(
            &context,
            &flags,
            hull_pb,
            &admitted_map.admission.placed[hull_pb.instance],
            hull_slot,
            "spa6 capital_ship_hull",
        );
        assert_eq!(code, hull_slot as i32);
        println!(
            "ray_proof spa6 capital_ship_hull (model={:?}, inst {}, slot {hull_slot}, extent={hull_extent:.1}m): code={code} dist={dist:.2} hit=[{:.1},{:.1},{:.1}]",
            admitted_map.admission.placed[hull_pb.instance].model,
            hull_pb.instance,
            pt[0],
            pt[1],
            pt[2]
        );

        // 2. Turret ray: armedbuilding (preferring com_bldg_chaingun_roof).
        let turret_pb = battle
            .content
            .prop_bodies
            .iter()
            .find(|pb| {
                pb.built.is_some()
                    && admitted_map.admission.placed[pb.instance].base == "armedbuilding"
                    && admitted_map.admission.placed[pb.instance]
                        .model
                        .as_deref()
                        .is_some_and(|m| m.eq_ignore_ascii_case("com_bldg_chaingun_roof"))
            })
            .or_else(|| {
                battle.content.prop_bodies.iter().find(|pb| {
                    pb.built.is_some()
                        && admitted_map.admission.placed[pb.instance].base == "armedbuilding"
                })
            })
            .expect("spa6: expected turret (armedbuilding) collision body");
        let turret_slot = slot_by_inst_look[&(turret_pb.instance, 0)];
        let (code, dist, pt) = cast_prop_surface_ray(
            &context,
            &flags,
            turret_pb,
            &admitted_map.admission.placed[turret_pb.instance],
            turret_slot,
            "spa6 turret",
        );
        assert_eq!(code, turret_slot as i32);
        println!(
            "ray_proof spa6 turret (name='{}', model={:?}, inst {}, slot {turret_slot}): code={code} dist={dist:.2} hit=[{:.1},{:.1},{:.1}]",
            admitted_map.admission.placed[turret_pb.instance].name,
            admitted_map.admission.placed[turret_pb.instance].model,
            turret_pb.instance,
            pt[0],
            pt[1],
            pt[2]
        );

        // 3. Door ray: exact-slot hit on a door collision body.
        let door_pb = battle
            .content
            .prop_bodies
            .iter()
            .find(|pb| pb.built.is_some() && admitted_map.admission.placed[pb.instance].base == "door")
            .expect("spa6: expected door collision body");
        let door_slot = slot_by_inst_look[&(door_pb.instance, 0)];
        let (code, dist, pt) = cast_prop_surface_ray(
            &context,
            &flags,
            door_pb,
            &admitted_map.admission.placed[door_pb.instance],
            door_slot,
            "spa6 door",
        );
        assert_eq!(code, door_slot as i32);
        println!(
            "ray_proof spa6 door (name='{}', model={:?}, inst {}, slot {door_slot}): code={code} dist={dist:.2} hit=[{:.1},{:.1},{:.1}]",
            admitted_map.admission.placed[door_pb.instance].name,
            admitted_map.admission.placed[door_pb.instance].model,
            door_pb.instance,
            pt[0],
            pt[1],
            pt[2]
        );

        // 4. Orbiting moving frigate ray: rep_frig3 or cis_frig3 admitted as a single moving body.
        let frigate_pb = battle
            .content
            .prop_bodies
            .iter()
            .find(|pb| {
                pb.built.is_some()
                    && pb.world_animated
                    && matches!(
                        admitted_map.admission.placed[pb.instance].name.to_ascii_lowercase().as_str(),
                        "rep_frig3" | "cis_frig3"
                    )
            })
            .expect("spa6: expected world-animated frigate (rep_frig3 or cis_frig3) body");
        let frigate_slot = slot_by_inst_look[&(frigate_pb.instance, 0)];
        assert!(
            bounds_by_slot[&frigate_slot].0,
            "spa6: frigate body slot {frigate_slot} must be marked moving=true"
        );
        let (code, dist, pt) = cast_prop_surface_ray(
            &context,
            &flags,
            frigate_pb,
            &admitted_map.admission.placed[frigate_pb.instance],
            frigate_slot,
            "spa6 orbiting_frigate",
        );
        assert_eq!(code, frigate_slot as i32);
        println!(
            "ray_proof spa6 orbiting_frigate (name='{}', inst {}, slot {frigate_slot}, moving=true): code={code} dist={dist:.2} hit=[{:.1},{:.1},{:.1}]",
            admitted_map.admission.placed[frigate_pb.instance].name,
            frigate_pb.instance,
            pt[0],
            pt[1],
            pt[2]
        );

        // 5. Hangar deck ray beneath first spawn point: MUST hit geometry, NEVER MISS (-2).
        let first_spawn_arr = admitted_map
            .admission
            .spawns
            .as_object()
            .and_then(|o| o.values().next())
            .and_then(bfe_content::Value::as_array)
            .and_then(|pts| pts.first())
            .and_then(bfe_content::Value::as_array)
            .expect("spa6: expected at least one spawn point");
        let first_spawn = [
            first_spawn_arr[0].as_f64().unwrap(),
            first_spawn_arr[1].as_f64().unwrap(),
            first_spawn_arr[2].as_f64().unwrap(),
        ];
        let deck_res = context
            .collision_query(
                CollisionQuery::Cast {
                    world: 0,
                    ray: Ray {
                        ox: first_spawn[0],
                        oy: first_spawn[1] + 2.0,
                        oz: first_spawn[2],
                        dx: 0.0,
                        dy: -1.0,
                        dz: 0.0,
                    },
                    far: 30.0,
                    hypot_xz: 0.0,
                    ignore: -1,
                    static_only: false,
                    moving_only: false,
                },
                &flags,
            )
            .unwrap();
        assert_ne!(
            deck_res.code, MISS,
            "spa6 hangar_floor beneath spawn {first_spawn:?}: ray missed (code=MISS)"
        );
        assert!(
            deck_res.code == STATIC || deck_res.code >= 0,
            "spa6 hangar_floor beneath spawn {first_spawn:?}: unexpected hit code {}",
            deck_res.code
        );
        let deck_dist = deck_res.io[10];
        let deck_pt = [deck_res.io[11], deck_res.io[12], deck_res.io[13]];
        assert!(
            deck_dist.is_finite() && deck_dist > 0.0 && deck_dist < 15.0,
            "spa6 hangar_floor: invalid hit distance {deck_dist}"
        );
        println!(
            "ray_proof spa6 hangar_floor (spawn=[{:.1},{:.1},{:.1}]): code={} dist={deck_dist:.2} hit=[{:.1},{:.1},{:.1}]",
            first_spawn[0],
            first_spawn[1],
            first_spawn[2],
            deck_res.code,
            deck_pt[0],
            deck_pt[1],
            deck_pt[2]
        );
    }
    context.dispose().unwrap();
}

fn verify_swbf2_fingerprint(
    root: &Path,
    exp: &TargetExpectations,
    admitted_map: &bfe_content_native::map::AdmittedMap,
    battle: &bfe_content::battle::Battle,
) {
    let dir = DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf2);
    let query = format!("?map={}&mission={}&side={}", exp.map, exp.mission, exp.side);
    let h1 = fingerprint::hash_for_family(&dir, GameFamily::Swbf2, &query)
        .unwrap_or_else(|e| panic!("{}: fingerprint::hash_for_family failed: {e}", exp.map));
    let h2 = fingerprint::hash_for_family(&dir, GameFamily::Swbf2, &query).unwrap();
    assert_eq!(h1, h2, "{}: 2005 fingerprint must be deterministic", exp.map);
    assert!(
        h1.starts_with("bfe-content-sha256-"),
        "{}: unexpected fingerprint format '{h1}'",
        exp.map
    );

    // 2005 fingerprint must differ from 2004 fingerprint on the same tree.
    if let Ok(h2004) = fingerprint::hash(&dir, &query) {
        assert_ne!(
            h1, h2004,
            "{}: 2005 family fingerprint must differ from 2004 fingerprint",
            exp.map
        );
    }

    // Mutating sides/{side}/anim_banks.json must change the 2005 fingerprint.
    let anim_rel = format!("sides/{}/anim_banks.json", exp.side);
    let raw_anim = dir
        .read(&anim_rel)
        .unwrap_or_else(|| panic!("{}: missing {anim_rel}", exp.map));
    let mut anim_json: serde_json::Value = serde_json::from_slice(&raw_anim).unwrap();
    if let Some(obj) = anim_json.as_object_mut() {
        obj.insert("fingerprintProbe".into(), serde_json::json!(1));
    }
    let mut ov = HashMap::new();
    ov.insert(
        anim_rel.clone(),
        Rc::<[u8]>::from(serde_json::to_vec(&anim_json).unwrap()),
    );
    let mutated = OverlayFiles { base: &dir, overrides: ov };
    let h_mut = fingerprint::hash_for_family(&mutated, GameFamily::Swbf2, &query)
        .unwrap_or_else(|e| panic!("{}: mutated fingerprint failed: {e}", exp.map));
    assert_ne!(
        h1, h_mut,
        "{}: mutating {anim_rel} must change 2005 content fingerprint",
        exp.map
    );

    let mesh_hash = |path: &str, stride: usize, uv: bool| {
        let mut bytes = dir.read(path).unwrap().to_vec();
        let mut rows = bytes.chunks_exact_mut(stride);
        for row in &mut rows {
            let fields = if uv { 6..7 } else { 0..3 };
            for i in fields {
                let word = &mut row[4 * i..4 * i + 4];
                let value = f32::from_le_bytes((&*word).try_into().unwrap());
                word.copy_from_slice(&(if uv { value + 0.25 } else { value * 2.0 }).to_le_bytes());
            }
        }
        assert!(rows.into_remainder().is_empty());
        let overrides = HashMap::from([(path.to_string(), Rc::from(bytes))]);
        fingerprint::hash_for_family(&OverlayFiles { base: &dir, overrides }, GameFamily::Swbf2, &query).unwrap()
    };
    let props = format!("maps/{}/props.bin", exp.map);
    assert_ne!(h1, mesh_hash(&props, 32, false), "{} prop extents", exp.map);
    let actor = battle.soldier_def.get("actor").and_then(bfe_content::Value::as_str).unwrap();
    let actor = actor.strip_suffix(".json").unwrap_or(actor);
    for (path, stride) in [
        (format!("sides/{}/{actor}.bin", battle.side), 36),
        (format!("sides/{}/weapons.bin", battle.side), 32),
    ] {
        assert_eq!(h1, mesh_hash(&path, stride, true), "{path} texture coordinates");
        assert_ne!(h1, mesh_hash(&path, stride, false), "{path} gameplay extents");
    }
    {
        let mut ov_ai = HashMap::new();
        ov_ai.insert(
            format!("sides/{}/ai.json", exp.side),
            Rc::<[u8]>::from(br#"{"fingerprintProbe":1}"#.as_slice()),
        );
        let mutated_ai = OverlayFiles { base: &dir, overrides: ov_ai };
        let h_ai = fingerprint::hash_for_family(&mutated_ai, GameFamily::Swbf2, &query)
            .unwrap_or_else(|e| panic!("{}: ai.json fingerprint failed: {e}", exp.map));
        assert_ne!(
            h1, h_ai,
            "{}: adding sides/{}/ai.json must change 2005 content fingerprint",
            exp.map, exp.side
        );
    }

    // On trees carrying mission scripts (v5/v6 layout), mutating a compiled
    // mission script body or its manifest must change the 2005 fingerprint.
    let script_rel = admitted_map
        .admission
        .map_data
        .get("missions")
        .and_then(|m| m.get(exp.mission))
        .and_then(|m| m.get("scripts"))
        .and_then(bfe_content::Value::as_array)
        .and_then(|arr| {
            arr.iter().find_map(|rec| {
                rec.as_str()
                    .or_else(|| rec.get("file").and_then(bfe_content::Value::as_str))
            })
        })
        .map(|s| s.trim_start_matches('/').to_string());
    let mut script_checked = false;
    if let Some(rel) = script_rel {
        let bytes = dir
            .read(&rel)
            .unwrap_or_else(|| panic!("{}: declared mission script '{rel}' missing", exp.map));
        let mut altered = bytes.to_vec();
        if let Some(last) = altered.last_mut() {
            *last ^= 0xff;
        } else {
            altered.push(0x42);
        }
        let mut ov_script = HashMap::new();
        ov_script.insert(rel.clone(), Rc::<[u8]>::from(altered));
        let mutated_script = OverlayFiles { base: &dir, overrides: ov_script };
        let h_script = fingerprint::hash_for_family(&mutated_script, GameFamily::Swbf2, &query)
            .unwrap_or_else(|e| panic!("{}: script-mutated fingerprint failed: {e}", exp.map));
        assert_ne!(
            h1, h_script,
            "{}: mutating mission script '{rel}' must change 2005 content fingerprint",
            exp.map
        );
        script_checked = true;
    }
    println!(
        "fingerprint_2005 {}: hash={h1} anim_mutation_changed=true script_mutation_checked={script_checked}",
        exp.map
    );
}

fn verify_hero_animation_slots(
    battle: &bfe_content_native::BattleContent,
    defs: &bfe_content::defs::Defs,
) {
    // 1. Grievous binds only `grevious` with 0% unbound tracks and fills all
    //    core locomotion / air / dive / thrown slots from `grevious_sabre_*`.
    let (_, g_stats) = battle
        .content
        .unit_anim_stats
        .iter()
        .find(|(k, _)| k == "cis/cis_hero_grievous")
        .expect("tat2: missing unit_anim_stats for cis/cis_hero_grievous");
    assert_eq!(
        g_stats.banks.len(),
        1,
        "tat2: Grievous should bind only its own bank, got {:?}",
        g_stats.banks
    );
    assert!(
        g_stats.banks[0].eq_ignore_ascii_case("grevious"),
        "tat2: expected Grievous bank 'grevious', got {:?}",
        g_stats.banks
    );
    assert_eq!(
        g_stats.unbound_share(),
        0.0,
        "tat2: Grievous must have 0% unbound animation tracks"
    );
    let (_, g_clips) = battle
        .content
        .unit_clips
        .iter()
        .find(|(k, _)| k == "cis/cis_hero_grievous")
        .expect("tat2: missing unit_clips for cis/cis_hero_grievous");
    let (_, g_input) = battle
        .units
        .iter()
        .find(|(k, _)| k == "cis/cis_hero_grievous")
        .expect("tat2: missing presentation unit for cis/cis_hero_grievous");
    let g_slots: [(&str, Option<usize>); 12] = [
        ("stand_idle", g_input.tables.sets[0][0]),
        ("stand_walk", g_input.tables.sets[0][1]),
        ("stand_run", g_input.tables.sets[0][2]),
        ("stand_walkBack", g_input.tables.sets[0][3]),
        ("crouch_idle", g_input.tables.sets[1][0]),
        ("alert_idle", g_input.tables.alert[0]),
        ("jump", g_input.tables.jump),
        ("fall", g_input.tables.fall),
        ("land_soft", g_input.tables.land_soft),
        ("land_hard", g_input.tables.land_hard),
        ("dive", g_input.tables.dive),
        ("thrown_front_flying", g_input.tables.thrown[0][0]),
    ];
    for (slot_name, idx_opt) in g_slots {
        let idx = idx_opt.unwrap_or_else(|| {
            panic!("tat2: Grievous animation slot '{slot_name}' was None")
        });
        let clip_name = &g_clips.list[idx].name;
        assert!(
            clip_name.starts_with("grevious_sabre_"),
            "tat2: Grievous slot '{slot_name}' resolved to '{clip_name}', expected grevious_sabre_*"
        );
    }
    let g_def = defs
        .classes
        .iter()
        .find(|c| defs.names.text(c.name) == "cis_hero_grievous")
        .expect("tat2: missing Defs class for cis_hero_grievous");
    assert!(
        g_def.dive_time > 0.0,
        "tat2: Grievous Defs.dive_time must be > 0, got {}",
        g_def.dive_time
    );

    // 2. Luke Jedi binds `human_sabre` (not another hero bank like `vader`)
    //    and resolves stand_runforward & jump to `human_sabre_*`.
    let (_, l_stats) = battle
        .content
        .unit_anim_stats
        .iter()
        .find(|(k, _)| k == "all/all_hero_luke_jedi")
        .expect("tat2: missing unit_anim_stats for all/all_hero_luke_jedi");
    assert!(
        l_stats.banks.iter().any(|b| b.eq_ignore_ascii_case("human_sabre")),
        "tat2: Luke should bind human_sabre, got {:?}",
        l_stats.banks
    );
    assert!(
        !l_stats.banks.iter().any(|b| b.eq_ignore_ascii_case("vader")),
        "tat2: Luke must not bind vader bank, got {:?}",
        l_stats.banks
    );
    let (_, l_clips) = battle
        .content
        .unit_clips
        .iter()
        .find(|(k, _)| k == "all/all_hero_luke_jedi")
        .expect("tat2: missing unit_clips for all/all_hero_luke_jedi");
    let (_, l_input) = battle
        .units
        .iter()
        .find(|(k, _)| k == "all/all_hero_luke_jedi")
        .expect("tat2: missing presentation unit for all/all_hero_luke_jedi");
    for (slot_name, idx_opt) in [
        ("stand_run", l_input.tables.sets[0][2]),
        ("jump", l_input.tables.jump),
    ] {
        let idx = idx_opt.unwrap_or_else(|| panic!("tat2: Luke slot '{slot_name}' was None"));
        let clip_name = &l_clips.list[idx].name;
        assert!(
            clip_name.starts_with("human_sabre_"),
            "tat2: Luke slot '{slot_name}' resolved to '{clip_name}', expected human_sabre_*"
        );
    }

    // 3. Saber heroes across tat2 resolve `stand_runforward` to `*_sabre_*`
    //    instead of falling back to `human_rifle_*`.
    let mut saber_run_heroes = 0usize;
    for (unit_key, clips) in &battle.content.unit_clips {
        let Some((_, input)) = battle.units.iter().find(|(k, _)| k == unit_key) else {
            continue;
        };
        let Some(run_idx) = input.tables.sets[0][2] else { continue };
        if clips.list[run_idx].name.contains("_sabre_") {
            saber_run_heroes += 1;
        }
    }
    assert!(
        saber_run_heroes >= 10,
        "tat2: expected >= 10 saber heroes resolving stand_runforward to *_sabre_*, got {saber_run_heroes}"
    );
    println!(
        "hero_anim_slots tat2: grievous_slots=12/12_grevious_sabre (dive_time={:.3}s) luke_banks={:?} saber_run_heroes={saber_run_heroes}",
        g_def.dive_time, l_stats.banks
    );
}

struct TargetExpectations {
    map: &'static str,
    mission: &'static str,
    side: &'static str,
    expect_terrain: bool,
    world_meshes: usize,
    collision_tris: usize,
    terrain_tris: usize,
    command_posts: usize,
    spawn_paths: usize,
    spawn_points: usize,
    paths: usize,
    lights: usize,
    regions: usize,
    gameplay_regions: usize,
    auxiliary_regions: usize,
    placed_total: usize,
    placed_base: usize,
    placed_layer: usize,
    placed_by_base: &'static [(&'static str, usize)],
    kernel_prop_bodies: usize,
    expected_placed_batches: usize,
    expected_skinned_stream_instances: usize,
    expected_doors: usize,
    expected_turrets: usize,
    expected_moving_min: usize,
    expected_moving_max: usize,
    expected_explosions_total: usize,
    expected_explosions_positive: usize,
    min_living: usize,
    min_moved_m: f64,
}

fn verify_opening_target(root: &Path, exp: &TargetExpectations) -> RunSummary {
    let loader = Loader::detect(root);

    // 1. Typed map admission without a roster (shared by inspector, client, server).
    let admitted_map = loader
        .admit_map(GameFamily::Swbf2, exp.map, Some(exp.mission), &[])
        .unwrap_or_else(|e| panic!("admit_map({}, {}) failed: {e}", exp.map, exp.mission));
    let counts = admitted_map.counts();
    assert_eq!(admitted_map.admission.collision.has_terrain, exp.expect_terrain);
    assert_eq!(admitted_map.terrain_bvh.is_some(), exp.expect_terrain);
    assert_eq!(counts.world_meshes, exp.world_meshes, "{}: world_meshes", exp.map);
    assert_eq!(counts.collision_triangles, exp.collision_tris, "{}: collision_tris", exp.map);
    assert_eq!(counts.terrain_triangles, exp.terrain_tris, "{}: terrain_tris", exp.map);
    assert_eq!(counts.command_posts, exp.command_posts, "{}: command_posts", exp.map);
    assert_eq!(counts.spawn_paths, exp.spawn_paths, "{}: spawn_paths", exp.map);
    assert_eq!(counts.spawn_points, exp.spawn_points, "{}: spawn_points", exp.map);
    assert_eq!(counts.paths, exp.paths, "{}: paths", exp.map);
    assert_eq!(counts.lights, exp.lights, "{}: lights", exp.map);
    assert_eq!(counts.regions, exp.regions, "{}: regions", exp.map);
    assert_eq!(counts.gameplay_regions, exp.gameplay_regions, "{}: gameplay_regions", exp.map);
    assert_eq!(counts.auxiliary_regions, exp.auxiliary_regions, "{}: auxiliary_regions", exp.map);
    assert_eq!(counts.placed_total, exp.placed_total, "{}: placed_total", exp.map);
    let actual_base_placed = admitted_map
        .admission
        .placed
        .iter()
        .filter(|p| p.layer.is_none())
        .count();
    let actual_layer_placed = admitted_map
        .admission
        .placed
        .iter()
        .filter(|p| p.layer.is_some())
        .count();
    assert_eq!(actual_base_placed, exp.placed_base, "{}: placed_base", exp.map);
    assert_eq!(actual_layer_placed, exp.placed_layer, "{}: placed_layer", exp.map);
    let expected_by_base: BTreeMap<String, usize> = exp
        .placed_by_base
        .iter()
        .map(|&(k, v)| (k.to_string(), v))
        .collect();
    assert_eq!(
        counts.placed_by_base, expected_by_base,
        "{}: placed_by_base exact mismatch",
        exp.map
    );
    assert!(counts.prop_skeletons > 0, "{}: expected prop skeletons from props.json", exp.map);
    assert!(
        admitted_map.admission.bounds.is_some(),
        "{}: expected map bounds on MapAdmission",
        exp.map
    );

    // Verify 2005 material fields match `material.{blendMode,alphaCutout,doubleSided}` exactly.
    let mut mat_mismatches = 0usize;
    for mesh in admitted_map.world.iter().chain(admitted_map.prop_models.iter()) {
        if let Some(mat) = mesh.material.record.get("material").and_then(serde_json::Value::as_object) {
            if let Some(ds) = mat.get("doubleSided").and_then(serde_json::Value::as_bool) {
                if mesh.material.double_sided != ds {
                    mat_mismatches += 1;
                }
            }
            if let Some(bm) = mat.get("blendMode").and_then(serde_json::Value::as_str) {
                let want = match bm {
                    "alpha" => Blend::Alpha,
                    "additive" => Blend::Additive,
                    _ => Blend::Opaque,
                };
                if mesh.material.blend != want {
                    mat_mismatches += 1;
                }
            }
            if let Some(ac) = mat.get("alphaCutout") {
                let want = ac.as_f64().is_some_and(|v| v > 0.0);
                if mesh.material.cutout != want {
                    mat_mismatches += 1;
                }
            }
        }
    }
    assert_eq!(mat_mismatches, 0, "{}: material fields diverged from authored material records", exp.map);

    // Verify 2005 scrolling UV speeds are normalized image repeats per second in [0, 1].
    let mut scroll_meshes = 0usize;
    for mesh in admitted_map.world.iter().chain(admitted_map.prop_models.iter()) {
        if let Some(arr) = mesh.material.record.get("scroll").and_then(serde_json::Value::as_array) {
            assert_eq!(arr.len(), 2, "{}: scroll must be a 2-element [U, V] vector", exp.map);
            for c in arr {
                let v = c.as_f64().expect("finite scroll component");
                assert!(
                    v.is_finite() && (0.0..=1.0).contains(&v),
                    "{}: unexpected scroll speed {v} outside [0, 1] repeats/s",
                    exp.map
                );
            }
            scroll_meshes += 1;
        }
    }
    assert!(scroll_meshes > 0, "{}: expected scrolling meshes", exp.map);
    if exp.map == "tat2" {
        assert_eq!(
            admitted_map
                .admission
                .map_data
                .get("layerIndex")
                .and_then(|idx| idx.get("fb419a4e"))
                .and_then(bfe_content::Value::as_str),
            Some("tat2_hero1flag"),
            "tat2 layerIndex should resolve fb419a4e to tat2_hero1flag"
        );
    }

    let map_placed = placed::admit_map(&admitted_map)
        .unwrap_or_else(|e| panic!("placed::admit_map({}, {}) failed: {e}", exp.map, exp.mission));
    assert_eq!(map_placed.deferred, 0, "{}: unexpected deferred props in admit_map", exp.map);
    assert_eq!(
        map_placed.parts.len(),
        exp.expected_placed_batches,
        "{}: exact placed batch count mismatch",
        exp.map
    );

    // Verify drawn-vs-raw vertex alignment, skinned streams, and turret attachments.
    verify_drawn_rigged_alignment(exp, &admitted_map, &map_placed);

    // Verify 2005 family-aware content fingerprint.

    // 2. Full battle content admission (`Battle::load_for_family` -> `Content::build` -> `Defs::build`).
    let query = format!("?mission={}&side={}", exp.mission, exp.side);
    let battle = loader
        .load_for_family(GameFamily::Swbf2, exp.map, &query)
        .unwrap_or_else(|e| panic!("load_for_family({}, {query}) failed: {e}", exp.map));
    verify_swbf2_fingerprint(root, exp, &admitted_map, &battle.content.battle);
    assert_eq!(battle.world.len(), admitted_map.world.len());
    let defs = battle
        .defs()
        .unwrap_or_else(|e| panic!("Defs::build({}, {}) failed: {e}", exp.map, exp.mission));
    assert!(!defs.classes.is_empty(), "{}: expected built class defs", exp.map);

    // Verify stand-in teams are marked stand-in and balanced with equal team sizes.
    let mission_val = battle
        .content
        .battle
        .world
        .get("mission")
        .expect("synthesized stand-in mission");
    assert!(
        matches!(mission_val.get("standIn"), Some(bfe_content::Value::Bool(true))),
        "{}: stand-in mission must declare standIn=true",
        exp.map
    );
    assert!(
        matches!(mission_val.get("synthesized"), Some(bfe_content::Value::Bool(true))),
        "{}: stand-in mission must declare synthesized=true",
        exp.map
    );
    let t1_units = mission_val
        .get("teams")
        .and_then(|t| t.get("1"))
        .and_then(|t| t.get("units"))
        .and_then(bfe_content::Value::as_f64)
        .unwrap_or(0.0);
    let t2_units = mission_val
        .get("teams")
        .and_then(|t| t.get("2"))
        .and_then(|t| t.get("units"))
        .and_then(bfe_content::Value::as_f64)
        .unwrap_or(0.0);
    assert!(t1_units > 0.0, "{}: stand-in team 1 must have units", exp.map);
    assert_eq!(
        t1_units, t2_units,
        "{}: stand-in teams must have equal unit counts ({t1_units} != {t2_units})",
        exp.map
    );

    let battle_placed = placed::admit(&battle)
        .unwrap_or_else(|e| panic!("placed::admit({}, {}) failed: {e}", exp.map, exp.mission));
    assert_eq!(battle_placed.parts.len(), map_placed.parts.len());

    // Verify destroyed look lookup on every placed object with a destroyed model.
    let mut destroyed_looks = 0usize;
    if let Some(instances) = battle
        .content
        .battle
        .props
        .as_ref()
        .and_then(|p| p.get("instances"))
        .and_then(bfe_content::Value::as_array)
    {
        for (idx, inst) in instances.iter().enumerate() {
            if inst
                .get("destroyed")
                .and_then(|d| d.get("model"))
                .and_then(bfe_content::Value::as_str)
                .is_some()
            {
                let wreck = placed::admit_look(&battle, idx, 1)
                    .unwrap_or_else(|e| panic!("admit_look destroyed {}[{idx}] failed: {e}", exp.map));
                assert!(!wreck.parts.is_empty(), "{}[{idx}] destroyed look had 0 parts", exp.map);
                destroyed_looks += 1;
            }
        }
    }

    // Verify world animations move animated props when present.
    let mut animated_moved = 0usize;
    if counts.world_anim_clips > 0 {
        let moved = placed::admit_map_at(&admitted_map, 1.5)
            .unwrap_or_else(|e| panic!("admit_map_at({}) failed: {e}", exp.map));
        assert_eq!(moved.parts.len(), map_placed.parts.len());
        for (a, b) in map_placed.parts.iter().zip(moved.parts.iter()) {
            if a.matrix != b.matrix {
                animated_moved += 1;
            }
        }
        assert!(animated_moved > 0, "{}: expected world animations to move placed parts", exp.map);
    }

    // 3. Verify world-space prop collision bounds, exact-slot rays, explosions,
    //    host content tables, and network admission.
    verify_world_space_prop_collision(root, exp, &battle, &admitted_map);

    // Material census (alpha / cutout / double-sided / bump).
    let alpha_world = battle.world.iter().filter(|m| m.material.blend == Blend::Alpha).count();
    let cutout_world = battle.world.iter().filter(|m| m.material.cutout).count();
    let double_sided_world = battle.world.iter().filter(|m| m.material.double_sided).count();
    let bump_props = battle.models.iter().filter(|m| m.material.bump.is_some()).count();

    // Roster + animation binding census.
    let swbf2 = battle.content.battle.swbf2.as_ref().expect("Swbf2 battle payload");
    let report = &swbf2.roster;
    let partial_count = report.partial_count();
    assert!(
        report.infantry_baseline_count() + partial_count > 0,
        "{}: expected infantry-baseline or partial classes",
        exp.map
    );

    println!("=== SWBF2 OPENING TARGET: {} / {} ===", exp.map, exp.mission);
    println!(
        "map_counts: layers={:?} world_meshes={} (alpha={} cutout={} double_sided={}) collision_tris={} terrain_tris={} kill_y={:.1} bounds={:?}",
        admitted_map.admission.active_layers,
        counts.world_meshes,
        alpha_world,
        cutout_world,
        double_sided_world,
        counts.collision_triangles,
        counts.terrain_triangles,
        admitted_map.admission.collision.kill_y,
        admitted_map.admission.bounds,
    );
    println!(
        "placed: total={} (base={} layer={}) visible={} logic_only={} batches={} deferred={} destroyed_looks_verified={} prop_bump_meshes={} skeletons={} (multi_bone={}) attributes_bytes={} by_base={:?}",
        counts.placed_total,
        actual_base_placed,
        actual_layer_placed,
        counts.placed_visible,
        counts.placed_logic_only,
        map_placed.parts.len(),
        map_placed.deferred,
        destroyed_looks,
        bump_props,
        counts.prop_skeletons,
        counts.prop_multi_bone_skeletons,
        admitted_map.admission.props_attributes_buf.as_ref().map_or(0, |b| b.len()),
        counts.placed_by_base,
    );
    println!(
        "world_entities: lights={} regions={} (gameplay={} auxiliary={}) death_regions={} paths={} spawn_paths={} spawn_points={} command_posts={}",
        counts.lights,
        counts.regions,
        counts.gameplay_regions,
        counts.auxiliary_regions,
        counts.death_regions,
        counts.paths,
        counts.spawn_paths,
        counts.spawn_points,
        counts.command_posts,
    );
    println!(
        "world_anims: clips={} groups={} hierarchies={} moved_batches_at_1.5s={} canonicalized_refs={}",
        counts.world_anim_clips,
        counts.world_anim_groups,
        counts.world_anim_hierarchies,
        animated_moved,
        admitted_map.admission.canonicalized_refs.len(),
    );
    println!(
        "roster: total_classes={} infantry_baseline={} partial={} not_yet_simulable={} non_unit_packages={:?}",
        report.classes.len(),
        report.infantry_baseline_count(),
        partial_count,
        report.not_yet_simulable_classes().len(),
        report.non_unit_packages,
    );
    for entry in &report.classes {
        let status = match &entry.support {
            SimulationSupport::InfantryBaseline => "InfantryBaseline".to_string(),
            SimulationSupport::Partial { reasons } => format!("Partial({})", reasons.join(", ")),
            SimulationSupport::NotYetSimulable { reasons } => {
                format!("NotYetSimulable({})", reasons.join(", "))
            }
        };
        println!(
            "  class {}/{} base={} actor={} weapons={} banks={:?} support={}",
            entry.side, entry.class, entry.base, entry.has_actor, entry.weapons, entry.anim_banks, status
        );
    }
    let total_matched_tracks: usize = battle
        .content
        .unit_anim_stats
        .iter()
        .map(|(_, s)| s.matched_tracks)
        .sum();
    let total_source_tracks: usize = battle
        .content
        .unit_anim_stats
        .iter()
        .map(|(_, s)| s.total_tracks())
        .sum();
    println!(
        "anim_crc: unit_sets={} matched_tracks={}/{} ({:.2}% unbound) actors={} presentation_units={}",
        battle.content.unit_anim_stats.len(),
        total_matched_tracks,
        total_source_tracks,
        if total_source_tracks == 0 {
            0.0
        } else {
            100.0 * (total_source_tracks - total_matched_tracks) as f64 / total_source_tracks as f64
        },
        battle.actors.len(),
        battle.units.len(),
    );
    for (unit_key, stats) in &battle.content.unit_anim_stats {
        println!(
            "  anim {unit_key}: banks={:?} clips={} matched_tracks={}/{} unbound={:.2}%",
            stats.banks,
            stats.clips,
            stats.matched_tracks,
            stats.total_tracks(),
            stats.unbound_share() * 100.0,
        );
        assert!(
            stats.unbound_share() < 0.10,
            "{}: unit {unit_key} has {:.2}% unbound animation tracks (banks={:?})",
            exp.map,
            stats.unbound_share() * 100.0,
            stats.banks
        );
    }

    // Verify Grievous, Luke, and saber hero animation slots plus v3 saberHilts & clipTiming on tat2.
    if exp.map == "tat2" {
        verify_hero_animation_slots(&battle, &defs);

        let mut saber_weapons = 0usize;
        let mut hilt_mounts = 0usize;
        let mut blades = 0usize;
        for side in &battle.content.battle.sides {
            for (_class_name, def) in side.units.as_object().unwrap().entries() {
                for w in def.get("weapons").and_then(bfe_content::Value::as_array).unwrap_or(&[]) {
                    if let Some(hilts) = w.get("saberHilts").and_then(bfe_content::Value::as_array) {
                        if !hilts.is_empty() {
                            saber_weapons += 1;
                            hilt_mounts += hilts.len();
                            for h in hilts {
                                assert!(h.get("model").and_then(bfe_content::Value::as_str).is_some());
                                assert!(h.get("attachment").is_some());
                                blades += h
                                    .get("blades")
                                    .and_then(bfe_content::Value::as_array)
                                    .map_or(0, <[bfe_content::Value]>::len);
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(saber_weapons, 12, "tat2: expected 12 equipped saber weapons with saberHilts");
        assert_eq!(hilt_mounts, 16, "tat2: expected 16 saber hilt mounts");
        assert_eq!(blades, 17, "tat2: expected 17 saber blades");

        let mut checked_clip_timings = 0usize;
        let mut frames_by_action: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        for side in &battle.content.battle.sides {
            for (class_name, def) in side.units.as_object().unwrap().entries() {
                let unit_key = format!("{}/{class_name}", side.name);
                for w in def.get("weapons").and_then(bfe_content::Value::as_array).unwrap_or(&[]) {
                    for cb in w
                        .get("comboBindings")
                        .and_then(bfe_content::Value::as_array)
                        .unwrap_or(&[])
                    {
                        let Some(timing) = cb.get("clipTiming").and_then(bfe_content::Value::as_object) else {
                            continue;
                        };
                        let (_, clips) = battle
                            .content
                            .unit_clips
                            .iter()
                            .find(|(k, _)| k == &unit_key)
                            .unwrap_or_else(|| panic!("missing unit_clips for {unit_key}"));
                        for (action, entry) in timing.entries() {
                            if entry.is_null() {
                                continue;
                            }
                            let want_frames = entry
                                .get("frames")
                                .and_then(bfe_content::Value::as_f64)
                                .unwrap_or_else(|| panic!("{unit_key}/{action}: missing frames"));
                            let want_fps = entry
                                .get("fps")
                                .and_then(bfe_content::Value::as_f64)
                                .unwrap_or(30.0);
                            let clip = clips.get(action).unwrap_or_else(|| {
                                panic!("{unit_key}: missing clipTiming action '{action}'")
                            });
                            assert_eq!(
                                clip.frames_f64(),
                                want_frames,
                                "{unit_key}/{action}: clip frame count mismatch"
                            );
                            let want_dur = (want_frames - 1.0).max(0.0) / want_fps;
                            assert!(
                                (clip.duration - want_dur).abs() < 1e-9,
                                "{unit_key}/{action}: duration {:.6} != expected {want_dur:.6}",
                                clip.duration
                            );
                            frames_by_action
                                .entry(action.to_string())
                                .or_default()
                                .insert(want_frames as u32);
                            checked_clip_timings += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(
            checked_clip_timings, 249,
            "tat2: expected all 249 hero clipTiming actions verified"
        );
        let max_distinct_lengths = frames_by_action
            .values()
            .map(BTreeSet::len)
            .max()
            .unwrap_or(0);
        assert!(
            max_distinct_lengths >= 4,
            "tat2: expected per-character clipTiming to preserve distinct frame counts per hero (max={max_distinct_lengths})"
        );
        println!(
            "swbf2_v3_contract tat2: saber_weapons={saber_weapons} hilt_mounts={hilt_mounts} blades={blades} clip_timing_actions={checked_clip_timings} max_distinct_per_action={max_distinct_lengths} scroll_meshes={scroll_meshes}"
        );
    }

    // 4. Headless simulation admission + 1,800 ticks without a human player, twice.
    let files = DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf2);
    let run1 = step_headless_world(&files, exp.map, exp.mission, exp.side, 1800)
        .unwrap_or_else(|e| panic!("headless run 1 failed for {}/{}: {e}", exp.map, exp.mission));
    let run2 = step_headless_world(&files, exp.map, exp.mission, exp.side, 1800)
        .unwrap_or_else(|e| panic!("headless run 2 failed for {}/{}: {e}", exp.map, exp.mission));
    println!(
        "headless_1800: {}/{} run1={:016x} (bytes={}, peak_living={}, moved={:.1}m, classes={:?}) run2={:016x}",
        exp.map,
        exp.mission,
        run1.digest,
        run1.pub_bytes,
        run1.peak_living,
        run1.total_distance_moved,
        run1.spawned_classes,
        run2.digest,
    );
    for line in &run1.census_lines {
        println!("  census {}: {line}", exp.map);
    }
    assert_eq!(run1.digest, run2.digest, "{}: 1800-tick digests diverged", exp.map);
    assert_eq!(run1.pub_bytes, run2.pub_bytes, "{}: publication byte counts diverged", exp.map);
    assert!(run1.pub_bytes > 0, "{}: expected non-empty publications", exp.map);
    assert!(
        run1.peak_living >= exp.min_living,
        "{}: expected at least {} living units during 1800 ticks, got {}",
        exp.map,
        exp.min_living,
        run1.peak_living
    );
    assert!(
        run1.total_distance_moved >= exp.min_moved_m,
        "{}: expected bots to move at least {:.1}m across 1800 ticks, got {:.1}m",
        exp.map,
        exp.min_moved_m,
        run1.total_distance_moved
    );
    run1
}

/// Proves the 1,800-tick digest is sensitive to real content changes under two
/// independent mutations: (1) deleting half the world's meshes and command
/// posts, and (2) altering a loaded class's movement and weapon definitions.
fn verify_mutation_sensitivity(root: &Path, baseline: &RunSummary) {
    let dir = DirFiles::from_resolved_root(root, bfe_family::GameFamily::Swbf2);

    // Mutation 1: delete half of `tat2`'s world meshes and command posts in `map.json`.
    let raw_map: serde_json::Value =
        serde_json::from_slice(&dir.read("maps/tat2/map.json").unwrap()).unwrap();
    let mut mut_map = raw_map.clone();
    if let Some(meshes) = mut_map.get_mut("meshes").and_then(serde_json::Value::as_array_mut) {
        let half = meshes.len() / 2;
        meshes.truncate(half);
    }
    if let Some(cps) = mut_map
        .get_mut("layers")
        .and_then(|l| l.get_mut("tat2_tdm"))
        .and_then(|l| l.get_mut("commandPosts"))
        .and_then(serde_json::Value::as_array_mut)
    {
        let half = (cps.len() / 2).max(2);
        cps.truncate(half);
    }
    let mut ov1 = HashMap::new();
    ov1.insert(
        "maps/tat2/map.json".to_string(),
        Rc::<[u8]>::from(serde_json::to_vec(&mut_map).unwrap()),
    );
    let mutated_world = OverlayFiles { base: &dir, overrides: ov1 };
    let m1 = step_headless_world(&mutated_world, "tat2", "tat2g_eli", "all", 1800)
        .expect("mutated-world run should still step");
    assert_ne!(
        m1.digest, baseline.digest,
        "mutation 1 (deleting half the world) must change the 1800-tick digest"
    );
    println!(
        "mutation_1_half_world: baseline={:016x} mutated={:016x} (changed as expected)",
        baseline.digest, m1.digest
    );

    // Mutation 2: swap two unit classes' definitions in `sides/all/units.json`.
    let raw_units: serde_json::Value =
        serde_json::from_slice(&dir.read("sides/all/units.json").unwrap()).unwrap();
    let mut mut_units = raw_units.clone();
    if let Some(units) = mut_units.get_mut("units").and_then(serde_json::Value::as_object_mut) {
        let keys: Vec<String> = units.keys().cloned().collect();
        assert!(keys.len() >= 2, "expected at least two classes in sides/all/units.json");
        let a = units.get(&keys[0]).unwrap().clone();
        let b = units.get(&keys[1]).unwrap().clone();
        units.insert(keys[0].clone(), b);
        units.insert(keys[1].clone(), a);
    }
    let mut ov2 = HashMap::new();
    ov2.insert(
        "sides/all/units.json".to_string(),
        Rc::<[u8]>::from(serde_json::to_vec(&mut_units).unwrap()),
    );
    let mutated_class = OverlayFiles { base: &dir, overrides: ov2 };
    let m2 = step_headless_world(&mutated_class, "tat2", "tat2g_eli", "all", 1800)
        .expect("mutated-class run should still step");
    assert_ne!(
        m2.digest, baseline.digest,
        "mutation 2 (swapping unit classes) must change the 1800-tick digest"
    );
    println!(
        "mutation_2_swap_class: baseline={:016x} mutated={:016x} (changed as expected)",
        baseline.digest, m2.digest
    );
}

#[test]
#[ignore]
fn swbf2_opening_targets_admit_and_step_deterministically() {
    let root = std::env::var("BFE_SWBF2_CONTENT")
        .expect("set BFE_SWBF2_CONTENT to the prepared 2005 content directory");
    let root = Path::new(&root);

    let tat2_baseline = verify_opening_target(
        root,
        &TargetExpectations {
            map: "tat2",
            mission: "tat2g_eli",
            side: "all",
            expect_terrain: true,
            world_meshes: 103,
            collision_tris: 56_392,
            terrain_tris: 8_192,
            command_posts: 7,
            spawn_paths: 7,
            spawn_points: 124,
            paths: 21,
            lights: 2,
            regions: 21,
            gameplay_regions: 16,
            auxiliary_regions: 5,
            placed_total: 27,
            placed_base: 21,
            placed_layer: 6,
            placed_by_base: &[("commandpost", 7), ("powerupstation", 20)],
            kernel_prop_bodies: 40,
            expected_placed_batches: 37,
            expected_skinned_stream_instances: 20,
            expected_doors: 0,
            expected_turrets: 0,
            expected_moving_min: 0,
            expected_moving_max: 0,
            expected_explosions_total: 29,
            expected_explosions_positive: 23,
            min_living: 10,
            min_moved_m: 100.0,
        },
    );
    verify_mutation_sensitivity(root, &tat2_baseline);

    let _spa6_baseline = verify_opening_target(
        root,
        &TargetExpectations {
            map: "spa6",
            mission: "spa6c_ass",
            side: "rep",
            expect_terrain: false,
            world_meshes: 60,
            collision_tris: 6_792,
            terrain_tris: 0,
            command_posts: 2,
            spawn_paths: 2,
            spawn_points: 24,
            paths: 59,
            lights: 4,
            regions: 32,
            gameplay_regions: 6,
            auxiliary_regions: 26,
            placed_total: 149,
            placed_base: 107,
            placed_layer: 42,
            placed_by_base: &[
                ("armedbuilding", 73),
                ("commandpost", 2),
                ("destructablebuilding", 26),
                ("door", 10),
                ("powerupstation", 12),
                ("prop", 2),
                ("remoteterminal", 12),
                ("vehiclespawn", 12),
            ],
            kernel_prop_bodies: 218,
            expected_placed_batches: 323,
            expected_skinned_stream_instances: 50,
            expected_doors: 10,
            expected_turrets: 73,
            expected_moving_min: 1,
            expected_moving_max: 218,
            expected_explosions_total: 16,
            expected_explosions_positive: 8,
            min_living: 2,
            min_moved_m: 10.0,
        },
    );
}


#[test]
#[ignore = "requires prepared Space Assault content"]
fn local_space_crew_are_admitted_before_each_soldier_gets_its_first_input() {
    let files = DirFiles::from_resolved_root(std::env::var("BFE_SWBF2_CONTENT").unwrap(), bfe_family::GameFamily::Swbf2);
    let options = Options {
        family: GameFamily::Swbf2,
        map: "spa6".into(),
        mode: "ass".into(),
        side: "rep".into(),
        bots: Bots { fielded: false, ..Default::default() },
        ..Default::default()
    };
    let ctx = Context::new();
    let mission = bfe_sim::world::admit::mission::MissionSelection {
        family: options.family, map: options.map.clone(), era: options.era().into(),
        mode: options.mode.clone(), seed: options.seed,
    };
    let prefs = bfe_sim::world::admit::mission::MissionPreferences {
        bots_per_team: Some(0), direct_spawn: true, ..Default::default()
    };
    let (admitted, journal) = ctx.world_mission_content(&files, &options, &mission, prefs).unwrap();
    assert_eq!(journal.selection, mission);
    ctx.world_step_init(admitted.setup.clone()).unwrap();
    let pads = ctx.owned(|w| {
        assert!(w.connect_space_pilot(admitted.handles.player, admitted.handles.mailbox));
        w.observe_local_space();
        assert!(w.start_local_space_developer(admitted.handles.player, false, false, true));
        assert!(w.prepare_local_space_crew());
        assert!(w.prepare_local_space_crew());
        let pads: Vec<_> = w.space().unwrap().fighters.iter()
            .filter(|f| f.pilot.is_some()).map(|f| (f.id, f.position)).collect();
        assert_eq!(pads.len(), 2);
        pads
    }).unwrap();

    let mut clock = TickRows::new();
    let mut left_pad = false;
    for tick in 1..=240 {
        let units = ctx.world_prepare_units().unwrap();
        let at = CLOCK_START_MS + f64::from(tick) * 1000. / 60.;
        let out = ctx.world_tick(clock.input(units, true, true, 0, true, 0, at, 1.)).unwrap();
        assert!(out.accepted, "first or delayed takeoff rejected at tick {tick}");
        clock.complete(&out);
        if tick % 30 == 0 {
            left_pad |= ctx.owned(|w| pads.iter().any(|(slot, start)| {
                let f = &w.space().unwrap().fighters[*slot];
                f.alive && f.pilot.is_some()
                    && (f.position[0] - start[0]).hypot(f.position[2] - start[2]) > 20.
            })).unwrap();
        }
    }
    assert!(left_pad, "scripted pilots remained above their pads");
}
