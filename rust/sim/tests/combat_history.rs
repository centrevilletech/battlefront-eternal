use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use three::Vector3;

struct Allocator;
thread_local! { static TRACK: Cell<bool> = const { Cell::new(false) }; static ALLOCATIONS: Cell<usize> = const { Cell::new(0) }; }
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|on| {
            if on.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|on| {
            if on.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.realloc(pointer, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

use bfe_sim::combat::history::{Entry, History, World};
use bfe_sim::gen::{Half, HitShape, ShapePart, ShapeType};

struct Bodies {
    x: [f64; 3],
    life: [f64; 3],
    alive: [bool; 3],
    hidden: [bool; 3],
    topology: bool,
}
impl Default for Bodies {
    fn default() -> Self {
        Self {
            x: [0.0; 3],
            life: [1.0; 3],
            alive: [true; 3],
            hidden: [false; 3],
            topology: false,
        }
    }
}
impl World for Bodies {
    type Key = usize;
    fn count(&self) -> usize {
        3
    }
    fn key(&self, i: usize) -> usize {
        i
    }
    fn capture(&mut self, key: usize, out: &mut Entry<usize>) -> bool {
        if !self.alive[key] {
            return false;
        }
        out.key = key;
        out.life = self.life[key];
        out.position = Vector3::new(self.x[key], 0.0, 5.0);
        out.center = out.position;
        out.radius = 0.5;
        out.shapes.clear();
        if !self.hidden[key] {
            out.shapes.push((&HitShape {
                r#type: if self.topology {
                    ShapeType::Box
                } else {
                    ShapeType::Sphere
                },
                part: ShapePart::Body,
                center: [self.x[key], 0.0, 5.0],
                radius: 0.5,
                half: if self.topology {
                    Half::Vec3([0.5; 3])
                } else {
                    Half::F64(0.0)
                },
                half_size: [0.5; 3],
                yaw: 0.0,
            }).into());
        }
        true
    }
    fn eligible(&mut self, key: usize, life: f64, riders: bool) -> bool {
        self.alive[key] && self.life[key] == life && (!self.hidden[key] || riders)
    }
}
fn history() -> History<usize> {
    let mut h = History::new(48);
    h.reserve(3, 0, 10);
    h
}
#[test]
fn instant_rewind_uses_past_shapes_and_current_life_and_keeps_ordered_ties() {
    let mut w = Bodies::default();
    let mut h = history();
    h.capture(0, &mut w);
    w.x = [2.0; 3];
    h.capture(1, &mut w);
    let o = Vector3::default();
    let d = Vector3::new(0.0, 0.0, 1.0);
    let hit = h.raycast(0.0, &o, &d, 10.0, None, true, &mut w, &mut |_| true).unwrap();
    assert_eq!(hit.key, 0);
    assert_eq!(hit.distance, 4.5);
    w.life[0] = 2.0;
    w.alive[1] = false;
    assert_eq!(
        h.raycast(0.0, &o, &d, 10.0, None, true, &mut w, &mut |_| true)
            .unwrap()
            .key,
        2
    );
    assert!(h
        .raycast(0.0, &o, &d, 10.0, Some(2), true, &mut w, &mut |_| true)
        .is_none());
}
#[test]
fn interpolation_skips_teleports_and_new_lives_and_keeps_earlier_stance_topology() {
    let mut w = Bodies::default();
    let mut h = history();
    h.capture(0, &mut w);
    w.x = [2.0, 3.0, 1.0];
    w.life[2] = 2.0;
    h.capture(1, &mut w);
    assert_eq!(h.position(0, 1.0, 0.5), Some(Vector3::new(1.0, 0.0, 5.0)));
    assert!(h.position(1, 1.0, 0.5).is_none());
    assert!(h.position(2, 1.0, 0.5).is_none());
    w.topology = true;
    w.x[0] = 0.0;
    h.capture(2, &mut w);
    assert_eq!(h.position(0, 1.0, 1.5), Some(Vector3::new(2.0, 0.0, 5.0)));
}
#[test]
fn shot_budget_requires_the_shooters_captured_life_and_a_finite_current_view() {
    let mut w = Bodies::default();
    let mut h = history();
    for tick in 0..100 {
        h.capture(tick, &mut w);
    }
    assert_eq!(
        h.shot_tick(0, 1.0, true, true, false, 1.0, Some(0.0), Some(0.0)),
        Some(67.0)
    );
    assert_eq!(h.shot_tick(0, 1.0, true, true, false, 1.0, None, None), Some(51.0));
    assert!(h.shot_tick(0, 2.0, true, true, false, 90.0, None, None).is_none());
    assert!(h.shot_tick(0, 1.0, true, true, true, 90.0, None, None).is_none());
    assert!(h.shot_tick(0, 1.0, true, true, false, f64::NAN, None, None).is_none());
    assert!(h.shot_tick(0, 1.0, true, true, false, 100.0, None, None).is_none());
}

#[test]
fn warmed_history_capture_and_fractional_rays_keep_their_storage() {
    let mut w = Bodies::default();
    let mut h = history();
    h.capture(0, &mut w);
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|on| on.set(true));
    for tick in 1..=1200 {
        w.x[0] = (tick % 2) as f64;
        h.capture(tick, &mut w);
        let _ = h.raycast(
            tick as f64 - 0.5,
            &Vector3::default(),
            &Vector3::new(0.0, 0.0, 1.0),
            10.0,
            None,
            true,
            &mut w,
            &mut |_| true,
        );
    }
    TRACK.with(|on| on.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

// The saved half replaces publication-only halfSize, without quantizing any
// geometry. Test each topology through capture and a fractional rewind ray.
#[test]
fn saved_shapes_keep_all_intersection_topologies_and_normals() {
    struct ShapeWorld(HitShape);
    impl World for ShapeWorld {
        type Key = usize;
        fn count(&self) -> usize { 1 }
        fn key(&self, _: usize) -> usize { 0 }
        fn capture(&mut self, _: usize, out: &mut Entry<usize>) -> bool {
            out.life=1.0;out.center=Vector3::new(0.0,0.0,5.0);out.radius=2.0;
            out.position=out.center;out.shapes.clear();out.shapes.push((&self.0).into());true
        }
        fn eligible(&mut self, _: usize, _: f64, _: bool) -> bool { true }
    }
    let origin=Vector3::default();let dir=Vector3::new(0.0,0.0,1.0);
    for kind in [ShapeType::Sphere,ShapeType::Cylinder,ShapeType::Box,ShapeType::Capsule] {
        let mut world=ShapeWorld(HitShape {r#type:kind,part:ShapePart::Body,center:[0.0,0.0,5.0],
            radius:0.5,half:if matches!(kind,ShapeType::Box|ShapeType::Capsule) {
                Half::Vec3([0.0,0.75,0.0])
            } else {Half::F64(0.75)},yaw:0.0,half_size:[0.0,0.75,0.0]});
        // Use nondegenerate box extents. Capsule half is its axis vector.
        if kind==ShapeType::Box {world.0.half=Half::Vec3([0.5,0.75,0.5]);}
        let expected=4.5f64;
        let point=Vector3::new(0.0,0.0,4.5);
        let normal=Vector3::new(0.0,0.0,-1.0);
        let mut history=History::new(48);history.reserve(1,0,10);
        history.capture(0,&mut world);history.capture(1,&mut world);
        let hit=history.raycast(0.5,&origin,&dir,10.0,None,true,&mut world,&mut |_|true).unwrap();
        assert_eq!(hit.distance.to_bits(),expected.to_bits());assert_eq!(hit.point,point);assert_eq!(hit.normal,normal);
    }
    assert!(std::mem::size_of::<bfe_sim::combat::history::SavedShape>()<std::mem::size_of::<HitShape>());
}
