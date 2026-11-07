//! Item flight, contact and placement for a page online.
//!
//! Ports `EntityItemPresentation` (receive, present, show, seed, step,
//! reconcile, adopt, draw) and the `Items` helpers it flew with (fly,
//! contact, settle, place, presentLight). A page online copies the world's
//! items from their records: each is flown here by the world's own rules one
//! 60 Hz tick at a time as the playout clock crosses ticks, and drawn between
//! the last two at the clock's fraction. Nothing here calls the host, so the
//! two queries `fly` makes of the page (the ray against its collision, the
//! water crossing) leave as request rows. A slot's work is a resumable
//! state machine: the page answers the round's requests, calls `resume`, and
//! repeats until none remain. A round limit degrades by leaving the items
//! where they are; a full store refuses the record. Neither throws.
//!
//! The page owns the models: it registers each definition's numbers once
//! (`define`), makes and removes meshes from the view serials the kernel
//! publishes, and copies the placed position and orientation onto them.
use crate::items_layout::{
    item_answer as an, item_definition as df, item_frame as fr, item_header as hd,
    item_limits as lim, item_record as rc, item_release as rl, item_request as rq,
    item_request_kind as kind, item_ripple as rp, item_view as vw, VERSION,
};
use three::{Matrix4, Quaternion, Vector3};

const SLOTS: usize = lim::SLOTS;
const LIVES: usize = lim::LIVES;
const RECORDS: usize = lim::RECORDS;
const DEFINITIONS: usize = lim::DEFINITIONS;
const BATCH: usize = lim::BATCH;
const ROUNDS: usize = lim::ROUNDS;

// network-entity-items.js constants.
const STEP: f64 = 1.0 / 60.0;
const SMOOTH: f64 = 0.08;
const TOLERANCE: f64 = 0.05;
const SPEED_TOLERANCE: f64 = 1.0;
const NUDGE: f64 = 0.5;
const MAX_OFFSET: f64 = 2.0;
const JUMP: f64 = 12.0;
const MAX_STEPS: f64 = 60.0;
const PROBE_LIMIT: f64 = 600.0;
// items.js constants.
const GRAVITY: f64 = 9.8;
const HARD: f64 = 1.0;
const MINE_SINK: f64 = 0.2;

/// One authority record as the copy keeps it: tick, yaw, position, velocity
/// and surface normal.
const REC: usize = 11;

// The wire's record, decoded by the page to doubles. EntityItemPresentation
// kept these as objects in `life.records` (at most MAX_RECORDS of them); here
// they are copied into a fixed ring per life.
#[derive(Clone, Copy, Default)]
struct Rec {
    tick: f64,
    yaw: f64,
    position: [f64; 3],
    velocity: [f64; 3],
    up: [f64; 3],
}

/// What a definition gives the kernel (items.js itemState and the item
/// class's Gravity, Rebound and Friction, already defaulted by the page).
#[derive(Clone, Copy, Default)]
struct Def {
    gravity: f64,
    rebound: f64,
    friction: f64,
    mine: bool,
    /// The model's bounding sphere, where a stuck mine sits off the surface.
    radius: f64,
    /// Above zero the item asks for a light every time it is drawn.
    light_radius: f64,
    /// Whether the page has a model to orient; without one no turn is made.
    mesh: bool,
    /// Registered by the page; a record naming an unset one is refused.
    set: bool,
}

/// items.js itemState: the part of an item `fly` reads and writes.
#[derive(Clone, Copy, Default)]
struct Body {
    position: Vector3,
    /// Where the last `fly` started: contact casts along previous to position.
    previous: Vector3,
    velocity: Vector3,
    /// The surface normal it rests or last stuck on.
    up: Vector3,
    yaw: f64,
    landed: bool,
    /// A mine in the water, falling at a fixed speed.
    sinking: bool,
    mine: bool,
    radius: f64,
    def: usize,
}

/// What one `fly` step asks of the page.
#[derive(Clone, Copy, Default)]
struct Pending {
    /// Ask whether the move went down into the water between from and to.
    water: bool,
    /// Ask for the first surface along origin + direction * far.
    cast: bool,
    from: Vector3,
    to: Vector3,
    origin: Vector3,
    direction: Vector3,
    far: f64,
}

// The page's reply to a `Pending`: whether the ray hit and where, and where a
// mine crossed the water. It is written into a typed block, one row per request.
#[derive(Clone, Copy, Default)]
struct Answer {
    hit: bool,
    point: Vector3,
    normal: Vector3,
    wet: bool,
    wet_point: Vector3,
}

/// Where a slot is in `show`. Each stage ends by naming the next, so a slot
/// that waits on the page resumes in the same place.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    /// Finished for this frame (or nothing to show).
    Idle,
    /// present's loop body: expire lives, pick the front one, seed it.
    Begin,
    /// show's loop over the records the clock has reached.
    Scan,
    /// step(): fly the copy to a tick.
    Step,
    /// reconcile(): fly the probe from a record to the copy's tick, compare.
    Probe,
    /// step to the target, then draw.
    Final,
}

#[derive(Clone, Copy, PartialEq)]
enum Which {
    /// The life's copy.
    Sim,
    /// The slot's reconcile probe.
    Probe,
}

/// Where `show` is for a slot's front life, between suspensions.
#[derive(Clone, Copy)]
// The state of one slot's present while it waits for the page's answers: the
// kernel cannot call the ray kernel, so a flight is a state machine that
// stops at each cast and resumes from the same place. (Task fields are the
// locals of the JavaScript's loops: how many ticks to fly, the record being
// reconciled, the probe copy.)
struct Task {
    stage: Stage,
    /// A query is out and its answers have not been applied yet.
    waiting: bool,
    /// Which body the pending query flies.
    which: Which,
    /// Row of the request block this slot's pending query occupies.
    req: usize,
    /// Next record of the front life that scan looks at.
    scan: usize,
    /// floor(tick) and the tick after it: where the copy must be.
    base: f64,
    target: f64,
    /// The tick the current step flies to, and the ticks it has left.
    step_to: f64,
    left: i64,
    /// Whether the step moved the life's tick (a step of no ticks leaves it).
    step_set: bool,
    /// After a step: reconcile the record just reached, or draw.
    then_record: bool,
    /// The record being reconciled, and the probe's tick as it flies on.
    cur_tick: f64,
    cur: Rec,
    pt: f64,
    /// EntityItemPresentation.probe: the record flown on to the copy's tick.
    probe: Body,
    pending: Pending,
}

#[derive(Clone, Copy)]
struct Life {
    generation: f64,
    def: usize,
    /// The first record's tick: the item appears when the clock reaches it.
    start: f64,
    /// A removal, or the next life's first record: it goes when the clock
    /// reaches this.
    end: f64,
    /// The life's records are a ring of RECORDS in the record store.
    head: usize,
    len: usize,
    /// The newest record already reconciled.
    checked: f64,
    /// The page's view of this life (0: none yet).
    serial: u32,
    has_sim: bool,
    /// The tick the copy has been flown to.
    tick: f64,
    /// Where the copy was a tick before, for drawing between the two.
    from: Vector3,
    from_up: Vector3,
    /// The fading offset that hides a jump to the world's state.
    offset: Vector3,
    /// The clock when it was last drawn (NaN: not yet).
    shown: f64,
    sim: Body,
}

impl Life {
    // A life starts when a record for a generation the slot has not seen
    // arrives (`receive`'s `life = {generation, def, start, ...}`).
    fn new(generation: f64, def: usize, start: f64) -> Life {
        Life {
            generation,
            def,
            start,
            end: f64::INFINITY,
            head: 0,
            len: 0,
            checked: f64::NEG_INFINITY,
            serial: 0,
            has_sim: false,
            tick: 0.0,
            from: Vector3::default(),
            from_up: Vector3::default(),
            offset: Vector3::default(),
            shown: f64::NAN,
            sim: Body::default(),
        }
    }
}

// One id's presentation: its newest record, and the queue of lives (a slot
// can be reused inside the playout delay, so the old item is drawn while the
// next waits for its first record).
struct Slot {
    /// Seen since the last reset (the page's `slots` Map had the id).
    known: bool,
    /// The newest record taken for this id: records older than it by
    /// generation or tick are ignored.
    generation: f64,
    active: bool,
    tick: f64,
    /// The lives queue: a ring of LIVES, oldest first.
    head: usize,
    len: usize,
    lives: [Life; LIVES],
    task: Task,
    /// Drawn this frame, and the row the page will read.
    drawn: bool,
    row: [f64; vw::STRIDE],
}

pub struct Items {
    // The page's blocks; layouts come from rust/sim/schema/items-layout.json.
    pub frame: Vec<f64>,
    pub records: Vec<f64>,
    pub definitions: Vec<f64>,
    pub requests: Vec<f64>,
    pub answers: Vec<f64>,
    pub views: Vec<f64>,
    pub releases: Vec<f64>,
    pub ripples: Vec<f64>,
    pub header: Vec<f64>,
    defs: Vec<Def>,
    slots: Vec<Slot>,
    /// Slot ids in the order their first record arrived (the Map order the
    /// page iterated).
    order: Vec<u16>,
    /// Every life's records: SLOTS x LIVES x RECORDS rows of REC numbers.
    rec: Vec<f64>,
    /// Slots whose flight waits on the page this round, and last round's.
    waiting: Vec<u16>,
    previous: Vec<u16>,
    epoch: f64,
    has_epoch: bool,
    /// Views are numbered so the page can tell a new one from the old.
    serial: u32,
    tick: f64,
    water: bool,
    /// How far a record can round a position (see the page's ROUNDING).
    rounding: [f64; 3],
    /// Set when the round limit is reached: flights stop, items stay put.
    no_fly: bool,
    rounds: usize,
    releases_count: usize,
    ripples_count: usize,
    refused: f64,
    abandoned: f64,
}

// An idle task, for a slot that has nothing to fly.
fn blank_task() -> Task {
    Task {
        stage: Stage::Idle,
        waiting: false,
        which: Which::Sim,
        req: 0,
        scan: 0,
        base: 0.0,
        target: 0.0,
        step_to: 0.0,
        left: 0,
        step_set: false,
        then_record: false,
        cur_tick: 0.0,
        cur: Rec::default(),
        pt: 0.0,
        probe: Body::default(),
        pending: Pending::default(),
    }
}

/// Where a life's ring slot lives in the record store.
fn record_at(s: usize, phys: usize, ring: usize) -> usize {
    ((s * LIVES + phys) * RECORDS + ring) * REC
}

// Three doubles of a typed block as a vector, and back.
fn vec3(a: &[f64], at: usize) -> Vector3 {
    Vector3::new(a[at], a[at + 1], a[at + 2])
}

fn put3(a: &mut [f64], at: usize, v: &Vector3) {
    a[at] = v.x;
    a[at + 1] = v.y;
    a[at + 2] = v.z;
}

/// network-entity-items.js follow: the world's state from a record. At rest
/// is landed; a mine going straight down at its sinking speed (within the
/// record's rounding) is sinking.
fn follow(b: &mut Body, r: &Rec) {
    b.position = Vector3::new(r.position[0], r.position[1], r.position[2]);
    b.previous = b.position;
    b.velocity = Vector3::new(r.velocity[0], r.velocity[1], r.velocity[2]);
    b.up = Vector3::new(r.up[0], r.up[1], r.up[2]);
    b.up.normalize();
    b.yaw = r.yaw;
    let v = b.velocity;
    b.landed = v.x == 0.0 && v.y == 0.0 && v.z == 0.0;
    b.sinking = b.mine && !b.landed && v.x == 0.0 && v.z == 0.0 && (v.y + MINE_SINK).abs() < 0.15;
}

/// items.js fly up to the queries: gravity in two half steps around the move,
/// then what the move needs asked of the page (the mine's water crossing,
/// contact's ray). Returns whether anything was asked.
fn fly_begin(b: &mut Body, d: &Def, water_on: bool, p: &mut Pending) -> bool {
    // Gravity is the class's Gravity times 9.8 (the page registers the nullish default of 1
    // once). A sinking mine falls at a fixed speed and takes no gravity.
    let g = -GRAVITY * d.gravity;
    if b.sinking {
        b.velocity.set(0.0, -MINE_SINK, 0.0);
    } else {
        b.velocity.y += g * 0.5 * STEP;
    }
    b.previous = b.position;
    // The move sits between the two half steps of gravity, as the original wrote it, so the
    // position uses the half-stepped velocity.
    b.position.add_scaled_vector(&b.velocity, STEP);
    if !b.sinking {
        b.velocity.y += g * 0.5 * STEP;
    }
    // Only a mine not yet sinking asks about the water, and only on a page that has water; the
    // answer cannot change the ray below, so both go out together.
    p.water = b.mine && !b.sinking && water_on;
    if p.water {
        p.from = b.previous;
        p.to = b.position;
    }
    // items.js contact: what the item meets between where it was and where
    // it is, a ray a little behind the start.
    // The ray starts 5 cm behind the old position so a copy resting on a surface still meets
    // it; a move shorter than a micrometre casts nothing.
    p.cast = false;
    let mut ray = Vector3::default();
    ray.sub_vectors(&b.position, &b.previous);
    let length = ray.length();
    if length >= 1e-6 {
        ray.divide_scalar(length);
        let mut start = b.previous;
        start.add_scaled_vector(&ray, -0.05);
        p.origin = start;
        p.direction = ray;
        p.far = length + 0.1;
        p.cast = true;
    }
    p.water || p.cast
}

/// items.js fly from the answers: the water, then contact's bounce or settle.
/// Returns where a mine went into the water (the ripple).
fn fly_end(b: &mut Body, d: &Def, p: &Pending, a: &Answer) -> Option<Vector3> {
    let mut ripple = None;
    // Water comes first, as in the original: a mine meeting the surface sinks and the page
    // ripples it, and the contact below then sees the sinking velocity.
    if p.water && a.wet {
        b.sinking = true;
        b.velocity.set(0.0, -MINE_SINK, 0.0);
        ripple = Some(a.wet_point);
    }
    // The hit's distance is unused; only its point and normal matter.
    if p.cast && a.hit {
        contact(b, d, a);
    }
    ripple
}

/// items.js contact, from the hit.
fn contact(b: &mut Body, d: &Def, hit: &Answer) {
    let n = hit.normal;
    // Speed into the surface; a copy moving off it meets nothing.
    let dd = b.velocity.dot(&n);
    if dd >= 0.0 {
        return;
    }
    if b.mine {
        // Stuck, upright on the surface (resting on it, the port's choice).
        b.position.copy(&hit.point);
        b.position.add_scaled_vector(&n, b.radius * 0.3);
        settle(b, &n);
        return;
    }
    // Slower than 1 m/s into the surface a power-up settles on the spot.
    if dd > -HARD {
        b.position.copy(&hit.point);
        settle(b, &n);
        return;
    }
    b.position.copy(&hit.point);
    b.position.add_scaled_vector(&n, 0.02);
    // A harder hit loses its normal part, then friction takes a share of the rest in
    // proportion to the impact, and the bounce returns Rebound of the impact.
    let rebound = d.rebound;
    b.velocity.add_scaled_vector(&n, -dd);
    let along = b.velocity.length();
    if along > 1e-6 {
        b.velocity
            .multiply_scalar(1.0 - v8math::min((d.friction * -dd * (1.0 + rebound)) / along, 1.0));
    }
    b.velocity.add_scaled_vector(&n, rebound * -dd);
}

/// items.js settle.
fn settle(b: &mut Body, normal: &Vector3) {
    b.landed = true;
    b.sinking = false;
    b.velocity.set(0.0, 0.0, 0.0);
    b.up.copy(normal);
}

/// items.js place: the model upright on its surface, facing its way.
fn place(up: &Vector3, yaw: f64) -> Quaternion {
    // The facing is the yaw's heading made level with the surface: the part along the normal
    // is taken out, and a heading straight up the normal falls back to +z.
    let mut forward = Vector3::new(v8math::sin(yaw), 0.0, v8math::cos(yaw));
    let d = forward.dot(up);
    forward.add_scaled_vector(up, -d);
    if forward.length_sq() < 1e-8 {
        forward.set(0.0, 0.0, 1.0);
        forward.add_scaled_vector(up, -up.z);
    }
    forward.normalize();
    let mut right = Vector3::default();
    right.cross_vectors(up, &forward);
    right.normalize();
    let mut basis = Matrix4::default();
    // The model's frame is (right, up, forward); three turns it into the orientation the page
    // copies onto the mesh.
    basis.make_basis(&right, up, &forward);
    let mut turn = Quaternion::default();
    turn.set_from_rotation_matrix(&basis);
    turn
}

impl Items {
    // All the stores are made here, once, at their limits; no frame allocates.
    pub fn new() -> Items {
        let life = Life::new(0.0, 0, 0.0);
        let slot = || Slot {
            known: false,
            generation: 0.0,
            active: false,
            tick: 0.0,
            head: 0,
            len: 0,
            lives: [life; LIVES],
            task: blank_task(),
            drawn: false,
            row: [0.0; vw::STRIDE],
        };
        // Every store is sized here, once; frames only fill them. The release block holds two
        // per slot so a reset and a replacement in one frame both fit.
        let mut header = vec![0.0; hd::STRIDE];
        header[hd::VERSION] = VERSION as f64;
        Items {
            frame: vec![0.0; fr::STRIDE],
            records: vec![0.0; BATCH * rc::STRIDE],
            definitions: vec![0.0; DEFINITIONS * df::STRIDE],
            requests: vec![0.0; SLOTS * rq::STRIDE],
            answers: vec![0.0; SLOTS * an::STRIDE],
            views: vec![0.0; SLOTS * vw::STRIDE],
            // A release per view, and a spare for a frame's reset.
            releases: vec![0.0; 2 * SLOTS * rl::STRIDE],
            ripples: vec![0.0; 2 * SLOTS * rp::STRIDE],
            header,
            defs: vec![Def::default(); DEFINITIONS],
            slots: (0..SLOTS).map(|_| slot()).collect(),
            order: Vec::with_capacity(SLOTS),
            rec: vec![0.0; SLOTS * LIVES * RECORDS * REC],
            waiting: Vec::with_capacity(SLOTS),
            previous: Vec::with_capacity(SLOTS),
            epoch: 0.0,
            has_epoch: false,
            serial: 0,
            tick: 0.0,
            water: false,
            rounding: [0.0; 3],
            no_fly: false,
            rounds: 0,
            releases_count: 0,
            ripples_count: 0,
            refused: 0.0,
            abandoned: 0.0,
        }
    }

    /// Registers definition `index` from its row of the definition block. A
    /// definition is a number of the page's own registry, once.
    pub fn define(&mut self, index: usize) {
        if index >= DEFINITIONS {
            return;
        }
        let r = &self.definitions[index * df::STRIDE..(index + 1) * df::STRIDE];
        self.defs[index] = Def {
            gravity: r[df::GRAVITY],
            rebound: r[df::REBOUND],
            friction: r[df::FRICTION],
            mine: r[df::MINE] != 0.0,
            radius: r[df::RADIUS],
            light_radius: r[df::LIGHT_RADIUS],
            mesh: r[df::MESH] != 0.0,
            set: true,
        };
    }

/// The counts a call reports (releases, ripples, requests, views) belong to that call alone;
/// the page drains them after each one.
    fn begin_call(&mut self) {
        self.releases_count = 0;
        self.ripples_count = 0;
        self.header[hd::REQUESTS] = 0.0;
        self.header[hd::VIEWS] = 0.0;
    }

/// The running totals (records refused, frames abandoned) are reported by the page once,
/// whenever either is above zero.
    fn end_call(&mut self) {
        self.header[hd::RELEASES] = self.releases_count as f64;
        self.header[hd::RIPPLES] = self.ripples_count as f64;
        self.header[hd::REFUSED_RECORDS] = self.refused;
        self.header[hd::ABANDONED_FRAMES] = self.abandoned;
    }

/// A life's k-th record, oldest first.
    fn rec_get(&self, s: usize, phys: usize, k: usize) -> Rec {
        let life = &self.slots[s].lives[phys];
        let at = record_at(s, phys, (life.head + k) % RECORDS);
        let r = &self.rec[at..at + REC];
        Rec {
            tick: r[0],
            yaw: r[1],
            position: [r[2], r[3], r[4]],
            velocity: [r[5], r[6], r[7]],
            up: [r[8], r[9], r[10]],
        }
    }

/// The ring position of a slot's front (oldest) life, the only one shown.
    fn front(&self, s: usize) -> usize {
        self.slots[s].head % LIVES
    }

    /// EntityItemPresentation.hide: the page takes the view down.
    fn hide_front(&mut self, s: usize) {
        let slot = &mut self.slots[s];
        let phys = slot.head % LIVES;
        let serial = slot.lives[phys].serial;
        if serial != 0 && self.releases_count < 2 * SLOTS {
            let at = self.releases_count * rl::STRIDE;
            self.releases[at + rl::SLOT] = s as f64;
            self.releases[at + rl::SERIAL] = serial as f64;
            self.releases_count += 1;
        }
        slot.head = (slot.head + 1) % LIVES;
        slot.len -= 1;
    }

    /// EntityItemPresentation.reset: every slot forgotten, every view released.
    fn reset(&mut self, epoch: f64) {
        for i in 0..self.order.len() {
            let s = self.order[i] as usize;
            while self.slots[s].len > 0 {
                self.hide_front(s);
            }
            self.slots[s].known = false;
        }
        self.order.clear();
        self.epoch = epoch;
        self.has_epoch = true;
    }

    /// EntityItemPresentation.receive for `count` rows of the record block.
    /// A record the store cannot take (an id past the slots, a definition the
    /// page never registered) is refused and counted; the rest are kept.
    pub fn apply_records(&mut self, count: usize) {
        self.begin_call();
        self.no_fly = false;
        for i in 0..count.min(BATCH) {
            let at = i * rc::STRIDE;
            let mut row = [0.0; rc::STRIDE];
            row.copy_from_slice(&self.records[at..at + rc::STRIDE]);
            self.receive(&row);
        }
        self.end_call();
    }

    /// EntityItemPresentation.reset(epoch), asked by the page when its
    /// connection starts a new epoch: records of an older one are dropped
    /// from then on.
    pub fn reset_epoch(&mut self, epoch: f64) {
        self.begin_call();
        self.reset(epoch);
        self.end_call();
    }

    // EntityItemPresentation.receive: pairs a record with its slot by id and
    // generation (a stale or repeated one is ignored), queues a new life on
    // its first record, and keeps the newest MAX_RECORDS for the clock to reach.
    fn receive(&mut self, r: &[f64; rc::STRIDE]) {
        let epoch = r[rc::EPOCH];
        // A record of an older epoch than the page has seen is dropped; a newer one forgets
        // every item first, as the original reset did.
        if self.has_epoch && epoch < self.epoch {
            return;
        }
        if !self.has_epoch || epoch != self.epoch {
            self.reset(epoch);
        }
        let id = r[rc::ID];
        // The id comes off the wire: one the slot table cannot hold is refused and counted,
        // never indexed.
        if !(id >= 0.0 && id < SLOTS as f64) || id.fract() != 0.0 {
            self.refused += 1.0;
            return;
        }
        let s = id as usize;
        let generation = r[rc::GENERATION];
        let tick = r[rc::TICK];
        let active = r[rc::ACTIVE] != 0.0;
        let slot = &mut self.slots[s];
        if slot.known
            // A record older than the newest for this id (an older generation, or the same one
            // at an earlier tick, or after its removal) changes nothing.
            && (generation < slot.generation
                || generation == slot.generation
                    && (!slot.active || tick < slot.tick || tick == slot.tick && active))
        {
            return;
        }
        if !slot.known {
            slot.known = true;
            slot.head = 0;
            slot.len = 0;
            self.order.push(s as u16);
        }
        slot.generation = generation;
        slot.active = active;
        slot.tick = tick;
        // The slot's lives queue (a slot can be reused inside the playout
        // delay); a removal or the next life's first record ends one.
        // Records pair with a life by generation, never by position.
        let mut found = None;
        for k in 0..slot.len {
            let phys = (slot.head + k) % LIVES;
            if slot.lives[phys].generation == generation {
                found = Some(phys);
                break;
            }
        }
        if !active {
            if let Some(phys) = found {
                let life = &mut slot.lives[phys];
                life.end = v8math::min(life.end, tick);
            }
            return;
        }
        let phys = match found {
            Some(phys) => phys,
            None => {
                let def = r[rc::DEFINITION];
                if !(def >= 0.0 && def < DEFINITIONS as f64) || !self.defs[def as usize].set {
                    self.refused += 1.0;
                    return;
                }
                for k in 0..slot.len {
                    let phys = (slot.head + k) % LIVES;
                    slot.lives[phys].end = v8math::min(slot.lives[phys].end, tick);
                }
                // A slot reused faster than the queue holds loses its oldest life, as the
                // original's fourth shift did.
                if slot.len == LIVES {
                    self.hide_front(s);
                }
                let slot = &mut self.slots[s];
                let phys = (slot.head + slot.len) % LIVES;
                slot.lives[phys] = Life::new(generation, def as usize, tick);
                slot.len += 1;
                phys
            }
        };
        let slot = &mut self.slots[s];
        let life = &mut slot.lives[phys];
        // A burst past the ring drops the oldest record: the newest are the ones the clock
        // reaches.
        if life.len == RECORDS {
            life.head = (life.head + 1) % RECORDS;
            life.len -= 1;
        }
        let ring = (life.head + life.len) % RECORDS;
        life.len += 1;
        let at = record_at(s, phys, ring);
        let w = &mut self.rec[at..at + REC];
        w[0] = tick;
        w[1] = r[rc::YAW];
        w[2..5].copy_from_slice(&r[rc::POSITION..rc::POSITION + 3]);
        w[5..8].copy_from_slice(&r[rc::VELOCITY..rc::VELOCITY + 3]);
        w[8..11].copy_from_slice(&r[rc::UP..rc::UP + 3]);
    }

    /// EntityItemPresentation.present for the frame's clock: every slot runs
    /// until it finishes or needs the page. `requests` in the header says how
    /// many rows await answers; zero means the frame is done and the views
    /// are published.
    pub fn present(&mut self) {
        self.begin_call();
        self.tick = self.frame[fr::TICK];
        self.water = self.frame[fr::WATER] != 0.0;
        self.rounding = [self.frame[fr::ROUND_X], self.frame[fr::ROUND_Y], self.frame[fr::ROUND_Z]];
        self.no_fly = false;
        self.rounds = 0;
        // The first round starts every slot afresh; later rounds only continue the ones
        // waiting.
        self.waiting.clear();
        self.previous.clear();
        for i in 0..self.order.len() {
            let s = self.order[i] as usize;
            let slot = &mut self.slots[s];
            slot.drawn = false;
            slot.task.stage = Stage::Begin;
            slot.task.waiting = false;
            self.run(s);
        }
        self.finish_round();
    }

    /// The page has written an answer row for every request. Continues the
    /// waiting slots; `abandon` (the page's round limit) answers the rest of
    /// the frame with nothing, so every item stays where it is.
    pub fn resume(&mut self, abandon: bool) {
        self.begin_call();
        if abandon {
            self.no_fly = true;
            self.abandoned += 1.0;
        }
        self.rounds += 1;
        if self.rounds > ROUNDS {
            self.no_fly = true;
        }
        // Last round's waiting slots are read while this round's are collected, in two stores
        // that were sized for every slot.
        std::mem::swap(&mut self.waiting, &mut self.previous);
        self.waiting.clear();
        for i in 0..self.previous.len() {
            let s = self.previous[i] as usize;
            self.run(s);
        }
        self.finish_round();
    }

/// Ends a round: writes the waiting slots' queries as request rows, or, with none waiting,
/// publishes the frame's views.
    fn finish_round(&mut self) {
        // Requests are written once a round has run every slot as far as it can, so the page
        // reads one tight block and slots never see each other's rows.
        let n = self.waiting.len();
        for i in 0..n {
            let s = self.waiting[i] as usize;
            let p = &self.slots[s].task.pending;
            let at = i * rq::STRIDE;
            let row = &mut self.requests[at..at + rq::STRIDE];
            row[rq::KIND] = (match (p.water, p.cast) {
                (true, true) => kind::BOTH,
                (true, false) => kind::WATER,
                (false, true) => kind::CAST,
                (false, false) => kind::NONE,
            }) as f64;
            put3(row, rq::ORIGIN, &p.origin);
            put3(row, rq::DIRECTION, &p.direction);
            row[rq::FAR] = p.far;
            put3(row, rq::FROM, &p.from);
            put3(row, rq::TO, &p.to);
        }
        self.header[hd::REQUESTS] = n as f64;
        if n == 0 {
            // With nothing left to answer the frame is done.
            // The frame's draws, in the order the page visited the slots.
            let mut views = 0;
            for i in 0..self.order.len() {
                let slot = &self.slots[self.order[i] as usize];
                if slot.drawn {
                    let at = views * vw::STRIDE;
                    self.views[at..at + vw::STRIDE].copy_from_slice(&slot.row);
                    views += 1;
                }
            }
            self.header[hd::VIEWS] = views as f64;
        }
        self.end_call();
    }

/// Runs a slot's state machine until it is done or waits on the page. Every loop it contains
/// is bounded (records, MAX_STEPS, PROBE_LIMIT), so it always returns.
    fn run(&mut self, s: usize) {
        loop {
            match self.slots[s].task.stage {
                Stage::Idle => return,
                Stage::Begin => self.begin(s),
                Stage::Scan => self.scan(s),
                Stage::Step => {
                    if self.step(s) {
                        return;
                    }
                }
                Stage::Probe => {
                    if self.probe(s) {
                        return;
                    }
                }
                Stage::Final => self.finish(s),
            }
        }
    }

    /// present's loop body for one slot, then show's start.
    fn begin(&mut self, s: usize) {
        let tick = self.tick;
        // present: lives whose end the clock has reached go (their views are released); the
        // next one may show in the same frame.
        while self.slots[s].len > 0 && tick >= self.slots[s].lives[self.front(s)].end {
            self.hide_front(s);
        }
        if self.slots[s].len == 0 {
            self.slots[s].task.stage = Stage::Idle;
            return;
        }
        let phys = self.front(s);
        let life = &mut self.slots[s].lives[phys];
        if !(tick >= life.start || life.serial != 0) {
            self.slots[s].task.stage = Stage::Idle;
            return;
        }
        // The copy is drawn between tick base and base + 1 at the clock's fraction.
        let base = tick.floor();
        let target = base + 1.0;
        if !life.shown.is_nan() && tick > life.shown {
            // The jump-hiding offset fades with time constant SMOOTH seconds of clock.
            life.offset.multiply_scalar(v8math::exp((life.shown - tick) / (60.0 * SMOOTH)));
        }
        life.shown = tick;
        // First shown, or the clock jumped (a stall, a resume): from the
        // world's newest state at or before it. Before its first record (a
        // clock run back) it holds where it was drawn.
        if !life.has_sim || target < life.tick || target - life.tick > JUMP {
            let len = life.len;
            let mut at = None;
            // findLast: the newest record at or before the clock's tick.
            for k in (0..len).rev() {
                if self.rec_get(s, phys, k).tick <= base {
                    at = Some(k);
                    break;
                }
            }
            match at {
                None => {
                    self.slots[s].task.stage = Stage::Idle;
                    return;
                }
                Some(k) => {
                    let rec = self.rec_get(s, phys, k);
                    self.seed(s, phys, &rec);
                }
            }
        }
        let task = &mut self.slots[s].task;
        task.base = base;
        task.target = target;
        task.scan = 0;
        task.stage = Stage::Scan;
    }

    /// EntityItemPresentation.seed.
    fn seed(&mut self, s: usize, phys: usize, rec: &Rec) {
        // The first seed gives the life its view (the original made the mesh here); a later
        // seed after a jump keeps it.
        let fresh = self.slots[s].lives[phys].serial == 0;
        if fresh {
            self.serial = self.serial.wrapping_add(1).max(1);
        }
        let serial = self.serial;
        let life = &mut self.slots[s].lives[phys];
        let d = self.defs[life.def];
        if fresh {
            life.serial = serial;
        }
        if !life.has_sim {
            life.sim = Body {
                mine: d.mine,
                radius: d.radius,
                def: life.def,
                ..Body::default()
            };
            life.has_sim = true;
        }
        follow(&mut life.sim, rec);
        life.tick = rec.tick;
        life.checked = rec.tick;
        life.from = life.sim.position;
        life.from_up = life.sim.up;
        life.offset.set(0.0, 0.0, 0.0);
    }

    /// show's loop over the records the clock has reached.
    fn scan(&mut self, s: usize) {
        let phys = self.front(s);
        loop {
            let (len, checked) = {
                let life = &self.slots[s].lives[phys];
                (life.len, life.checked)
            };
            let task = self.slots[s].task;
            let mut reached = None;
            if task.scan < len {
                let rec = self.rec_get(s, phys, task.scan);
                self.slots[s].task.scan += 1;
                if rec.tick <= checked {
                    continue;
                }
                if rec.tick <= task.target {
                    reached = Some(rec);
                }
            }
            // A record at or before the target is flown to and reconciled; none left means fly
            // to the target and draw.
            let t = &mut self.slots[s].task;
            match reached {
                Some(rec) => {
                    t.cur = rec;
                    t.cur_tick = rec.tick;
                    t.step_to = rec.tick;
                    t.then_record = true;
                }
                None => {
                    t.step_to = t.target;
                    t.then_record = false;
                }
            }
            self.step_init(s, phys);
            return;
        }
    }

    /// EntityItemPresentation.step's start: how many ticks to fly.
    fn step_init(&mut self, s: usize, phys: usize) {
        let slot = &mut self.slots[s];
        let life = &slot.lives[phys];
        let mut n = v8math::min(slot.task.step_to - life.tick, MAX_STEPS);
        slot.task.step_set = n > 0.0;
        if n > 0.0 && life.sim.landed {
            n = 1.0;
        }
        // A copy at rest takes one step: it neither falls nor moves, but its previous position
        // is still taken.
        slot.task.left = if n > 0.0 { n as i64 } else { 0 };
        slot.task.which = Which::Sim;
        slot.task.stage = Stage::Step;
    }

    /// Asks `fly` of the body; whether it needs the page.
    fn start_fly(&mut self, s: usize, which: Which) -> bool {
        if self.no_fly {
            return false;
        }
        // The query uses the numbers of the body's definition.
        let phys = self.front(s);
        let def = match which {
            Which::Sim => self.slots[s].lives[phys].sim.def,
            Which::Probe => self.slots[s].task.probe.def,
        };
        let d = self.defs[def];
        let water = self.water;
        let slot = &mut self.slots[s];
        let body = match which {
            Which::Sim => &mut slot.lives[phys].sim,
            Which::Probe => &mut slot.task.probe,
        };
        if !fly_begin(body, &d, water, &mut slot.task.pending) {
            return false;
        }
        // The query waits for the page: this slot's request is the next row.
        slot.task.req = self.waiting.len();
        slot.task.which = which;
        slot.task.waiting = true;
        self.waiting.push(s as u16);
        true
    }

    /// The answers to the query `start_fly` left, applied.
    fn complete_fly(&mut self, s: usize) {
        self.slots[s].task.waiting = false;
        if self.no_fly {
            return;
        }
        let phys = self.front(s);
        let task = self.slots[s].task;
        let at = task.req * an::STRIDE;
        let a = &self.answers[at..at + an::STRIDE];
        let answer = Answer {
            hit: a[an::HIT] != 0.0,
            point: vec3(a, an::POINT),
            normal: vec3(a, an::NORMAL),
            wet: a[an::WET] != 0.0,
            wet_point: vec3(a, an::WET_POINT),
        };
        let def = match task.which {
            Which::Sim => self.slots[s].lives[phys].sim.def,
            Which::Probe => task.probe.def,
        };
        let d = self.defs[def];
        let slot = &mut self.slots[s];
        let body = match task.which {
            Which::Sim => &mut slot.lives[phys].sim,
            Which::Probe => &mut slot.task.probe,
        };
        // A ripple is a side effect on the page's water; it leaves as a row.
        let ripple = fly_end(body, &d, &task.pending, &answer);
        if let Some(p) = ripple {
            // A full ripple block drops the ripple, never the frame.
            if self.ripples_count < 2 * SLOTS {
                put3(&mut self.ripples, self.ripples_count * rp::STRIDE + rp::POSITION, &p);
                self.ripples_count += 1;
            }
        }
    }

    /// EntityItemPresentation.step: fly the copy on, keeping where it was a
    /// tick before for drawing. True when it waits on the page.
    fn step(&mut self, s: usize) -> bool {
        let phys = self.front(s);
        if self.slots[s].task.waiting {
            self.complete_fly(s);
        }
        while self.slots[s].task.left > 0 {
            self.slots[s].task.left -= 1;
            let life = &mut self.slots[s].lives[phys];
            life.from = life.sim.position;
            life.from_up = life.sim.up;
            if !life.sim.landed && self.start_fly(s, Which::Sim) {
                return true;
            }
        }
        let task = self.slots[s].task;
        if task.step_set {
            self.slots[s].lives[phys].tick = task.step_to;
        }
        // The record reached: the probe starts from the world's state at it (reconcile's
        // start).
        if task.then_record {
            // reconcile's start: the probe is the world's state at the record.
            let life = &self.slots[s].lives[phys];
            let mut probe = Body {
                mine: life.sim.mine,
                radius: life.sim.radius,
                def: life.def,
                ..Body::default()
            };
            follow(&mut probe, &task.cur);
            let t = &mut self.slots[s].task;
            t.probe = probe;
            t.pt = task.cur_tick;
            t.stage = Stage::Probe;
        } else {
            self.slots[s].task.stage = Stage::Final;
        }
        false
    }

    /// EntityItemPresentation.reconcile: a record the clock has reached,
    /// flown on to the copy's tick if it came late, then compared.
    fn probe(&mut self, s: usize) -> bool {
        let phys = self.front(s);
        if self.slots[s].task.waiting {
            self.complete_fly(s);
            self.slots[s].task.pt += 1.0;
        }
        // The page's copy never runs further behind a record than a ten
        // second stall; a record older than that is compared where it is.
        while self.slots[s].task.pt < self.slots[s].lives[phys].tick
            && self.slots[s].task.pt - self.slots[s].task.cur_tick < PROBE_LIMIT
            && !self.slots[s].task.probe.landed
        {
            if self.start_fly(s, Which::Probe) {
                return true;
            }
            self.slots[s].task.pt += 1.0;
        }
        // The copy and the probe now both stand at the copy's tick: compare them.
        let rounding = self.rounding;
        let slot = &mut self.slots[s];
        let probe = slot.task.probe;
        let life = &mut slot.lives[phys];
        // The error is the probe's position against the copy's.
        let mut error = Vector3::default();
        error.sub_vectors(&probe.position, &life.sim.position);
        let off = error.length();
        // A probe at rest is trusted when the copy is not, when the two differ by more than
        // rounding, or when their surfaces differ; one still moving is compared on distance
        // and speed.
        let adopt = if probe.landed {
            // `rounded`: whether the error is within what the snapshot's position codes can
            // round.
            let rounded = error.x.abs() <= rounding[0]
                && error.y.abs() <= rounding[1]
                && error.z.abs() <= rounding[2];
            !life.sim.landed || !rounded || life.sim.up.dot(&probe.up) < 0.995
        } else {
            off > TOLERANCE
                || !life.sim.landed
                    && life.sim.velocity.distance_to(&probe.velocity) > SPEED_TOLERANCE
        };
        // Far apart, or at rest differently, the copy takes the world's state behind the
        // offset; close enough, it is nudged halfway so the records' rounding averages out.
        if adopt {
            Self::adopt(life, &probe);
        } else if !probe.landed && !life.sim.landed {
            life.sim.position.add_scaled_vector(&error, NUDGE);
            life.from.add_scaled_vector(&error, NUDGE);
            life.offset.add_scaled_vector(&error, -NUDGE);
        }
        life.checked = slot.task.cur_tick;
        slot.task.stage = Stage::Scan;
        false
    }

    /// EntityItemPresentation.adopt: the world's state, the jump hidden by an
    /// offset that fades over SMOOTH.
    fn adopt(life: &mut Life, state: &Body) {
        let mut shift = Vector3::default();
        shift.sub_vectors(&life.sim.position, &state.position);
        let sim = &mut life.sim;
        sim.position.copy(&state.position);
        sim.previous.copy(&state.position);
        sim.velocity.copy(&state.velocity);
        sim.up.copy(&state.up);
        sim.landed = state.landed;
        sim.sinking = state.sinking;
        life.from.sub(&shift);
        life.offset.add(&shift);
        // An offset too big to hide is dropped: the item just jumps.
        if life.offset.length() > MAX_OFFSET {
            life.offset.set(0.0, 0.0, 0.0);
        }
    }

    /// The end of show: draw, then the records behind the clock go (only the
    /// newest stays, to start again from).
    fn finish(&mut self, s: usize) {
        let phys = self.front(s);
        // draw(): f is the clock's fraction between the copy's last two ticks.
        let f = self.tick - self.slots[s].task.base;
        let d = self.defs[self.slots[s].lives[phys].def];
        let slot = &mut self.slots[s];
        let life = &slot.lives[phys];
        // draw: the copy between the last two ticks, plus the fading offset.
        let mut position = Vector3::default();
        // The drawn position is the copy between its last two ticks, plus the fading offset.
        position.lerp_vectors(&life.from, &life.sim.position, f);
        position.add(&life.offset);
        let mut up = Vector3::default();
        // The drawn surface normal blends the same way; a blend that cancels out keeps the
        // copy's.
        up.lerp_vectors(&life.from_up, &life.sim.up, f);
        if up.length_sq() < 1e-8 {
            up.copy(&life.sim.up);
        }
        up.normalize();
        let row = &mut slot.row;
        row[vw::SLOT] = s as f64;
        row[vw::SERIAL] = life.serial as f64;
        row[vw::GENERATION] = life.generation;
        row[vw::DEFINITION] = life.def as f64;
        // presentLight: the page adds the light at the drawn position when the definition has
        // a light radius.
        row[vw::LIGHT] = if d.light_radius > 0.0 { 1.0 } else { 0.0 };
        put3(row, vw::POSITION, &position);
        if d.mesh {
            // Only an item with a model is oriented; the others have only a position.
            let q = place(&up, life.sim.yaw);
            row[vw::QUATERNION] = q.x;
            row[vw::QUATERNION + 1] = q.y;
            row[vw::QUATERNION + 2] = q.z;
            row[vw::QUATERNION + 3] = q.w;
        } else {
            for i in 0..4 {
                row[vw::QUATERNION + i] = 0.0;
            }
        }
        slot.drawn = true;
        slot.task.stage = Stage::Idle;
        let life = &mut slot.lives[phys];
        // Records behind the clock: only the newest stays, to start again from.
        while life.len > 1 {
            let at = record_at(s, phys, (life.head + 1) % RECORDS);
            if self.rec[at] <= life.checked {
                life.head = (life.head + 1) % RECORDS;
                life.len -= 1;
            } else {
                break;
            }
        }
    }
}

impl Default for Items {
    fn default() -> Self {
        Items::new()
    }
}
