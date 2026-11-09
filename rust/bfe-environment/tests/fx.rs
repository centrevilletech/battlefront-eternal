//! The pooled state's rules, and that a frame allocates nothing.

use bfe_environment::fx::{Anchor, Fx, Smoke};
use bfe_environment::layout::result as rs;
use bfe_environment::random::Random;
use bfe_environment::Destruction;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use three::{Matrix4, Vector3};

/// Counts the allocations each test thread makes, so a frame can be shown to
/// make none whatever the other tests are doing.
struct Counting;
thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
fn allocations() -> usize {
    ALLOCATIONS.with(|c| c.get())
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|c| c.set(c.get() + 1));
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.with(|c| c.set(c.get() + 1));
        System.realloc(ptr, layout, new_size)
    }
}
#[global_allocator]
static COUNTING: Counting = Counting;

fn spark(fx: &mut Fx, rng: &mut Random, life: f64, x: f64) {
    fx.hot_particle(rng, 0.0, &Vector3::new(x, 0.0, 0.0), &Vector3::default(), life, 0.1, 0.2, 1.0, 1.0, 1.0, -1e4, 0.0);
}

fn step(fx: &mut Fx, dt: f64) -> Vec<f64> {
    let mut result = vec![0.0; rs::STRIDE];
    fx.update(dt, &mut result);
    result
}

#[test]
fn a_spent_particle_hands_its_slot_to_the_last_and_a_full_pool_drops() {
    let mut fx = Fx::new(2, 2, 1);
    let mut rng = Random::new(1);
    spark(&mut fx, &mut rng, 0.1, 1.0);
    spark(&mut fx, &mut rng, 50.0, 9.0);
    spark(&mut fx, &mut rng, 1.0, 5.0);
    assert_eq!((fx.hot.count, fx.hot.dropped, fx.hot.emitted), (2, 1, 2));
    for _ in 0..12 {
        step(&mut fx, 1.0 / 60.0);
    }
    assert_eq!(fx.hot.count, 1);
    assert_eq!(fx.hot.attributes[0][0], 9.0, "the survivor moved into the empty slot");
}

#[test]
fn the_clock_rebases_and_births_move_with_it() {
    let mut fx = Fx::new(2, 2, 1);
    let mut rng = Random::new(1);
    fx.smoke_particle(&mut rng, 0.0, &Vector3::default(), &Vector3::default(), 10_000.0, 0.1, 0.2, 0.0, 1.0, 1.0, Smoke::default());
    step(&mut fx, 1.0);
    let born = fx.smoke.attributes[0][3];
    let result = step(&mut fx, 700.0);
    assert!((fx.time - 101.0).abs() < 1e-9);
    assert!((fx.smoke.attributes[0][3] - (born - 600.0)).abs() < 1e-3);
    assert_eq!(result[rs::SMOKE_DIRTY], 0.0, "a rebase rewrites every record");
}

#[test]
fn arcs_follow_what_they_hang_from_and_end() {
    let mut fx = Fx::new(1, 1, 2);
    let mut rng = Random::new(1);
    let mut m = Matrix4::default();
    m.set_position(0.0, 1.0, 0.0);
    let carried = Anchor::piece(1, m.elements);
    assert!(fx.arc(&mut rng, carried, &Vector3::default(), Anchor::WORLD, &Vector3::new(1.0, 1.0, 0.0), 0.1, 0.01, 1.0));
    step(&mut fx, 1.0 / 60.0);
    assert_eq!(fx.arcs.live, 1);
    assert!(fx.arcs.index >= 72, "the main strand at least");
    assert_eq!(fx.arcs.positions[1], 1.0, "it starts where its holder is");
    // The holder moves up: the page's owner refreshes the matrix before the step.
    m.set_position(0.0, 2.0, 0.0);
    fx.arcs.refresh(&mut |a| a.matrix = m.elements);
    step(&mut fx, 1.0 / 60.0);
    assert_eq!(fx.arcs.positions[1], 2.0);
    for _ in 0..8 {
        step(&mut fx, 1.0 / 60.0);
    }
    assert_eq!(fx.arcs.live, 0);
}

#[test]
fn a_frame_of_the_effects_allocates_nothing() {
    let mut fx = Fx::new(64, 64, 4);
    let mut rng = Random::new(5);
    let mut m = Matrix4::default();
    m.set_position(0.0, 1.0, 0.0);
    let mut result = vec![0.0; rs::STRIDE];
    // From the first frame: nothing is allocated after construction.
    let before = allocations();
    for frame in 0..400 {
        for n in 0..4 {
            if n % 2 == 0 {
                spark(&mut fx, &mut rng, 0.3, frame as f64);
            } else {
                fx.smoke_particle(&mut rng, 0.0, &Vector3::default(), &Vector3::default(), 0.6, 0.1, 0.2, 0.0, 1.0, 1.0, Smoke::default());
            }
        }
        if frame % 20 == 0 {
            fx.arc(&mut rng, Anchor::piece(1, m.elements), &Vector3::default(), Anchor::WORLD, &Vector3::new(1.0, 1.0, 1.0), 0.2, 0.01, 1.0);
        }
        fx.update(if frame == 200 { 700.0 } else { 1.0 / 60.0 }, &mut result);
    }
    assert_eq!(allocations() - before, 0, "400 frames of births, arcs, retirement and a rebase allocated");
}

#[test]
fn the_random_stream_repeats_from_its_seed() {
    let (mut a, mut b) = (Random::new(20041), Random::new(20041));
    assert!((0..100).all(|_| a.next() == b.next()));
    let mut c = Random::new(1);
    assert!((0..1000).map(|_| c.next()).all(|v| (0.0..1.0).contains(&v)));
    let _ = Destruction::new(Fx::new(1, 1, 1), 1);
}

mod atmosphere {
    use super::*;
    use bfe_environment::common::sort_descending;
    use bfe_environment::layout::{atmosphere_config as ac, atmosphere_stats as st, dust_config as dc, square_batch as sb, view as v};
    use bfe_environment::AtmospherePhase;

    #[test]
    fn the_sort_is_stable_and_largest_first() {
        let keys = [1.0, 3.0, 3.0, 2.0, 3.0, 0.5];
        let mut order: Vec<u32> = (0..6).collect();
        let mut scratch = vec![0; 6];
        sort_descending(&keys, &mut order, &mut scratch);
        assert_eq!(order, [1, 2, 4, 3, 0, 5], "equal depths keep the order they were gathered in");
    }

    fn phase() -> AtmospherePhase {
        let mut p = AtmospherePhase::new(1, 1, true);
        let c = &mut p.config;
        c[ac::HAS_FOG] = 1.0;
        c[ac::NEAR] = 90.0;
        c[ac::FAR] = 120.0;
        c[ac::CELL] = 256.0;
        c[ac::SIZE] = 28.0;
        c[ac::DENSITY] = 60.0;
        c[ac::VELOCITY] = 5.0;
        c[ac::ROTATION] = 0.1;
        c[ac::ALPHA] = 0.5;
        c[ac::HEIGHT] = 16.0;
        c[ac::CLASSES] = 1.0;
        let d = &mut c[ac::STRIDE..];
        d[dc::COUNT] = 25.0;
        d[dc::MIN_POS..dc::MIN_POS + 3].copy_from_slice(&[-50.0, 4.0, -50.0]);
        d[dc::MAX_POS..dc::MAX_POS + 3].copy_from_slice(&[-10.0, 12.0, -10.0]);
        d[dc::MIN_VEL..dc::MIN_VEL + 3].copy_from_slice(&[2.0, 0.0, 2.0]);
        d[dc::MAX_VEL..dc::MAX_VEL + 3].copy_from_slice(&[10.0, 1.0, 10.0]);
        d[dc::MIN_SIZE] = 15.0;
        d[dc::MAX_SIZE] = 20.0;
        d[dc::MIN_LIFETIME] = 3.0;
        d[dc::MAX_LIFETIME] = 5.0;
        d[dc::ALPHA] = 0.5;
        d[dc::FADE_MIN] = 50.0;
        d[dc::FADE_MAX] = 60.0;
        d[dc::RADIUS] = 70.0;
        d[dc::MAX_DISTANCE] = 150.0;
        d[dc::HEIGHT_SCALE] = 0.2;
        d[dc::SEED] = 7.0;
        d[dc::INSTANCES] = 1.0;
        p.instances[..3].copy_from_slice(&[30.0, 0.0, 0.0]);
        p.start();
        p
    }

    /// A camera at (x, 2, 0) looking down +x, 60 degrees, 16:9.
    fn look(p: &mut AtmospherePhase, x: f64, dt: f64) {
        let v = &mut p.view;
        v[v::DT] = dt;
        let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        // Looking down +x: right is -z, forward (-column 2) is +x.
        let world = [0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, x, 2.0, 0.0, 1.0];
        v[v::CAMERA..v::CAMERA + 16].copy_from_slice(&world);
        v[v::INVERSE..v::INVERSE + 16].copy_from_slice(&identity);
        let f = 1.0 / (std::f64::consts::PI / 6.0).tan();
        let projection = [f / (16.0 / 9.0), 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 0.0, 0.0, -1.0, 0.0];
        v[v::PROJECTION..v::PROJECTION + 16].copy_from_slice(&projection);
        v[v::FOV] = 60.0;
        v[v::ASPECT] = 16.0 / 9.0;
    }

    #[test]
    fn a_frame_gathers_squares_far_to_near_and_allocates_nothing() {
        let mut p = phase();
        // From the first frame: nothing is allocated after the phase is built.
        let before = allocations();
        let mut asked = 0;
        let mut flat = |_: f64, _: f64| {
            asked += 1;
            0.0
        };
        for frame in 0..4 {
            look(&mut p, 0.0, 1.0 / 30.0);
            p.update(&mut flat);
            let _ = frame;
        }
        let fog = p.result[sb::COUNT + st::STRIDE] as usize;
        assert!(fog > 0, "the band has squares in view");
        // Depth is the square's x here: the batch is farthest first.
        let c = &p.atmosphere.as_ref().unwrap().batches[0].center;
        assert!((1..fog).all(|i| c[i * 4] <= c[(i - 1) * 4] + 1e-3));
        let mut calm = |_: f64, _: f64| 0.0;
        for frame in 0..300 {
            look(&mut p, frame as f64 * 0.5, if frame % 7 == 0 { 0.5 } else { 1.0 / 60.0 });
            p.update(&mut calm);
        }
        assert_eq!(allocations() - before, 0, "a frame allocates nothing");
    }
}

mod snow {
    use super::*;
    use bfe_environment::layout::{snow_config as sc, snow_result as sr, view as v};
    use bfe_environment::SnowPhase;

    fn phase() -> SnowPhase {
        let mut p = SnowPhase::new();
        let c = &mut p.config;
        c[sc::COUNT] = 100.0;
        c[sc::FALL] = 2.0;
        c[sc::WANDER] = 0.8;
        c[sc::ALPHA_MIN] = 0.3;
        c[sc::ALPHA_MAX] = 0.45;
        c[sc::SPIN_RANGE] = 2.0;
        c[sc::WIND] = 4.0;
        c[sc::WIND + 1] = 0.3;
        c[sc::GUST_RANGE] = 1.0;
        c[sc::GUST_RATE] = 0.2;
        c[sc::RANGE] = 15.0;
        c[sc::BOX_LIMIT] = 327.0;
        p.start();
        p
    }

    fn look(p: &mut SnowPhase, x: f64, dt: f64) {
        let view = &mut p.view;
        view[v::DT] = dt;
        let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let world = [0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, x, 2.0, 0.0, 1.0];
        view[v::CAMERA..v::CAMERA + 16].copy_from_slice(&world);
        view[v::INVERSE..v::INVERSE + 16].copy_from_slice(&identity);
        let f = 1.0 / (std::f64::consts::PI / 6.0).tan();
        let projection = [f / (16.0 / 9.0), 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 0.0, 0.0, -1.0, 0.0];
        view[v::PROJECTION..v::PROJECTION + 16].copy_from_slice(&projection);
        view[v::FOV] = 60.0;
        view[v::ASPECT] = 16.0 / 9.0;
        view[v::POSITION..v::POSITION + 3].copy_from_slice(&[x, 2.0, 0.0]);
    }

    #[test]
    fn snow_fills_the_view_with_boxes_and_a_frame_allocates_nothing() {
        let mut p = phase();
        let mut open = |_: f64, _: f64, _: f64| -100000.0;
        look(&mut p, 0.0, 1.0 / 60.0);
        p.update(&mut open);
        assert!(p.result[sr::BOXES] > 0.0);
        assert_eq!(p.result[sr::INSTANCES], p.result[sr::BOXES] * 100.0);
        assert_eq!(p.result[sr::COVERED], 1.0, "the first frame fills the whole cover cache");
        let before = allocations();
        for frame in 0..300 {
            look(&mut p, frame as f64 * 0.3, if frame % 11 == 0 { 0.5 } else { 1.0 / 60.0 });
            p.update(&mut open);
        }
        assert_eq!(allocations() - before, 0, "a frame allocates nothing");
    }

    #[test]
    fn the_cover_cache_asks_for_new_cells_within_its_budget() {
        use bfe_environment::cover::Cover;
        let mut cover = Cover::new(12.5, 16);
        let asked = Cell::new(0);
        let mut roof = |_: f64, _: f64, _: f64| {
            asked.set(asked.get() + 1);
            20.0
        };
        assert_eq!((cover.update([0.0, 10.0, 0.0], 256, &mut roof).0, asked.get()), (true, 256));
        assert!(cover.heights.iter().all(|&h| h == 20.0));
        assert!(!cover.update([1000.0, 10.0, 0.0], 32, &mut roof).0, "a teleport spends the budget over several frames");
        assert_eq!(asked.get(), 256 + 32);
        cover.reset(12.5, 16);
        assert!(cover.heights.iter().all(|&h| h == -100000.0), "a reset cache is empty");
    }
}

mod sky {
    use super::*;
    use bfe_environment::layout::{sky_class as sc, sky_config as k, sky_frame as sf, sky_result as sr};
    use bfe_environment::sky::haze;
    use bfe_environment::SkyPhase;

    fn phase(enhanced: bool) -> SkyPhase {
        let mut p = SkyPhase::new(1);
        p.config[k::SEED] = 9.0;
        p.config[k::CLASSES] = 1.0;
        p.config[k::ENHANCED] = f64::from(enhanced);
        let c = &mut p.config[k::STRIDE..];
        c[sc::COUNT] = 20.0;
        c[sc::DISTANCE] = 600.0;
        c[sc::HEIGHT..sc::HEIGHT + 2].copy_from_slice(&[80.0, 140.0]);
        c[sc::IN_DIRECTION] = 0.5;
        c[sc::VELOCITY..sc::VELOCITY + 6].copy_from_slice(&[0.0, 0.0, -10.0, 10.0, 80.0, 120.0]);
        c[sc::LIFE_TIME] = 30.0;
        c[sc::RADIUS] = 5.0;
        c[sc::CLASSIC] = 1.0;
        c[sc::ENHANCED] = 1.0;
        p.start();
        p
    }

    /// With the page's sky probe in place a far, low ship takes on the sky behind
    /// it; without one, none does.
    #[test]
    fn far_low_ships_take_on_the_sky_only_with_the_probe() {
        let mist = |misty: f64| {
            let mut p = phase(true);
            p.frame[sf::DT] = 1.0 / 60.0;
            p.frame[sf::HAS_EYE] = 1.0;
            p.frame[sf::EYE..sf::EYE + 3].copy_from_slice(&[0.0, 100.0, 0.0]);
            p.frame[sf::FAR] = 500.0;
            p.frame[sf::VISIBLE] = 1.0;
            p.frame[sf::MISTY] = misty;
            for _ in 0..400 {
                p.update();
            }
            let sky = p.sky.as_ref().unwrap();
            (0..20).map(|i| sky.array(0, 1)[i * 4 + 1].max(sky.array(0, 3)[i * 4 + 1])).fold(0.0f32, f32::max)
        };
        assert_eq!(mist(0.0), 0.0, "no probe, no haze");
        assert!(mist(1.0) > 0.0, "the probe gives the far ones their haze");
    }

    #[test]
    fn haze_is_none_overhead_and_near_and_most_far_and_low() {
        assert_eq!(haze(100.0, 100.0, 500.0), 0.0);
        assert_eq!(haze(100.0, 0.0, 500.0), 0.0, "near");
        assert!((haze(1500.0, 0.0, 500.0) - 0.7).abs() < 1e-12);
    }

    #[test]
    fn ships_fade_in_are_sorted_far_to_near_and_a_frame_allocates_nothing() {
        for enhanced in [false, true] {
            let mut p = phase(enhanced);
            let frame = |p: &mut SkyPhase, dt: f64| {
                p.frame[sf::DT] = dt;
                p.frame[sf::HAS_EYE] = 1.0;
                p.frame[sf::EYE..sf::EYE + 3].copy_from_slice(&[0.0, 50.0, 0.0]);
                p.frame[sf::FAR] = 500.0;
                p.frame[sf::VISIBLE] = 1.0;
                p.update();
            };
            frame(&mut p, 1.0 / 60.0);
            assert_eq!(p.result[sr::SOLIDS] + p.result[sr::FADINGS], 20.0, "every ship is drawn");
            let before = allocations();
            for i in 0..600 {
                frame(&mut p, if i % 50 == 0 { 0.5 } else { 1.0 / 60.0 });
            }
            assert_eq!(allocations() - before, 0, "a frame allocates nothing (enhanced {enhanced})");
        }
    }
}

mod weather {
    use super::*;
    use bfe_environment::layout::{weather_frame as wf, weather_result as wr};
    use bfe_environment::weather::Impact;
    use bfe_environment::WeatherPhase;

    #[test]
    fn rain_lands_splashes_in_a_ring_and_a_frame_allocates_nothing() {
        let mut p = WeatherPhase::new(8192, true, 12.5, 16);
        let mut height = |_: f64, _: f64, _: f64| 0.0;
        let mut cast = |_: f64, _: f64, _: f64| Impact { y: 0.0, normal: [0.0, 1.0, 0.0], water: false };
        for frame in 0..400 {
            p.frame[wf::DT] = if frame % 40 == 0 { 0.3 } else { 1.0 / 60.0 };
            p.frame[wf::POSITION..wf::POSITION + 3].copy_from_slice(&[frame as f64 * 0.1, 1.0, 0.0]);
            if frame == 100 {
                let before = allocations();
                for _ in 0..200 {
                    p.update(&mut height, &mut cast);
                }
                assert_eq!(allocations() - before, 0, "a frame allocates nothing");
            }
            p.update(&mut height, &mut cast);
        }
        assert!(p.result[wr::IMPACTS_NEXT] < 96.0);
        assert!(p.weather.impacts.locations.iter().any(|&v| v > 0.0), "splashes landed");
        // Switching modes keeps the memory where it is.
        let ptr = p.weather.cover.heights.as_ptr();
        p.weather.set_mode(false, 12.5, 16);
        assert_eq!(p.weather.cover.heights.as_ptr(), ptr);
    }
}

mod placed {
    use super::*;
    use bfe_environment::layout::{environment_frame as ef, environment_instance as ei, environment_result as er, view as v};
    use bfe_environment::EnvironmentPhase;

    #[test]
    fn instances_in_view_are_found_and_a_frame_allocates_nothing() {
        let mut p = EnvironmentPhase::new(3);
        // A steam vent ahead, a torch behind the camera, a fountain ahead.
        let rows = [([0.0, 0.0, -20.0], false, false), ([0.0, 0.0, 30.0], true, false), ([5.0, 0.0, -30.0], false, true)];
        for (i, (at, classic, jet)) in rows.iter().enumerate() {
            let c = &mut p.config[i * ei::STRIDE..(i + 1) * ei::STRIDE];
            c[ei::SPHERE..ei::SPHERE + 3].copy_from_slice(at);
            c[ei::SPHERE + 3] = 7.0;
            c[ei::POSITION..ei::POSITION + 3].copy_from_slice(at);
            c[ei::CLASSIC] = f64::from(*classic);
            c[ei::JET] = f64::from(*jet);
        }
        p.start();
        let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        // At the origin looking down -z.
        p.view[v::CAMERA..v::CAMERA + 16].copy_from_slice(&identity);
        p.view[v::INVERSE..v::INVERSE + 16].copy_from_slice(&identity);
        let f = 1.0 / (std::f64::consts::PI / 6.0).tan();
        p.view[v::PROJECTION..v::PROJECTION + 16].copy_from_slice(&[f, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 0.0, 0.0, -1.0, 0.0]);
        p.frame[ef::REACH] = 90.0;
        p.frame[ef::ENHANCED] = 1.0;
        p.update();
        assert_eq!((p.result[er::SOURCES], p.result[er::VOLUMES], p.result[er::FOUNTAINS]), (2.0, 1.0, 1.0));
        assert_eq!(p.result[er::STRIDE + 3], 0.0, "the volume is instance 0");
        assert_eq!(p.result[er::STRIDE + 6], 2.0, "the fountain is instance 2");
        let before = allocations();
        for _ in 0..300 {
            p.update();
        }
        assert_eq!(allocations() - before, 0, "a frame allocates nothing");
    }
}

mod destruction {
    use super::*;
    use bfe_environment::layout::{
        dst, dst_burst as burst_at, dst_chunk as dc, dst_chunk_at as ca, dst_command as cmd, dst_command_kind as kind, dst_frame as df, dst_piece as dp, dst_result as dr,
        dst_result_at as ra, dst_slot_state as sl, dst_slot_static as sst, dst_station as dstn, dst_station_state as dss,
    };
    use bfe_environment::solver::Hit;

    /// A floor at y = 0.
    fn floor(origin: &Vector3, direction: &Vector3, far: f64) -> Option<Hit> {
        if direction.y >= 0.0 || origin.y < 0.0 {
            return None;
        }
        let t = origin.y / -direction.y;
        if t > far {
            return None;
        }
        let mut p = *origin;
        p.add_scaled_vector(direction, t);
        Some(Hit { point: p, normal: Vector3::new(0.0, 1.0, 0.0) })
    }

    const IDENTITY: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

    fn setup() -> Destruction {
        let mut d = Destruction::new(Fx::new(1536, 1536, 48), 20041);
        // One shape: a box's corners.
        let mut k = 0;
        for x in [-0.1, 0.1] {
            for y in [-0.05, 0.05] {
                for z in [-0.05, 0.05] {
                    d.shape_points[k * 3..k * 3 + 3].copy_from_slice(&[x, y, z]);
                    k += 1;
                }
            }
        }
        d.register_shape(0, [0.0; 3], [0.1, 0.05, 0.05], 0.15, [1.0 / 0.01, 1.0 / 0.01, 1.0 / 0.01], 8);
        d
    }

    fn command(d: &mut Destruction, n: usize, kind: f64) -> &mut [f64] {
        let c = &mut d.commands[n * cmd::STRIDE..(n + 1) * cmd::STRIDE];
        c.fill(0.0);
        c[cmd::KIND] = kind;
        c
    }

    /// A droid blown apart whose `chunks` chunks (one shape each) are written
    /// from frame `first` of the commands' block.
    fn blow_apart(d: &mut Destruction, n: usize, first: usize, chunks: usize) {
        for k in 0..chunks {
            let at = (first + k) * dc::STRIDE;
            d.chunks[at + dc::SHAPE] = 0.0;
            d.chunks[at + dc::SPEED] = 8.0;
            d.chunks[at + dc::MATRIX..at + dc::MATRIX + 16].copy_from_slice(&IDENTITY);
        }
        let c = command(d, n, kind::BLOW_APART);
        c[cmd::SLOT] = dst::SLOTS as f64;
        c[cmd::FRAME_START] = first as f64;
        c[cmd::FRAME_COUNT] = chunks as f64;
        c[cmd::WEAPON] = -1.0;
    }

    /// One frame the way the page makes it: the droids about to burst bring their
    /// chunks, then the phase steps.
    fn frame(d: &mut Destruction, n: usize, commands: usize, wrecked: bool) {
        let mut cast = floor;
        d.frame[df::DT] = if n.is_multiple_of(7) { 0.05 } else { 1.0 / 60.0 };
        d.frame[df::ENHANCED] = 1.0;
        d.frame[df::HAS_EYE] = 1.0;
        d.frame[df::EYE + 1] = 3.0;
        d.frame[df::COMMANDS] = commands as f64;
        d.frame[df::STATIONS] = 1.0;
        d.station_state[dss::DESTROYED] = f64::from(wrecked);
        d.station_state[dss::MAX_HEALTH] = 80.0;
        d.slot_state[sl::VALID] = 1.0;
        d.prepare();
        for k in 0..d.result[dr::BURSTS] as usize {
            let slot = d.result[ra::BURSTS + k] as usize;
            // Its chunks: one piece at the origin.
            let at = (ca::BURSTS + slot * dst::UNIT_CHUNKS) * dc::STRIDE;
            d.chunks[at + dc::SHAPE] = 0.0;
            d.chunks[at + dc::SPEED] = 8.0;
            d.chunks[at + dc::MATRIX..at + dc::MATRIX + 16].copy_from_slice(&IDENTITY);
            let b = slot * burst_at::STRIDE;
            d.burst_blocks[b + burst_at::START] = (ca::BURSTS + slot * dst::UNIT_CHUNKS) as f64;
            d.burst_blocks[b + burst_at::COUNT] = 1.0;
            d.burst_blocks[b + burst_at::WEAPON] = -1.0;
        }
        d.update(&mut cast);
    }

    fn station(d: &mut Destruction) {
        d.station_config[dstn::MIN..dstn::MIN + 3].copy_from_slice(&[-0.5, 0.0, -0.5]);
        d.station_config[dstn::MAX..dstn::MAX + 3].copy_from_slice(&[0.5, 1.2, 0.5]);
        d.station_config[dstn::CENTER..dstn::CENTER + 3].copy_from_slice(&[0.0, 0.6, 0.0]);
        d.watch(1);
    }

    /// A droid's slot 0: two bones and a joint, at rest.
    fn droid(d: &mut Destruction) {
        let base = 0;
        d.slot_static[base + sst::BONE_COUNT] = 2.0;
        d.slot_static[base + sst::JOINT_COUNT] = 2.0;
        d.slot_static[base + sst::BONES] = 0.0;
        d.slot_static[base + sst::BONES + 1] = 1.0;
        d.slot_static[base + sst::JOINTS] = 1.0;
        d.slot_static[base + sst::JOINTS + 1] = 0.0;
        for b in 0..2 {
            d.slot_matrices[b * 16..(b + 1) * 16].copy_from_slice(&IDENTITY);
        }
    }

    #[test]
    fn a_battle_of_blasts_deaths_and_wrecks_allocates_nothing_a_frame() {
        // From the very first frame: nothing is allocated after construction, so
        // a view of the module's memory the page took once can never go stale.
        let mut d = setup();
        station(&mut d);
        droid(&mut d);
        let before = allocations();
        for n in 0..900 {
            let mut commands = 0;
            if n % 100 == 5 {
                let c = command(&mut d, 0, kind::BLAST);
                c[cmd::AT + 1] = 0.4;
                c[cmd::SCALE] = 1.2;
                c[cmd::ELECTRIC] = 1.0;
                commands = 1;
            } else if n % 100 == 20 {
                blow_apart(&mut d, 0, 0, 1);
                commands = 1;
            } else if n % 100 == 40 {
                let c = command(&mut d, 0, kind::DIED);
                c[cmd::SLOT] = 0.0;
                commands = 1;
            }
            frame(&mut d, n, commands, n % 400 > 100 && n % 400 < 300);
        }
        assert_eq!(allocations() - before, 0, "900 frames of blasts, deaths, wrecks and pieces allocated");
        assert!(d.result[dr::BLASTS] > 5.0, "blasts were made");
    }

    /// The page may write anything: counts past the block, handles and bones past
    /// their tables, frames that are not numbers. None of it may panic, and the
    /// phase must still make its frames afterwards.
    #[test]
    fn a_page_that_writes_out_of_range_numbers_cannot_stop_the_phase() {
        let mut d = setup();
        droid(&mut d);
        // More commands than the block holds, and commands pointing past every block.
        d.frame[df::COMMANDS] = 100_000.0;
        for n in 0..dst::COMMANDS {
            let c = command(&mut d, n, kind::BLOW_APART);
            c[cmd::SLOT] = 1.0e9;
            c[cmd::FRAME_START] = 1.0e9;
            c[cmd::FRAME_COUNT] = 1.0e9;
            c[cmd::WEAPON] = 1.0e9;
            c[cmd::MATRIX_START] = 1.0e9;
        }
        let c = command(&mut d, 0, kind::DEATH_BLAST);
        c[cmd::MATRIX_START] = 1.0e9;
        c[cmd::MATRIX_COUNT] = 1.0e9;
        let c = command(&mut d, 1, kind::DIED);
        c[cmd::SLOT] = 99.0;
        // A body with more bones and joints than the stores, and indices past them.
        d.slot_static[sst::BONE_COUNT] = 1.0e6;
        d.slot_static[sst::JOINT_COUNT] = 1.0e6;
        for k in 0..dst::BONES {
            d.slot_static[sst::BONES + k] = 1.0e6;
        }
        for k in 0..dst::JOINT_NUMBERS {
            d.slot_static[sst::JOINTS + k] = 1.0e6;
        }
        let c = command(&mut d, 2, kind::DIED);
        c[cmd::SLOT] = 0.0;
        // Shapes: a handle past the table, a registration of too many points.
        d.register_shape(10_000, [0.0; 3], [1.0; 3], 1.0, [1.0; 3], 1000);
        d.register_shape(dst::SHAPES - 1, [0.0; 3], [1.0; 3], 1.0, [1.0; 3], 1000);
        d.frame[df::STATIONS] = 1.0e6;
        for n in 0..300 {
            frame(&mut d, n, if n == 0 { 100_000 } else { 0 }, false);
        }
        assert!(d.fx.time > 0.0, "the phase kept making frames");
        assert!(d.result[dr::EVENTS] <= dst::EVENTS as f64);
        // Frames that are not numbers are refused, not spread.
        for k in 0..dc::STRIDE * 2 {
            d.chunks[k] = if k % dc::STRIDE >= dc::MATRIX { f64::NAN } else { d.chunks[k] };
        }
        let before = d.result[dr::PIECES];
        blow_apart(&mut d, 0, 0, 2);
        for k in 0..2 {
            d.chunks[k * dc::STRIDE + dc::MATRIX..(k + 1) * dc::STRIDE].fill(f64::NAN);
        }
        frame(&mut d, 1, 1, false);
        assert!(d.result[dr::PIECES] <= before, "a frame that is not a number makes no piece");
        assert!((0..d.result[dr::PIECES] as usize).all(|k| d.result[ra::PIECES + k * dp::STRIDE + dp::POSITION].is_finite()), "and nothing is left that is not a number");
    }

    /// Events written with no frame to run them (a load, a pause) fill a queue the
    /// page then runs in pieces: running a full queue and a half one later gives the
    /// same births as one frame would.
    #[test]
    fn commands_run_in_pieces_make_the_same_births_as_in_one_frame() {
        let run = |split: bool| {
            let mut d = setup();
            let mut cast = floor;
            for k in 0..dst::COMMANDS {
                let c = command(&mut d, k, kind::BLAST);
                c[cmd::AT] = k as f64 * 0.1;
                c[cmd::AT + 1] = 0.4;
                c[cmd::SCALE] = 0.6;
            }
            d.frame[df::ENHANCED] = 1.0;
            if split {
                d.frame[df::COMMANDS] = (dst::COMMANDS / 2) as f64;
                d.apply(&mut cast);
                for k in 0..dst::COMMANDS / 2 {
                    let src: Vec<f64> = d.commands[(k + dst::COMMANDS / 2) * cmd::STRIDE..(k + dst::COMMANDS / 2 + 1) * cmd::STRIDE].to_vec();
                    d.commands[k * cmd::STRIDE..(k + 1) * cmd::STRIDE].copy_from_slice(&src);
                }
                d.frame[df::COMMANDS] = (dst::COMMANDS / 2) as f64;
            } else {
                d.frame[df::COMMANDS] = dst::COMMANDS as f64;
            }
            d.frame[df::DT] = 0.0;
            d.update(&mut cast);
            (d.result[dr::BLASTS], d.fx.hot.count, d.fx.smoke.count, d.fx.hot.attributes[0].iter().map(|v| v.to_bits() as u64).sum::<u64>())
        };
        assert_eq!(run(true), run(false));
    }

    /// The oldest piece gives way when a new one needs the room, at the 40th.
    #[test]
    fn the_oldest_piece_gives_way_at_forty() {
        let mut d = setup();
        let mut cast = floor;
        let mut uids = Vec::new();
        for n in 0..(dst::PIECES / 2 + 4) {
            blow_apart(&mut d, 0, 0, 2);
            d.frame[df::COMMANDS] = 1.0;
            d.frame[df::ENHANCED] = 1.0;
            d.frame[df::DT] = 0.0;
            d.apply(&mut cast);
            let count = d.result[dr::PIECES] as usize;
            uids.push((n, d.result[ra::PIECES + (count - 1) * dp::STRIDE + dp::UID]));
            assert!(count <= dst::PIECES, "never more than the room");
        }
        assert_eq!(d.result[dr::PIECES] as usize, dst::PIECES);
        let first = d.result[ra::PIECES + dp::UID];
        assert!(first > 4.0, "the first pieces made are the ones that went, the oldest left is uid {first}");
    }

    /// A piece that falls out of the world (30 m below where it started) is let go;
    /// far ones and crumbling ones cast no shadow.
    #[test]
    fn a_piece_lost_below_the_world_goes_and_far_ones_cast_no_shadow() {
        let mut d = setup();
        let mut cast = |_: &Vector3, _: &Vector3, _: f64| None;
        blow_apart(&mut d, 0, 0, 1);
        d.frame[df::COMMANDS] = 1.0;
        d.frame[df::ENHANCED] = 1.0;
        d.frame[df::HAS_EYE] = 1.0;
        d.frame[df::EYE..df::EYE + 3].copy_from_slice(&[100.0, 0.0, 0.0]);
        d.frame[df::DT] = 0.0;
        d.update(&mut cast);
        assert_eq!(d.result[dr::PIECES], 1.0);
        d.frame[df::DT] = 1.0 / 60.0;
        d.update(&mut cast);
        assert_eq!(d.result[ra::PIECES + dp::SHADOWS], 0.0, "a piece 100 m from the eye casts no shadow");
        for _ in 0..400 {
            d.update(&mut cast);
        }
        assert_eq!(d.result[dr::PIECES], 0.0, "it fell out of the world and was let go");
    }
}

mod engine_random {
    use bfe_environment::common::EngineRandom;

    /// Two steps of a 32-bit LCG make a draw, the seed xored first with
    /// 0xC71C2830, written out as the rule is told.
    #[test]
    fn two_lcg_steps_a_draw_the_seed_xored_first() {
        let mut s = 1u32 ^ 0xc71c2830;
        let mut reference = || {
            let s1 = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let s2 = s1.wrapping_mul(1664525).wrapping_add(1013904223);
            s = s2;
            (s2 & 0xffff0000) | (s1 >> 16)
        };
        let mut random = EngineRandom::new(1.0);
        for _ in 0..8 {
            assert_eq!(random.int(), reference());
        }
        let floats: Vec<f64> = (0..1000).map(|_| random.float()).collect();
        assert!(floats.iter().all(|f| (0.0..1.0).contains(f)));
        assert!((floats.iter().sum::<f64>() / 1000.0 - 0.5).abs() < 0.05);
    }
}
