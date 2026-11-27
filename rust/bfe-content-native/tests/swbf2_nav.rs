//! Reachability and movement on player-supplied navigation data.
use bfe_content::files::DirFiles;
use bfe_content::Value;
use bfe_content::{battle::{Battle, Loader}, GameFamily};
use bfe_content_native::setup::{admission::CLOCK_START_MS, TickRows};
use bfe_sim::abi::context::Context;
use bfe_sim::world::admit::{Bots, Options};

fn content_root() -> std::path::PathBuf {
    let raw = std::env::var("BFE_SWBF2_CONTENT")
        .expect("set BFE_SWBF2_CONTENT to a prepared 2005 content directory");
    let p = std::path::PathBuf::from(raw);
    assert!(p.is_dir(), "BFE_SWBF2_CONTENT does not exist: {}", p.display());
    p
}

fn spawn_areas(spawns: &Value) -> Vec<(String, [f64; 3])> {
    let mut out = Vec::new();
    let Some(obj) = spawns.as_object() else {
        return out;
    };
    for (name, val) in obj.entries() {
        let Some(arr) = val.as_array() else { continue };
        let Some(first) = arr.first().and_then(Value::as_array) else { continue };
        let x = first.first().and_then(Value::as_f64).unwrap_or(0.0);
        let y = first.get(1).and_then(Value::as_f64).unwrap_or(0.0);
        let z = first.get(2).and_then(Value::as_f64).unwrap_or(0.0);
        out.push((name.to_string(), [x, y, z]));
    }
    out
}

#[test]
#[ignore]
fn walk_spawn_routes() {
    let root = content_root();
    let files = DirFiles::from_resolved_root(&root, bfe_family::GameFamily::Swbf2);
    let map = Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "tat2", "?mission=tat2g_eli")
        .expect("load the battle's composed spawn areas");

    let options = Options {
        family: GameFamily::Swbf2,
        map: "tat2".into(),
        mode: "eli".into(),
        side: "all".into(),
        enhanced: false,
        seed: 1,
        heroes: true,
        unit: None,
        practice_target: None,
        name: None,
        bots: Bots::default(),
    };
    let context = Context::new();
    let admitted = context
        .world_admit_for_family(&files, GameFamily::Swbf2, &options, Some("tat2g_eli"), true)
        .expect("admit tat2 world");
    context.world_step_init(admitted.setup.clone()).expect("step_init tat2");

    let info = context
        .swbf2_nav_info()
        .expect("swbf2_nav_info")
        .expect("expected 2005 nav graph on tat2");
    println!(
        "tat2 nav_info: hubs={} arcs={} soldier_components={:?} boundary_polygons={} barriers={} hints={} errors=(dangling={} bad_slots={} nan_hubs={} zero_arcs={} weight_mismatch={} bad_delta={})",
        info.hubs.len(),
        info.arcs.len(),
        info.soldier_components,
        info.boundary_polygons,
        info.barriers,
        info.hints,
        info.dangling_arcs,
        info.bad_hub_slots,
        info.non_finite_hubs,
        info.zero_length_arcs,
        info.weight_mismatches,
        info.bad_delta_blocks,
    );
    assert!(!info.hubs.is_empty());
    assert!(!info.arcs.is_empty());
    assert_eq!(info.dangling_arcs, 0);
    assert_eq!(info.bad_hub_slots, 0);
    assert_eq!(info.non_finite_hubs, 0);
    assert_eq!(info.zero_length_arcs, 0);
    assert_eq!(info.weight_mismatches, 0);


    let spawns = spawn_areas(map.world.get("spawns").expect("composed spawn areas"));
    let routes: Vec<_> = spawns.iter().filter(|(name, _)| name != "spawn").collect();
    assert!(routes.len() >= 2);
    let mut clock = TickRows::new();
    for (i, (start_name, start)) in routes.iter().enumerate() {
        for (j, (end_name, end)) in routes.iter().enumerate() {
            if i == j { continue; }
            let units = context.world_prepare_units().unwrap();
            let at = CLOCK_START_MS + clock.next_tick() * (1000.0 / 60.0);
            let input = clock.input(units, true, true, 0, true, 0, at, 1.0);
            let tick = context.world_tick(input).unwrap();
            assert!(tick.accepted);
            clock.complete(&tick);
            let route = context.nav_route_distance(*start, *end, 0.5, 1, false).unwrap();
            assert!(route.reachable, "{start_name} to {end_name}");
            let walk = context.swbf2_walk_route(*start, *end, 7200, (i + j) as u32).unwrap();
            assert!(walk.arrived, "{start_name} to {end_name}: {walk:?}");
        }
    }
    context.dispose().unwrap();
}
