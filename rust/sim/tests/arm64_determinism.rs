//! Cross-architecture (x86_64 vs aarch64) simulation and wire-publication
//! determinism gate. Runs in CI without game files: steps a multi-tick
//! scenario exercising `v8math` transcendentals (`sin`, `cos`, `tan`, `atan2`,
//! `exp`, `log`, `pow`, `cbrt`, `hypot`), movement/orientation math, canonical
//! store capture (`wire::store_capture::Capture`), retained snapshot ring
//! (`wire::retained::Ring`), and `wire::publisher::Publisher::run` (including
//! baseline selection, shared entity deltas, per-client own corrections,
//! summaries, cosmetic outbox packing, reliable gameplay records, and edge
//! outcomes), then asserts that the SHA-256 digest of all published bytes and
//! descriptor words across all ticks matches the committed `x86_64` digest bit
//! for bit.

use bfe_sim::{
    combat::defs::WeaponDefs,
    gen::*,
    store::{DefId, Ref, StrId, Strings},
    wire::{
        codec,
        edges::Host as EdgeHost,
        host::Host,
        journal::Events as JournalEvents,
        publisher::{Actor, Correction, Fields, Personal, Publisher, World as PublishWorld},
        shots::Counters,
        store_capture::{Capture, Turret as TurretSample, World as CaptureWorld},
        Runtime,
    },
};

struct ScenarioReaders {
    walk_phase: (f64, f64),
    turret: TurretSample,
    remote: Option<(u32, Ref<ReconDroid>)>,
}

impl CaptureWorld for ScenarioReaders {
    fn walk(&self, _: Ref<Vehicle>) -> (f64, f64) {
        self.walk_phase
    }
    fn remote_count(&self) -> usize {
        usize::from(self.remote.is_some())
    }
    fn remote(&self, _: usize) -> Option<(u32, Ref<ReconDroid>)> {
        self.remote
    }
    fn turret_count(&self) -> usize {
        1
    }
    fn turret(&self, _: usize) -> Option<TurretSample> {
        Some(self.turret)
    }
}

struct ScenarioPublishWorld {
    store: Store,
    strings: Strings,
    defs: WeaponDefs,
    shots: Counters,
    capture: Capture,
    readers: ScenarioReaders,
    edges: [EdgeHost; 2],
    status_ok: StrId,
    own_buf: Vec<u8>,
}

impl PublishWorld for ScenarioPublishWorld {
    fn capture(&mut self, tick: f64, epoch: f64, runtime: &mut Runtime) -> codec::Result<()> {
        self.capture
            .capture(&self.store, &self.defs, &self.readers, &self.shots)?;
        let angle = tick * 0.137;
        let world_field = WireField {
            id: 1.0,
            schema: 1.0,
            generation: 1.0,
            payload: {
                let s = v8math::sin(angle);
                let c = v8math::cos(angle);
                let t = v8math::tan(angle * 0.5);
                let p = v8math::pow(1.0001 + (tick * 0.01), 2.345);
                let mut b = Vec::with_capacity(32);
                b.extend_from_slice(&s.to_le_bytes());
                b.extend_from_slice(&c.to_le_bytes());
                b.extend_from_slice(&t.to_le_bytes());
                b.extend_from_slice(&p.to_le_bytes());
                b
            },
        };
        runtime.capture_canonical(
            tick,
            epoch,
            self.capture.records(),
            std::iter::once(&world_field),
            &self.strings,
        )
    }

    fn actor(&self, peer: u32) -> Actor {
        let u = Ref::<Unit>::new(peer, 1);
        if let Some(unit) = self.store.unit.get(u) {
            let pos = match &unit.soldier {
                UnitBody::Soldier(s) => s.position,
                _ => [0.0; 3],
            };
            Actor {
                present: !unit.removed,
                spectator: false,
                unit: Some((peer as f64, unit.generation)),
                position: Some(pos),
                ack: 1.0,
                ack_tick: 1.0,
                discarded_input: 0.0,
                discarded_actions: 0.0,
            }
        } else {
            Actor::default()
        }
    }

    fn summaries(&mut self, peer: u32, _epoch: f64, fields: &mut Fields) -> codec::Result<()> {
        fields.clear();
        let x = v8math::exp((peer as f64) * 0.07) + v8math::log(1.5 + peer as f64);
        let y = v8math::cbrt(peer as f64 + 0.125) + v8math::atan2(0.3, 0.7);
        let mut payload = Vec::with_capacity(16);
        payload.extend_from_slice(&x.to_le_bytes());
        payload.extend_from_slice(&y.to_le_bytes());
        fields.push(&WireField {
            id: 10.0 + peer as f64,
            schema: 1.0,
            generation: 1.0,
            payload,
        })
    }

    fn own(&mut self, peer: u32, _epoch: f64) -> codec::Result<Correction<'_>> {
        self.own_buf.clear();
        let u = Ref::<Unit>::new(peer, 1);
        if let Some(unit) = self.store.unit.get(u) {
            if let UnitBody::Soldier(s) = &unit.soldier {
                for &coord in &s.position {
                    self.own_buf.extend_from_slice(&coord.to_le_bytes());
                }
                for &vel in &s.velocity {
                    self.own_buf.extend_from_slice(&vel.to_le_bytes());
                }
                self.own_buf.extend_from_slice(&s.aim_yaw.to_le_bytes());
                self.own_buf.extend_from_slice(&s.aim_pitch.to_le_bytes());
            }
        }
        Ok(Correction {
            id: peer as f64,
            generation: 1.0,
            schema: 1.0,
            payload: &self.own_buf,
        })
    }

    fn events(&mut self, events: &mut JournalEvents) {
        events.begin();
    }

    fn status(&self, id: StrId) -> &[u16] {
        self.strings.units(id)
    }

    fn edges(&self, handle: usize) -> Option<&EdgeHost> {
        self.edges.get(handle)
    }
}

/// Minimal self-contained SHA-256 implementation so `sim` needs no extra crate
/// dependency to verify the committed `x86_64` digest.
struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_len: u64,
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_len: 0,
        }
    }

    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(Self::K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_len += data.len() as u64;
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                Self::compress(&mut self.state, &self.buf);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let block: &[u8; 64] = data[..64].try_into().unwrap();
            Self::compress(&mut self.state, block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn finish_hex(mut self) -> String {
        let bit_len = self.total_len * 8;
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            self.buf[self.buf_len..64].fill(0);
            Self::compress(&mut self.state, &self.buf);
            self.buf_len = 0;
        }
        self.buf[self.buf_len..56].fill(0);
        self.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
        Self::compress(&mut self.state, &self.buf);
        let mut out = String::with_capacity(64);
        for word in self.state {
            for byte in word.to_be_bytes() {
                out.push_str(&format!("{byte:02x}"));
            }
        }
        out
    }
}

#[test]
fn published_bytes_match_committed_x86_64_digest() {
    let mut strings = Strings::default();
    let infantry = strings.intern_str("unit");
    let vehicle_class = strings.intern_str("vehicle");
    let built = strings.intern_str("built");
    let status_ok = strings.intern_str("accepted");

    let setup = WireStoreSetup {
        classes: vec![
            WireStoreName {
                name: infantry,
                id: 1,
            },
            WireStoreName {
                name: vehicle_class,
                id: 2,
            },
        ],
        items: vec![WireStoreDefinition {
            definition: 7,
            id: 7,
        }],
        remotes: vec![WireStoreDefinition {
            definition: 8,
            id: 8,
        }],
        item_capacity: 4,
        remote_capacity: 2,
        entity_capacity: 256,
        record_capacity: 512,
        seat_capacity: 8,
        ..Default::default()
    };
    let capture = Capture::new(&setup, &strings).unwrap();

    let mut store = Store::default();
    let u1 = store.unit.insert_at(
        1,
        1,
        Unit {
            class_name: infantry,
            generation: 1.0,
            team: 1.0,
            soldier: UnitBody::Soldier(Soldier {
                position: [1.25, 2.5, -3.75],
                velocity: [0.5, -0.0, 1.25],
                aim_yaw: -0.0,
                aim_pitch: 0.3,
                ..Default::default()
            }),
            combat: Combat {
                health: 100.0,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let u2 = store.unit.insert_at(
        2,
        1,
        Unit {
            class_name: infantry,
            generation: 1.0,
            team: 2.0,
            soldier: UnitBody::Soldier(Soldier {
                position: [-14.0, 0.0, 8.5],
                velocity: [-1.0, 0.25, 0.0],
                aim_yaw: 1.1,
                aim_pitch: -0.2,
                ..Default::default()
            }),
            combat: Combat {
                health: 85.0,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    store.units.list.push(Member::Unit(u1));
    store.units.list.push(Member::Unit(u2));

    let v = store.vehicle.insert(Vehicle {
        name: vehicle_class,
        target_id: 10.0,
        target_generation: 2.0,
        team: Some(2.0),
        alive: true,
        position: [20.0, -10.0, 3.0],
        velocity: [0.0, 0.2, 31.75],
        quaternion: [0.0, 0.0, 0.0, 1.0],
        health: 500.0,
        power: Some(3.5),
        slot_index: 0.0,
        tilt: -0.0,
        roll: -0.0,
        seats: vec![VehicleSeat {
            aim_yaw: 0.25,
            pitch: -0.0,
            ..Default::default()
        }],
        ..Default::default()
    });
    store.vehicles.vehicles.push(v);

    let item = store.item.insert(Item {
        def: DefId(7),
        position: [4.0, 1.0, -2.0],
        velocity: [0.1, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        yaw: 0.3,
        ..Default::default()
    });
    store.items.items.push(item);

    let droid = store.recon_droid.insert(ReconDroid {
        class: 8,
        team: 2.0,
        position: [2.0, 3.0, 4.0],
        health: 21.5,
        yaw: 0.2,
        pitch: -0.5,
        ..Default::default()
    });
    let turret_ref = Ref::new(19, 7);

    let mut world = ScenarioPublishWorld {
        store,
        strings,
        defs: WeaponDefs::default(),
        shots: Counters::default(),
        capture,
        readers: ScenarioReaders {
            walk_phase: (1.5, 0.5),
            turret: TurretSample {
                reference: turret_ref,
                id: 20.0,
                life: 3.0,
                building: 0.0,
                state: built,
                team: 2.0,
                health: 80.0,
                yaw: 0.1,
                pitch: 0.2,
                gun_yaw: 0.3,
                gun_pitch: -0.0,
            },
            remote: Some((1, droid)),
        },
        edges: [EdgeHost::default(), EdgeHost::default()],
        status_ok,
        own_buf: Vec::with_capacity(128),
    };

    let mut runtime = Runtime::default();
    runtime.reserve_world(8, 4).unwrap();

    let mut host = Host::new(false, 65536.0, 5000.0, 1.0);
    host.add(101, 0.0);
    host.add(102, 0.0);
    for (idx, (cid, peer)) in [(101u32, 1u32), (102u32, 2u32)].into_iter().enumerate() {
        let c = host.client_mut(cid);
        c.started = true;
        c.ready = true;
        c.publication = Some(Personal::new(peer, idx).unwrap());
    }

    let mut publisher = Publisher::default();
    publisher.admit(2);

    let mut hasher = Sha256::new();

    for step in 1..=24 {
        let tick = (step * 2) as f64;
        let at = tick * (1000.0 / 60.0);
        let unix = 1_700_000_000_000.0 + at;

        // Advance unit, vehicle, item, droid, and turret states using v8math
        // transcendentals at angles that previously diverged under arm64 FMA
        // (e.g., i * 0.021, i * 0.037, i * 0.09).
        let a1 = (step as f64) * 0.021 * 29.0;
        let a2 = (step as f64) * 0.037 * 16.0;
        let a3 = (step as f64) * 0.09 * 29.0;
        let (s1, c1) = v8math::sin_cos(a1);
        let s2 = v8math::sin(a2);
        let c2 = v8math::cos(a2);
        let s3 = v8math::sin(a3);
        let c3 = v8math::cos(a3);
        let hyp = v8math::hypot(s1 * 3.0, v8math::hypot(c2 * 4.0, s3 * 5.0));

        {
            let unit1 = world.store.unit.get_mut(u1).unwrap();
            unit1.combat.health = (100.0 - (step as f64) * 1.25).max(1.0);
            if let UnitBody::Soldier(s) = &mut unit1.soldier {
                s.position[0] += s1 * 0.75;
                s.position[1] += c2 * 0.15;
                s.position[2] += c1 * 0.75;
                s.velocity = [s3 * 6.0, c2 * 1.5, c3 * 6.0];
                s.aim_yaw = s1 * 3.0;
                s.aim_pitch = c2 * 0.7;
            }
        }
        {
            let unit2 = world.store.unit.get_mut(u2).unwrap();
            if let UnitBody::Soldier(s) = &mut unit2.soldier {
                s.position[0] -= c3 * 0.5;
                s.position[2] += s2 * 0.5;
                s.velocity = [-c1 * 4.0, 0.0, s1 * 4.0];
                s.aim_yaw = v8math::atan2(s2, c1);
                s.aim_pitch = v8math::tan(a2 * 0.1).clamp(-0.7, 0.7);
            }
        }
        {
            let hull = world.store.vehicle.get_mut(v).unwrap();
            hull.position[0] += c1 * 1.5;
            hull.position[1] += s2 * 0.25;
            hull.position[2] += s1 * 1.5;
            hull.velocity = [c3 * 12.0, s2 * 0.5, s3 * 12.0];
            let half = a1 * 0.5;
            hull.quaternion = [0.0, v8math::sin(half), 0.0, v8math::cos(half)];
            hull.health = (500.0 - hyp).max(50.0);
            hull.seats[0].aim_yaw = s3 * 1.2;
            hull.seats[0].pitch = c2 * 0.4;
        }
        {
            let it = world.store.item.get_mut(item).unwrap();
            it.position[0] += s2 * 0.1;
            it.yaw = a1;
        }
        {
            let d = world.store.recon_droid.get_mut(droid).unwrap();
            d.position[0] += c2 * 0.3;
            d.yaw = s1;
            d.pitch = -c1 * 0.4;
        }
        world.readers.walk_phase = (v8math::sin(a1) * 2.0, v8math::cos(a2));
        world.readers.turret.yaw = s2 * 0.8;
        world.readers.turret.gun_pitch = c3 * 0.3;

        world.edges[0].epoch = 1.0;
        world.edges[0].generation = 1.0;
        world.edges[0].received.clear();
        world.edges[0].received.push(step as f64);
        world.edges[0].results.clear();
        if step % 3 == 0 {
            world.edges[0].results.push(WeOutcome {
                id: step as f64,
                tick,
                status: world.status_ok,
            });
        }

        for cid in [101u32, 102u32] {
            host.client_mut(cid).heard_at = at;
        }

        publisher.run(&mut host, &mut runtime, &mut world, tick, at, unix, 0.0);

        // Acknowledge previous snapshots on staggered schedules to exercise
        // shared baseline selection (`chooseSharedBaselines`) and delta encoding.
        if step >= 2 {
            host.accept_ack(101, tick - 2.0, 1.0);
        }
        if step >= 3 && step % 2 == 1 {
            host.accept_ack(102, tick - 4.0, 1.0);
        }

        hasher.update(&(publisher.bytes.len() as u64).to_le_bytes());
        hasher.update(&publisher.bytes);
        hasher.update(&(publisher.block.words.len() as u64).to_le_bytes());
        for &w in &publisher.block.words {
            hasher.update(&w.to_bits().to_le_bytes());
        }
    }

    let digest = hasher.finish_hex();
    // Committed x86_64 SHA-256 digest of all 24 published byte arenas and
    // output descriptor blocks.
    assert_eq!(
        digest,
        "7337c8fd3896f59d7369ed60f31ab62dcb77c259a26ca8788599750eff7cbef8",
        "cross-architecture published-bytes determinism mismatch on {}",
        std::env::consts::ARCH
    );
}
