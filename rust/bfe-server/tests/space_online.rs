//! Headless Space Assault online server tests:
//! 1. Scripted network client on real prepared `spa6` deploys on the hangar deck,
//!    boards a fighter, takes off, fires, returns and lands, then boards, aims and
//!    fires a chair gun; every published Space shared field (`0x0211`..`0x0215`) and
//!    `0x91` own correction passes `bfe_sim::wire::revisions::Checks` and fits well
//!    within the 100 KB/s wire budget.
//! 2. Idle local `spa6` battle vs idle server `spa6` battle digest comparison.
use bfe_content::files::DirFiles;
use bfe_family::GameFamily;
use bfe_presentation::network::RecordChecks;
use bfe_server::udp_peer::INPUT_MAGIC;
use bfe_server::world_worker::NativeWorldHost;
use bfe_sim::abi::context::Context;
use bfe_sim::movement::soldier::swbf2::body as swbf2_body;
use bfe_sim::prediction::corrections::{decode_compact_soldier_fixed, decode_own_player_header};
use bfe_sim::space::flight::{SpaceFlightState, SpacePilotInput};
use bfe_sim::space::local::PilotControls;
use bfe_sim::wire::revisions::{
    plan, split_revision, write_section, Checks, Section, Supports, Table, PILOT, SUPPORTS,
    WRITES_SPACE,
};
use bfe_sim::wire::space::{
    read_fighter_field, read_frigates, read_ordnance_field, read_own_body, read_props_field,
    write_pilot_entries, FighterField, OrdnanceField, OwnChairState, OwnPilotState, OwnSpaceBody,
    PilotEntry, PropsField, FIGHTER_SCHEMA, FRIGATES_SCHEMA, ORDNANCE_SCHEMA, OWN_BODY_SCHEMA,
    PROPS_SCHEMA, REVISION,
};
use bfe_sim::world::admit::{
    mission::{MissionPreferences, MissionSelection},
    Bots, Options,
};
use bfe_test_gate::OPEN;
use bfe_transport::layout::SWBF2_INPUT_CODES;
use std::path::PathBuf;

/// Apply a `codec.rs::changed_delta` patch (`[1, cur_len: u16, words: u16, bitmap, words...]`) onto `base`.
fn apply_word_delta(delta: &[u8], base: &[u8]) -> Vec<u8> {
    assert_eq!(delta[0], 1, "unexpected word delta tag");
    let cur_len = u16::from_le_bytes([delta[1], delta[2]]) as usize;
    let words = u16::from_le_bytes([delta[3], delta[4]]) as usize;
    assert_eq!(base.len(), cur_len, "word delta baseline length mismatch");
    let bm_len = words.div_ceil(8);
    let bitmap = &delta[5..5 + bm_len];
    let mut at = 5 + bm_len;
    let mut out = base.to_vec();
    for i in 0..words {
        if bitmap[i / 8] & (1 << (i % 8)) != 0 {
            let start = i * 8;
            let end = (start + 8).min(cur_len);
            out[start..end].copy_from_slice(&delta[at..at + (end - start)]);
            at += 8;
        }
    }
    assert_eq!(at, delta.len());
    out
}

/// Extract world fields (`(id, schema, raw_wire_len, reconstructed_payload)`) from one `BNS1` part,
/// updating `cur_fields` relative to `base_fields` for `mode == 2` word deltas.
fn snapshot_world_fields(
    part: &[u8],
    base_fields: Option<&std::collections::BTreeMap<(u32, u16), Vec<u8>>>,
    cur_fields: &mut std::collections::BTreeMap<(u32, u16), Vec<u8>>,
) -> Vec<(u32, u16, usize, Vec<u8>)> {
    assert_eq!(&part[..4], b"BNS1");
    fn read<'a>(part: &'a [u8], at: &mut usize, n: usize) -> &'a [u8] {
        *at += n;
        &part[*at - n..*at]
    }
    let mut at = if part[15] == 0 { 24 } else { 20 };
    let length = u16::from_le_bytes(read(part, &mut at, 2).try_into().unwrap()) as usize;
    let end = at + length;
    let (mut count, mut shift) = (0usize, 0);
    loop {
        let byte = read(part, &mut at, 1)[0];
        count |= usize::from(byte & 127) << shift;
        shift += 7;
        if byte & 128 == 0 {
            break;
        }
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let id = u32::from_le_bytes(read(part, &mut at, 4).try_into().unwrap());
        let schema = u16::from_le_bytes(read(part, &mut at, 2).try_into().unwrap());
        read(part, &mut at, 4);
        let mode = read(part, &mut at, 1)[0];
        if mode == 1 {
            cur_fields.remove(&(id, schema));
            continue;
        }
        let length = u16::from_le_bytes(read(part, &mut at, 2).try_into().unwrap()) as usize;
        let raw = read(part, &mut at, length);
        let payload = if mode == 0 {
            raw.to_vec()
        } else {
            assert_eq!(mode, 2);
            let base = base_fields
                .and_then(|m| m.get(&(id, schema)))
                .expect("mode 2 world field requires baseline payload");
            apply_word_delta(raw, base)
        };
        cur_fields.insert((id, schema), payload.clone());
        out.push((id, schema, length, payload));
    }
    assert!(at <= end);
    out
}

/// Extract own-correction payload from one `BNO1` datagram (reconstructing word deltas against `base`).
fn own_correction(datagram: &[u8], base: Option<&[u8]>) -> Option<Vec<u8>> {
    assert_eq!(&datagram[..4], b"BNO1");
    if datagram[20] & 1 == 0 {
        return None;
    }
    let section_len = u16::from_le_bytes([datagram[21], datagram[22]]) as usize;
    let section = &datagram[23..23 + section_len];
    assert_eq!(u16::from_le_bytes([section[8], section[9]]), 11);
    match section[10] {
        0 => {
            let length = u16::from_le_bytes([section[11], section[12]]) as usize;
            Some(section[13..13 + length].to_vec())
        }
        1 => {
            let base = base.expect("word-delta BNO1 correction requires baseline payload");
            Some(apply_word_delta(&section[10..], base))
        }
        other => panic!("unexpected BNO1 correction mode {other}"),
    }
}

/// Read store unit rows via the public ABI export (`KIND_UNIT`).
fn unit_rows(context: &Context) -> Result<Vec<(u32, u32, bfe_sim::gen::Unit)>, String> {
    use bfe_sim::gen::BlockHeader as B;
    let units = context
        .export(u32::from(bfe_sim::gen::KIND_UNIT), 1)
        .map_err(|e| e.message().to_string())?;
    let stride = units[B::STRIDE] as usize;
    let rows = units[B::ROWS] as usize;
    let heap_at = units[B::HEAP] as usize;
    let heap = &units[heap_at..heap_at + units[B::HEAP_WORDS] as usize];
    let mut out = Vec::with_capacity(rows);
    for i in 0..rows {
        let row = &units[B::WORDS + i * stride..B::WORDS + (i + 1) * stride];
        let mut unit = bfe_sim::gen::Unit::default();
        unit.take_full(row, heap);
        out.push((row[0] as u32, row[1] as u32, unit));
    }
    Ok(out)
}

/// Build one 2005 `BNI1` input datagram with optional `PILOT` rev 1 record and optional snapshot ack.
#[allow(clippy::too_many_arguments)]
fn space_input_packet(
    epoch: u32,
    generation: u32,
    seq: u32,
    tick: u32,
    held: &[usize],
    look: [f64; 2],
    edges: &[(u32, u8, Vec<u8>)],
    pilot: Option<PilotControls>,
    ack: Option<(u16, u32)>,
) -> Vec<u8> {
    let mut bytes = INPUT_MAGIC.to_le_bytes().to_vec();
    bytes.extend_from_slice(&epoch.to_le_bytes());
    bytes.extend_from_slice(&generation.to_le_bytes());
    let flags = u8::from(ack.is_some()) | (u8::from(pilot.is_some()) << 1);
    bytes.extend_from_slice(&[1, 1, edges.len() as u8, flags]);
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&seq.to_le_bytes());
    bytes.extend_from_slice(&tick.to_le_bytes());
    bytes.push(0);
    let mut mask = [0u8; 16];
    for &h in held {
        mask[h / 8] |= 1 << (h % 8);
    }
    bytes.extend_from_slice(&mask);
    bytes.extend_from_slice(&look[0].to_le_bytes());
    bytes.extend_from_slice(&look[1].to_le_bytes());
    for (id, kind, payload) in edges {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&tick.to_le_bytes());
        bytes.push(*kind);
        bytes.extend_from_slice(payload);
    }
    if let Some((ack_epoch, ack_tick)) = ack {
        bytes.extend_from_slice(&ack_epoch.to_le_bytes());
        bytes.extend_from_slice(&ack_tick.to_le_bytes());
    }
    if let Some(controls) = pilot {
        let mut pbuf = Vec::new();
        write_pilot_entries(&[PilotEntry { sequence: seq, controls }], &mut pbuf).unwrap();
        write_section(
            &[Section {
                kind: PILOT,
                revision: REVISION,
                body: &pbuf,
            }],
            &mut bytes,
        )
        .unwrap();
    }
    bytes
}

fn swbf2_prepared_root() -> Option<PathBuf> {
    std::env::var_os("BFE_SWBF2_PREPARED")
        .or_else(|| std::env::var_os("BFE_SWBF2_CONTENT"))
        .map(PathBuf::from)
}

struct Probe {
    now: f64,
    seq: u32,
    unit_slot: u32,
    generation: u32,
    checks: Checks,
    last_ack_tick: Option<u32>,
    saw_fighter_chunks: usize,
    saw_seat_fighter: bool,
    saw_seat_chair: bool,
    saw_frigates: usize,
    saw_ordnance_launches: usize,
    saw_ordnance_impacts: usize,
    saw_ordnance_guided: usize,
    saw_turrets: usize,
    saw_doors: usize,
    saw_own_pilot: usize,
    saw_own_chair: usize,
    last_pilot_body: Option<OwnPilotState>,
    last_chair_body: Option<OwnChairState>,
    last_soldier_pos: [f64; 3],
    last_soldier_yaw: f64,
    look_total: [f64; 2],
    turret_initial_angles: Option<Vec<[i16; 2]>>,
    turret_moved: bool,
    bns_bytes_per_pub: Vec<usize>,
    bno_bytes_per_pub: Vec<usize>,
    schema_bytes: std::collections::BTreeMap<u16, usize>,
    world_history: std::collections::BTreeMap<u32, std::collections::BTreeMap<(u32, u16), Vec<u8>>>,
    own_history: std::collections::BTreeMap<u32, Vec<u8>>,
    checks_validated: usize,
}

impl Probe {
    fn step_with(
        &mut self,
        host: &mut NativeWorldHost,
        held: &[usize],
        look_delta_x: f64,
        pilot: Option<PilotControls>,
    ) {
        let tick_ms = 1000.0 / 60.0;
        self.look_total[0] += look_delta_x;
        let tick = host.tick + 1;
        let ack = self.last_ack_tick.map(|t| (host.epoch as u16, t));
        let bytes = space_input_packet(
            host.epoch,
            self.generation,
            self.seq,
            tick,
            held,
            self.look_total,
            &[],
            pilot,
            ack,
        );
        self.seq += 1;
        host.handle_input("pilot", &bytes, self.now);
        if let Some((ae, at)) = ack {
            let mut ack_pkt = bfe_server::udp_peer::ACK_MAGIC.to_le_bytes().to_vec();
            ack_pkt.extend_from_slice(&ae.to_le_bytes());
            ack_pkt.extend_from_slice(&at.to_le_bytes());
            host.handle_input("watcher", &ack_pkt, self.now);
        }
        self.now += tick_ms;
        let Some(pubn) = host
            .step(self.now, (1_700_000_000_000.0 + self.now) as _)
            .unwrap()
        else {
            return;
        };

        let mut tick_bns = 0usize;
        for group in pubn
            .shared
            .iter()
            .filter(|g| g.clients.iter().any(|c| c == "watcher"))
        {
            if let Some(first) = group.parts.first() {
                let snap_tick = u32::from_le_bytes(first[6..10].try_into().unwrap());
                let base_tick = u32::from_le_bytes(first[11..15].try_into().unwrap());
                let base_fields = (base_tick != 0)
                    .then(|| self.world_history.get(&base_tick))
                    .flatten()
                    .cloned();
                let mut cur_fields = base_fields.clone().unwrap_or_default();
                for part in &group.parts {
                    tick_bns += part.len();
                    for (id, schema, wire_len, payload) in
                        snapshot_world_fields(part, base_fields.as_ref(), &mut cur_fields)
                    {
                        if payload.is_empty() {
                            continue;
                        }
                        if self.bns_bytes_per_pub.len() >= 5 {
                            *self.schema_bytes.entry(schema).or_insert(0) += wire_len;
                        }
                        self.checks.field(schema, id, &payload).unwrap_or_else(|e| {
                            panic!("Checks::field rejected schema=0x{schema:04x} id={id}: {e}")
                        });
                        self.checks_validated += 1;
                        let Ok((rev, body)) = split_revision(&payload) else {
                            continue;
                        };
                        if rev != REVISION {
                            continue;
                        }
                        match schema {
                            FIGHTER_SCHEMA => match read_fighter_field(id, body).unwrap() {
                                FighterField::Chunk(_) => self.saw_fighter_chunks += 1,
                                FighterField::Seats(seats) => {
                                    for s in seats {
                                        if u32::from(s.unit_slot) == self.unit_slot {
                                            if s.is_chair {
                                                self.saw_seat_chair = true;
                                            } else {
                                                self.saw_seat_fighter = true;
                                            }
                                        }
                                    }
                                }
                            },
                            FRIGATES_SCHEMA => {
                                let clocks = read_frigates(id, body).unwrap();
                                if !clocks.is_empty() {
                                    self.saw_frigates += 1;
                                }
                            }
                            ORDNANCE_SCHEMA => match read_ordnance_field(id, body).unwrap() {
                                OrdnanceField::Guided(g) => self.saw_ordnance_guided += g.len(),
                                OrdnanceField::Events(ev) => {
                                    self.saw_ordnance_launches += ev.launches.len();
                                    self.saw_ordnance_impacts += ev.impacts.len();
                                }
                            },
                            PROPS_SCHEMA => match read_props_field(id, body).unwrap() {
                                PropsField::Turrets(t) => {
                                    self.saw_turrets += 1;
                                    if let Some(init) = &self.turret_initial_angles {
                                        if t.len() == init.len()
                                            && t.iter().zip(init.iter()).any(|(a, b)| {
                                                (i32::from(a[0]) - i32::from(b[0])).abs() > 120
                                                    || (i32::from(a[1]) - i32::from(b[1])).abs()
                                                        > 120
                                            })
                                        {
                                            self.turret_moved = true;
                                        }
                                    } else if !t.is_empty() {
                                        self.turret_initial_angles = Some(t);
                                    }
                                }
                                PropsField::Doors(d) => {
                                    if !d.is_empty() {
                                        self.saw_doors += 1;
                                    }
                                }
                            },
                            _ => {}
                        }
                    }
                }
                self.world_history.insert(snap_tick, cur_fields);
                while self.world_history.len() > 64 {
                    self.world_history.pop_first();
                }
                self.last_ack_tick = Some(
                    self.last_ack_tick
                        .map_or(snap_tick, |prev| prev.max(snap_tick)),
                );
            }
        }
        if tick_bns > 0 {
            self.bns_bytes_per_pub.push(tick_bns);
        }

        let mut tick_bno = 0usize;
        for own in pubn.own_packets.iter().filter(|p| p.peer_id == "pilot") {
            tick_bno += own.bytes.len();
            let own_tick = u32::from_le_bytes(own.bytes[6..10].try_into().unwrap());
            let own_base = u32::from_le_bytes(own.bytes[11..15].try_into().unwrap());
            let base_payload = (own_base != 0)
                .then(|| self.own_history.get(&own_base).map(Vec::as_slice))
                .flatten();
            let Some(payload) = own_correction(&own.bytes, base_payload) else {
                continue;
            };
            self.own_history.insert(own_tick, payload.clone());
            while self.own_history.len() > 64 {
                self.own_history.pop_first();
            }
            let head = decode_own_player_header(&payload).unwrap();
            self.checks
                .correction(
                    f64::from(host.epoch),
                    f64::from(host.tick),
                    self.unit_slot,
                    self.generation,
                    11,
                    &payload,
                )
                .unwrap_or_else(|e| {
                    panic!(
                        "Checks::correction rejected schema=0x{:02x}: {e}",
                        head.body_schema
                    )
                });
            self.checks_validated += 1;
            if head.body_schema == swbf2_body::SCHEMA {
                if let Some(raw) = head.body.get(&payload) {
                    if let Ok(rest) = swbf2_body::revision_body(raw) {
                        if let Some(split) = swbf2_body::compact_len(rest) {
                            if let Ok(compact) =
                                decode_compact_soldier_fixed(Some(&rest[..split]), &[true; 6])
                            {
                                self.last_soldier_pos = compact.vectors[0];
                                self.last_soldier_yaw = compact.numbers[1];
                            }
                        }
                    }
                }
            } else if head.body_schema == OWN_BODY_SCHEMA {
                let raw = head.body.get(&payload).unwrap();
                let (rev, body) = split_revision(raw).unwrap();
                assert_eq!(rev, REVISION);
                match read_own_body(body).unwrap() {
                    OwnSpaceBody::Pilot(p) => {
                        self.saw_own_pilot += 1;
                        self.last_pilot_body = Some(p);
                    }
                    OwnSpaceBody::Chair(c) => {
                        self.saw_own_chair += 1;
                        self.last_chair_body = Some(c);
                    }
                }
            }
        }
        if tick_bno > 0 {
            self.bno_bytes_per_pub.push(tick_bno);
        }
    }
}

#[test]
#[ignore = "requires prepared SWBF2 spa6 content (BFE_SWBF2_PREPARED)"]
fn space_assault_scripted_client_boards_flies_fires_lands_and_operates_chair_gun() {
    let root = swbf2_prepared_root().expect("set BFE_SWBF2_PREPARED to a prepared SWBF2 root");
    let query = "?map=spa6&era=cw&mode=ass&side=rep&seed=41";
    let mut host = NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).unwrap();
    assert!(host.has_space(), "spa6 must attach SpaceBattlefield");

    let space_plan = plan(&Table::of(WRITES_SPACE), &Supports::of(SUPPORTS)).unwrap();
    let welcome = host.add_player("pilot", "Pilot", false, false, false).unwrap();
    let _watcher = host.add_player("watcher", "Watcher", false, false, false).unwrap();
    host.set_plan("pilot", Some(space_plan.clone()));
    host.set_plan("watcher", Some(space_plan.clone()));

    let mut now = 0.0;
    let tick_ms = 1000.0 / 60.0;
    host.handle_start("pilot", now);
    host.handle_start("watcher", now);

    let classes_vec = {
        let rows = host.registry_descriptor["classes"].as_array().unwrap();
        let admitted = host.swbf2_classes();
        let mut out = vec![None; rows.len() + 1];
        for row in rows {
            let id = row[0].as_u64().unwrap() as usize;
            let name = row[1].as_str().unwrap();
            if let Some((_, t)) = admitted.iter().find(|(c, _)| c == name) {
                out[id] = Some(*t);
            }
        }
        out
    };
    let checks = Checks::new(space_plan, classes_vec);

    // Deploy pilot on the hangar deck.
    let class_id = host.registry_descriptor["classes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r[1].as_str() == Some(welcome.class_name.as_str()))
        .map(|r| r[0].as_u64().unwrap() as u16)
        .unwrap_or(1);
    let spawn_payload = [class_id.to_le_bytes(), 0u16.to_le_bytes()].concat();
    let pkt = space_input_packet(
        host.epoch,
        welcome.generation,
        1,
        host.tick + 1,
        &[],
        [0.0, 0.0],
        &[(1, 3, spawn_payload)],
        None,
        None,
    );
    host.handle_input("pilot", &pkt, now);

    // Wait for mission spawn wave to place pilot alive on the hangar deck.
    let mut generation = None;
    let mut spawn_pos = [0.0; 3];
    let unit_slot = welcome.unit_id;
    for _ in 0..300 {
        now += tick_ms;
        host.step(now, (1_700_000_000_000.0 + now) as _).unwrap();
        if let Some((_, _, unit)) = unit_rows(&host.context)
            .unwrap()
            .into_iter()
            .find(|(id, _, u)| *id == unit_slot && u.alive)
        {
            generation = Some(unit.generation as u32);
            let p = unit.position();
            spawn_pos = [p.x, p.y, p.z];
            break;
        }
    }
    let generation = generation.expect("pilot deploys on hangar deck");

    let mut probe = Probe {
        now,
        seq: 2,
        unit_slot,
        generation,
        checks,
        last_ack_tick: None,
        saw_fighter_chunks: 0,
        saw_seat_fighter: false,
        saw_seat_chair: false,
        saw_frigates: 0,
        saw_ordnance_launches: 0,
        saw_ordnance_impacts: 0,
        saw_ordnance_guided: 0,
        saw_turrets: 0,
        saw_doors: 0,
        saw_own_pilot: 0,
        saw_own_chair: 0,
        last_pilot_body: None,
        last_chair_body: None,
        last_soldier_pos: spawn_pos,
        last_soldier_yaw: 0.0,
        look_total: [0.0; 2],
        turret_initial_angles: None,
        turret_moved: false,
        bns_bytes_per_pub: Vec::new(),
        bno_bytes_per_pub: Vec::new(),
        schema_bytes: std::collections::BTreeMap::new(),
        world_history: std::collections::BTreeMap::new(),
        own_history: std::collections::BTreeMap::new(),
        checks_validated: 0,
    };

    let forward_code = SWBF2_INPUT_CODES.iter().position(|c| *c == "KeyW").unwrap();
    let sprint_code = SWBF2_INPUT_CODES.iter().position(|c| *c == "ShiftLeft").unwrap();

    let (fighter_id, fighter_pos, landing_zone_center, chair_info) = host
        .context
        .owned(|w| {
            let space = w.space().unwrap();
            let mut fighters: Vec<_> = space
                .fighters
                .iter()
                .filter(|f| {
                    f.alive
                        && f.team == i32::from(welcome.team)
                        && f.state == SpaceFlightState::Landed
                })
                .collect();
            fighters.sort_by(|a, b| {
                let da = (a.position[0] - spawn_pos[0]).powi(2)
                    + (a.position[2] - spawn_pos[2]).powi(2);
                let db = (b.position[0] - spawn_pos[0]).powi(2)
                    + (b.position[2] - spawn_pos[2]).powi(2);
                da.total_cmp(&db)
            });
            let f = fighters.first().expect("friendly landed fighter on deck");
            let lz = space
                .bounds
                .landing_zones
                .iter()
                .find(|z| z.contains(f.position))
                .or_else(|| space.bounds.landing_zones.first())
                .map(|z| z.center)
                .unwrap_or(f.position);
            let chair = space
                .buildings
                .iter()
                .filter(|b| b.terminal.is_some())
                .filter_map(|b| {
                    let snap = w.space_building_snapshot(b.building_index)?;
                    (snap.alive && (snap.team == 0 || snap.team == i32::from(welcome.team)))
                        .then_some((b.building_index, b.center))
                })
                .min_by(|a, b| {
                    let da = (a.1[0] - spawn_pos[0]).powi(2) + (a.1[2] - spawn_pos[2]).powi(2);
                    let db = (b.1[0] - spawn_pos[0]).powi(2) + (b.1[2] - spawn_pos[2]).powi(2);
                    da.total_cmp(&db)
                });
            (f.id, f.position, lz, chair)
        })
        .unwrap();
    let (_chair_bidx, chair_pos) =
        chair_info.expect("spa6 must have a friendly turret chair terminal");
    println!(
        "spa6 spawn={spawn_pos:?} nearest_fighter(id={fighter_id})={fighter_pos:?} chair={chair_pos:?}"
    );

    // 1. Walk/sprint across the hangar deck toward the nearest parked fighter and board via PILOT `use_pressed`.
    for step in 0..360u32 {
        let dx = fighter_pos[0] - probe.last_soldier_pos[0];
        let dz = fighter_pos[2] - probe.last_soldier_pos[2];
        let dist = (dx * dx + dz * dz).sqrt();
        let target_yaw = dx.atan2(dz);
        let mut diff = target_yaw - probe.last_soldier_yaw;
        while diff > std::f64::consts::PI {
            diff -= std::f64::consts::TAU;
        }
        while diff < -std::f64::consts::PI {
            diff += std::f64::consts::TAU;
        }
        let look_x = (-diff * 120.0).clamp(-80.0, 80.0);
        let try_board = dist < 12.0 && step % 6 == 0;
        let held: &[usize] = if dist > 5.0 {
            &[forward_code, sprint_code]
        } else {
            &[]
        };
        probe.step_with(
            &mut host,
            held,
            look_x,
            Some(PilotControls {
                use_pressed: try_board,
                ..Default::default()
            }),
        );
        if probe.last_pilot_body.is_some() {
            break;
        }
    }
    let boarded = *probe
        .last_pilot_body
        .as_ref()
        .unwrap_or_else(|| {
            panic!(
                "pilot must reach fighter={fighter_pos:?} (ended at {:?}) and receive 0x91 OwnSpaceBody::Pilot",
                probe.last_soldier_pos
            )
        });
    assert_eq!(boarded.state, SpaceFlightState::Landed as u8);
    assert!(probe.saw_seat_fighter, "watcher must see pilot in 0x0211 seat occupancy");
    let start_pos = boarded.position;

    // 2. Take off via PILOT rev 1 `takeoff_or_land`.
    probe.step_with(
        &mut host,
        &[],
        0.0,
        Some(PilotControls {
            flight: SpacePilotInput {
                takeoff_or_land: true,
                throttle: 1.0,
                ..Default::default()
            },
            ..Default::default()
        }),
    );
    // The craft lifts off the pad by its take-off clip before it flies off.
    for _ in 0..240 {
        probe.step_with(
            &mut host,
            &[],
            0.0,
            Some(PilotControls {
                flight: SpacePilotInput {
                    throttle: 1.0,
                    ..Default::default()
                },
                ..Default::default()
            }),
        );
    }
    let airborne = *probe.last_pilot_body.as_ref().unwrap();
    assert_eq!(
        airborne.state,
        SpaceFlightState::Flying as u8,
        "fighter must reach Flying state after takeoff"
    );
    let dist_from_pad = ((airborne.position[0] - start_pos[0]).powi(2)
        + (airborne.position[1] - start_pos[1]).powi(2)
        + (airborne.position[2] - start_pos[2]).powi(2))
    .sqrt();
    assert!(
        dist_from_pad > 30.0,
        "fighter must move out from hangar pad (moved {dist_from_pad:.1} m)"
    );

    // 3. Fire primary & secondary weapons while flying.
    let launches_before_fire = probe.saw_ordnance_launches;
    for _ in 0..45 {
        probe.step_with(
            &mut host,
            &[],
            0.0,
            Some(PilotControls {
                flight: SpacePilotInput {
                    throttle: 0.5,
                    fire_primary: true,
                    fire_secondary: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
        );
    }
    assert!(
        probe.saw_ordnance_launches > launches_before_fire,
        "firing fighter weapons must emit 0x0214 launch events"
    );

    // 4. Return to friendly landing zone and land via `takeoff_or_land`.
    let active_fighter_id = airborne.fighter_id as usize;
    host.context
        .owned(|w| {
            let space = w.space_mut().unwrap();
            if let Some(f) = space.fighters.get_mut(active_fighter_id) {
                f.position = [
                    landing_zone_center[0],
                    landing_zone_center[1] + 5.0,
                    landing_zone_center[2],
                ];
                f.velocity = [0.0, 0.0, 0.0];
                f.airspeed = 5.0;
            }
        })
        .unwrap();
    for _ in 0..6 {
        probe.step_with(&mut host, &[], 0.0, Some(PilotControls::default()));
    }
    assert!(
        probe.last_pilot_body.as_ref().unwrap().can_land,
        "0x91 must advertise can_land=true inside friendly hangar landing zone"
    );
    probe.step_with(
        &mut host,
        &[],
        0.0,
        Some(PilotControls {
            flight: SpacePilotInput {
                takeoff_or_land: true,
                throttle: -1.0,
                ..Default::default()
            },
            ..Default::default()
        }),
    );
    // The craft lowers its gear over its class's landing time, then settles on the deck.
    let mut landing_ticks = 0;
    for _ in 0..600 {
        probe.step_with(&mut host, &[], 0.0, Some(PilotControls::default()));
        landing_ticks += 1;
        if probe.last_pilot_body.as_ref().unwrap().state == SpaceFlightState::Landed as u8 {
            break;
        }
    }
    println!("landed after {landing_ticks} ticks");
    assert_eq!(
        probe.last_pilot_body.as_ref().unwrap().state,
        SpaceFlightState::Landed as u8,
        "fighter must land back on hangar deck via takeoff_or_land"
    );

    // 5. Leave fighter via `use_pressed`, then walk to a friendly chair gun terminal and board via `use_pressed`.
    probe.step_with(
        &mut host,
        &[],
        0.0,
        Some(PilotControls {
            use_pressed: true,
            ..Default::default()
        }),
    );
    for _ in 0..10 {
        probe.step_with(&mut host, &[], 0.0, Some(PilotControls::default()));
    }

    // Walk up the hangar right staircase -> through the proximity door ->
    // into the round turret room -> to an unoccupied turret chair.
    let waypoints: &[[f64; 2]] = &[
        [-434.0, -1238.0],
        [-432.0, -1250.5],
        [-437.9, -1257.7],
        [-435.5, -1268.0],
        [-438.5, -1273.5],
    ];
    let mut wp_idx = 0usize;
    let mut door_opened = false;
    for step in 0..900u32 {
        let target_xz = if wp_idx < waypoints.len() {
            let wp = waypoints[wp_idx];
            let d = ((wp[0] - probe.last_soldier_pos[0]).powi(2)
                + (wp[1] - probe.last_soldier_pos[2]).powi(2))
            .sqrt();
            if d < 2.2 {
                wp_idx += 1;
            }
            wp
        } else {
            // Inside the round room: pick the nearest currently unoccupied chair terminal.
            host.context
                .owned(|w| {
                    let s = w.space().unwrap();
                    s.buildings
                        .iter()
                        .filter(|b| {
                            b.terminal
                                .as_ref()
                                .is_some_and(|t| t.occupant.is_none() && t.remote_turret.is_some())
                        })
                        .filter(|b| b.center[2] < -1150.0)
                        .min_by(|a, b| {
                            let da = (a.center[0] - probe.last_soldier_pos[0]).powi(2)
                                + (a.center[2] - probe.last_soldier_pos[2]).powi(2);
                            let db = (b.center[0] - probe.last_soldier_pos[0]).powi(2)
                                + (b.center[2] - probe.last_soldier_pos[2]).powi(2);
                            da.total_cmp(&db)
                        })
                        .map(|b| [b.center[0], b.center[2]])
                })
                .unwrap()
                .unwrap_or([chair_pos[0], chair_pos[2]])
        };
        let dx = target_xz[0] - probe.last_soldier_pos[0];
        let dz = target_xz[1] - probe.last_soldier_pos[2];
        let dist = (dx * dx + dz * dz).sqrt();
        let target_yaw = dx.atan2(dz);
        let mut diff = target_yaw - probe.last_soldier_yaw;
        while diff > std::f64::consts::PI {
            diff -= std::f64::consts::TAU;
        }
        while diff < -std::f64::consts::PI {
            diff += std::f64::consts::TAU;
        }
        let look_x = (-diff * 120.0).clamp(-80.0, 80.0);
        let try_board = wp_idx >= waypoints.len() && dist < 8.0 && step % 6 == 0;
        let held: &[usize] = if dist > 1.8 {
            &[forward_code, sprint_code]
        } else {
            &[]
        };
        probe.step_with(
            &mut host,
            held,
            look_x,
            Some(PilotControls {
                use_pressed: try_board,
                ..Default::default()
            }),
        );
        if probe.last_soldier_pos[2] < -1260.0 {
            door_opened = true;
        }
        if probe.last_chair_body.is_some() {
            break;
        }
    }
    assert!(door_opened, "pilot must pass through the hangar proximity door into the turret room");
    let chair_before = *probe
        .last_chair_body
        .as_ref()
        .unwrap_or_else(|| {
            panic!(
                "pilot must reach chair={chair_pos:?} (wp={wp_idx}, ended at {:?}) and receive 0x91 OwnSpaceBody::Chair",
                probe.last_soldier_pos
            )
        });
    assert!(probe.saw_seat_chair, "watcher must see chair occupancy in 0x0211");

    // 6. Aim and fire exterior turret from the chair gun!
    let launches_before_chair = probe.saw_ordnance_launches;
    for _ in 0..45 {
        probe.step_with(
            &mut host,
            &[],
            0.0,
            Some(PilotControls {
                turret_look: [1.5, -0.4],
                flight: SpacePilotInput {
                    fire_primary: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
        );
    }
    let chair_after = probe.last_chair_body.as_ref().unwrap();
    assert!(
        (chair_after.turret_yaw - chair_before.turret_yaw).abs() > 0.05
            || (chair_after.turret_pitch - chair_before.turret_pitch).abs() > 0.05,
        "chair turret aim must move: before=({:.3},{:.3}) after=({:.3},{:.3})",
        chair_before.turret_yaw,
        chair_before.turret_pitch,
        chair_after.turret_yaw,
        chair_after.turret_pitch
    );
    assert!(
        chair_after.shots_fired > chair_before.shots_fired,
        "chair turret must fire shots: before={} after={}",
        chair_before.shots_fired,
        chair_after.shots_fired
    );
    assert!(
        probe.saw_ordnance_launches > launches_before_chair,
        "chair turret firing must emit 0x0214 launch events"
    );
    assert!(probe.turret_moved, "0x0215 turret aims must reflect live turret rotation");
    assert!(
        probe.saw_fighter_chunks > 0
            && probe.saw_frigates > 0
            && probe.saw_turrets > 0
            && probe.saw_doors > 0
    );
    assert!(probe.saw_own_pilot > 0 && probe.saw_own_chair > 0);

    let steady_bns = &probe.bns_bytes_per_pub[probe.bns_bytes_per_pub.len().min(5)..];
    let steady_bno = &probe.bno_bytes_per_pub[probe.bno_bytes_per_pub.len().min(5)..];
    let mean_bns = steady_bns.iter().sum::<usize>() as f64 / steady_bns.len().max(1) as f64;
    let peak_bns = steady_bns.iter().copied().max().unwrap_or(0);
    let mean_bno = steady_bno.iter().sum::<usize>() as f64 / steady_bno.len().max(1) as f64;
    let peak_bno = steady_bno.iter().copied().max().unwrap_or(0);
    let total_bps = (mean_bns + mean_bno) * 20.0;
    let steady_pubs = steady_bns.len().max(1) as f64;
    let schema_breakdown: String = probe
        .schema_bytes
        .iter()
        .map(|(s, b)| format!("0x{s:04x}={:.1}B", *b as f64 / steady_pubs))
        .collect::<Vec<_>>()
        .join(" ");
    println!(
        "spa6 online scripted test: validated={} fighter_chunks={} \
         frigates={} launches={} impacts={} guided={} turrets={} doors={} \
         own_pilot={} own_chair={} | \
         BNS1 mean={mean_bns:.1} B peak={peak_bns} B | BNO1 mean={mean_bno:.1} B peak={peak_bno} B | \
         steady total={total_bps:.0} B/s (budget 100000 B/s) | per-pub schemas: {schema_breakdown}",
        probe.checks_validated,
        probe.saw_fighter_chunks,
        probe.saw_frigates,
        probe.saw_ordnance_launches,
        probe.saw_ordnance_impacts,
        probe.saw_ordnance_guided,
        probe.saw_turrets,
        probe.saw_doors,
        probe.saw_own_pilot,
        probe.saw_own_chair,
    );
    assert!(
        total_bps < 100_000.0,
        "steady bandwidth {total_bps:.0} B/s must stay well within 100,000 B/s"
    );
}

#[test]
#[ignore = "requires prepared SWBF2 spa6 content (BFE_SWBF2_PREPARED)"]
fn space_assault_idle_local_vs_server_digest_comparison() {
    let root = swbf2_prepared_root().expect("set BFE_SWBF2_PREPARED to a prepared SWBF2 root");
    let seed = 41u32;
    let ticks = 600u32;

    // 1. Local Space Assault battle with 1 idle local human player.
    let files = DirFiles::from_resolved_root(&root, GameFamily::Swbf2);
    let options = Options {
        family: GameFamily::Swbf2,
        mode: "ass".into(),
        map: "spa6".into(),
        side: "rep".into(),
        bots: Bots::default(),
        enhanced: false,
        heroes: false,
        seed,
        unit: None,
        practice_target: None,
        name: None,
    };
    let selection = MissionSelection {
        family: GameFamily::Swbf2,
        map: "spa6".into(),
        era: "cw".into(),
        mode: "ass".into(),
        seed,
    };
    let local_ctx = Context::new();
    let (admitted, _) = local_ctx
        .world_mission_content(&files, &options, &selection, MissionPreferences::default())
        .unwrap();
    local_ctx
        .owned(|w| {
            let _ = w.bind_mission_deployment(admitted.handles.player);
            w.connect_space_pilot(admitted.handles.player, admitted.handles.mailbox);
        })
        .unwrap();
    let mut rows = bfe_content_native::setup::clock::TickRows::new();
    let mut local_digests = Vec::with_capacity(ticks as usize);
    for t in 1..=ticks {
        let now_ms = f64::from(t) * 1000.0 / 60.0;
        let units = local_ctx.world_prepare_units().unwrap();
        let input = rows.input(units, options.bots.fielded, true, 0, false, 0, now_ms, 1.0);
        let out = local_ctx.world_tick(input).unwrap();
        assert!(out.accepted, "local tick {t} refused");
        rows.clock = out.clock;
        let d = local_ctx.owned(|w| w.space().unwrap().state_digest(Some(w))).unwrap();
        local_digests.push(d);
    }
    let (local_digest, local_summary) = local_ctx
        .owned(|w| {
            let s = w.space().unwrap();
            let alive_fighters = s.fighters.iter().filter(|f| f.alive).count();
            let airborne = s
                .fighters
                .iter()
                .filter(|f| f.alive && f.state != SpaceFlightState::Landed)
                .count();
            let active_ord = s.ordnance.active_count();
            (
                s.state_digest(Some(w)),
                (s.tick, alive_fighters, airborne, active_ord),
            )
        })
        .unwrap();

    // 2. Server Space Assault battle (`NativeWorldHost`) with 1 idle human player joined.
    let query = format!("?map=spa6&era=cw&mode=ass&side=rep&seed={seed}");
    let mut host = NativeWorldHost::load_for(&root, &query, 1, GameFamily::Swbf2, OPEN).unwrap();
    let _ = host.add_player("idle", "Idle", false, false, false).unwrap();
    host.handle_start("idle", 0.0);
    let mut server_digests = Vec::with_capacity(ticks as usize);
    for t in 1..=ticks {
        let now_ms = f64::from(t) * 1000.0 / 60.0;
        host.step(now_ms, (1_700_000_000_000.0 + now_ms) as _).unwrap();
        let d = host.context.owned(|w| w.space().unwrap().state_digest(Some(w))).unwrap();
        server_digests.push(d);
    }
    let (server_digest, server_summary) = host
        .context
        .owned(|w| {
            let s = w.space().unwrap();
            let alive_fighters = s.fighters.iter().filter(|f| f.alive).count();
            let airborne = s
                .fighters
                .iter()
                .filter(|f| f.alive && f.state != SpaceFlightState::Landed)
                .count();
            let active_ord = s.ordnance.active_count();
            (
                s.state_digest(Some(w)),
                (s.tick, alive_fighters, airborne, active_ord),
            )
        })
        .unwrap();

    // 3. Second server battle with identical setup, but with Space wire observations disabled,
    // proving the Space wire capture/observation layer perturbs 0 simulation bits over all 600 ticks.
    let mut host_no_obs =
        NativeWorldHost::load_for(&root, &query, 1, GameFamily::Swbf2, OPEN).unwrap();
    host_no_obs
        .context
        .owned(|w| {
            if let Some(s) = w.space_mut() {
                s.ordnance.observations = None;
            }
        })
        .unwrap();
    let _ = host_no_obs.add_player("idle", "Idle", false, false, false).unwrap();
    host_no_obs.handle_start("idle", 0.0);
    for t in 1..=ticks {
        let now_ms = f64::from(t) * 1000.0 / 60.0;
        host_no_obs
            .step(now_ms, (1_700_000_000_000.0 + now_ms) as _)
            .unwrap();
    }
    let no_obs_digest = host_no_obs
        .context
        .owned(|w| w.space().unwrap().state_digest(Some(w)))
        .unwrap();

    let first_diff = local_digests
        .iter()
        .zip(server_digests.iter())
        .position(|(a, b)| a != b)
        .map(|i| i + 1);
    println!(
        "spa6 idle 600-tick digest comparison:\n  \
         first_diff_tick={first_diff:?}\n  \
         tick_1=(0x{:016x}, 0x{:016x}) tick_60=(0x{:016x}, 0x{:016x})\n  \
         local_600=0x{local_digest:016x} {local_summary:?}\n  \
         server_600=0x{server_digest:016x} {server_summary:?}\n  \
         server_no_obs_600=0x{no_obs_digest:016x} (obs_zero_perturbation={})",
        local_digests[0],
        server_digests[0],
        local_digests[59],
        server_digests[59],
        server_digest == no_obs_digest
    );
    assert_eq!(
        local_summary.0, server_summary.0,
        "space ticks must advance identically"
    );
    assert_eq!(
        local_digests[59], server_digests[59],
        "first 60 ticks before the initial bot spawn wave must match bit-for-bit"
    );
    assert_eq!(
        server_digest, no_obs_digest,
        "Space wire observations and publication must not perturb SpaceBattlefield simulation digest"
    );
}

fn percentile_usize(sorted: &[usize], p: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).ceil() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn build_classes_vec(
    host: &NativeWorldHost,
) -> Vec<Option<bfe_sim::movement::soldier::swbf2::Tuning>> {
    let rows = host.registry_descriptor["classes"].as_array().unwrap();
    let admitted = host.swbf2_classes();
    let mut out = vec![None; rows.len() + 1];
    for row in rows {
        let id = row[0].as_u64().unwrap() as usize;
        let name = row[1].as_str().unwrap();
        if let Some((_, t)) = admitted.iter().find(|(c, _)| c == name) {
            out[id] = Some(*t);
        }
    }
    out
}

/// Real ten-minute (36,000-tick / 12,000-publication) Space Assault bandwidth and
/// 10,000-byte burst-ceiling measurement on prepared `spa6` against the 100,000 B/s
/// default state budget, plus explicit verification of what a client observes when a
/// snapshot crosses the 10,000-byte burst ceiling.
#[test]
#[ignore = "requires prepared SWBF2 spa6 content (BFE_SWBF2_PREPARED)"]
fn space_assault_ten_minute_bandwidth_and_burst_ceiling() {
    const TOTAL_TICKS: u32 = 36_000; // 10 minutes at 60 Hz
    const STATE_BUDGET_BPS: f64 = 100_000.0;
    const BURST_CEILING_BYTES: usize = 10_000; // host.rs:134 budget * 0.1

    let root = swbf2_prepared_root().expect("set BFE_SWBF2_PREPARED to a prepared SWBF2 root");
    let mut seed = 41u32;
    let mut epoch = 1u32;
    let forward_code = SWBF2_INPUT_CODES
        .iter()
        .position(|c| *c == "KeyW")
        .unwrap();
    let sprint_code = SWBF2_INPUT_CODES
        .iter()
        .position(|c| *c == "ShiftLeft")
        .unwrap();

    let open_match = |seed: u32, epoch: u32| -> (NativeWorldHost, Probe, u32, u16, u32, bool) {
        let query = format!("?map=spa6&era=cw&mode=ass&side=rep&seed={seed}");
        let mut host =
            NativeWorldHost::load_for(&root, &query, epoch, GameFamily::Swbf2, OPEN).unwrap();
        host.set_state_budget(STATE_BUDGET_BPS);
        let space_plan = plan(&Table::of(WRITES_SPACE), &Supports::of(SUPPORTS)).unwrap();
        let pilot = host
            .add_player("pilot", "Pilot", false, false, false)
            .unwrap();
        let _watcher = host
            .add_player("watcher", "Watcher", false, false, false)
            .unwrap();
        host.set_plan("pilot", Some(space_plan.clone()));
        host.set_plan("watcher", Some(space_plan.clone()));
        host.handle_start("pilot", 0.0);
        host.handle_start("watcher", 0.0);

        let checks = Checks::new(space_plan, build_classes_vec(&host));
        let class_id = host.registry_descriptor["classes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r[1].as_str() == Some(pilot.class_name.as_str()))
            .map(|r| r[0].as_u64().unwrap() as u16)
            .unwrap_or(1);
        let spawn_payload = [class_id.to_le_bytes(), 0u16.to_le_bytes()].concat();
        let pkt = space_input_packet(
            host.epoch,
            pilot.generation,
            1,
            host.tick + 1,
            &[],
            [0.0, 0.0],
            &[(1, 3, spawn_payload)],
            None,
            None,
        );
        host.handle_input("pilot", &pkt, 0.0);

        let mut now = 0.0;
        let tick_ms = 1000.0 / 60.0;
        let mut generation = pilot.generation;
        let mut spawn_pos = [0.0; 3];
        for _ in 0..300 {
            now += tick_ms;
            host.step(now, (1_700_000_000_000.0 + now) as _).unwrap();
            if let Some((_, _, unit)) = unit_rows(&host.context)
                .unwrap()
                .into_iter()
                .find(|(id, _, u)| *id == pilot.unit_id && u.alive)
            {
                generation = unit.generation as u32;
                let p = unit.position();
                spawn_pos = [p.x, p.y, p.z];
                break;
            }
        }
        let mut probe = Probe {
            now,
            seq: 2,
            unit_slot: pilot.unit_id,
            generation,
            checks,
            last_ack_tick: None,
            saw_fighter_chunks: 0,
            saw_seat_fighter: false,
            saw_seat_chair: false,
            saw_frigates: 0,
            saw_ordnance_launches: 0,
            saw_ordnance_impacts: 0,
            saw_ordnance_guided: 0,
            saw_turrets: 0,
            saw_doors: 0,
            saw_own_pilot: 0,
            saw_own_chair: 0,
            last_pilot_body: None,
            last_chair_body: None,
            last_soldier_pos: spawn_pos,
            last_soldier_yaw: 0.0,
            look_total: [0.0; 2],
            turret_initial_angles: None,
            turret_moved: false,
            bns_bytes_per_pub: Vec::new(),
            bno_bytes_per_pub: Vec::new(),
            schema_bytes: std::collections::BTreeMap::new(),
            world_history: std::collections::BTreeMap::new(),
            own_history: std::collections::BTreeMap::new(),
            checks_validated: 0,
        };

        // Walk to nearest parked Republic fighter, board and take off so BNO1 carries
        // full `0x91` OwnPilotState corrections alongside all 64 bots.
        let target_pad = host
            .context
            .owned(|w| {
                w.space().and_then(|s| {
                    s.fighters
                        .iter()
                        .filter(|f| {
                            f.alive
                                && f.team == 1
                                && f.state == SpaceFlightState::Landed
                                && f.pilot.is_none()
                        })
                        .min_by(|a, b| {
                            let da = (a.position[0] - probe.last_soldier_pos[0]).powi(2)
                                + (a.position[2] - probe.last_soldier_pos[2]).powi(2);
                            let db = (b.position[0] - probe.last_soldier_pos[0]).powi(2)
                                + (b.position[2] - probe.last_soldier_pos[2]).powi(2);
                            da.total_cmp(&db)
                        })
                        .map(|f| [f.position[0], f.position[2]])
                })
            })
            .unwrap();
        let mut mounted = false;
        if let Some(pad) = target_pad {
            for step in 0..240u32 {
                let dx = pad[0] - probe.last_soldier_pos[0];
                let dz = pad[1] - probe.last_soldier_pos[2];
                let dist = (dx * dx + dz * dz).sqrt();
                let mut diff = dx.atan2(dz) - probe.last_soldier_yaw;
                while diff > std::f64::consts::PI {
                    diff -= std::f64::consts::TAU;
                }
                while diff < -std::f64::consts::PI {
                    diff += std::f64::consts::TAU;
                }
                let look_x = (-diff * 120.0).clamp(-80.0, 80.0);
                let held: &[usize] = if dist > 2.5 {
                    &[forward_code, sprint_code]
                } else {
                    &[]
                };
                probe.step_with(
                    &mut host,
                    held,
                    look_x,
                    Some(PilotControls {
                        use_pressed: dist < 8.0 && step.is_multiple_of(6),
                        ..Default::default()
                    }),
                );
                if probe.last_pilot_body.is_some() {
                    mounted = true;
                    break;
                }
            }
            if mounted {
                probe.step_with(
                    &mut host,
                    &[],
                    0.0,
                    Some(PilotControls {
                        flight: SpacePilotInput {
                            takeoff_or_land: true,
                            throttle: 1.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                );
            }
        }
        (host, probe, pilot.unit_id, class_id, 2u32, mounted)
    };

    let (mut host, mut probe, mut pilot_uid, mut pilot_class_id, mut next_edge_id, mut pilot_mounted) =
        open_match(seed, epoch);

    let keyframe_bns_bytes = probe.bns_bytes_per_pub.first().copied().unwrap_or(0);
    let keyframe_bno_bytes = probe.bno_bytes_per_pub.first().copied().unwrap_or(0);

    let mut steady_bns: Vec<usize> = Vec::with_capacity(12_000);
    let mut steady_bno: Vec<usize> = Vec::with_capacity(12_000);
    let mut steady_total: Vec<usize> = Vec::with_capacity(12_000);
    let mut steady_schema_bytes: std::collections::BTreeMap<u16, usize> =
        std::collections::BTreeMap::new();
    let mut steady_over_burst = 0usize;
    let mut steady_budget_skips = 0usize;
    let mut steady_own_during_skip = 0usize;
    let mut total_checks = 0usize;
    let mut total_pubs_due = 0usize;
    let mut matches_played = 1usize;
    let mut first_match_end_tick: Option<u32> = None;

    let mut airborne_sum = 0usize;
    let mut airborne_peak = 0usize;
    let mut ord_peak = 0usize;
    let mut walking_sum = 0usize;
    let mut walking_peak = 0usize;
    let mut census_samples = 0usize;

    let mut elapsed_ticks = 0u32;

    while elapsed_ticks < TOTAL_TICKS {
        let mut spawn_edge: Vec<(u32, u8, Vec<u8>)> = Vec::new();
        if elapsed_ticks.is_multiple_of(60) {
            let alive_now = unit_rows(&host.context)
                .unwrap()
                .into_iter()
                .find(|(id, _, _)| *id == pilot_uid)
                .map(|(_, gen, u)| {
                    if u.alive {
                        probe.generation = gen;
                    }
                    u.alive
                })
                .unwrap_or(false);
            if !alive_now {
                let payload = [pilot_class_id.to_le_bytes(), 0u16.to_le_bytes()].concat();
                spawn_edge.push((next_edge_id, 3, payload));
                next_edge_id += 1;
                pilot_mounted = false;
            }
        }

        let match_over = host
            .context
            .owned(|w| w.mission_view().is_some_and(|v| v.battle.over))
            .unwrap();
        if match_over {
            if first_match_end_tick.is_none() {
                first_match_end_tick = Some(elapsed_ticks);
            }
            seed += 1;
            epoch += 1;
            matches_played += 1;
            let (next_host, next_probe, next_uid, next_cid, next_eid, next_mounted) =
                open_match(seed, epoch);
            host = next_host;
            probe = next_probe;
            pilot_uid = next_uid;
            pilot_class_id = next_cid;
            next_edge_id = next_eid;
            pilot_mounted = next_mounted;
            continue;
        }

        let pilot_ctrl = if pilot_mounted {
            Some(PilotControls {
                flight: SpacePilotInput {
                    throttle: 0.6,
                    stick_x: 0.15,
                    ..Default::default()
                },
                ..Default::default()
            })
        } else {
            None
        };

        let tick_ms = 1000.0 / 60.0;
        let next_tick = host.tick + 1;
        let ack = probe.last_ack_tick.map(|t| (host.epoch as u16, t));
        let bytes = space_input_packet(
            host.epoch,
            probe.generation,
            probe.seq,
            next_tick,
            &[],
            probe.look_total,
            &spawn_edge,
            pilot_ctrl,
            ack,
        );
        probe.seq += 1;
        host.handle_input("pilot", &bytes, probe.now);
        if let Some((ae, at)) = ack {
            let mut ack_pkt = bfe_server::udp_peer::ACK_MAGIC.to_le_bytes().to_vec();
            ack_pkt.extend_from_slice(&ae.to_le_bytes());
            ack_pkt.extend_from_slice(&at.to_le_bytes());
            host.handle_input("watcher", &ack_pkt, probe.now);
        }
        probe.now += tick_ms;
        let pubn_opt = host
            .step(probe.now, (1_700_000_000_000.0 + probe.now) as _)
            .unwrap();
        elapsed_ticks += 1;

        if elapsed_ticks.is_multiple_of(60) {
            let (airborne, ord, mounted_cnt) = host
                .context
                .owned(|w| {
                    let s = w.space().unwrap();
                    let air = s
                        .fighters
                        .iter()
                        .filter(|f| f.alive && f.state != SpaceFlightState::Landed)
                        .count();
                    let mounted: usize = s
                        .fighters
                        .iter()
                        .filter(|f| f.alive)
                        .map(|f| {
                            usize::from(f.pilot.is_some())
                                + f.gunners.iter().filter(|g| g.is_some()).count()
                        })
                        .sum();
                    let ord = s.ordnance.active_count();
                    (air, ord, mounted)
                })
                .unwrap();
            let alive_soldiers = unit_rows(&host.context)
                .unwrap()
                .into_iter()
                .filter(|(_, _, u)| u.alive)
                .count();
            let on_foot = alive_soldiers.saturating_sub(mounted_cnt);
            airborne_sum += airborne;
            airborne_peak = airborne_peak.max(airborne);
            ord_peak = ord_peak.max(ord);
            walking_sum += on_foot;
            walking_peak = walking_peak.max(on_foot);
            census_samples += 1;
        }

        let Some(pubn) = pubn_opt else {
            continue;
        };
        total_pubs_due += 1;

        let mut tick_bns = 0usize;
        let mut is_keyframe = false;
        let mut delivered_shared = false;
        for group in pubn
            .shared
            .iter()
            .filter(|g| g.clients.iter().any(|c| c == "watcher"))
        {
            if let Some(first) = group.parts.first() {
                delivered_shared = true;
                let snap_tick = u32::from_le_bytes(first[6..10].try_into().unwrap());
                let base_tick = u32::from_le_bytes(first[11..15].try_into().unwrap());
                is_keyframe = base_tick == 0;
                let base_fields = (base_tick != 0)
                    .then(|| probe.world_history.get(&base_tick))
                    .flatten()
                    .cloned();
                let mut cur_fields = base_fields.clone().unwrap_or_default();
                for part in &group.parts {
                    tick_bns += part.len();
                    for (id, schema, wire_len, payload) in
                        snapshot_world_fields(part, base_fields.as_ref(), &mut cur_fields)
                    {
                        if payload.is_empty() {
                            continue;
                        }
                        if !is_keyframe {
                            *steady_schema_bytes.entry(schema).or_insert(0) += wire_len;
                        }
                        probe
                            .checks
                            .field(schema, id, &payload)
                            .unwrap_or_else(|e| {
                                panic!(
                                    "10-min Checks::field rejected schema=0x{schema:04x} id={id}: {e}"
                                )
                            });
                        total_checks += 1;
                    }
                }
                probe.world_history.insert(snap_tick, cur_fields);
                while probe.world_history.len() > 64 {
                    probe.world_history.pop_first();
                }
                probe.last_ack_tick = Some(
                    probe
                        .last_ack_tick
                        .map_or(snap_tick, |prev| prev.max(snap_tick)),
                );
            }
        }

        let mut tick_bno = 0usize;
        for own in pubn.own_packets.iter().filter(|p| p.peer_id == "pilot") {
            tick_bno += own.bytes.len();
            let own_tick = u32::from_le_bytes(own.bytes[6..10].try_into().unwrap());
            let own_base = u32::from_le_bytes(own.bytes[11..15].try_into().unwrap());
            let base_payload = (own_base != 0)
                .then(|| probe.own_history.get(&own_base).map(Vec::as_slice))
                .flatten();
            if let Some(payload) = own_correction(&own.bytes, base_payload) {
                probe.own_history.insert(own_tick, payload.clone());
                while probe.own_history.len() > 64 {
                    probe.own_history.pop_first();
                }
                let head = decode_own_player_header(&payload).unwrap();
                probe
                    .checks
                    .correction(
                        f64::from(host.epoch),
                        f64::from(host.tick),
                        probe.unit_slot,
                        probe.generation,
                        11,
                        &payload,
                    )
                    .unwrap_or_else(|e| {
                        panic!(
                            "10-min Checks::correction rejected schema=0x{:02x}: {e}",
                            head.body_schema
                        )
                    });
                total_checks += 1;
            }
        }

        if !delivered_shared {
            steady_budget_skips += 1;
            if tick_bno > 0 {
                steady_own_during_skip += 1;
            }
        } else if !is_keyframe {
            let total = tick_bns + tick_bno;
            if total > BURST_CEILING_BYTES {
                steady_over_burst += 1;
            }
            steady_bns.push(tick_bns);
            steady_bno.push(tick_bno);
            steady_total.push(total);
        }
    }

    // Controlled mid-battle burst-ceiling proof: join a fresh late client into the live
    // mid-battle server under the real 100,000 B/s (10,000 B burst ceiling) budget,
    // verify its first full keyframe crosses 10,000 B, measure the exact whole-snapshot
    // skip cadence while credit refills, and verify 0 torn snapshots and 0 check faults
    // upon recovery.
    let late_plan = plan(&Table::of(WRITES_SPACE), &Supports::of(SUPPORTS)).unwrap();
    let late = host
        .add_player("late", "Late", false, true, false)
        .unwrap();
    host.set_plan("late", Some(late_plan.clone()));
    host.handle_start("late", probe.now);
    let late_checks = Checks::new(late_plan, build_classes_vec(&host));
    type FieldMap = std::collections::BTreeMap<(u32, u16), Vec<u8>>;
    let mut late_world: std::collections::BTreeMap<u32, FieldMap> =
        std::collections::BTreeMap::new();
    let mut late_own_hist: std::collections::BTreeMap<u32, Vec<u8>> =
        std::collections::BTreeMap::new();
    let mut late_ack: Option<u32> = None;
    let mut late_kf_bytes = 0usize;
    let mut late_kf_parts = 0usize;
    let mut late_skips_after_kf = 0usize;
    let mut late_own_during_skips = 0usize;
    let mut late_recovered_deltas = 0usize;
    let mut late_validated = 0usize;

    for _ in 0..180 {
        let tick_ms = 1000.0 / 60.0;
        if let Some(at) = late_ack {
            let mut ack_pkt = bfe_server::udp_peer::ACK_MAGIC.to_le_bytes().to_vec();
            ack_pkt.extend_from_slice(&(host.epoch as u16).to_le_bytes());
            ack_pkt.extend_from_slice(&at.to_le_bytes());
            host.handle_input("late", &ack_pkt, probe.now);
        }
        probe.now += tick_ms;
        let Some(pubn) = host
            .step(probe.now, (1_700_000_000_000.0 + probe.now) as _)
            .unwrap()
        else {
            continue;
        };
        let mut shared_bytes = 0usize;
        let mut shared_parts = 0usize;
        let mut is_kf = false;
        for group in pubn
            .shared
            .iter()
            .filter(|g| g.clients.iter().any(|c| c == "late"))
        {
            if let Some(first) = group.parts.first() {
                let snap_tick = u32::from_le_bytes(first[6..10].try_into().unwrap());
                let base_tick = u32::from_le_bytes(first[11..15].try_into().unwrap());
                is_kf = base_tick == 0;
                let part_total = first[15] as usize;
                let expected_parts = if part_total == 0 {
                    u16::from_le_bytes([first[22], first[23]]) as usize
                } else {
                    part_total
                };
                assert_eq!(
                    group.parts.len(),
                    expected_parts,
                    "burst-limited server must skip whole snapshots, never send a torn part set"
                );
                let base_fields = (base_tick != 0)
                    .then(|| late_world.get(&base_tick))
                    .flatten()
                    .cloned();
                let mut cur_fields = base_fields.clone().unwrap_or_default();
                for part in &group.parts {
                    shared_bytes += part.len();
                    shared_parts += 1;
                    for (id, schema, _, payload) in
                        snapshot_world_fields(part, base_fields.as_ref(), &mut cur_fields)
                    {
                        if payload.is_empty() {
                            continue;
                        }
                        late_checks.field(schema, id, &payload).unwrap();
                        late_validated += 1;
                    }
                }
                late_world.insert(snap_tick, cur_fields);
                late_ack = Some(late_ack.map_or(snap_tick, |p| p.max(snap_tick)));
            }
        }
        let mut own_bytes = 0usize;
        for own in pubn.own_packets.iter().filter(|p| p.peer_id == "late") {
            own_bytes += own.bytes.len();
            let own_tick = u32::from_le_bytes(own.bytes[6..10].try_into().unwrap());
            let own_base = u32::from_le_bytes(own.bytes[11..15].try_into().unwrap());
            let base_payload = (own_base != 0)
                .then(|| late_own_hist.get(&own_base).map(Vec::as_slice))
                .flatten();
            if let Some(payload) = own_correction(&own.bytes, base_payload) {
                late_own_hist.insert(own_tick, payload.clone());
                let _ = late_checks.correction(
                    f64::from(host.epoch),
                    f64::from(host.tick),
                    late.unit_id,
                    late.generation.max(1),
                    11,
                    &payload,
                );
            }
        }
        if late_kf_bytes == 0 {
            if is_kf && shared_bytes > 0 {
                late_kf_bytes = shared_bytes;
                late_kf_parts = shared_parts;
            }
        } else if shared_bytes == 0 {
            late_skips_after_kf += 1;
            if own_bytes > 0 {
                late_own_during_skips += 1;
            }
        } else if !is_kf {
            late_recovered_deltas += 1;
        }
    }

    let n = steady_total.len().max(1);
    let mut bns_sorted = steady_bns.clone();
    let mut bno_sorted = steady_bno.clone();
    let mut tot_sorted = steady_total.clone();
    bns_sorted.sort_unstable();
    bno_sorted.sort_unstable();
    tot_sorted.sort_unstable();

    let mean_bns = steady_bns.iter().sum::<usize>() as f64 / n as f64;
    let mean_bno = steady_bno.iter().sum::<usize>() as f64 / n as f64;
    let mean_tot = steady_total.iter().sum::<usize>() as f64 / n as f64;
    let p50_tot = percentile_usize(&tot_sorted, 0.50);
    let p95_tot = percentile_usize(&tot_sorted, 0.95);
    let p99_tot = percentile_usize(&tot_sorted, 0.99);
    let max_tot = *tot_sorted.last().unwrap_or(&0);
    let p99_bns = percentile_usize(&bns_sorted, 0.99);
    let max_bns = *bns_sorted.last().unwrap_or(&0);
    let p99_bno = percentile_usize(&bno_sorted, 0.99);
    let max_bno = *bno_sorted.last().unwrap_or(&0);

    let mut worst_1s_bps = 0usize;
    let mut rolling = 0usize;
    for (i, &b) in steady_total.iter().enumerate() {
        rolling += b;
        if i >= 20 {
            rolling -= steady_total[i - 20];
        }
        if i + 1 >= 20 {
            worst_1s_bps = worst_1s_bps.max(rolling);
        }
    }
    let mean_bps = mean_tot * 20.0;
    let schema_breakdown: String = steady_schema_bytes
        .iter()
        .map(|(s, b)| format!("0x{s:04x}={:.1}B", *b as f64 / n as f64))
        .collect::<Vec<_>>()
        .join(" ");
    let mean_airborne = airborne_sum as f64 / census_samples.max(1) as f64;
    let mean_walking = walking_sum as f64 / census_samples.max(1) as f64;

    println!(
        "spa6 10-minute ({TOTAL_TICKS} ticks, {matches_played} match(es), first_end={first_match_end_tick:?}) bandwidth:\n  \
         census: airborne mean={mean_airborne:.1} peak={airborne_peak}, walking mean={mean_walking:.1} peak={walking_peak}, active_ord peak={ord_peak}\n  \
         initial keyframe: BNS1={keyframe_bns_bytes} B BNO1={keyframe_bno_bytes} B\n  \
         steady ({n}/{total_pubs_due} pubs, validated={total_checks} checks, 0 errors):\n    \
           BNS1:  mean={mean_bns:.1} B  p99={p99_bns} B  worst={max_bns} B\n    \
           BNO1:  mean={mean_bno:.1} B  p99={p99_bno} B  worst={max_bno} B\n    \
           TOTAL: mean={mean_tot:.1} B  p50={p50_tot} B  p95={p95_tot} B  p99={p99_tot} B  worst={max_tot} B\n    \
           rate:  mean={mean_bps:.0} B/s ({:.1}% of 100000 B/s)  worst_1s={worst_1s_bps} B/s ({:.1}%)\n    \
           burst (>10000 B ceiling): steady_over_burst={steady_over_burst} steady_budget_skips={steady_budget_skips} own_during_skip={steady_own_during_skip}\n    \
           per-pub schemas: {schema_breakdown}\n  \
         late-join burst-ceiling proof: kf={late_kf_bytes} B ({late_kf_parts} parts) skips_after_kf={late_skips_after_kf} own_during_skips={late_own_during_skips} recovered_deltas={late_recovered_deltas} validated={late_validated}",
        mean_bps / 1000.0,
        worst_1s_bps as f64 / 1000.0,
    );

    assert!(
        n >= 10_000,
        "expected >= 10,000 steady 20 Hz snapshots in 10 minutes, got {n}"
    );
    assert!(
        mean_bps < STATE_BUDGET_BPS,
        "10-minute mean bandwidth {mean_bps:.0} B/s exceeded {STATE_BUDGET_BPS} B/s"
    );
    assert!(
        late_kf_bytes > 0 && late_recovered_deltas >= 10,
        "late joiner must complete keyframe ({late_kf_bytes} B) and recover steady deltas ({late_recovered_deltas})"
    );
}

#[test]
#[ignore = "requires prepared SWBF2 content root via BFE_SWBF2_PREPARED"]
fn space_client_data_matches_server_battlefield_on_real_spa6() {
    let root = swbf2_prepared_root().expect("set BFE_SWBF2_PREPARED to a prepared SWBF2 root");
    let files = DirFiles::from_resolved_root(&root, GameFamily::Swbf2);
    let options = Options {
        family: GameFamily::Swbf2,
        mode: "ass".into(),
        map: "spa6".into(),
        side: "rep".into(),
        bots: Bots::default(),
        enhanced: false,
        heroes: false,
        seed: 1,
        unit: None,
        practice_target: None,
        name: None,
    };
    let selection = MissionSelection {
        family: GameFamily::Swbf2,
        map: "spa6".into(),
        era: "cw".into(),
        mode: "ass".into(),
        seed: 1,
    };
    let ctx = Context::new();
    let _ = ctx
        .world_mission_content(&files, &options, &selection, MissionPreferences::default())
        .expect("admit spa6 world");

    let battle = bfe_content::battle::Battle::load_for_family(
        &bfe_content::battle::Loader::new(&files),
        GameFamily::Swbf2,
        "spa6",
        "?map=spa6&era=cw&mode=ass&side=rep",
    )
    .expect("load spa6 battle");
    let content = bfe_content::content::Content::build(battle).expect("build spa6 content");
    let buildings_bvh = bfe_content::bvh::build(
        bfe_content::bvh::Positions::F32(&content.collision.positions),
        &content.collision.indices,
    );
    let mut client_collision = bfe_sim::collision::CollisionOwner::new();
    let installed = bfe_content::install::install(
        &mut client_collision,
        &content.collision,
        &buildings_bvh,
        None,
    )
    .expect("install client collision");
    // Mirror `Props::new` / server prop-body installation before `SpaceClientData::from_battle`,
    // since hangar deck floors are prop collision bodies on 2005 Space maps.
    let instances = content
        .battle
        .props
        .as_ref()
        .and_then(|p| p.get("instances"))
        .and_then(bfe_content::Value::as_array)
        .unwrap_or(&[]);
    let mut serial = client_collision.state.bodies.len() as f64;
    for prop in &content.prop_bodies {
        let desc = &instances[prop.instance];
        let first = if desc
            .get("unbuilt")
            .is_some_and(bfe_content::Value::truthy)
        {
            2
        } else {
            0
        };
        for (look, shape) in [&prop.built, &prop.destroyed, &prop.unbuilt]
            .into_iter()
            .enumerate()
        {
            let Some(shape) = shape else { continue };
            use bfe_content::install::CollisionKernel;
            let pos_id = client_collision.positions(&shape.positions);
            let bvh = bfe_content::bvh::build(
                bfe_content::bvh::Positions::F32(&shape.positions),
                &shape.indices,
            );
            let tree = client_collision.tree(pos_id, &shape.indices, &bvh);
            client_collision.state.trees[tree as usize]
                .polygons
                .extend_from_slice(&shape.polygons);
            let body = client_collision.body_new(tree, u32::from(prop.world_animated));
            assert!(body >= 0);
            let body = body as u32;
            serial += 1.0;
            client_collision.body_offset(body, serial * 16777216.0);
            let (lo, hi) = bfe_sim::vehicles::build::local_bounds(&shape.positions, &shape.indices);
            let io = client_collision.io_mut();
            io[20..23].copy_from_slice(&lo);
            io[23..26].copy_from_slice(&hi);
            client_collision.body_set(body);
            let w = &mut client_collision.state.worlds[installed.world as usize];
            w.order.push(body);
            w.version += 1;
            client_collision.enable_body(body, look == first);
        }
    }
    let client = bfe_sim::space::SpaceClientData::from_battle(
        &content.battle,
        &mut client_collision,
        installed.world as usize,
    )
    .expect("build SpaceClientData");

    ctx.owned(|w| {
        let server = w.space().expect("server SpaceBattlefield");
        assert_eq!(client.profiles.len(), server.profiles.len());
        for (c, s) in client.profiles.iter().zip(&server.profiles) {
            assert_eq!(c.class_name, s.class_name);
            assert_eq!(c.weapons.len(), s.weapons.len());
        }
        assert_eq!(client.pads.len(), server.pads.len());
        for (idx, (c, s)) in client.pads.iter().zip(&server.pads).enumerate() {
            assert_eq!(c.name, s.name);
            assert_eq!(c.profile_index, s.profile_index);
            assert_eq!(c.spawn_count, s.spawn_count);
            assert_eq!(c.position, s.position);
            assert!((c.yaw - s.yaw).abs() < 1e-12);
            assert!(
                (c.floor_y - s.floor_y).abs() < 1e-12,
                "pad {idx} ({}) floor_y mismatch: client={} server={}",
                c.name,
                c.floor_y,
                s.floor_y
            );
        }
        assert_eq!(client.fighter_slots, server.fighters.len());
        assert_eq!(client.fighter_pad_indices.len(), server.fighters.len());
        for (id, f) in server.fighters.iter().enumerate() {
            assert_eq!(
                client.fighter_pad_indices[id], f.pad_index,
                "fighter {id} pad index mismatch"
            );
        }
        assert_eq!(client.buildings.len(), server.buildings.len());
        for (c, s) in client.buildings.iter().zip(&server.buildings) {
            assert_eq!(c.building_index, s.building_index);
            assert_eq!(c.name, s.name);
            assert_eq!(c.turret.is_some(), s.turret.is_some());
            assert_eq!(c.terminal.is_some(), s.terminal.is_some());
        }
        assert_eq!(client.doors.len(), server.doors.len());
        for (c, s) in client.doors.iter().zip(&server.doors) {
            assert_eq!(c.name, s.name);
            assert_eq!(c.collidable, s.collidable);
        }
        assert_eq!(client.world_anims.objects.len(), server.world_anims.objects.len());
        for (c, s) in client.world_anims.objects.iter().zip(&server.world_anims.objects) {
            assert_eq!(c.name, s.name);
            assert_eq!(c.prop_instance, s.prop_instance);
        }
        let server_hashes =
            bfe_sim::wire::space::weapon_hash_table(&server.profiles, &server.buildings);
        assert_eq!(client.weapon_hashes, server_hashes);
    })
    .unwrap();
}

/// Drives the real 60 Hz `NativeWorldHost` loop and its 20 Hz `BNS1` snapshot
/// publication on prepared `spa6` for 6,000 ticks (100 s, 2,000 publications)
/// and checks every launch, impact and destruction burst the server recorded
/// against what four receivers draw: each event rides in exactly the two
/// publications after it (tick offsets 0..=5), so a receiver misses an event
/// if and only if it lost both of them, never draws one twice, and a lossless
/// receiver draws all of them.
#[test]
#[ignore = "requires prepared SWBF2 spa6 content (BFE_SWBF2_PREPARED)"]
fn space_assault_two_window_event_carry_survives_loss_on_real_server() {
    use bfe_sim::wire::space::encode_pos_u24;
    use std::collections::{BTreeMap, BTreeSet};

    const WARMUP_TICKS: u32 = 300;
    const SAMPLE_END_TICK: u32 = 5_994;
    const TOTAL_TICKS: u32 = 6_000;

    let root = swbf2_prepared_root().expect("set BFE_SWBF2_PREPARED to a prepared SWBF2 root");
    let query = "?map=spa6&era=cw&mode=ass&side=rep&seed=41";
    let mut host = NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).unwrap();
    let space_plan = plan(&Table::of(WRITES_SPACE), &Supports::of(SUPPORTS)).unwrap();
    let _watcher = host.add_player("watcher", "Watcher", false, false, false).unwrap();
    host.set_plan("watcher", Some(space_plan.clone()));
    host.handle_start("watcher", 0.0);
    let checks = Checks::new(space_plan, build_classes_vec(&host));

    let wire_bounds = host.wire_bounds.expect("spa6 must admit wire bounds");

    struct ClientRing {
        name: &'static str,
        loss_ppm: u32,
        alternating_drop: bool,
        rng: u32,
        seen_launches: [u16; 128],
        seen_launch_len: usize,
        seen_launch_at: usize,
        seen_impacts: [u16; 128],
        seen_impact_len: usize,
        seen_impact_at: usize,
        seen_deaths: [(bool, u16, [u32; 3]); 64],
        seen_death_len: usize,
        seen_death_at: usize,
        last_pub_tick: u32,
        drawn_launches: BTreeSet<(u32, u16)>,
        drawn_impacts: BTreeSet<(u32, u16)>,
        drawn_deaths: usize,
        suppressed_launches: usize,
        suppressed_impacts: usize,
        suppressed_deaths: usize,
        redrawn: usize,
        launch_offsets: [usize; 6],
        impact_offsets: [usize; 6],
        pub_index: u32,
        dropped_pubs: usize,
        delivered_pubs: usize,
        dropped_ticks: BTreeSet<u32>,
    }
    impl ClientRing {
        fn new(name: &'static str, loss_ppm: u32, alternating_drop: bool, seed: u32) -> Self {
            Self {
                name,
                loss_ppm,
                alternating_drop,
                rng: seed,
                seen_launches: [0; 128],
                seen_launch_len: 0,
                seen_launch_at: 0,
                seen_impacts: [0; 128],
                seen_impact_len: 0,
                seen_impact_at: 0,
                seen_deaths: [(false, 0, [0; 3]); 64],
                seen_death_len: 0,
                seen_death_at: 0,
                last_pub_tick: 0,
                drawn_launches: BTreeSet::new(),
                drawn_impacts: BTreeSet::new(),
                drawn_deaths: 0,
                suppressed_launches: 0,
                suppressed_impacts: 0,
                suppressed_deaths: 0,
                redrawn: 0,
                launch_offsets: [0; 6],
                impact_offsets: [0; 6],
                pub_index: 0,
                dropped_pubs: 0,
                delivered_pubs: 0,
                dropped_ticks: BTreeSet::new(),
            }
        }
        fn next_u32(&mut self) -> u32 {
            let mut x = self.rng;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.rng = x;
            x
        }
        fn mark_u16(ring: &mut [u16; 128], len: &mut usize, at: &mut usize, key: u16) -> bool {
            if ring[..*len].contains(&key) {
                return false;
            }
            ring[*at] = key;
            *at = (*at + 1) % ring.len();
            if *len < ring.len() {
                *len += 1;
            }
            true
        }
        fn mark_death(
            ring: &mut [(bool, u16, [u32; 3]); 64],
            len: &mut usize,
            at: &mut usize,
            key: (bool, u16, [u32; 3]),
        ) -> bool {
            if ring[..*len].contains(&key) {
                return false;
            }
            ring[*at] = key;
            *at = (*at + 1) % ring.len();
            if *len < ring.len() {
                *len += 1;
            }
            true
        }
        fn deliver(
            &mut self,
            snap_tick: u32,
            events: &bfe_sim::wire::space::EventsSnap,
            in_window: impl Fn(u32) -> bool,
            srv_death_keys: &BTreeSet<(bool, u16, [u32; 3])>,
            seen_death_keys: &mut BTreeSet<(bool, u16, [u32; 3])>,
        ) {
            self.pub_index += 1;
            let drop = if self.alternating_drop {
                self.pub_index % 2 == 1
            } else if self.loss_ppm > 0 {
                (self.next_u32() % 1_000_000) < self.loss_ppm
            } else {
                false
            };
            if drop {
                self.dropped_pubs += 1;
                self.dropped_ticks.insert(snap_tick);
                return;
            }
            self.delivered_pubs += 1;
            let prev_tick = if self.last_pub_tick > 0 && self.last_pub_tick < snap_tick {
                self.last_pub_tick
            } else {
                snap_tick.saturating_sub(1)
            };
            for l in &events.launches {
                self.launch_offsets[usize::from(l.tick_offset)] += 1;
                if !Self::mark_u16(
                    &mut self.seen_launches,
                    &mut self.seen_launch_len,
                    &mut self.seen_launch_at,
                    l.serial,
                ) {
                    self.suppressed_launches += 1;
                    continue;
                }
                let born = snap_tick.saturating_sub(u32::from(l.tick_offset));
                if in_window(born) {
                    assert!(
                        born > prev_tick && born <= snap_tick,
                        "{}: launch serial={} born={} outside ({}, {}]",
                        self.name,
                        l.serial,
                        born,
                        prev_tick,
                        snap_tick
                    );
                    if !self.drawn_launches.insert((born, l.serial)) {
                        self.redrawn += 1;
                    }
                }
            }
            for imp in &events.impacts {
                self.impact_offsets[usize::from(imp.tick_offset)] += 1;
                if !Self::mark_u16(
                    &mut self.seen_impacts,
                    &mut self.seen_impact_len,
                    &mut self.seen_impact_at,
                    imp.serial,
                ) {
                    self.suppressed_impacts += 1;
                    continue;
                }
                let hit_tick = snap_tick.saturating_sub(u32::from(imp.tick_offset));
                if in_window(hit_tick) && !self.drawn_impacts.insert((hit_tick, imp.serial)) {
                    self.redrawn += 1;
                }
            }
            for d in &events.deaths {
                let key = (d.is_building, d.class_idx, d.pos);
                if !Self::mark_death(
                    &mut self.seen_deaths,
                    &mut self.seen_death_len,
                    &mut self.seen_death_at,
                    key,
                ) {
                    self.suppressed_deaths += 1;
                    continue;
                }
                if srv_death_keys.contains(&key) {
                    if !seen_death_keys.insert(key) {
                        self.redrawn += 1;
                    } else {
                        self.drawn_deaths += 1;
                    }
                }
            }
            self.last_pub_tick = snap_tick;
        }
    }

    let mut clients = [
        ClientRing::new("0% loss", 0, false, 0x1111_2222),
        ClientRing::new("2% loss", 20_000, false, 0x3333_4444),
        ClientRing::new("10% loss", 100_000, false, 0x5555_6666),
        ClientRing::new("50% alternating", 0, true, 0x7777_8888),
    ];
    let mut client_death_sets: [BTreeSet<(bool, u16, [u32; 3])>; 4] = Default::default();

    let mut srv_launches: BTreeSet<(u32, u16)> = BTreeSet::new();
    let mut srv_impacts: BTreeSet<(u32, u16)> = BTreeSet::new();
    let mut srv_deaths: BTreeSet<(bool, u16, [u32; 3])> = BTreeSet::new();
    let mut srv_death_ticks: BTreeMap<(bool, u16, [u32; 3]), u32> = BTreeMap::new();
    let mut pub_ticks: Vec<u32> = Vec::with_capacity(2_100);

    let mut world_history: BTreeMap<u32, BTreeMap<(u32, u16), Vec<u8>>> = BTreeMap::new();
    let mut last_ack_tick: Option<u32> = None;
    let mut events_field_bytes: Vec<usize> = Vec::with_capacity(2_000);
    let mut bns_bytes: Vec<usize> = Vec::with_capacity(2_000);

    let mut now = 0.0f64;
    let mut wall = 1_700_000_000_000;
    let tick_ms = 1000.0 / 60.0;

    for _ in 0..TOTAL_TICKS {
        if let Some(at) = last_ack_tick {
            let mut ack_pkt = bfe_server::udp_peer::ACK_MAGIC.to_le_bytes().to_vec();
            ack_pkt.extend_from_slice(&(host.epoch as u16).to_le_bytes());
            ack_pkt.extend_from_slice(&at.to_le_bytes());
            host.handle_input("watcher", &ack_pkt, now);
        }
        now += tick_ms;
        wall += 17;
        let pubn_opt = host.step(now, wall).unwrap();
        let cur_tick = host.tick;

        // Collect ground-truth server events born on `cur_tick` inside the measured window.
        if (WARMUP_TICKS..=SAMPLE_END_TICK).contains(&cur_tick) {
            host.context
                .owned(|w| {
                    let Some(space) = w.space() else { return };
                    let Some(obs) = space.ordnance.observations.as_ref() else { return };
                    let st = space.tick;
                    for l in obs.wire_launches().iter().filter(|e| e.tick == st) {
                        srv_launches.insert((cur_tick, l.serial));
                    }
                    for imp in obs.wire_impacts().iter().filter(|e| e.tick == st) {
                        srv_impacts.insert((cur_tick, imp.serial));
                    }
                    for d in obs.wire_deaths().iter().filter(|e| e.tick == st) {
                        let pos = encode_pos_u24(&wire_bounds, d.position);
                        srv_deaths.insert((d.is_building, d.class_idx, pos));
                        srv_death_ticks.entry((d.is_building, d.class_idx, pos)).or_insert(cur_tick);
                    }
                })
                .unwrap();
        }

        let Some(pubn) = pubn_opt else {
            continue;
        };
        for group in pubn
            .shared
            .iter()
            .filter(|g| g.clients.iter().any(|c| c == "watcher"))
        {
            let Some(first) = group.parts.first() else {
                continue;
            };
            let snap_tick = u32::from_le_bytes(first[6..10].try_into().unwrap());
            let base_tick = u32::from_le_bytes(first[11..15].try_into().unwrap());
            let base_fields = (base_tick != 0)
                .then(|| world_history.get(&base_tick))
                .flatten()
                .cloned();
            let mut cur_fields = base_fields.clone().unwrap_or_default();
            let mut snap_bns = 0usize;
            let mut snap_ev_bytes = 0usize;
            let mut snap_events: Option<bfe_sim::wire::space::EventsSnap> = None;

            for part in &group.parts {
                snap_bns += part.len();
                for (id, schema, wire_len, payload) in
                    snapshot_world_fields(part, base_fields.as_ref(), &mut cur_fields)
                {
                    if payload.is_empty() {
                        continue;
                    }
                    checks.field(schema, id, &payload).unwrap();
                    if schema == ORDNANCE_SCHEMA && id == 1 {
                        snap_ev_bytes = wire_len;
                        let (rev, body) = split_revision(&payload).unwrap();
                        assert_eq!(rev, REVISION);
                        if let OrdnanceField::Events(ev) = read_ordnance_field(id, body).unwrap() {
                            snap_events = Some(ev);
                        }
                    }
                }
            }
            // If `0x0214` id=1 was unchanged vs baseline, reconstruct from `cur_fields`.
            if snap_events.is_none() {
                if let Some(payload) = cur_fields.get(&(1, ORDNANCE_SCHEMA)) {
                    if let Ok((REVISION, body)) = split_revision(payload) {
                        if let Ok(OrdnanceField::Events(ev)) = read_ordnance_field(1, body) {
                            snap_events = Some(ev);
                        }
                    }
                }
            }
            pub_ticks.push(snap_tick);
            if snap_tick >= WARMUP_TICKS {
                bns_bytes.push(snap_bns);
                events_field_bytes.push(snap_ev_bytes);
            }
            if let Some(ev) = &snap_events {
                let in_win = |t: u32| (WARMUP_TICKS..=SAMPLE_END_TICK).contains(&t);
                for (idx, client) in clients.iter_mut().enumerate() {
                    client.deliver(
                        snap_tick,
                        ev,
                        in_win,
                        &srv_deaths,
                        &mut client_death_sets[idx],
                    );
                }
            }
            world_history.insert(snap_tick, cur_fields);
            while world_history.len() > 64 {
                world_history.pop_first();
            }
            last_ack_tick = Some(last_ack_tick.map_or(snap_tick, |p| p.max(snap_tick)));
        }
    }

    assert!(
        srv_launches.len() >= 500 && srv_impacts.len() >= 300 && !srv_deaths.is_empty(),
        "expected active spa6 combat: launches={} impacts={} deaths={}",
        srv_launches.len(),
        srv_impacts.len(),
        srv_deaths.len()
    );

    events_field_bytes.sort_unstable();
    bns_bytes.sort_unstable();
    let mean_ev = events_field_bytes.iter().sum::<usize>() as f64 / events_field_bytes.len().max(1) as f64;
    let p99_ev = percentile_usize(&events_field_bytes, 0.99);
    let max_ev = *events_field_bytes.last().unwrap_or(&0);
    let mean_bns = bns_bytes.iter().sum::<usize>() as f64 / bns_bytes.len().max(1) as f64;
    println!(
        "real-server Space event carry ({} ticks, server truth: launches={} impacts={} deaths={}):\n  \
         0x0214 id=1 bytes/pub: mean={mean_ev:.1} B p99={p99_ev} B max={max_ev} B | BNS1 mean={mean_bns:.1} B",
        SAMPLE_END_TICK - WARMUP_TICKS + 1,
        srv_launches.len(),
        srv_impacts.len(),
        srv_deaths.len(),
    );
    for c in &clients {
        let l_miss = srv_launches.len().saturating_sub(c.drawn_launches.len());
        let i_miss = srv_impacts.len().saturating_sub(c.drawn_impacts.len());
        let d_miss = srv_deaths.len().saturating_sub(c.drawn_deaths);
        let l_pct = 100.0 * l_miss as f64 / srv_launches.len().max(1) as f64;
        let i_pct = 100.0 * i_miss as f64 / srv_impacts.len().max(1) as f64;
        println!(
            "  {:16}: pubs={}/{} dropped={} | launches={}/{} (miss {} = {:.3}%) | \
             impacts={}/{} (miss {} = {:.3}%) | deaths={}/{} (miss {}) | \
             suppressed=(l:{} i:{} d:{}) redrawn={} | launch_offsets={:?} impact_offsets={:?}",
            c.name,
            c.delivered_pubs,
            c.delivered_pubs + c.dropped_pubs,
            c.dropped_pubs,
            c.drawn_launches.len(),
            srv_launches.len(),
            l_miss,
            l_pct,
            c.drawn_impacts.len(),
            srv_impacts.len(),
            i_miss,
            i_pct,
            c.drawn_deaths,
            srv_deaths.len(),
            d_miss,
            c.suppressed_launches,
            c.suppressed_impacts,
            c.suppressed_deaths,
            c.redrawn,
            c.launch_offsets,
            c.impact_offsets,
        );
        assert_eq!(c.redrawn, 0, "{}: duplicate event drawn", c.name);
    }

    // The two publications that carry an event born on `tick`: the earliest one at
    // or after it and the next one.
    let carriers = |tick: u32| -> [u32; 2] {
        let at = pub_ticks.partition_point(|&t| t < tick);
        let pair = [pub_ticks.get(at).copied(), pub_ticks.get(at + 1).copied()];
        pair.map(|t| t.expect("two publications after every measured event"))
    };
    for c in &clients {
        let lost = |tick: u32| carriers(tick).iter().all(|t| c.dropped_ticks.contains(t));
        let want_launches: BTreeSet<(u32, u16)> = srv_launches.iter().copied().filter(|e| !lost(e.0)).collect();
        let want_impacts: BTreeSet<(u32, u16)> = srv_impacts.iter().copied().filter(|e| !lost(e.0)).collect();
        let want_deaths = srv_death_ticks.values().filter(|&&t| !lost(t)).count();
        let pairs_lost = pub_ticks.windows(2).filter(|w| w.iter().all(|t| c.dropped_ticks.contains(t))).count();
        println!(
            "  {:16}: snapshot loss {:.2}% (squared {:.3}%), both carriers lost for {} of {} publication pairs; \
             missed launches {:.3}%, impacts {:.3}%",
            c.name,
            100.0 * c.dropped_pubs as f64 / pub_ticks.len() as f64,
            100.0 * (c.dropped_pubs as f64 / pub_ticks.len() as f64).powi(2),
            pairs_lost,
            pub_ticks.len().saturating_sub(1),
            100.0 * (srv_launches.len() - c.drawn_launches.len()) as f64 / srv_launches.len() as f64,
            100.0 * (srv_impacts.len() - c.drawn_impacts.len()) as f64 / srv_impacts.len() as f64,
        );
        assert_eq!(c.drawn_launches, want_launches, "{}: launches drawn are not those of a received carrier", c.name);
        assert_eq!(c.drawn_impacts, want_impacts, "{}: impacts drawn are not those of a received carrier", c.name);
        assert_eq!(c.drawn_deaths, want_deaths, "{}: bursts drawn are not those of a received carrier", c.name);
    }
    // A lossless receiver draws everything, and receives each event twice.
    let c0 = &clients[0];
    assert_eq!(c0.drawn_launches.len(), srv_launches.len(), "0% loss must deliver 100% of launches");
    assert_eq!(c0.drawn_impacts.len(), srv_impacts.len(), "0% loss must deliver 100% of impacts");
    assert_eq!(c0.drawn_deaths, srv_deaths.len(), "0% loss must deliver 100% of destruction bursts");
    assert!(c0.suppressed_launches > 0 && c0.suppressed_impacts > 0, "0% loss must see carried duplicates");
    for off in 0..6 {
        assert!(
            c0.launch_offsets[off] > 0 && c0.impact_offsets[off] > 0,
            "expected events at tick_offset={off}: l={:?} i={:?}",
            c0.launch_offsets,
            c0.impact_offsets
        );
    }
    // Every other publication lost: nothing is missed.
    assert_eq!(clients[3].drawn_launches.len(), srv_launches.len(), "single-snapshot drops must lose 0 launches");
}
