//! Headless measurement probes (ignored tests, no drawing): real 2005 battles in the Instant Action runtime, every
//! soldier posed per tick as the client poses it (`present_records`, then the animation player), and each cloth
//! piece's fixed points read in its piece frame, with the cloth stepped as the pool steps it (restarts counted).
//! `BFE_DEV_SWBF2_CLOTH_NO_RESTART=1` turns the pose-jump restart off, as in the client; `PROBE_WAYS=restart,
//! norestart,game,game30` steps each piece those ways side by side on the same poses (`game`: one update per drawn
//! frame over its whole time, as the game steps, no restart; `game30`: the same at 30 frames a second). The battles follow the cloth for a second after each pose
//! jump at a clip change it was not restarted on.
//!
//! `BFE_SWBF2_PREPARED=<prepared .content> cargo test --release -p bfe-client --lib characters::probe -- --ignored --nocapture`
//! `PROBE_UNITS=a,b` (class names) are the jump probe's heroes and `PROBE_PLAYER` the battles' player;
//! `PROBE_SIDE=<side>` overrides the side taken from a class name's prefix; `PROBE_SEEDS=1-20`, `PROBE_TICKS`,
//! `PROBE_BOTS` set the battles.
use super::cloth::{self, Particles, V3};
use super::Characters;
use crate::generated_input::{ClientAction, ClientCommand, ClientFlightCommand, ClientHeld, ClientLook};
use crate::local::{launch::Prepared, publication::Publication, runtime::Runtime, thread::PublicationSlot};
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_sim::{abi::context::Context, units::infantry::Controls, world::admit::{mission::*, Bots, Options}};
use std::collections::HashMap;

/// The fastest a fixed point of a body's cloth moves in its piece frame in ordinary play here (m/s): above it a
/// pose jumped (the restart rule's bound).
const FAST: f32 = cloth::SNAP_SPEED;

struct Battle {
    rt: Runtime<crate::local::context::Native>,
    slot: PublicationSlot,
    receiver: crate::online::receiver::Receiver,
    characters: Characters,
    now_ms: f64,
    tick: u32,
}

/// How a probe steps a piece's cloth.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum Way {
    /// As the client, with the pose-jump restart.
    Restart,
    /// As the client with the restart off.
    NoRestart,
    /// As the game: one update per drawn frame over the frame's whole time, no restart.
    Game,
    /// As the game drawn at 30 frames a second: one update every other tick.
    Game30,
}
/// `PROBE_WAYS`, else the client's own way.
fn ways(c: &Characters) -> Vec<Way> {
    match std::env::var("PROBE_WAYS") {
        Ok(v) => v
            .split(',')
            .map(|w| match w {
                "restart" => Way::Restart,
                "norestart" => Way::NoRestart,
                "game" => Way::Game,
                "game30" => Way::Game30,
                _ => panic!("PROBE_WAYS: {w}"),
            })
            .collect(),
        Err(_) => vec![if c.cloth_restart { Way::Restart } else { Way::NoRestart }],
    }
}

/// One piece of one soldier, followed from tick to tick.
#[derive(Default)]
struct Follow {
    id: (u32, u32),
    pins: Vec<V3>,
    shown: f64,
    pos: Vec<V3>,
    old: Vec<V3>,
    normal: Vec<V3>,
    clock: Option<cloth::Clock>,
    /// The clip and tumble phase of the last tick, to name what a clip change changed.
    clip: String,
    phase: String,
    /// The cloth's shape after the last tick.
    shape: Shape,
    /// Time not yet stepped (the 30 frames a second way).
    pending: f32,
}

/// What a probe tick saw of one piece.
struct Seen {
    way: Way,
    slot: usize,
    class: String,
    piece: usize,
    /// The fastest fixed point since the last tick, in m/s in the piece frame.
    fastest: f32,
    mean_y: f32,
    restarted: bool,
    cut: bool,
    /// The pose crosses one of the body's clip changes, and what changed ("inside" when none).
    crossing: bool,
    cause: &'static str,
    /// The cloth after this tick's steps, and after the last tick's.
    shape: Shape,
    before: Shape,
    context: String,
}

/// A piece's cloth after a tick's steps, in metres.
#[derive(Clone, Copy, Default, Debug)]
struct Shape {
    /// Its longest mesh edge with a free end against that edge's rest length, and how much longer it is.
    edge: f32,
    excess: f32,
    /// The free points' largest distance from the rest shape carried by the pose (the restart's shape).
    off: f32,
    /// How far its highest free point stands above its highest fixed one, in the world's up direction.
    rise: f32,
}

fn shape(piece: &cloth::Piece, pos: &[V3], scratch: &[cloth::Affine], frame: &three::Matrix4) -> Shape {
    let e = &frame.elements;
    let up = |x: &V3| (e[1] * f64::from(x[0]) + e[5] * f64::from(x[1]) + e[9] * f64::from(x[2])) as f32;
    let mut s = Shape::default();
    for t in &piece.triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (i, j) = (usize::from(a), usize::from(b));
            let rest = dist(piece.rest[i], piece.rest[j]);
            if i.max(j) < piece.fixed || rest <= 1e-4 {
                continue;
            }
            let length = dist(pos[i], pos[j]);
            if length / rest > s.edge {
                (s.edge, s.excess) = (length / rest, length - rest);
            }
        }
    }
    let mut carried = Vec::new();
    piece.carried(scratch, &mut carried);
    s.off = (piece.fixed..pos.len()).map(|i| dist(pos[i], carried[i])).fold(0f32, f32::max);
    let (fixed, free) = pos.split_at(piece.fixed);
    let top = |xs: &[V3]| xs.iter().map(up).fold(f32::MIN, f32::max);
    s.rise = top(free) - top(fixed);
    s
}

fn root() -> std::path::PathBuf {
    std::env::var_os("BFE_SWBF2_PREPARED").map(std::path::PathBuf::from).expect("BFE_SWBF2_PREPARED")
}

fn open(unit: &str, seed: u32, bots: u32) -> Result<Battle, String> {
    let root = root();
    let side = std::env::var("PROBE_SIDE")
        .unwrap_or_else(|_| if unit.starts_with("all") || unit.starts_with("rep") { "all" } else { "imp" }.into());
    let selection = MissionSelection { family: bfe_family::GameFamily::Swbf2, map: "tat2".into(), mode: "eli".into(), era: "gcw".into(), seed };
    let options = Options {
        family: selection.family,
        map: selection.map.clone(),
        mode: selection.mode.clone(),
        side,
        unit: Some(unit.into()),
        seed,
        bots: Bots::read("classic", "medium", "full"),
        ..Default::default()
    };
    let ctx = Context::new();
    let (prepared, journal) = Prepared::developer_mission_in(
        ctx,
        &root,
        &options,
        &selection,
        MissionPreferences { bots_per_team: Some(bots), direct_spawn: true, ..Default::default() },
    )?;
    let mut content = bfe_content_native::loader::Loader::detect(&root).load_scripted_mission(
        "tat2",
        &journal.resources.query,
        &journal.resources.mission_json,
    )?;
    let assets = super::assets::prepare(&mut content)?.ok_or("no 2005 characters")?;
    let characters = Characters::headless(assets)?;
    let mut slot = PublicationSlot::new_preallocated();
    // As the battle thread readies its slots for a 2005 battle: the infantry side records (contacts, powers with
    // the tumble and combo poses, hero audio).
    slot.contacts = Some(Box::default());
    slot.powers = Some(Box::new(bfe_sim::combat::powers::PowerFrame::with_capacity(bfe_presentation::layout::unit_limits::ACTORS)));
    slot.hero_audio = Some(Box::default());
    let mut receiver = crate::online::receiver::Receiver::with_bounds(prepared.admission.registry.clone(), prepared.admission.wire_bounds);
    receiver.control_family = prepared.admission.family;
    receiver.registry = true;
    receiver.welcomed = true;
    receiver.own.id = prepared.admission.player_id;
    let rt = Runtime::new_into_slot(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS, &mut slot)?;
    Publication::new(&slot.block, &slot.bytes)?.deliver(slot.client, &mut receiver, 0.0)?;
    Ok(Battle { rt, slot, receiver, characters, now_ms: 0., tick: 0 })
}

impl Battle {
    /// One tick with the player's keys (`held`, `actions` as the client packs them).
    fn step(&mut self, held: u32, actions: u32) -> Result<(), String> {
        let n = self.tick + 1;
        let command = ClientCommand {
            tick: f64::from(n),
            sequence: f64::from(n),
            generation: self.slot.infantry.map_or(0., |s| s.generation),
            held,
            actions,
            has_mouse_scale: true,
            mouse_scale: 1.,
            has_aspect: true,
            aspect: 16. / 9.,
            look_count: 1,
            ..Default::default()
        };
        let flight = ClientFlightCommand { scale: 1., curve: 1., ..Default::default() };
        self.rt.submit(&command, &ClientLook::default(), &flight, self.rt.tick(), None, false)?;
        self.rt.infantry_input(f64::from(n), Controls::default())?;
        self.rt.step_into_slot(&mut self.slot)?;
        self.now_ms += 1000. / 60.;
        Publication::new(&self.slot.block, &self.slot.bytes)?.deliver(self.slot.client, &mut self.receiver, self.now_ms)?;
        // What the battle thread hands the receiver with each publication.
        self.receiver.infantry = self.slot.infantry;
        if let Some(contacts) = self.slot.contacts.as_deref() {
            **self.receiver.contacts.get_or_insert_with(Default::default) = *contacts;
        }
        if let Some(powers) = self.slot.powers.as_deref() {
            powers.copy_into(&mut self.receiver.powers);
        }
        // One drawn frame a tick, as the client advances its clock each frame.
        self.receiver.clock.advance(1. / 60.).map_err(str::to_string)?;
        self.characters.present_records(&self.receiver, None, None, false);
        self.tick = n;
        Ok(())
    }

    /// Poses every soldier with cloth at the shown time, reads each piece's fixed points and steps its cloth each
    /// of the `ways`.
    fn measure(&mut self, ways: &[Way], follows: &mut HashMap<(usize, usize, Way), Follow>, out: &mut Vec<Seen>) {
        let c = &mut self.characters;
        let shown = c.shown;
        let mut scratch = Vec::new();
        let mut pins = Vec::new();
        for i in 0..c.slots.len() {
            let Some(id) = c.slots[i].id else { continue };
            let m = &c.models[c.slots[i].model];
            if m.cloth.is_empty() {
                continue;
            }
            let state = c.slots[i].actor.state_at(shown);
            let drawn = c.slots[i].actor.clip_at(&m.class, shown);
            let clip_name = drawn.map_or("", |f| f.name);
            let phase = format!("{:?}", state.tumble.map(|t| t.phase));
            let clip = drawn.map_or_else(String::new, |f| format!("{} t={:.3} blend={:.2}/{:.2}", f.name, f.time, f.blend, f.blend_progress));
            let joints = m.class.animation.rig.joints.len();
            let mut pose = c.poses.slot(i, joints);
            let s = &mut c.slots[i];
            if let Some(frame) = s.seat {
                s.actor.place_on(&m.class, shown, frame, &mut pose);
            } else {
                s.actor.place_at(&m.class, shown, false, &mut pose);
            }
            let cut_flag = std::mem::take(&mut s.cloth_cut);
            let crossing = s.cloth_crossing(shown);
            for (k, piece) in m.cloth.iter().enumerate() {
                scratch.resize(piece.scratch_len(), cloth::Affine::default());
                let frame = piece.frame(&pose.world[..]);
                piece.prepare(&frame, &pose.world[..], &mut scratch);
                piece.pin_targets(&scratch, &mut pins);
                for &way in ways {
                    let f = follows.entry((i, k, way)).or_default();
                    let fresh = f.id != id || f.clock.is_none();
                    let cut = fresh || cut_flag;
                    let dt = ((shown - f.shown) / 60.) as f32;
                    let fastest = if fresh || dt <= 0. || f.pins.len() != pins.len() {
                        0.
                    } else {
                        pins.iter().zip(&f.pins).map(|(a, b)| dist(*a, *b) / dt).fold(0f32, f32::max)
                    };
                    let n = piece.rest.len();
                    if fresh {
                        *f = Follow { id, pos: vec![[0.; 3]; n], old: vec![[0.; 3]; n], normal: vec![[0.; 3]; n], clock: Some(cloth::Clock::default()), ..Follow::default() };
                    }
                    let e = &frame.elements;
                    let origin = [e[12] as f32, e[13] as f32, e[14] as f32];
                    let mut p = Particles { pos: &mut f.pos, old: &mut f.old, normal: &mut f.normal };
                    let clock = f.clock.as_mut().unwrap();
                    let elapsed = if cut { 0. } else { dt.max(0.) };
                    let restarted = if matches!(way, Way::Game | Way::Game30) {
                        if cut {
                            piece.reset(&mut p, clock, &scratch, id.0 | 1);
                            f.pending = 0.;
                        }
                        f.pending += elapsed;
                        if way == Way::Game || f.pending >= 1. / 30. - 1e-4 {
                            piece.advance_once(&mut p, clock, &scratch, origin, std::mem::take(&mut f.pending));
                        }
                        cut
                    } else {
                        piece.frame_step(&mut p, clock, &scratch, origin, elapsed, cut, crossing && way == Way::Restart, id.0 | 1).1
                    };
                    let mean_y = pins.iter().map(|x| x[1]).sum::<f32>() / pins.len().max(1) as f32;
                    let before = f.shape;
                    f.shape = shape(piece, &f.pos, &scratch, &frame);
                    let cause = if !crossing {
                        "inside"
                    } else if f.clip != clip_name {
                        "new-clip"
                    } else if f.phase != phase {
                        "tumble-phase"
                    } else {
                        "clip-restarted"
                    };
                    f.pins.clone_from(&pins);
                    f.shown = shown;
                    f.clip.clear();
                    f.clip.push_str(clip_name);
                    f.phase.clone_from(&phase);
                    out.push(Seen {
                        way,
                        slot: i,
                        class: m.class.name.clone(),
                        piece: k,
                        fastest,
                        mean_y,
                        restarted: restarted && !cut,
                        cut,
                        crossing,
                        cause,
                        shape: f.shape,
                        before,
                        context: format!(
                            "shown={shown:.2} air={:?} tumble={:?} action={:?} combo={:?} roll={} clip={clip}",
                            state.air,
                            state.tumble.map(|t| t.phase),
                            state.action,
                            state.combo,
                            state.roll.is_some(),
                        ),
                    });
                }
            }
        }
    }
}

fn dist(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}


/// Each cloth hero as the player, alone, standing still and then jumping straight up: per tick around the jump, the
/// fastest fixed point of each piece (m/s in its piece frame), their mean height and the animation clip.
/// `PROBE_JUMP_AT` (0: no jump), `PROBE_FROM`, `PROBE_TO` (ticks printed) and `PROBE_TICKS` change the take.
#[test]
#[ignore = "needs prepared 2005 content (BFE_SWBF2_PREPARED)"]
fn jump_from_standing() {
    let units = std::env::var("PROBE_UNITS").expect("PROBE_UNITS");
    let seed = std::env::var("PROBE_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    for unit in units.split(',') {
        let mut b = match open(unit, seed, 0) {
            Ok(b) => b,
            Err(e) => {
                println!("== {unit}: {e}");
                continue;
            }
        };
        let mut follows = HashMap::new();
        let ways = ways(&b.characters);
        let mut worst = (0f32, 0u32, String::new());
        let knob = |name: &str, default: u32| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
        let jump_at = knob("PROBE_JUMP_AT", 150);
        let (from, to) = (knob("PROBE_FROM", jump_at.saturating_sub(6)), knob("PROBE_TO", jump_at + 40));
        for n in 1..=knob("PROBE_TICKS", 260) {
            let actions = if n == jump_at { 1 << ClientAction::Jump as u8 } else { 0 };
            let held = if n == jump_at { 1 << ClientHeld::Space as u8 } else { 0 };
            b.step(held, actions).unwrap();
            let mut seen = Vec::new();
            b.measure(&ways, &mut follows, &mut seen);
            let own = b.receiver.own.id as u32;
            for s in seen.iter().filter(|s| b.characters.slots[s.slot].id.map(|i| i.0) == Some(own)) {
                if (from..=to).contains(&n) {
                    println!(
                        "{unit} {:?} tick {n} piece {} pins_fastest {:.2} m/s pins_mean_y {:.3} {} edge {:.2} excess_cm {:.0} off_cm {:.0} rise_cm {:.0} {}{}",
                        s.way, s.piece, s.fastest, s.mean_y, s.cause, s.shape.edge, s.shape.excess * 100., s.shape.off * 100., s.shape.rise * 100., s.context, if s.restarted { " RESTART" } else { "" }
                    );
                }
                if n > 30 && s.fastest > worst.0 {
                    worst = (s.fastest, n, s.context.clone());
                }
            }
        }
        println!("== {unit}: fastest fixed point {:.2} m/s at tick {} ({})", worst.0, worst.1, worst.2);
        b.rt.close();
    }
}

/// How long a pose jump's aftermath is followed (ticks), and the stretch the cloth counts as back within.
const FOLLOW: u32 = 60;
const BACK: f32 = 1.2;

/// The cloth of one piece followed for `FOLLOW` ticks after a fast tick it did not restart on (`cause` "inside": a
/// fast tick inside a clip).
struct Episode {
    tick: u32,
    last: u32,
    speed: f32,
    cause: &'static str,
    context: String,
    before: Shape,
    /// The largest of each measure while followed, the longest edge's tick and excess.
    peak: Shape,
    peak_at: u32,
    /// The last tick the longest edge was past `BACK`, and the last the free points were more than 5 cm farther
    /// from the carried rest shape than before the jump.
    over: Option<u32>,
    away: Option<u32>,
}
impl Episode {
    fn new(tick: u32, s: &Seen) -> Self {
        let lowest = Shape { edge: 0., excess: f32::MIN, off: 0., rise: f32::MIN };
        let mut e = Self { tick, last: tick, speed: s.fastest, cause: s.cause, context: s.context.clone(), before: s.before, peak: lowest, peak_at: 0, over: None, away: None };
        e.see(tick, s);
        e
    }
    fn see(&mut self, tick: u32, s: &Seen) {
        self.last = tick;
        if s.shape.edge > self.peak.edge {
            (self.peak.edge, self.peak.excess, self.peak_at) = (s.shape.edge, s.shape.excess, tick - self.tick);
        }
        self.peak.off = self.peak.off.max(s.shape.off);
        self.peak.rise = self.peak.rise.max(s.shape.rise);
        if s.shape.edge > BACK {
            self.over = Some(tick);
        }
        if s.shape.off > self.before.off + 0.05 {
            self.away = Some(tick);
        }
    }
    /// Ticks until the cloth was back after `over` (None: not within the window).
    fn back(&self, over: Option<u32>) -> Option<u32> {
        match over {
            None => Some(0),
            Some(t) if t == self.last && self.last + 1 - self.tick >= FOLLOW => None,
            Some(t) => Some(t + 1 - self.tick),
        }
    }
}

/// Per class and way: piece-ticks, fast ones, restarts and the fastest; fast ticks and restarts by cause; every
/// tick's longest edge and carried-shape distance; the followed jumps: count, past 1.5 times rest from below it,
/// already past it before the jump, largest edge and distance, ticks back.
#[derive(Default)]
struct Tally {
    ticks: u32,
    fast: u32,
    restarts: u32,
    fastest: f32,
    fast_by: HashMap<&'static str, u32>,
    restarts_by: HashMap<&'static str, u32>,
    edges: Vec<f32>,
    offs: Vec<f32>,
    episodes: u32,
    thrown: u32,
    already: u32,
    peak: f32,
    off: f32,
    rise: f32,
    backs: Vec<u32>,
    never_back: u32,
    returns: Vec<u32>,
    never_return: u32,
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_unstable_by(f32::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.)
}

/// Mos Eisley Hero Assault with bots on both sides, the player standing: every tick every cloth piece in the
/// battle measured; prints each tick a fixed point moves faster than `FAST` (with whether its pose crosses a clip
/// change), every restart the pose-jump rule makes and, for each fast tick the cloth was not restarted on, what it
/// did in the following second; a summary per class.
#[test]
#[ignore = "needs prepared 2005 content (BFE_SWBF2_PREPARED)"]
fn battles() {
    let seeds = std::env::var("PROBE_SEEDS").unwrap_or_else(|_| "1-20".into());
    let (a, z) = seeds.split_once('-').map_or((seeds.as_str(), seeds.as_str()), |(a, z)| (a, z));
    let (a, z): (u32, u32) = (a.parse().unwrap(), z.parse().unwrap());
    let ticks = std::env::var("PROBE_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(3600u32);
    let bots = std::env::var("PROBE_BOTS").ok().and_then(|v| v.parse().ok()).unwrap_or(8u32);
    let unit = std::env::var("PROBE_PLAYER").expect("PROBE_PLAYER");
    let mut per_class: HashMap<(String, Way), Tally> = HashMap::new();
    let end = |seed: u32, key: (usize, usize, Way), class: &str, e: Episode, per_class: &mut HashMap<(String, Way), Tally>| {
        let (back, home) = (e.back(e.over), e.back(e.away));
        let ticks = |b: Option<u32>| b.map_or_else(|| format!(">{FOLLOW}"), |b| format!("+{b}"));
        let (b, p) = (e.before, e.peak);
        println!(
            "episode {:?} seed {seed} tick {} {class} piece {} pins {:.1} m/s {} | before edge {:.2} off_cm {:.0} rise_cm {:.0} | peak edge {:.2} (+{:.0} cm) at +{} off_cm {:.0} rise_cm {:.0} | back within {BACK} {} | off back within 5 cm {} | slot {} {}",
            key.2, e.tick, key.1, e.speed, e.cause, b.edge, b.off * 100., b.rise * 100., p.edge, p.excess * 100., e.peak_at, p.off * 100., p.rise * 100.,
            ticks(back), ticks(home), key.0, e.context
        );
        let t = per_class.entry((class.to_string(), key.2)).or_default();
        t.episodes += 1;
        t.thrown += u32::from(p.edge > 1.5 && b.edge <= 1.5);
        t.already += u32::from(b.edge > 1.5);
        t.peak = t.peak.max(p.edge);
        t.off = t.off.max(p.off);
        t.rise = t.rise.max(p.rise);
        match back {
            Some(b) => t.backs.push(b),
            None => t.never_back += 1,
        }
        match home {
            Some(b) => t.returns.push(b),
            None => t.never_return += 1,
        }
    };
    for seed in a..=z {
        let mut b = match open(&unit, seed, bots) {
            Ok(b) => b,
            Err(e) => {
                println!("== seed {seed}: {e}");
                continue;
            }
        };
        let mut follows = HashMap::new();
        let ways = ways(&b.characters);
        let mut episodes: HashMap<(usize, usize, Way), (String, Episode)> = HashMap::new();
        let (mut fast, mut restarts, mut inside) = (0u32, 0u32, 0u32);
        for _ in 0..ticks {
            if b.step(0, 0).is_err() {
                break;
            }
            let mut seen = Vec::new();
            b.measure(&ways, &mut follows, &mut seen);
            for s in &seen {
                let key = (s.slot, s.piece, s.way);
                let t = per_class.entry((s.class.clone(), s.way)).or_default();
                t.ticks += 1;
                t.fastest = t.fastest.max(s.fastest);
                t.edges.push(s.shape.edge);
                t.offs.push(s.shape.off);
                let jumped = s.fastest > FAST;
                if jumped {
                    fast += u32::from(s.way == ways[0]);
                    t.fast += 1;
                    *t.fast_by.entry(s.cause).or_default() += 1;
                    if s.way == ways[0] || s.restarted {
                        println!("seed {seed} tick {} {} piece {} pins_fastest {:.1} m/s {} {:?} {}{}", b.tick, s.class, s.piece, s.fastest, s.cause, s.way, s.context, if s.restarted { " RESTART" } else { "" });
                    }
                }
                if s.restarted {
                    restarts += 1;
                    t.restarts += 1;
                    *t.restarts_by.entry(s.cause).or_default() += 1;
                    inside += u32::from(!s.crossing);
                }
                // A fast tick the cloth was not restarted on starts a follow (at a clip change, or inside a clip
                // when none runs); it ends at the window, a cut or the next pose jump at a clip change.
                let running = episodes.contains_key(&key);
                let starts = jumped && !s.restarted && !s.cut && (s.crossing || !running);
                let done = episodes.get(&key).is_some_and(|(_, e)| s.cut || starts || b.tick - e.tick >= FOLLOW);
                if done {
                    let (class, e) = episodes.remove(&key).unwrap();
                    end(seed, key, &class, e, &mut per_class);
                } else if let Some((_, e)) = episodes.get_mut(&key) {
                    e.see(b.tick, s);
                }
                if starts {
                    episodes.insert(key, (s.class.clone(), Episode::new(b.tick, s)));
                }
            }
        }
        for (key, (class, e)) in episodes.drain() {
            end(seed, key, &class, e, &mut per_class);
        }
        println!("== seed {seed}: {ticks} ticks, {fast} piece-ticks over {FAST} m/s, {restarts} pose-jump restarts, {inside} inside a clip ({ways:?})");
        b.rt.close();
    }
    let mut classes: Vec<_> = per_class.into_iter().collect();
    classes.sort_by(|x, y| x.0.cmp(&y.0));
    for ((class, way), mut t) in classes {
        let sorted = |m: &HashMap<&'static str, u32>| {
            let mut v: Vec<_> = m.iter().map(|(k, n)| format!("{k} {n}")).collect();
            v.sort();
            v.join(", ")
        };
        t.backs.sort_unstable();
        t.returns.sort_unstable();
        let at = |v: &[u32]| v.get(v.len() / 2).copied().unwrap_or(0);
        println!(
            "== {class} {way:?}: {} piece-ticks, {} over {FAST} m/s ({}), {} restarts ({}), fastest {:.1} m/s; every tick: median edge {:.2}, median off {:.0} cm; {} followed jumps: {} past 1.5 times rest from below it, {} already past it before, largest edge {:.2}, off {:.0} cm, rise {:.0} cm; median {} ticks back within {BACK} ({} not within {FOLLOW}), median {} ticks off back within 5 cm ({} not within {FOLLOW})",
            t.ticks, t.fast, sorted(&t.fast_by), t.restarts, sorted(&t.restarts_by), t.fastest, median(&mut t.edges), median(&mut t.offs) * 100.,
            t.episodes, t.thrown, t.already, t.peak, t.off * 100., t.rise * 100., at(&t.backs), t.never_back, at(&t.returns), t.never_return
        );
    }
}
