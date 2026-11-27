//! A crowded 2005 battle through the whole publication chain: generated
//! records and staged world fields into the retained ring, the real publisher
//! and its per-peer budget at the default 100,000 bytes a second, a lossy and
//! reordering path, and complete receivers that acknowledge what they finish.
//! One player is there from the start, one joins late; soldiers and hulls die
//! and come back as new lives throughout. Every player must keep completing
//! snapshots within the budget, and the cadence they get is printed.
mod crowded;
use bfe_presentation::{network::Session, network_layout::net_header as h};
use bfe_sim::{
    gen::{BlockHeader, WirePublicationOutput, WireRecord},
    store::{StrId, Strings},
    wire::{
        bounds::Capacity, codec, edges::Host as EdgeHost, host::Host, journal::Events, publisher::{Actor, Correction, Fields, Personal,
        Publisher, World}, stagger::Stagger, Runtime,
    },
};

const BUDGET: f64 = 100_000.;
const TICKS: u32 = 60 * 30;
const LATE: u32 = 60 * 10;

struct Battle { records: Vec<WireRecord>, stage: Stagger, strings: Strings, own: Vec<u8>, edges: Vec<EdgeHost> }

impl World for Battle {
    fn capture(&mut self, tick: f64, epoch: f64, runtime: &mut Runtime) -> codec::Result<()> {
        let fields = self.stage.world();
        runtime.capture_canonical(tick, epoch, &self.records, fields.iter(), &self.strings)
    }
    fn actor(&self, peer: u32) -> Actor {
        Actor { present: true, spectator: false, unit: Some((f64::from(peer), 1.)), position: Some([0., 0., 0.]), ack: 1., ack_tick: 1.,
            discarded_input: 0., discarded_actions: 0. }
    }
    fn summaries(&mut self, _: u32, _: f64, fields: &mut Fields) -> codec::Result<()> { fields.clear(); Ok(()) }
    fn own(&mut self, peer: u32, _: f64) -> codec::Result<Correction<'_>> {
        self.own.clear();
        self.own.extend((0..141).map(|i| (i as u32 + peer) as u8));
        Ok(Correction { id: f64::from(peer), generation: 1., schema: 11., payload: &self.own })
    }
    fn events(&mut self, events: &mut Events) { events.begin(); }
    fn status(&self, id: StrId) -> &[u16] { self.strings.units(id) }
    fn edges(&self, handle: usize) -> Option<&EdgeHost> { self.edges.get(handle) }
}

struct Player { id: u32, session: Session, inbox: Vec<Vec<u8>>, complete: u32, longest_gap: u32, last: u32, bytes: f64 }

#[test]
fn a_crowded_battle_keeps_every_player_completing_within_the_budget() {
    let bounds = crowded::bounds();
    let plan = crowded::field_plan(286);
    let mut battle = Battle { records: Vec::new(), stage: Stagger::with_capacity(Capacity::FULL.fields), strings: Strings::default(), own: Vec::new(),
        edges: vec![EdgeHost::default(), EdgeHost::default()] };
    let mut runtime = Runtime::for_battle(bounds, Capacity::FULL);
    runtime.reserve_world(4, Capacity::FULL.fields).unwrap();
    let mut host = Host::new(false, BUDGET, 5000., 1.);
    let mut publisher = Publisher::for_battle(bounds, Capacity::FULL);
    publisher.admit(2);
    let mut players: Vec<Player> = Vec::new();
    let mut rng = crowded::Rng(bfe_wide::Wide::from_u32(0xc4a1_0e55));
    let mut delayed: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    // Time spent staging the world fields and publishing, for the cost per tick.
    let (mut staging, mut publishing) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    for tick in 1..=TICKS {
        // Players join: one at the start, one ten seconds in.
        for (id, at) in [(101u32, 1u32), (102, LATE)] {
            if tick == at {
                host.add(id, f64::from(tick) * 1000. / 60.);
                let c = host.client_mut(id);
                c.started = true;
                c.ready = true;
                c.publication = Some(Personal::new(id - 100, (id - 101) as usize).unwrap());
                let mut session = Session::new();
                session.reserve_receive();
                session.admit_swbf2();
                players.push(Player { id, session, inbox: Vec::new(), complete: 0, longest_gap: 0, last: tick, bytes: 0. });
            }
        }
        let at = f64::from(tick) * 1000. / 60.;
        // The world moves; its fields are offered to the stage on their cadences.
        battle.records = crowded::records(bounds, tick, true);
        let payloads: Vec<Vec<u8>> = plan.iter().map(|&(_, id, size)|
            (0..size).map(|i| (i as u32).wrapping_mul(31).wrapping_add(id).wrapping_add(tick / 5) as u8).collect()).collect();
        let started = std::time::Instant::now();
        battle.stage.begin();
        for (&(schema, id, _), payload) in plan.iter().zip(&payloads) {
            battle.stage.offer_native(f64::from(tick), f64::from(schema), f64::from(id), 1., payload, None).unwrap();
        }
        battle.stage.finish(&[]);
        staging += started.elapsed();
        for p in &players { host.client_mut(p.id).heard_at = at; }
        let started = std::time::Instant::now();
        publisher.run(&mut host, &mut runtime, &mut battle, f64::from(tick), at, 1_700_000_000_000. + at, 0.);
        publishing += started.elapsed();
        let words = &publisher.block.words;
        let heap = words[BlockHeader::HEAP] as usize;
        let output = WirePublicationOutput::take(&words[BlockHeader::WORDS..], &words[heap..]);
        let span = |s: &bfe_sim::gen::WireSpan| publisher.bytes[s.offset as usize..(s.offset + s.length) as usize].to_vec();
        for p in &mut players {
            let mut sent: Vec<Vec<u8>> = output.shared.iter().filter(|g| g.clients.contains(&p.id)).flat_map(|g| g.parts.iter().map(span)).collect();
            sent.extend(output.own.iter().filter(|o| o.client == p.id).map(|o| span(&o.span)));
            p.bytes += sent.iter().map(Vec::len).sum::<usize>() as f64;
            // The path loses one datagram in 40 and holds one in 30 back by a few ticks.
            for packet in sent {
                match rng.below(120) {
                    0..=2 => {}
                    3..=6 => delayed.push((tick + 2 + rng.below(4) as u32, p.id, packet)),
                    _ => p.inbox.push(packet),
                }
            }
        }
        let (due, waiting): (Vec<_>, Vec<_>) = delayed.drain(..).partition(|(t, _, _)| *t <= tick);
        delayed = waiting;
        for (_, id, packet) in due { if let Some(p) = players.iter_mut().find(|p| p.id == id) { p.inbox.push(packet); } }
        for p in &mut players {
            let before = p.session.header[h::ACK_TICK];
            for packet in p.inbox.drain(..) { p.session.receive_with_bounds(&packet, &bounds).unwrap(); }
            let ack = p.session.header[h::ACK_TICK];
            if ack > before {
                p.complete += 1;
                p.longest_gap = p.longest_gap.max(tick - p.last);
                p.last = tick;
                host.accept_ack(p.id, ack, 1.);
            }
        }
    }
    println!("space chain cost per tick: staging {:.0} us, capture and publication {:.0} us",
        staging.as_secs_f64() * 1e6 / f64::from(TICKS), publishing.as_secs_f64() * 1e6 / f64::from(TICKS));
    for p in &players {
        let seconds = f64::from(TICKS + 1 - if p.id == 102 { LATE } else { 1 }) / 60.;
        let rate = p.bytes / seconds;
        println!("space chain player {} complete={} ({:.1}/s) longest gap {} ticks, {:.0} B/s, receiver retired {} missed {}",
            p.id, p.complete, f64::from(p.complete) / seconds, p.longest_gap, rate, p.session.retired, p.session.missed);
        // Within a tenth of the budget's burst, and never stalled for more than two seconds.
        assert!(rate <= BUDGET * 1.1, "player {} at {rate} B/s", p.id);
        assert!(p.longest_gap <= 120, "player {} stalled for {} ticks", p.id, p.longest_gap);
        assert!(f64::from(p.complete) / seconds >= 2.0, "player {} completed only {}", p.id, p.complete);
    }
}
