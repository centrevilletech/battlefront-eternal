//! The item presentation phase against the frozen record of the JavaScript
//! it replaced (tests/fixtures/items-presentation/reference.json), bit for
//! bit, and against the stores a battle can fill. The page's part (the
//! collision world and the water, answering the kernel's rounds) is played
//! here by planes and a water level, with the arithmetic the fixture's
//! recorder used.
use bfe_presentation::items::Items;
use bfe_presentation::items_layout::{
    item_answer as an, item_definition as df, item_frame as fr, item_header as hd,
    item_limits as lim, item_record as rc, item_release as rl, item_request as rq,
    item_ripple as rp, item_view as vw, VERSION,
};
use serde_json::Value;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use three::Vector3;

thread_local! {static TRACK:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn hex(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap())
}
fn hex3(v: &Value) -> [f64; 3] {
    let a = v.as_array().unwrap();
    [hex(&a[0]), hex(&a[1]), hex(&a[2])]
}

/// The page's collision: the nearest plane the ray meets the front of.
struct Plane {
    point: Vector3,
    normal: Vector3,
}

struct Mirror {
    serial: f64,
    definition: usize,
    position: [f64; 3],
    quaternion: [f64; 4],
}

type ViewRecord = (usize, [f64; 3], [f64; 4], f64, usize);

#[derive(Default)]
struct Frame {
    /// (slot, position) of every view, by slot.
    views: Vec<ViewRecord>,
    lights: Vec<([f64; 3], f64)>,
    ripples: Vec<[f64; 3]>,
    rounds: usize,
}

struct Page {
    items: Items,
    planes: Vec<Plane>,
    level: Option<f64>,
    mirror: Vec<Option<Mirror>>,
    ripples: Vec<[f64; 3]>,
    limit: usize,
    light_radius: Vec<f64>,
    tracked: bool,
}

impl Page {
    fn new(planes: Vec<Plane>, level: Option<f64>) -> Page {
        Page {
            items: Items::new(),
            planes,
            level,
            mirror: (0..lim::SLOTS).map(|_| None).collect(),
            ripples: Vec::new(),
            limit: lim::ROUNDS,
            light_radius: vec![0.0; lim::DEFINITIONS],
            tracked: false,
        }
    }

    fn define(&mut self, index: usize, d: &Value) {
        let at = index * df::STRIDE;
        let r = &mut self.items.definitions;
        r[at + df::GRAVITY] = hex(&d["gravity"]);
        r[at + df::REBOUND] = hex(&d["rebound"]);
        r[at + df::FRICTION] = hex(&d["friction"]);
        r[at + df::MINE] = d["mine"].as_bool().unwrap() as u8 as f64;
        r[at + df::RADIUS] = hex(&d["radius"]);
        r[at + df::LIGHT_RADIUS] = hex(&d["lightRadius"]);
        r[at + df::MESH] = d["mesh"].as_bool().unwrap() as u8 as f64;
        self.light_radius[index] = hex(&d["lightRadius"]);
        self.items.define(index);
    }

    fn count(&self, name: usize) -> usize {
        self.items.header[name] as usize
    }

    /// Takes down released views and keeps the frame's ripples.
    fn drain(&mut self) {
        for i in 0..self.count(hd::RELEASES) {
            let at = i * rl::STRIDE;
            let slot = self.items.releases[at + rl::SLOT] as usize;
            let serial = self.items.releases[at + rl::SERIAL];
            if self.mirror[slot].as_ref().is_some_and(|m| m.serial == serial) {
                self.mirror[slot] = None;
            }
        }
        for i in 0..self.count(hd::RIPPLES) {
            let at = i * rp::STRIDE;
            let p = &self.items.ripples;
            self.ripples.push([p[at], p[at + 1], p[at + 2]]);
        }
    }

    fn track<R>(&mut self, f: impl FnOnce(&mut Items) -> R) -> R {
        if self.tracked {
            TRACK.with(|t| t.set(true));
        }
        let r = f(&mut self.items);
        TRACK.with(|t| t.set(false));
        r
    }

    fn cast(&self, origin: &Vector3, dir: &Vector3, far: f64) -> Option<(Vector3, Vector3)> {
        let mut best: Option<&Plane> = None;
        let mut at = f64::INFINITY;
        for p in &self.planes {
            let d = p.normal.dot(dir);
            if d >= 0.0 {
                continue;
            }
            let mut to = p.point;
            to.sub(origin);
            let t = p.normal.dot(&to) / d;
            if t < 0.0 || t > far || t >= at {
                continue;
            }
            at = t;
            best = Some(p);
        }
        let p = best?;
        let mut point = *origin;
        point.add_scaled_vector(dir, at);
        Some((point, p.normal))
    }

    fn crossing(&self, from: &Vector3, to: &Vector3) -> Option<Vector3> {
        let level = self.level?;
        if !(from.y >= level && to.y < level) {
            return None;
        }
        let t = (from.y - level) / (from.y - to.y);
        Some(Vector3::new(from.x + (to.x - from.x) * t, level, from.z + (to.z - from.z) * t))
    }

    /// One page frame: the packet's records, then the phase and its rounds.
    fn frame(&mut self, tick: f64, records: &[[f64; rc::STRIDE]]) -> Frame {
        self.ripples.clear();
        for chunk in records.chunks(lim::BATCH) {
            for (i, r) in chunk.iter().enumerate() {
                self.items.records[i * rc::STRIDE..(i + 1) * rc::STRIDE].copy_from_slice(r);
            }
            self.track(|items| items.apply_records(chunk.len()));
            self.drain();
        }
        let round = 2048.0 / 65535.0 / 2.0 + 0.002;
        let f = &mut self.items.frame;
        f[fr::TICK] = tick;
        f[fr::WATER] = self.level.is_some() as u8 as f64;
        f[fr::ROUND_X] = 2048.0 / 65535.0 / 2.0 + 0.002;
        f[fr::ROUND_Y] = 700.0 / 65535.0 / 2.0 + 0.002;
        f[fr::ROUND_Z] = round;
        self.track(|items| items.present());
        self.drain();
        let mut rounds = 0;
        while self.count(hd::REQUESTS) > 0 {
            for i in 0..self.count(hd::REQUESTS) {
                self.answer(i);
            }
            rounds += 1;
            let abandon = rounds >= self.limit;
            self.track(|items| items.resume(abandon));
            self.drain();
        }
        assert_eq!(self.items.header[hd::VERSION], VERSION as f64);
        let mut out = Frame { rounds, ripples: self.ripples.clone(), ..Frame::default() };
        for i in 0..self.count(hd::VIEWS) {
            let at = i * vw::STRIDE;
            let row = &self.items.views[at..at + vw::STRIDE];
            let slot = row[vw::SLOT] as usize;
            let fresh = self.mirror[slot].as_ref().is_none_or(|m| m.serial != row[vw::SERIAL]);
            if fresh {
                self.mirror[slot] = Some(Mirror { serial: row[vw::SERIAL], definition: row[vw::DEFINITION] as usize, position: [0.0; 3], quaternion: [0.0; 4] });
            }
            let m = self.mirror[slot].as_mut().unwrap();
            m.position.copy_from_slice(&row[vw::POSITION..vw::POSITION + 3]);
            m.quaternion.copy_from_slice(&row[vw::QUATERNION..vw::QUATERNION + 4]);
            if row[vw::LIGHT] != 0.0 {
                let d = row[vw::DEFINITION] as usize;
                out.lights.push((m.position, self.light_radius[d]));
            }
        }
        for (slot, m) in self.mirror.iter().enumerate() {
            if let Some(m) = m {
                out.views.push((slot, m.position, m.quaternion, m.serial, m.definition));
            }
        }
        out
    }

    fn answer(&mut self, i: usize) {
        let q = &self.items.requests[i * rq::STRIDE..(i + 1) * rq::STRIDE];
        let kind = q[rq::KIND] as u32;
        let v3 = |at: usize| Vector3::new(q[at], q[at + 1], q[at + 2]);
        let (origin, direction, far, from, to) = (v3(rq::ORIGIN), v3(rq::DIRECTION), q[rq::FAR], v3(rq::FROM), v3(rq::TO));
        let mut a = [0.0; an::STRIDE];
        if kind & 2 != 0 {
            if let Some((point, normal)) = self.cast(&origin, &direction, far) {
                a[an::HIT] = 1.0;
                a[an::POINT..an::POINT + 3].copy_from_slice(&[point.x, point.y, point.z]);
                a[an::NORMAL..an::NORMAL + 3].copy_from_slice(&[normal.x, normal.y, normal.z]);
            }
        }
        if kind & 1 != 0 {
            if let Some(p) = self.crossing(&from, &to) {
                a[an::WET] = 1.0;
                a[an::WET_POINT..an::WET_POINT + 3].copy_from_slice(&[p.x, p.y, p.z]);
            }
        }
        self.items.answers[i * an::STRIDE..(i + 1) * an::STRIDE].copy_from_slice(&a);
    }
}

// Retains the ported JS/GL call-site signature without an extra wrapper struct.
#[allow(clippy::too_many_arguments)]
fn record(epoch: f64, id: f64, generation: f64, tick: f64, active: bool, def: f64, p: [f64; 3], v: [f64; 3], yaw: f64, up: [f64; 3]) -> [f64; rc::STRIDE] {
    let mut r = [0.0; rc::STRIDE];
    r[rc::EPOCH] = epoch;
    r[rc::ID] = id;
    r[rc::GENERATION] = generation;
    r[rc::TICK] = tick;
    r[rc::ACTIVE] = active as u8 as f64;
    r[rc::DEFINITION] = def;
    r[rc::POSITION..rc::POSITION + 3].copy_from_slice(&p);
    r[rc::VELOCITY..rc::VELOCITY + 3].copy_from_slice(&v);
    r[rc::YAW] = yaw;
    r[rc::UP..rc::UP + 3].copy_from_slice(&up);
    r
}

fn fixture() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/items-presentation/reference.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn page_for(reference: &Value, scenario: &Value) -> Page {
    let planes = scenario["client"]["planes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let (a, b) = (hex3(&p["point"]), hex3(&p["normal"]));
            Plane { point: Vector3::new(a[0], a[1], a[2]), normal: Vector3::new(b[0], b[1], b[2]) }
        })
        .collect();
    let level = scenario["client"]["level"].as_str().map(|_| hex(&scenario["client"]["level"]));
    let mut page = Page::new(planes, level);
    for (i, d) in reference["definitions"].as_array().unwrap().iter().enumerate() {
        page.define(i + 1, d);
    }
    page
}

fn rows(frame: &Value) -> Vec<[f64; rc::STRIDE]> {
    frame["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let active = r["active"].as_bool().unwrap();
            let three = |k: &str| if active { hex3(&r[k]) } else { [0.0; 3] };
            record(
                r["epoch"].as_f64().unwrap(),
                r["id"].as_f64().unwrap(),
                r["generation"].as_f64().unwrap(),
                r["tick"].as_f64().unwrap(),
                active,
                r["definition"].as_f64().unwrap(),
                three("position"),
                three("velocity"),
                if active { hex(&r["yaw"]) } else { 0.0 },
                three("up"),
            )
        })
        .collect()
}

#[test]
fn reproduces_the_recorded_frames_bit_for_bit() {
    let reference = fixture();
    let meshes: Vec<bool> = reference["definitions"].as_array().unwrap().iter().map(|d| d["mesh"].as_bool().unwrap()).collect();
    let mut checked = 0;
    for scenario in reference["scenarios"].as_array().unwrap() {
        let name = scenario["name"].as_str().unwrap();
        let mut page = page_for(&reference, scenario);
        for (f, frame) in scenario["frames"].as_array().unwrap().iter().enumerate() {
            let out = page.frame(hex(&frame["tick"]), &rows(frame));
            assert!(out.rounds < lim::ROUNDS, "{name} frame {f} hit the round limit");
            let want = &frame["expect"];
            let views = want["views"].as_array().unwrap();
            assert_eq!(out.views.len(), views.len(), "{name} frame {f}: views");
            let drawn = out.views.iter().filter(|v| meshes[v.4 - 1]).count();
            assert_eq!(want["meshes"].as_u64().unwrap() as usize, drawn, "{name} frame {f}: meshes");
            for (got, want) in out.views.iter().zip(views) {
                assert_eq!(got.0 as u64, want["id"].as_u64().unwrap(), "{name} frame {f}: slot");
                let p = hex3(&want["position"]);
                for (k, (&g, &pv)) in got.1.iter().zip(&p).enumerate() {
                    assert_eq!(g.to_bits(), pv.to_bits(), "{name} frame {f} slot {} position {k}", got.0);
                }
                if let Some(q) = want["quaternion"].as_array() {
                    for (k, (&g, qv)) in got.2.iter().zip(q).enumerate() {
                        assert_eq!(g.to_bits(), hex(qv).to_bits(), "{name} frame {f} slot {} quaternion {k}", got.0);
                    }
                }
            }
            let lights = want["lights"].as_array().unwrap();
            assert_eq!(out.lights.len(), lights.len(), "{name} frame {f}: lights");
            for (got, want) in out.lights.iter().zip(lights) {
                let p = hex3(&want["position"]);
                for (k, (&g, &pv)) in got.0.iter().zip(&p).enumerate() {
                    assert_eq!(g.to_bits(), pv.to_bits(), "{name} frame {f} light {k}");
                }
                assert_eq!(got.1.to_bits(), hex(&want["radius"]).to_bits());
            }
            let ripples = want["ripples"].as_array().unwrap();
            assert_eq!(out.ripples.len(), ripples.len(), "{name} frame {f}: ripples");
            for (got, want) in out.ripples.iter().zip(ripples) {
                let p = hex3(want);
                for (k, (&g, &pv)) in got.iter().zip(&p).enumerate() {
                    assert_eq!(g.to_bits(), pv.to_bits(), "{name} frame {f} ripple {k}");
                }
            }
            checked += 1;
        }
    }
    assert!(meshes.iter().any(|m| !m) && checked > 500);
}

#[test]
fn frames_allocate_nothing_once_the_stores_exist() {
    let reference = fixture();
    let scenario = &reference["scenarios"].as_array().unwrap().iter().find(|s| s["name"] == "several-flying").unwrap().clone();
    let mut page = page_for(&reference, scenario);
    let frames = scenario["frames"].as_array().unwrap();
    // The first frames create views and lives; the stores are fixed, so even
    // those must not allocate in the kernel.
    page.tracked = true;
    ALLOCS.with(|n| n.set(0));
    for frame in frames {
        page.frame(hex(&frame["tick"]), &rows(frame));
    }
    assert_eq!(ALLOCS.with(Cell::get), 0, "kernel calls allocated");
}

fn flying(id: f64, generation: f64, tick: f64, def: f64) -> [f64; rc::STRIDE] {
    record(1.0, id, generation, tick, true, def, [id, 3.0, 0.0], [1.0, 2.0, 0.5], 0.3, [0.0, 1.0, 0.0])
}

fn ground() -> Vec<Plane> {
    vec![Plane { point: Vector3::new(0.0, 0.0, 0.0), normal: Vector3::new(0.0, 1.0, 0.0) }]
}

fn two_defs(page: &mut Page) {
    for (i, mine) in [(1, true), (2, false)] {
        let at = i * df::STRIDE;
        let r = &mut page.items.definitions;
        r[at + df::GRAVITY] = 1.0;
        r[at + df::REBOUND] = 0.3;
        r[at + df::FRICTION] = 0.5;
        r[at + df::MINE] = mine as u8 as f64;
        r[at + df::RADIUS] = 0.3;
        r[at + df::LIGHT_RADIUS] = if mine { 1.0 } else { 0.0 };
        r[at + df::MESH] = 1.0;
        page.light_radius[i] = r[at + df::LIGHT_RADIUS];
        page.items.define(i);
    }
}

/// More items than any map holds: ids past the store are refused and counted,
/// the rest fly on.
#[test]
fn a_full_store_refuses_and_keeps_flying() {
    let mut page = Page::new(ground(), Some(-5.0));
    two_defs(&mut page);
    let records: Vec<_> = (0..lim::SLOTS + 44).map(|i| flying(i as f64, 1.0, 10.0, 1.0 + (i % 2) as f64)).collect();
    let out = page.frame(10.0, &records);
    assert_eq!(out.views.len(), lim::SLOTS);
    assert_eq!(page.items.header[hd::REFUSED_RECORDS], 44.0);
    let out = page.frame(11.5, &[]);
    assert_eq!(out.views.len(), lim::SLOTS);
    // An unregistered definition is refused too, and a negative or fractional id.
    let bad = [flying(3.0, 2.0, 12.0, 40.0), flying(-1.0, 1.0, 12.0, 1.0), flying(2.5, 1.0, 12.0, 1.0)];
    page.frame(12.0, &bad);
    assert_eq!(page.items.header[hd::REFUSED_RECORDS], 47.0);
}

/// A burst of records in one packet: more than a life's ring, more lives than
/// a slot's queue, past the batch block.
#[test]
fn a_burst_of_records_degrades_without_failing() {
    let mut page = Page::new(ground(), None);
    two_defs(&mut page);
    let mut records = Vec::new();
    for generation in 1..=9 {
        for t in 0..200 {
            records.push(flying(0.0, generation as f64, (generation * 1000 + t) as f64, 1.0 + (generation % 2) as f64));
        }
    }
    let out = page.frame(1.0, &records);
    assert!(out.views.len() <= 1);
    // The clock reaches the newest life; the ring kept its newest 64 records,
    // so a clock before them holds back.
    assert_eq!(page.frame(9100.0, &[]).views.len(), 0);
    let out = page.frame(9150.0, &[]);
    assert_eq!(out.views.len(), 1);
    // A removal for a generation the queue has forgotten is skipped.
    let gone = record(1.0, 0.0, 1.0, 9200.0, false, 0.0, [0.0; 3], [0.0; 3], 0.0, [0.0; 3]);
    page.frame(9101.0, &[gone]);
}

/// A clock that stalls, runs back, jumps far ahead, or is not a number: the
/// items stay where they are or reseed; the frame always ends.
#[test]
fn a_stalled_or_wild_clock_never_hangs() {
    let mut page = Page::new(ground(), Some(-2.0));
    two_defs(&mut page);
    let records: Vec<_> = (0..40).map(|i| flying(i as f64, 1.0, (i % 5) as f64 + 1.0, 1.0 + (i % 2) as f64)).collect();
    page.frame(3.0, &records);
    for tick in [3.0, 3.0, 2.0, 0.0, 1.0e6, 3.0, f64::INFINITY, 4.0, f64::NAN, 5.0, -50.0, 1.0e300, 6.0] {
        page.frame(tick, &[]);
    }
    // A round limit leaves every item where it is.
    let mut limited = Page::new(ground(), Some(-2.0));
    two_defs(&mut limited);
    limited.limit = 1;
    limited.frame(3.0, &records);
    let moved: Vec<_> = (1..=60).map(|t| limited.frame(3.0 + t as f64, &[]).views.len()).collect();
    assert_eq!(*moved.last().unwrap(), 40);
    assert!(limited.items.header[hd::ABANDONED_FRAMES] > 0.0);
}

/// Slot reuse by generation, a stale record, an old epoch.
#[test]
fn pairs_records_by_id_and_generation() {
    let mut page = Page::new(ground(), None);
    two_defs(&mut page);
    page.frame(5.0, &[flying(7.0, 1.0, 4.0, 1.0)]);
    let first = page.frame(5.5, &[]).views[0].3;
    // The next life of the slot: a later generation, starting later.
    let out = page.frame(9.0, &[flying(7.0, 2.0, 8.0, 2.0)]);
    assert_eq!(out.views.len(), 1);
    // The first life ends when the second starts, so its view is replaced.
    assert_ne!(out.views[0].3, first);
    // A record of the old generation, or an older epoch, changes nothing.
    let before = out.views[0].3;
    let mut old = flying(7.0, 1.0, 20.0, 1.0);
    let stale_epoch = {
        old[rc::EPOCH] = 0.0;
        old
    };
    let out = page.frame(10.0, &[flying(7.0, 1.0, 20.0, 1.0), stale_epoch]);
    assert_eq!(out.views[0].3, before);
    // A new epoch forgets every view.
    let mut fresh = flying(7.0, 1.0, 30.0, 1.0);
    fresh[rc::EPOCH] = 2.0;
    let out = page.frame(11.0, &[]);
    assert_eq!(out.views.len(), 1);
    page.frame(31.0, &[fresh]);
    let out = page.frame(32.0, &[]);
    assert_eq!(out.views.len(), 1);
    assert_ne!(out.views[0].3, before);
}
