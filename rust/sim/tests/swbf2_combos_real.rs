//! Check an installed package against a separately supplied fixed-tick oracle.
//! The oracle indexes each equipped unit and weapon and supplies per-tick
//! inputs, expected states, stamina, outgoing intervals and stationary hits.
use bfe_wide::Wide;
use bfe_content::bvh::{build, Positions};
use bfe_content::collision::{triangle_bounds, MapCollision, TriangleGrid};
use bfe_family::GameFamily;
use bfe_sim::collision::spheres::{GatherCache, Gathered};
use bfe_sim::combat::combo::*;
use bfe_sim::combat::impacts::HealthScale;
use bfe_sim::gen::{Half, HitShape, ShapePart, ShapeType};
use bfe_sim::movement::scene::{GroundCache, Scene};
use bfe_sim::movement::soldier::{swbf2 as move2, Input as MoveInput, Mover, PART_CACHES};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const DT: f64 = COMBO_FIXED_DT;

fn zero_crc(name: &str) -> u32 {
    let mut r = 0xFFFF_FFFFu32;
    for b in name.bytes() {
        let ch = b.to_ascii_lowercase();
        let mut c = (((r >> 24) as u8) ^ ch) as u32;
        c <<= 24;
        for _ in 0..8 {
            c = if (c & 0x8000_0000) != 0 {
                (c << 1) ^ 0x04C1_1DB7
            } else {
                c << 1
            };
        }
        r = (r << 8) ^ c;
    }
    r ^ 0xFFFF_FFFF
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    u32::from_str_radix(
        s.trim().trim_start_matches("0x").trim_start_matches("0X"),
        16,
    )
    .ok()
}

fn prop_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    if let Some(v) = obj.get(key).and_then(Value::as_str) {
        return Some(v);
    }
    obj.get("properties")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter().rev().find_map(|pair| {
                let p = pair.as_array()?;
                let k = p.first()?.as_str()?;
                if k.eq_ignore_ascii_case(key) {
                    p.get(1)?.as_str()
                } else {
                    None
                }
            })
        })
}

fn prop_f64(obj: &Value, key: &str) -> Option<f64> {
    prop_str(obj, key).and_then(|s| s.trim().parse::<f64>().ok())
}

fn build_weapon_spec(weapon: &Value, combo_hash: u32) -> WeaponMeleeSpec {
    let mut spec = WeaponMeleeSpec {
        combo_hash,
        ..WeaponMeleeSpec::default()
    };
    let num_edges = prop_f64(weapon, "NumDamageEdges")
        .map(|n| (n.round() as usize).clamp(1, MAX_BLADE_EDGES))
        .unwrap_or(1);
    spec.num_edges = num_edges as u8;

    let base_len = prop_f64(weapon, "LightSaberLength")
        .or_else(|| prop_f64(weapon, "DamageEdgeLength"))
        .filter(|&v| v.is_finite() && v > 0.0)
        .unwrap_or(1.0);
    let base_wid = prop_f64(weapon, "LightSaberWidth")
        .or_else(|| prop_f64(weapon, "DamageEdgeWidth"))
        .filter(|&v| v.is_finite() && v > 0.0)
        .unwrap_or(0.1);

    for i in 0..MAX_BLADE_EDGES {
        let idx_len = prop_f64(weapon, &format!("DamageEdgeLength{i}"))
            .or_else(|| prop_f64(weapon, &format!("LightSaberLength{i}")))
            .filter(|&v| v.is_finite() && v > 0.0)
            .unwrap_or(base_len);
        let idx_wid = prop_f64(weapon, &format!("DamageEdgeWidth{i}"))
            .or_else(|| prop_f64(weapon, &format!("LightSaberWidth{i}")))
            .filter(|&v| v.is_finite() && v > 0.0)
            .unwrap_or(base_wid);
        spec.edges[i] = EdgeSpec {
            default_length: idx_len,
            default_width: idx_wid,
        };
    }

    if let Some(props) = weapon.get("properties").and_then(Value::as_array) {
        let mut edge_len_idx = 0usize;
        let mut edge_wid_idx = 0usize;
        for pair in props {
            let Some(arr) = pair.as_array() else {
                continue;
            };
            let (Some(k), Some(v)) = (
                arr.first().and_then(Value::as_str),
                arr.get(1).and_then(Value::as_str),
            ) else {
                continue;
            };
            if k.eq_ignore_ascii_case("DamageEdgeLength")
                || k.eq_ignore_ascii_case("LightSaberLength")
            {
                if let Ok(n) = v.trim().parse::<f64>() {
                    if n.is_finite() && n > 0.0 && edge_len_idx < MAX_BLADE_EDGES {
                        spec.edges[edge_len_idx].default_length = n;
                        edge_len_idx += 1;
                    }
                }
            } else if k.eq_ignore_ascii_case("DamageEdgeWidth")
                || k.eq_ignore_ascii_case("LightSaberWidth")
            {
                if let Ok(n) = v.trim().parse::<f64>() {
                    if n.is_finite() && n > 0.0 && edge_wid_idx < MAX_BLADE_EDGES {
                        spec.edges[edge_wid_idx].default_width = n;
                        edge_wid_idx += 1;
                    }
                }
            }
        }
        if edge_len_idx > spec.num_edges as usize {
            spec.num_edges = edge_len_idx.min(MAX_BLADE_EDGES) as u8;
        }
    }

    for (slot, key) in [
        (0, "PersonScale"),
        (1, "AnimalScale"),
        (2, "DroidScale"),
        (3, "VehicleScale"),
        (4, "BuildingScale"),
    ] {
        if let Some(s) = prop_f64(weapon, key) {
            if s.is_finite() && s >= 0.0 {
                spec.damage_scales[slot] = s;
            }
        }
    }
    if let Some(s) = prop_f64(weapon, "ShieldScale") {
        if s.is_finite() && s >= 0.0 {
            spec.shield_scale = s;
        }
    }
    spec.has_explosion = prop_str(weapon, "ExplosionName").is_some();
    spec
}

struct BankIndex {
    fps: f64,
    banks: BTreeMap<String, BTreeMap<u32, u32>>,
    packages: BTreeMap<String, Vec<String>>,
}

impl BankIndex {
    fn from_json(v: &Value) -> Self {
        let fps = v
            .get("fps")
            .and_then(Value::as_f64)
            .filter(|&f| f.is_finite() && f > 0.0)
            .unwrap_or(COMBO_ANIM_FPS);
        let mut banks = BTreeMap::new();
        if let Some(map) = v.get("banks").and_then(Value::as_object) {
            for (bank_name, rec) in map {
                let mut clips = BTreeMap::new();
                if let Some(arr) = rec.get("clips").and_then(Value::as_array) {
                    for c in arr {
                        let Some(crc_hex) = c.get("crc").and_then(Value::as_str) else {
                            continue;
                        };
                        let Some(crc_val) = parse_hex_u32(crc_hex) else {
                            continue;
                        };
                        let frames = c.get("frames").and_then(Value::as_u64).unwrap_or(0) as u32;
                        clips.insert(crc_val, frames);
                    }
                }
                banks.insert(bank_name.clone(), clips);
            }
        }
        let mut packages = BTreeMap::new();
        if let Some(pmap) = v.get("packages").and_then(Value::as_object) {
            for (pkg_key, rec) in pmap {
                let list = rec
                    .get("banks")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                packages.insert(pkg_key.to_ascii_lowercase(), list);
            }
        }
        Self {
            fps,
            banks,
            packages,
        }
    }

    fn action(
        &self,
        package: &[String],
        prefix: &str,
        action: &str,
        binding: &Value,
        expected: &Value,
    ) -> Option<f64> {
        let suffix = prefix.rsplit('_').next().unwrap();
        let stem = action
            .strip_suffix("_upper")
            .or_else(|| action.strip_suffix("_lower"))
            .unwrap_or(action);
        let variants = [
            action.to_string(),
            format!("{action}_full"),
            format!("{stem}_full"),
        ];
        let mut found = BTreeSet::new();
        let mut chosen = None;
        for bank in package {
            let clips = &self.banks[bank];
            let namespace = if bank.ends_with(&format!("_{suffix}")) {
                bank.clone()
            } else {
                format!("{bank}_{suffix}")
            };
            for variant in &variants {
                let crc = zero_crc(&format!("{namespace}_{variant}"));
                if let Some(&frames) = clips.get(&crc) {
                    found.insert((bank.as_str(), crc));
                    if frames > 0 {
                        chosen.get_or_insert((bank.as_str(), crc, frames));
                    }
                }
            }
        }
        if let Some(entries) = binding["clips"].get(action) {
            let cached: BTreeSet<_> = entries
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["bank"].as_str().unwrap(),
                        parse_hex_u32(e["crc"].as_str().unwrap()).unwrap(),
                    )
                })
                .collect();
            assert_eq!(cached, found, "candidates for {action}");
        }
        if chosen.is_some() {
            assert_eq!(expected[action]["fps"].as_f64(), Some(self.fps));
        }
        let timing = chosen.map_or(Value::Null, |(bank, crc, frames)| {
            serde_json::json!({
                "bank":bank, "crc":format!("{crc:08x}"),"frames":frames,"fps":self.fps as u32
            })
        });
        if let Some(cached) = binding["clipTiming"].get(action) {
            assert_eq!(*cached, timing, "cached timing for {action}");
        }
        assert_eq!(
            expected.get(action),
            Some(&timing),
            "installed timing for {action}"
        );
        chosen.map(|(_, _, n)| f64::from(n - 1) / self.fps)
    }
}
fn flat_terrain_world() -> (u32, bfe_sim::collision::State) {
    let floor = vec![
        -40.0f32, 0.0, -40.0, 40.0, 0.0, -40.0, 40.0, 0.0, 40.0, -40.0, 0.0, 40.0,
    ];
    let positions = vec![
        -6.0f32, -100.0, -6.0, 6.0, -100.0, -6.0, 6.0, -100.0, 6.0, -6.0, -100.0, 6.0,
    ];
    let indices = vec![0u32, 2, 1, 0, 3, 2];
    let buildings = build(Positions::F32(&positions), &indices);
    let terrain = build(Positions::F32(&floor), &indices);
    let map = MapCollision {
        tri_bounds: triangle_bounds(&positions, &indices),
        grid: Some(TriangleGrid::new(&floor, &indices)),
        positions: positions.into(),
        indices: indices.clone().into(),
        polygons: vec![0, 1].into(),
        masks: Vec::new().into(),
        has_terrain: true,
        terrain_positions: floor.into(),
        terrain_indices: indices.into(),
        kill_y: -200.0,
    };
    let mut col = bfe_sim::collision::CollisionOwner::new();
    let world = bfe_content::install::install(&mut col, &map, &buildings, Some(&terrain))
        .unwrap()
        .world;
    (world, col.state)
}

fn sample_move_tuning() -> move2::Tuning {
    let cs = |forward, strafe, turn| move2::ControlSpeed {
        forward,
        strafe,
        turn,
    };
    move2::Tuning {
        stand: cs(1.0, 1.0, 1.0),
        crouch: cs(0.6, 0.6, 1.0),
        sprint: cs(2.0, 0.5, 0.8),
        jump: cs(0.4, 0.4, 0.6),
        jet: cs(0.8, 0.6, 0.5),
        jetpack: None,
        roll: cs(0.0, 0.0, 0.5),
        tumble: cs(0.0, 0.0, 0.2),
        sprint_accelerate_time: 0.25,
        energy_capacity: 100.0,
        energy_floor: 0.0,
        energy_restore: 15.0,
        energy_restore_idle: 25.0,
        energy_drain_sprint: 15.0,
        energy_min_sprint: 10.0,
        energy_cost_jump: 10.0,
        energy_cost_jump_sprint: None,
        energy_cost_roll: 25.0,
        jump_height: 3.5,
        jump_forward_speed_factor: 1.0,
        jump_strafe_speed_factor: 1.0,
        roll_speed_factor: 1.5,
        roll_frames: 24,
        recover_from_tumble: false,
        getup_front_seconds: move2::DEFAULT_GETUP_FRONT_SECONDS,
        getup_back_seconds: move2::DEFAULT_GETUP_BACK_SECONDS,
    }
}

fn read(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn mix(hash: &mut Wide, bits: Wide) {
    *hash = hash.xor(bits).wrapping_mul(Wide::new(0x100, 0x1b3));
}

fn check_case(
    machine: &ComboMachine,
    weapon: &WeaponMeleeSpec,
    tuning: &move2::Tuning,
    case: &Value,
    digest: &mut Wide,
) {
    let (world, mut collision) = flat_terrain_world();
    let mut mover = Mover::for_family(world as usize, GameFamily::Swbf2);
    mover.grounded = true;
    let mut life = move2::State::new(tuning);
    let mut state = ComboState::new(machine);
    if let Some(start) = case["start"].as_str() {
        let mut cp = state.checkpoint();
        cp.state = machine.find_state(start).unwrap();
        let start = machine.state(cp.state);
        cp.deflecting = start.deflect.is_some();
        cp.blades_off_mask = start.turn_off_lightsaber;
        state.try_restore(machine, cp).unwrap();
    }
    let mut flight = SaberThrowState::new();
    let mut throw_def = SaberThrowDef::default();
    if let Some(params) = case["throw"].as_object() {
        throw_def.energy_drain = params["cost"].as_f64().unwrap();
        throw_def.velocity = params["speed"].as_f64().unwrap();
        throw_def.life_span = params["out"].as_f64().unwrap();
        throw_def.return_duration = params["back"].as_f64().unwrap();
        assert!(flight.launch(
            &throw_def,
            [0., 1., 0.],
            [0., 0., 1.],
            &mut SoldierEnergy {
                state: &mut life,
                tuning
            }
        ));
    }
    if let Some(recovered) = case["recover"].as_f64() {
        assert!(life.charge(tuning.energy_capacity + 1., tuning));
        life.recover(recovered, tuning);
    }
    let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
    let mut scene = Scene {
        s: &mut collision,
        world: world as usize,
        flags: &[],
        gathered: &mut Gathered::default(),
        caches: &mut caches,
        ground_cache: &mut GroundCache::default(),
    };
    let shapes = [HitShape {
        r#type: ShapeType::Sphere,
        part: ShapePart::Body,
        center: [0., 1., 0.],
        radius: 0.6,
        half_size: [0.; 3],
        half: Half::F64(0.),
        yaw: 0.,
    }];
    let target = MeleeTarget {
        target_id: bfe_sim::combat::combo::TargetId::Life(1),
        position: [0.; 3],
        aim_yaw: 0.,
        health_scale: HealthScale::Person,
        shapes: &shapes,
        deflect: None,
    };
    for (tick, row) in case["ticks"].as_array().unwrap().iter().enumerate() {
        let input =
            ComboInput::default().with_block_button(row["block"] == true, row["held"] == true);
        let input = if row["fire"] == true {
            input.press(ComboButton::Fire)
        } else {
            input
        };
        let (out, _) = step_soldier_combo_and_move(
            machine,
            &mut state,
            &input,
            &mut mover,
            &mut life,
            tuning,
            &MoveInput::default(),
            move2::Actions::default(),
            &mut scene,
            DT,
        )
        .unwrap();
        let throw = flight.step_with_combo(&throw_def, machine, &mut state, [0., 1., 0.], true, DT);
        assert_eq!(throw.caught, row["caught"] == true, "catch {tick}");
        assert_eq!(
            machine.state_name(state.state),
            row["state"].as_str().unwrap(),
            "state {tick}"
        );
        assert!(
            (life.energy - row["energy"].as_f64().unwrap()).abs() < 1e-8,
            "energy {tick}: {} vs {}",
            life.energy,
            row["energy"]
        );
        if let Some(exhausted) = row["exhausted"].as_bool() {
            assert_eq!(life.exhausted, exhausted, "latch {tick}");
        }
        let expected = row["active"].as_array().unwrap();
        assert_eq!(
            out.active_attacks.as_slice().len(),
            expected.len(),
            "active count {tick}"
        );
        for (a, expected) in out.active_attacks.as_slice().iter().zip(expected) {
            let state_def = machine.state(a.state_id);
            assert_eq!(
                machine.state_name(a.state_id),
                expected[0].as_str().unwrap(),
                "outgoing state {tick}"
            );
            assert_eq!(
                f64::from(a.attack_index),
                expected[1].as_f64().unwrap(),
                "record {tick}"
            );
            assert!(
                (a.sample_start - expected[2].as_f64().unwrap()).abs() < 1e-8
                    && (a.sample_end - expected[3].as_f64().unwrap()).abs() < 1e-8,
                "interval {tick}: {a:?}"
            );
            let samples = a.sample(machine, |_| ([0., 1., 0.], [0., 0., 1.]));
            let result = resolve_melee_hit_samples(
                [0.; 3],
                &state_def.attacks[a.attack_index as usize],
                weapon,
                samples.as_slice(),
                &mut state.hit_history,
                &target,
                None,
            );
            let damage = match result {
                MeleeHitOutcome::Hit { damage, .. } => Some(damage),
                MeleeHitOutcome::AlreadyHit => None,
                _ => panic!("stationary contact {tick}: {result:?}"),
            };
            assert_eq!(damage, expected[4].as_f64(), "applied damage at {tick}");
            for bits in [
                Wide::from_u32(u32::from(a.state_id.0)),
                Wide::from_u32(u32::from(a.attack_index)),
                Wide::from_f64_bits(a.sample_start),
                Wide::from_f64_bits(a.sample_end),
                Wide::from_f64_bits(damage.unwrap_or(0.)),
            ] {
                mix(digest, bits);
            }
        }
        let checkpoint = state.checkpoint();
        state.try_restore(machine, checkpoint).unwrap();
        for bits in [
            Wide::from_u32(u32::from(state.state.0)),
            Wide::from_f64_bits(state.state_time),
            Wide::from_f64_bits(state.anim_time),
            Wide::from_f64_bits(life.energy),
            Wide::from_f64_bits(mover.position.z),
            Wide::from_u32(u32::from(life.exhausted)),
        ] {
            mix(digest, bits);
        }
    }
}

#[test]
#[ignore = "requires an installed package and its independent combo oracle"]
fn installed_combo_timelines() {
    let root = std::env::var("BFE_SWBF2_CONTENT_DIR").expect("set BFE_SWBF2_CONTENT_DIR");
    let oracle = read(std::env::var("BFE_SWBF2_COMBO_ORACLE").expect("set BFE_SWBF2_COMBO_ORACLE"));
    let expected = oracle["bindings"].as_object().unwrap();
    let mut seen = BTreeSet::new();
    let mut machines = BTreeSet::new();
    let mut digest = Wide::new(0xcbf2_9ce4, 0x8422_2325);
    let mut sides: Vec<_> = fs::read_dir(Path::new(&root).join("sides"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    sides.sort();
    for side in sides {
        if !side.join("combos.json").is_file() {
            continue;
        }
        let combos = read(side.join("combos.json"));
        let units = read(side.join("units.json"));
        let banks = BankIndex::from_json(&read(side.join("anim_banks.json")));
        for (unit, data) in units["units"].as_object().unwrap() {
            for weapon in data["weapons"].as_array().into_iter().flatten() {
                for binding in weapon["comboBindings"].as_array().into_iter().flatten() {
                    let hash = binding["combo"].as_str().unwrap();
                    if combos.get(hash).is_none() {
                        assert_eq!(binding["missingMachine"], true);
                        continue;
                    }
                    let key = format!("{unit}/{hash}");
                    let spec = &expected[&key];
                    assert!(seen.insert(key.clone()), "duplicate equipped binding {key}");
                    machines.insert(hash.to_string());
                    let package_key = format!("{:08x}", zero_fnv1a_lower(unit));
                    let mut package = banks.packages[&package_key].clone();
                    let prefix = binding["bank"].as_str().unwrap();
                    let hero = prop_str(data, "AnimationName").unwrap_or("");
                    package.sort_by_key(|b| {
                        (
                            !b.eq_ignore_ascii_case(hero),
                            b.eq_ignore_ascii_case(prefix),
                        )
                    });
                    let timings = &spec["timing"];
                    let resolve = |a: &str| banks.action(&package, prefix, a, binding, timings);
                    for a in timings.as_object().unwrap().keys() {
                        resolve(a);
                    }
                    let w = build_weapon_spec(weapon, parse_hex_u32(hash).unwrap());
                    let m = ComboMachine::from_json_value(&combos[hash], &w, banks.fps, resolve)
                        .unwrap();
                    let mut tuning = sample_move_tuning();
                    for (slot, k) in [
                        (&mut tuning.energy_capacity, "EnergyBar"),
                        (&mut tuning.energy_restore_idle, "EnergyRestoreIdle"),
                        (&mut tuning.energy_restore, "EnergyRestore"),
                        (&mut tuning.energy_min_sprint, "EnergyMinSprint"),
                    ] {
                        *slot = prop_f64(data, k).unwrap();
                    }
                    for case in spec["cases"].as_array().unwrap() {
                        eprintln!("checking {key} {}", case["name"]);
                        check_case(&m, &w, &tuning, case, &mut digest);
                    }
                }
            }
        }
    }
    assert!(!expected.is_empty());
    assert_eq!(seen, expected.keys().cloned().collect());
    assert_eq!(
        format!("{digest:016x}"),
        oracle["fingerprint"].as_str().unwrap(),
        "fixed installed combat fingerprint"
    );
    eprintln!(
        "COMBO_DIGEST {digest:016x} ({} machines, {} equipped bindings)",
        machines.len(),
        seen.len()
    );
}
