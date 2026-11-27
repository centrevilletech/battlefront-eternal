use bfe_content::{files::DirFiles, GameFamily};
use bfe_sim::{
    collision::Ray,
    space::SPACE_FIXED_DT,
    world::{
        admit::{admit_for_family, Bots, Options},
        owner::World,
    },
};
#[test]
#[ignore]
fn prepared_frigate_collision_follows_the_render_transform() {
    let Ok(root) = std::env::var("BFE_SWBF2_CONTENT") else {
        return;
    };
    let files = DirFiles::from_resolved_root(&root, bfe_family::GameFamily::Swbf2);
    let loader = bfe_content::battle::Loader::new(&files);
    let battle = bfe_content::battle::Battle::load_for_family(
        &loader,
        GameFamily::Swbf2,
        "spa6",
        "?side=rep&mission=spa6c_ass",
    )
    .unwrap();
    let c = bfe_content::content::Content::build(battle).unwrap();
    let map = &c.battle.swbf2.as_ref().unwrap().map_admission;
    let mut world = World::new();
    let options = Options {
        family: GameFamily::Swbf2,
        mode: "ass".into(),
        practice_target: None,
        map: "spa6".into(),
        side: "rep".into(),
        bots: Bots::default(),
        enhanced: false,
        seed: 41,
        heroes: false,
        unit: None,
        name: None,
    };
    admit_for_family(
        &mut world,
        &files,
        GameFamily::Swbf2,
        &options,
        Some("spa6c_ass"),
        false,
    )
    .unwrap();
    let doors = &world.space().unwrap().doors;
    assert!(!doors.is_empty());
    for door in doors {
        let clip = door
            .opening_clip
            .as_ref()
            .expect("missing opening animation");
        assert!(clip.duration > 0.0 && clip.tracks.iter().any(|&bone| bone >= 0.));
    }
    println!(
        "bound door clips={} first_duration={}",
        doors.len(),
        doors[0].opening_clip.as_ref().unwrap().duration
    );
    let mut rays = Vec::new();
    for obj in &world.space().unwrap().world_anims.objects {
        if !obj.name.ends_with("frig3") {
            continue;
        }
        let shape = c
            .prop_bodies
            .iter()
            .find(|p| p.instance == obj.prop_instance)
            .unwrap()
            .built
            .as_ref()
            .unwrap();
        // A point on an actual triangle far from the origin in model space.
        let (dist, center, normal) = shape
            .indices
            .chunks_exact(3)
            .filter_map(|ids| {
                let v = |i: u32| {
                    let a = i as usize * 3;
                    [
                        f64::from(shape.positions[a]),
                        f64::from(shape.positions[a + 1]),
                        f64::from(shape.positions[a + 2]),
                    ]
                };
                let (a, b, d) = (v(ids[0]), v(ids[1]), v(ids[2]));
                let ab: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
                let ad: [f64; 3] = std::array::from_fn(|i| d[i] - a[i]);
                let n = [
                    ab[1] * ad[2] - ab[2] * ad[1],
                    ab[2] * ad[0] - ab[0] * ad[2],
                    ab[0] * ad[1] - ab[1] * ad[0],
                ];
                let len = v8math::hypot3(n[0], n[1], n[2]);
                let center: [f64; 3] = std::array::from_fn(|i| (a[i] + b[i] + d[i]) / 3.);
                (len > 1.0).then_some((
                    v8math::hypot3(center[0], center[1], center[2]),
                    center,
                    n.map(|k| k / len),
                ))
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        let (body, local) = obj.bodies[0].unwrap();
        println!(
            "{} slot {} local {:?} probe distance {:.1}m",
            obj.name, body, local, dist
        );
        rays.push((obj.prop_instance, body, center, normal));
    }
    assert_eq!(rays.len(), 2);
    let mut hits = vec![0u32; rays.len()];
    let mut inside = hits.clone();
    let mut frame_errors = hits.clone();
    let mut samples = 0;
    let ticks = map
        .world_animations
        .clips
        .iter()
        .map(|clip| (clip.duration / SPACE_FIXED_DT).ceil() as usize)
        .max()
        .unwrap()
        + 1;
    for t in 0..=ticks {
        if t > 0 {
            assert!(world.space_step_tick(SPACE_FIXED_DT));
        }
        samples += 1;
        let draw = map.placed_transforms_at(t as f64 * SPACE_FIXED_DT);
        for (i, &(inst, body, p, n)) in rays.iter().enumerate() {
            let m = draw[inst];
            let tr = |v: [f64; 3], w: f64| -> [f64; 3] {
                std::array::from_fn(|k| {
                    m[k] * v[0] + m[4 + k] * v[1] + m[8 + k] * v[2] + m[12 + k] * w
                })
            };
            let point = tr(p, 1.0);
            let normal = tr(n, 0.0);
            let actual = world.space_animated_transform_pair(inst).unwrap().1;
            if m.iter().zip(actual).any(|(&a, b)| (a - b).abs() > 1e-7) {
                frame_errors[i] += 1
            }
            let bb = world.space_body_bounds(body).unwrap();
            if (0..3).all(|k| point[k] >= bb[k] - 1e-4 && point[k] <= bb[k + 3] + 1e-4) {
                inside[i] += 1;
            }
            for s in [1.0, -1.0] {
                let ray = Ray {
                    ox: point[0] + s * normal[0] * 0.5,
                    oy: point[1] + s * normal[1] * 0.5,
                    oz: point[2] + s * normal[2] * 0.5,
                    dx: -s * normal[0],
                    dy: -s * normal[1],
                    dz: -s * normal[2],
                };
                if world
                    .space_ray_hit(ray, 1.0, -1)
                    .is_some_and(|(code, h)| code == body as i32 && (h.distance - 0.5).abs() < 0.02)
                {
                    hits[i] += 1;
                    break;
                }
            }
        }
    }
    println!("frigate cycles: samples={samples} triangle_hits={hits:?} bounds_hits={inside:?} transform_errors={frame_errors:?}");
    assert!(hits.iter().all(|&hits| hits == samples));
    assert!(inside.iter().all(|&hits| hits == samples));
    assert!(frame_errors.iter().all(|&errors| errors == 0));
}
