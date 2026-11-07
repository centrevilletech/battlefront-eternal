use bfe_presentation::ordnance::Ordnance;
use bfe_presentation::ordnance_layout::{
    call_kind as ck, net_command as nc, net_kind as nk, ordnance_call as cl,
    ordnance_frame as fr, ordnance_header as hd, ordnance_kind as kind, ordnance_limits as lim,
    ordnance_record as rc, ordnance_style as st, VERSION,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Count;
// Per thread: tests run in parallel, and only the tracked frames count.
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
fn tracking() -> bool {
    TRACK.try_with(Cell::get).unwrap_or(false)
}
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if tracking() {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if tracking() {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static ALLOCATOR: Count = Count;

/// A rifle-like class drawn into batch handles 0 (core), 1 (glow), 2 (beam).
fn register(o: &mut Ordnance, index: usize) {
    let s = &mut o.style_input;
    s.fill(0.0);
    s[st::CORE_BATCH] = 0.0;
    s[st::GLOW_BATCH] = 1.0;
    s[st::BEAM_BATCH] = 2.0;
    s[st::LASER_WIDTH] = 0.1;
    s[st::LASER_LENGTH] = 3.0;
    s[st::GLOW_LENGTH] = 6.0;
    s[st::GLOW_RAW] = 6.0;
    s[st::LIGHT_RADIUS] = 4.0;
    s[st::LIGHT_COLOR] = 1.0;
    s[st::GLOW_COLOR] = 1.0;
    s[st::LOOK_COLOR] = 1.0;
    s[st::LOOK_LIGHT_RADIUS] = 3.0;
    s[st::LOOK_LENGTH] = 3.0;
    s[st::LOOK_BRIGHTNESS] = 1.0;
    s[st::GRAVITY] = 1.0;
    o.set_style(index);
}

fn bolt(o: &mut Ordnance, i: usize, key: f64, flown: f64) {
    let at = fr::STRIDE + i * rc::STRIDE;
    let a = &mut o.input;
    a[at..at + rc::STRIDE].fill(0.0);
    a[at + rc::KIND] = kind::BOLT as f64;
    a[at + rc::KEY] = key;
    a[at + rc::ALPHA] = 1.0;
    a[at + rc::FLOWN] = flown;
    a[at + rc::DRAWN] = flown.min(3.0);
    a[at + rc::FADE] = 1.0;
    a[at + rc::SPEED] = 150.0;
    a[at + rc::LIFE] = 2.0;
    a[at + rc::MERGE] = 8.0;
    a[at + rc::DIRECTION + 2] = 1.0;
    a[at + rc::POSITION + 2] = flown;
}

fn frame(o: &mut Ordnance, records: usize, net: usize) {
    o.input[fr::VERSION] = VERSION as f64;
    o.input[fr::RECORDS] = records as f64;
    o.input[fr::NET_COMMANDS] = net as f64;
    o.present();
}

fn sample(o: &mut Ordnance, n: usize, slot: usize, tick: f64) {
    let at = n * nc::STRIDE;
    let a = &mut o.net_input;
    a[at..at + nc::STRIDE].fill(0.0);
    a[at + nc::KIND] = nk::SAMPLE as f64;
    a[at + nc::SLOT] = slot as f64;
    a[at + nc::GENERATION] = 1.0;
    a[at + nc::TYPE] = 1.0;
    a[at + nc::TICK] = tick;
    a[at + nc::EXPIRY_TICK] = tick + 18.0;
    a[at + nc::LIFE] = 1.0;
    a[at + nc::VELOCITY + 2] = 50.0;
    a[at + nc::QUATERNION + 3] = 1.0;
}

#[test]
fn frames_draw_bolts_beams_and_network_rounds_without_allocating() {
    let mut o = Ordnance::new();
    register(&mut o, 0);
    // A warm-up frame, then tracked ones: the stores are all made at setup.
    for tick in 0..4 {
        for i in 0..40 {
            bolt(&mut o, i, i as f64, 1.0 + tick as f64);
        }
        sample(&mut o, 0, 0, tick as f64);
        frame(&mut o, 40, 1);
    }
    TRACK.with(|t| t.set(true));
    for tick in 4..64 {
        for i in 0..40 {
            bolt(&mut o, i, i as f64, 1.0 + tick as f64);
        }
        sample(&mut o, 0, (tick % 8) as usize, tick as f64);
        sample(&mut o, 1, 9, tick as f64);
        frame(&mut o, 40, 2);
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0, "a frame allocated");
    assert_eq!(o.header[hd::STATUS], 0.0);
    assert!(o.counts()[0] > 0 && o.counts()[1] > 0, "core and glow quads were drawn");
    assert!(o.header[hd::CALLS] >= 40.0, "every bolt asked for its light");
}

#[test]
fn full_stores_refuse_work_and_a_frame_never_fails() {
    let mut o = Ordnance::new();
    register(&mut o, 0);
    // More bolts than a batch region holds: the rest are dropped and counted.
    let many = lim::QUADS_PER_HANDLE / 2 + 20;
    for i in 0..many {
        bolt(&mut o, i, i as f64, 2.0);
    }
    frame(&mut o, many, 0);
    assert_eq!(o.counts()[0] as usize, many);
    assert!(o.counts()[1] as usize <= lim::QUADS_PER_HANDLE);
    assert!(o.header[hd::DROPPED_QUADS] > 0.0, "the glow region filled and said so");
    // A count larger than the block is clamped, not read past.
    o.input[fr::RECORDS] = 1e18;
    o.present();
    o.input[fr::RECORDS] = f64::NAN;
    o.present();
    o.input[fr::NET_COMMANDS] = 1e18;
    o.present();
    // More distinct bolts over the frames than the view table holds: old
    // views are recycled, none fails.
    for round in 0..(lim::VIEWS * 3 / 40) {
        for i in 0..40 {
            bolt(&mut o, i, (round * 40 + i) as f64, 1.0);
        }
        o.input[fr::NET_COMMANDS] = 0.0;
        frame(&mut o, 40, 0);
        assert_eq!(o.header[hd::STATUS], 0.0);
    }
    // Commands to slots past the table, unknown kinds, ends for nothing.
    let a = &mut o.net_input;
    a[nc::KIND] = nk::SAMPLE as f64;
    a[nc::SLOT] = 1e12;
    a[nc::STRIDE + nc::KIND] = 99.0;
    a[nc::STRIDE + nc::SLOT] = 3.0;
    a[2 * nc::STRIDE + nc::KIND] = nk::END as f64;
    a[2 * nc::STRIDE + nc::SLOT] = 5.0;
    frame(&mut o, 0, 3);
    assert!(o.header[hd::SKIPPED_NET] >= 2.0);
    // A layout built for another version is refused outright, not drawn.
    o.input[fr::VERSION] = (VERSION + 1) as f64;
    o.present();
    assert_eq!(o.header[hd::STATUS], 1.0);
    assert_eq!(o.header[hd::CALLS], 0.0);
}

#[test]
fn network_projectiles_pair_by_slot_and_end_with_a_remove_call() {
    let mut o = Ordnance::new();
    register(&mut o, 0);
    // A round sampled twice, then silent: it flies on and the kernel ends it.
    sample(&mut o, 0, 7, 10.0);
    sample(&mut o, 1, 7, 16.0);
    let want = 7.0;
    let mut removed = 0;
    for tick in 10..120 {
        o.input[fr::VERSION] = VERSION as f64;
        let at = fr::STRIDE;
        o.input[at..at + rc::STRIDE].fill(0.0);
        o.input[at + rc::KIND] = kind::NET as f64;
        o.input[at + rc::CLOCK_TICK] = tick as f64;
        frame(&mut o, 1, if tick == 10 { 2 } else { 0 });
        for c in 0..o.header[hd::CALLS] as usize {
            if o.calls()[c * cl::STRIDE + cl::KIND] == ck::NET_REMOVE as f64 {
                assert_eq!(o.calls()[c * cl::STRIDE + cl::SOURCE], want);
                removed += 1;
            }
        }
    }
    assert_eq!(removed, 1, "ended once, by its slot");
    assert_eq!(o.header[hd::NET_LIVE], 0.0);
}
