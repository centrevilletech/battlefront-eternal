//! Native hull queries must not write a WASM instance's boundary scratch.
//! Empty BVHs keep the fixture independent of authored content while bodies
//! still exercise candidate order and eligibility in the production cast.
use bfe_sim::{collision::{self, Body, Corners, State, Tree, World}, vehicles::native::Collision};
use three::Vector3;

fn scene(reverse: bool) -> (State, Collision) {
    let mut state = State::default();
    state.positions.push(Corners::F32(Vec::new()));
    state.trees.push(Tree { roots: Vec::new(), positions: 0, index: Vec::new(),
        indexed: true, indirect: Vec::new(), polygons: Vec::new() });
    for _ in 0..2 {
        state.bodies.push(Body { tree: 0, moving: false, offset: 0.0,
            bounds: [-1.0,-1.0,-1.0,1.0,1.0,1.0], matrix: [0.0;16], inverse: [0.0;16] });
    }
    state.worlds.push(World { tree: 0, terrain: None, grid: None,
        order: if reverse {vec![1,0]} else {vec![0,1]}, prepared: Vec::new(),
        version: 0, statics: None, broad: Default::default() });
    let mut queries = Collision::new(0,None);
    queries.name_body(0,0); queries.name_body(1,1);
    // SAFETY: flags_ptr lends the fixed reservation owned by queries. No cast
    // or other loan is active, and both writes are within its prepared bank.
    unsafe { std::slice::from_raw_parts_mut(queries.flags_ptr(),2).copy_from_slice(&[1,0]); }
    (state,queries)
}
fn cast(scene: &mut (State,Collision)) {
    assert!(matches!(scene.1.cast(&mut scene.0,&Vector3::new(0.0,0.0,-2.0),
        &Vector3::new(0.0,0.0,1.0),4.0,None),Some(None)));
}
#[test]
fn hull_queries_are_isolated_when_interleaved_and_on_separate_threads() {
    let mut a=scene(false); let mut b=scene(true);
    // SAFETY: only this test accesses the process's facade reservations. All
    // following casts must use their own buffers, including on worker threads.
    unsafe {
        std::slice::from_raw_parts_mut(collision::candidates_ptr(),2).fill(9876);
        std::slice::from_raw_parts_mut(collision::enabled_ptr(),2).fill(87);
    }
    for _ in 0..32 {cast(&mut a);cast(&mut b);cast(&mut a);}
    let threads:Vec<_>=(0..4).map(|i|std::thread::spawn(move|| {
        let mut scene=scene(i%2==0);
        for _ in 0..2000 {cast(&mut scene);}
    })).collect();
    for thread in threads {thread.join().unwrap();}
    // SAFETY: threads have joined. These reservations are alive for the test
    // process and were initialized above, before any native query ran.
    unsafe {
        assert_eq!(std::slice::from_raw_parts(collision::candidates_ptr(),2),[9876,9876]);
        assert_eq!(std::slice::from_raw_parts(collision::enabled_ptr(),2),[87,87]);
    }
}
