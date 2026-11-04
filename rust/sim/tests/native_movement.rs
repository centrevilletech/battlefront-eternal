#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_content::{
    bvh::{build, Positions},
    collision::{triangle_bounds, MapCollision, TriangleGrid},
};
use bfe_sim::{
    abi::context::{Context, Error, ErrorKind, Failure, MovementError, Record},
    collision::{Body, Corners},
    movement::{self as m, soldier::Input},
};
use std::{cell::Cell, rc::Rc};

fn floor(y: f32) -> Vec<f32> {
    vec![
        -100., y, -100., 100., y, -100., 100., y, 100., -100., y, 100.,
    ]
}
fn install(cx: &Context, y: f32) -> u32 {
    let positions = floor(y - 100.);
    let terrain = floor(y);
    let indices = vec![0, 2, 1, 0, 3, 2];
    let c = MapCollision {
        tri_bounds: triangle_bounds(&positions, &indices),
        grid: Some(TriangleGrid::new(&terrain, &indices)),
        positions: positions.into(),
        indices: indices.clone().into(),
        polygons: vec![0, 1].into(),
        terrain_positions: terrain.into(),
        terrain_indices: indices.into(),
        kill_y: -200.,
    };
    let b = build(Positions::F32(&c.positions), &c.indices);
    let t = build(Positions::F32(&c.terrain_positions), &c.terrain_indices);
    cx.collision_install(&c, &b, Some(&t)).unwrap().world
}
fn class() -> [f64; 21] {
    [
        9., 7., 8., 2., 0.5, 70., -0.8, 1.1, 7., 14., 0.1, 0.2, 0., 0.1, 1., 10., 3., 0., 1., 0.,
        0.,
    ]
}
fn frame(y: f64) -> [f64; m::IO_LEN] {
    let mut v = [0.; m::IO_LEN];
    v[1] = y;
    v[4] = y;
    v[10] = 1.;
    v[m::GROUNDED] = 1.;
    v[m::STATE_TIME] = 1.;
    v[m::FUEL] = 0.5;
    v[m::MAX_FLY] = 200.;
    v[m::GETUP] = 0.9;
    v[m::DIVE_TIME] = 0.6;
    v[m::PASS_THROUGH] = -1.;
    v[m::DT] = 1. / 60.;
    v
}
fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|n| n.to_bits()).collect()
}
const IDENTITY: [f64; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

#[test]
fn native_movement_keeps_context_ids_and_per_world_ground_caches_separate() {
    let a = Context::new();
    let b = Context::new();
    let reference = Context::new();
    assert_eq!(install(&a, 0.), 0);
    assert_eq!(install(&b, 5.), 0);
    assert_eq!(install(&a, 0.15), 1);
    install(&reference, 0.15);
    assert_eq!(a.movement_new(0, class()).unwrap(), 0);
    assert_eq!(b.movement_new(0, class()).unwrap(), 0);
    let second = a.movement_new(1, class()).unwrap();
    let expected = reference.movement_new(0, class()).unwrap();
    let first = a.movement_tick(0, frame(0.2), vec![]).unwrap();
    let other = a.movement_tick(second, frame(0.2), vec![]).unwrap();
    let isolated = reference
        .movement_tick(expected, frame(0.2), vec![])
        .unwrap();
    assert_eq!(bits(&other.io), bits(&isolated.io));
    assert_ne!(
        first.mover.position.y.to_bits(),
        other.mover.position.y.to_bits()
    );
    a.movement_load(0, frame(0.)).unwrap();
    b.movement_load(0, frame(5.)).unwrap();
    for i in 0..180 {
        let input = Input {
            forward: 0.7,
            right: 0.2,
            turn: 0.03,
            jump: i == 90,
            ..Input::default()
        };
        let left = a.movement_advance(0, input, 1. / 60., vec![]).unwrap();
        let right = b.movement_advance(0, input, 1. / 60., vec![]).unwrap();
        assert_eq!(
            left.mover.position.x.to_bits(),
            right.mover.position.x.to_bits()
        );
        assert!((right.mover.position.y - left.mover.position.y - 5.).abs() < 1e-12);
    }
    a.dispose().unwrap();
    b.dispose().unwrap();
    reference.dispose().unwrap();
}

#[test]
fn native_movement_validates_handles_flags_codes_and_retains_owned_results() {
    let cx = Context::new();
    assert!(cx.movement_new(0, class()).is_err());
    install(&cx, 0.);
    let id = cx.movement_new(0, class()).unwrap();
    let original = cx.movement_tick(id, frame(0.), vec![]).unwrap();
    let mut bad = frame(12.);
    bad[m::POSTURE] = 17.;
    assert!(matches!(
        cx.movement_tick(id, bad, vec![]),
        Err(MovementError::UnknownState)
    ));
    assert!(matches!(
        cx.movement_tick(id, frame(9.), vec![1]),
        Err(MovementError::InvalidFlags)
    ));
    assert_eq!(
        bits(&cx.movement_snapshot(id).unwrap().io),
        bits(&original.io)
    );
    let shape = cx
        .collision_shape(Corners::F32(floor(2.)), vec![0, 2, 1, 0, 3, 2], vec![0, 1])
        .unwrap();
    let body = cx
        .collision_body(
            0,
            Body {
                tree: shape as usize,
                moving: true,
                offset: 16777216.,
                bounds: [-100., 2., -100., 100., 2., 100.],
                matrix: IDENTITY,
                inverse: IDENTITY,
            },
        )
        .unwrap();
    let raw = cx.movement_tick(id, frame(2.), vec![255]).unwrap();
    assert_eq!(raw.flags, vec![255]);
    cx.collision_remove(0, body).unwrap();
    assert!(matches!(
        cx.movement_tick(id, frame(0.), vec![255]),
        Err(MovementError::InvalidFlags)
    ));
    cx.movement_tick(id, frame(0.), vec![]).unwrap();
    for _ in 0..150 {
        cx.movement_new(0, class()).unwrap();
    }
    cx.scratch(&vec![3; 350_000]).unwrap();
    cx.movement_remove(id).unwrap();
    assert!(matches!(
        cx.movement_snapshot(id),
        Err(MovementError::InvalidMover)
    ));
    assert!(matches!(
        cx.movement_remove(id),
        Err(MovementError::InvalidMover)
    ));
    assert!(cx.movement_new(0, class()).unwrap() > 150);
    let alias = cx.clone();
    cx.dispose().unwrap();
    assert!(
        matches!(alias.movement_snapshot(id),Err(MovementError::Context(e)) if e.kind()==ErrorKind::Disposed)
    );
    assert_eq!(raw.flags, vec![255]);
    assert_eq!(bits(&original.io), bits(&original.clone().io));
}

#[test]
fn native_movement_nested_callbacks_release_owners_and_preserve_errors() {
    let a = Context::new();
    let b = Context::new();
    install(&a, 0.);
    install(&b, 4.);
    a.movement_new(0, class()).unwrap();
    b.movement_new(0, class()).unwrap();
    let original = Error::host("");
    let failure = original.clone();
    let first = Rc::new(Cell::new(true));
    let flag = first.clone();
    a.register(9202, move |cx, _| {
        assert_eq!(cx.dispose().unwrap_err().kind(), ErrorKind::Busy);
        let saved = cx.movement_tick(0, frame(0.), vec![]).unwrap();
        let retained = bits(&saved.io);
        for _ in 0..100 {
            cx.movement_new(0, class()).unwrap();
        }
        cx.scratch(&vec![7; 250_000])?;
        cx.movement_advance(
            0,
            Input {
                forward: 1.,
                ..Input::default()
            },
            1. / 60.,
            vec![],
        )
        .unwrap();
        assert_eq!(bits(&saved.io), retained);
        let out = Record {
            row: saved.io.to_vec(),
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
    b.register(9201, move |cx, args| {
        let saved = cx.movement_tick(0, frame(4.), vec![]).unwrap();
        let result = inner.call_import(9202, args.clone());
        assert_eq!(bits(&saved.io), bits(&cx.movement_snapshot(0).unwrap().io));
        assert_eq!(cx.dispose().unwrap_err().kind(), ErrorKind::Busy);
        result
    })
    .unwrap();
    let other = b.clone();
    a.register(9200, move |cx, args| {
        cx.movement_tick(0, frame(0.), vec![]).unwrap();
        other.call_import(9201, args.clone())
    })
    .unwrap();
    let error = a.call_import(9200, Record::default()).unwrap_err();
    assert!(error.error.same(&original));
    assert_eq!(error.partial.unwrap().row.len(), m::IO_LEN);
    assert_eq!(
        a.call_import(9200, Record::default()).unwrap().row.len(),
        m::IO_LEN
    );
    assert!(a.call_import(9299, Record::default()).is_err());
    a.register(9203, |cx, _| {
        cx.movement_tick(0, frame(0.), vec![]).unwrap();
        std::panic::resume_unwind(Box::new(783u32))
    })
    .unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.call_import(9203, Record::default())
    }))
    .unwrap_err();
    assert_eq!(*panic.downcast::<u32>().unwrap(), 783);
    a.movement_advance(0, Input::default(), 1. / 60., vec![])
        .unwrap();
    a.dispose().unwrap();
    b.dispose().unwrap();
}
