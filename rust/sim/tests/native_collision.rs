#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_content::bvh::{build, Positions};
use bfe_content::collision::{triangle_bounds, MapCollision, TriangleGrid};
use bfe_sim::abi::context::{
    CollisionError, CollisionQuery, Context, Error, ErrorKind, Failure, Record,
};
use bfe_sim::collision::{Body, Corners, Ray, MISS};
use std::cell::Cell;
use std::rc::Rc;

fn floor(y: f32) -> Vec<f32> {
    vec![-4., y, -4., 4., y, -4., 4., y, 4., -4., y, 4.]
}
fn map(y: f32) -> MapCollision {
    let positions = floor(y);
    let indices = vec![0, 2, 1, 0, 3, 2];
    let ground = floor(y - 4.0);
    MapCollision {
        tri_bounds: triangle_bounds(&positions, &indices),
        grid: Some(TriangleGrid::new(&ground, &indices)),
        positions: positions.into(),
        indices: indices.clone().into(),
        polygons: vec![17, 18].into(),
        terrain_positions: ground.into(),
        terrain_indices: indices.into(),
        kill_y: -110.0,
    }
}
fn install(cx: &Context, y: f32) -> u32 {
    let c = map(y);
    let b = build(Positions::F32(&c.positions), &c.indices);
    let t = build(Positions::F32(&c.terrain_positions), &c.terrain_indices);
    cx.collision_install(&c, &b, Some(&t)).unwrap().world
}
fn cast(world: u32) -> CollisionQuery {
    CollisionQuery::Cast {
        world,
        ray: Ray {
            ox: 0.,
            oy: 20.,
            oz: 0.,
            dx: -0.,
            dy: -1.,
            dz: 0.,
        },
        far: 50.,
        hypot_xz: 0.,
        ignore: -1,
        static_only: false,
        moving_only: false,
    }
}
fn gather(world: u32, cache: Option<u32>) -> CollisionQuery {
    CollisionQuery::Gather {
        world,
        center: [0., 0., 0.],
        radius: 20.,
        cache,
        blockers: None,
        ignore: -1,
    }
}
const IDENTITY: [f64; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];
fn body(cx: &Context, world: u32, y: f32) -> u32 {
    let shape = cx
        .collision_shape(Corners::F32(floor(y)), vec![0, 2, 1, 0, 3, 2], vec![33, 34])
        .unwrap();
    cx.collision_body(
        world,
        Body {
            tree: shape as usize,
            moving: false,
            offset: 0x1000000 as f64,
            bounds: [-4., y as f64, -4., 4., y as f64, 4.],
            matrix: IDENTITY,
            inverse: IDENTITY,
        },
    )
    .unwrap()
}

#[test]
fn native_collision_contexts_isolate_ids_flags_caches_growth_removal_and_disposal() {
    let a = Context::new();
    let b = Context::new();
    assert_eq!(install(&a, 0.), 0);
    assert_eq!(install(&b, 3.), 0);
    assert_eq!(body(&a, 0, 2.), 0);
    assert_eq!(body(&b, 0, 7.), 0);
    let cache_a = a.collision_cache(0, 1.5).unwrap();
    let cache_b = b.collision_cache(0, 0.5).unwrap();
    assert_eq!(cache_a, cache_b);
    let ah = a.collision_query(cast(0), &[1]).unwrap();
    let bh = b.collision_query(cast(0), &[255]).unwrap();
    assert_eq!((ah.code, bh.code), (0, 0));
    assert_eq!((ah.io[10], bh.io[10]), (18., 13.));
    assert_eq!((ah.enabled, bh.enabled), (vec![1], vec![255]));
    let saved = a.collision_query(gather(0, Some(cache_a)), &[1]).unwrap();
    assert_eq!(a.collision_query(cast(0), &[0]).unwrap().code, -1);
    assert_eq!(b.collision_query(cast(0), &[1]).unwrap().io[10], 13.);
    assert!(a.collision_remove(0, 0).unwrap());
    assert!(!a.collision_remove(0, 0).unwrap());
    assert!(a
        .collision_query(gather(0, Some(cache_a)), &[])
        .unwrap()
        .candidates
        .is_empty());
    assert_eq!(body(&a, 0, 5.), 1, "removed slots are not recycled");
    let retained = saved.words.clone();
    for n in 0..300 {
        body(&a, 0, (n % 9) as f32);
    }
    a.scratch(&vec![0; 400_000]).unwrap();
    let grown = a
        .collision_query(gather(0, Some(cache_a)), &vec![1; 301])
        .unwrap();
    assert_eq!(grown.candidates.len(), 301);
    assert!(grown.words.len() > saved.words.len());
    assert_eq!(saved.words, retained);
    assert_eq!(
        b.collision_query(gather(0, Some(cache_b)), &[2])
            .unwrap()
            .candidates,
        vec![0]
    );
    assert_eq!(a.collision_ground(0, 0., 0., 100.).unwrap().unwrap().0, -4.);
    assert_eq!(b.collision_ground(0, 0., 0., 100.).unwrap().unwrap().0, -1.);
    let alias = a.clone();
    a.dispose().unwrap();
    assert!(
        matches!(alias.collision_query(cast(0), &[]), Err(CollisionError::Context(e)) if e.kind()==ErrorKind::Disposed)
    );
    assert_eq!(saved.words, retained);
    assert_eq!(b.collision_query(cast(0), &[1]).unwrap().io[10], 13.);
    b.dispose().unwrap();
}

#[test]
fn native_collision_callbacks_reenter_a_b_a_without_owner_borrows_and_keep_errors() {
    let a = Context::new();
    let b = Context::new();
    install(&a, 0.);
    install(&b, 4.);
    let original = Error::host("");
    let failure = original.clone();
    let first = Rc::new(Cell::new(true));
    let flag = first.clone();
    a.register(9002, move |cx, _| {
        assert_eq!(cx.dispose().unwrap_err().kind(), ErrorKind::Busy);
        let saved = cx.collision_query(cast(0), &[]).unwrap();
        for n in 0..100 {
            cx.collision_shape(Corners::F32(floor(n as f32)), vec![0, 2, 1], vec![n])
                .unwrap();
        }
        cx.scratch(&vec![0; 250_000])?;
        let out = Record {
            row: saved.io[10..17].to_vec(),
            heap: vec![],
        };
        if flag.replace(false) {
            Err(Failure {
                error: failure.clone(),
                partial: Some(out),
            })
        } else {
            Ok(out)
        }
    })
    .unwrap();
    let inner = a.clone();
    b.register(9001, move |cx, args| {
        let before = cx.collision_query(cast(0), &[]).unwrap();
        let result = inner.call_import(9002, args.clone());
        assert_eq!(
            cx.collision_query(cast(0), &[]).unwrap().io[10],
            before.io[10]
        );
        assert_eq!(cx.dispose().unwrap_err().kind(), ErrorKind::Busy);
        result
    })
    .unwrap();
    let other = b.clone();
    a.register(9000, move |cx, args| {
        cx.collision_query(gather(0, None), &[]).unwrap();
        other.call_import(9001, args.clone())
    })
    .unwrap();
    let error = a.call_import(9000, Record::default()).unwrap_err();
    assert!(error.error.same(&original));
    assert_eq!(error.partial.unwrap().row[0], 20.);
    assert_eq!(a.call_import(9000, Record::default()).unwrap().row[0], 20.);
    a.dispose().unwrap();
    b.dispose().unwrap();
}

#[test]
fn native_collision_validation_and_partial_install_leave_next_query_usable() {
    let cx = Context::new();
    let mut bad = map(0.);
    let b = build(Positions::F32(&bad.positions), &bad.indices);
    let t = build(Positions::F32(&bad.terrain_positions), &bad.terrain_indices);
    bad.polygons = vec![].into();
    assert!(matches!(
        cx.collision_install(&bad, &b, Some(&t)),
        Err(CollisionError::Content(_))
    ));
    assert_eq!(
        install(&cx, 0.),
        1,
        "the failed install retains its preceding world allocation"
    );
    assert!(matches!(
        cx.collision_query(gather(0, None), &[]),
        Err(CollisionError::Invalid(_))
    ));
    let c = cx.collision_cache(1, 1.).unwrap();
    assert!(matches!(
        cx.collision_query(gather(0, Some(c)), &[]),
        Err(CollisionError::Invalid(_))
    ));
    assert!(matches!(
        cx.collision_shape(Corners::F32(floor(0.)), vec![999, 1, 2], vec![0]),
        Err(CollisionError::Invalid(_))
    ));
    body(&cx, 1, 2.);
    assert!(cx.collision_query(cast(1), &[]).is_err());
    assert!(cx.collision_order(1, &[77]).is_err());
    let good = cx.collision_query(cast(1), &[1]).unwrap();
    assert_eq!(good.code, 0);
    let miss = cx
        .collision_query(
            CollisionQuery::Cast {
                world: 1,
                ray: Ray {
                    ox: 0.,
                    oy: 20.,
                    oz: 0.,
                    dx: 0.,
                    dy: 1.,
                    dz: -0.,
                },
                far: f64::INFINITY,
                hypot_xz: 0.,
                ignore: -1,
                static_only: false,
                moving_only: false,
            },
            &[1],
        )
        .unwrap();
    assert_eq!(miss.code, MISS);
    assert_eq!(
        miss.io[10..17],
        good.io[10..17],
        "a miss retains the previous IO hit"
    );
    cx.dispose().unwrap();
}
