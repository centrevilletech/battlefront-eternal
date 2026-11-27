use super::{codec, combat, cosmetics, journal::Events, PositionBounds, Runtime};
use crate::{
    gen::{WbEncode, WireRecord, WireRecordKind},
    store::Strings,
};
use bfe_presentation::{
    network::{read, Output, Session, Words},
    network_layout::{net_header, net_kind, net_op},
};

fn space_bounds() -> PositionBounds {
    // The padded box is exactly 16 km on each axis.
    PositionBounds::from_map_extents([-7936.; 3], [7936.; 3]).unwrap()
}

fn assert_position(actual: [f64; 3], expected: [f64; 3], bounds: PositionBounds) {
    for (axis, step) in bounds.step().into_iter().enumerate() {
        let error = (actual[axis] - expected[axis]).abs();
        assert!(error <= step / 2. + 1e-10, "axis {axis}: error {error} m");
        assert!(step <= PositionBounds::LEGACY.step()[axis]);
    }
}

fn own_packet(tick: u32, baseline: u32, cosmetics: Option<&[u8]>) -> Vec<u8> {
    let mut w = codec::Writer::default();
    codec::own_datagram_into(&mut w, codec::OwnParts {
        epoch: 1.,
        tick: f64::from(tick),
        interval: 1.,
        baseline: f64::from(baseline),
        ack: 0.,
        second: false,
        count: 1.,
        correction: None,
        summaries: None,
        cosmetics,
    }).unwrap();
    w.0
}

fn inspect_bytes(op: u32, bytes: &[u8], suffix: &[f64], bounds: &PositionBounds) -> codec::Result<Vec<f64>> {
    let mut input = vec![bytes.len() as f64];
    for chunk in bytes.chunks(8) {
        let mut word = [0; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        input.push(f64::from_ne_bytes(word));
    }
    input.extend_from_slice(suffix);
    let mut values = [0.; 256];
    let mut output = Output { values: &mut values, at: 0 };
    read::run(op, &mut Words::new(&input), &mut output, bounds)?;
    Ok(output.values[..output.at].to_vec())
}

#[test]
fn wide_capture_keeps_the_edges_and_small_deltas_in_the_receiver() {
    let bounds = space_bounds();
    let mut runtime = Runtime::with_bounds(bounds);
    runtime.reserve_world(0, 0).unwrap();
    let mut receiver = Session::new();
    receiver.reserve_receive();
    let strings = Strings::default();
    let edge = [-8000., 7999.9997, 8000.];
    let mut items = [
        WireRecord { kind: WireRecordKind::Item, id: 1., generation: 1., active: true,
            position: Some(edge), ..Default::default() },
        WireRecord { kind: WireRecordKind::Item, id: 2., generation: 1., active: true,
            ..Default::default() },
    ];

    for (tick, near_origin) in [(1, [0.001, -0.014, 0.020]), (2, [0.016, -0.044, 0.045])] {
        items[1].position = Some(near_origin);
        runtime.capture_canonical(f64::from(tick), 1., &items, std::iter::empty(), &strings).unwrap();
        let baseline = tick - 1;
        let parts = runtime.encode(f64::from(tick), 1., f64::from(baseline), 1.).unwrap();
        if baseline != 0 {
            // Header, empty world section, one slot and record flag, then the
            // wide position flag and just three signed differences.
            assert_eq!(parts[0].len(), 31);
            assert_eq!(parts[0][27], 2);
        }
        for part in parts {
            receiver.receive_with_bounds(part, &bounds).unwrap();
        }
        receiver.receive_with_bounds(&own_packet(tick, baseline, None), &bounds).unwrap();
        assert_eq!(receiver.header[net_header::ACK_TICK], f64::from(tick));
        for (id, position) in [(1., edge), (2., near_origin)] {
            let lease = receiver.lookup(net_kind::ITEM, id, &bounds).unwrap();
            let record = receiver.capture_record(lease);
            let decoded = serde_json::from_value(record["position"].clone()).unwrap();
            assert_position(decoded, position, bounds);
        }
    }
}

#[test]
fn wide_journal_compacts_in_box_bolts_and_keeps_outside_impacts_raw() {
    let bounds = space_bounds();
    let mut journal = Events::with_bounds(bounds);
    let mut exchange = cosmetics::Session::default();
    let pairs = [-8000., 16000., -8000., 16000., -8000., 16000.];
    let mut bundle = codec::Writer::default();
    let mut receiver = Session::new();
    receiver.reserve_receive();
    let cases = [
        (1., [7925.1234, -7900.4567, 0.0123], 3),
        (2., [-7999.9998, 7890.4567, -0.0065], 4),
        (2., [8100.7654, 0., 0.], 0),
    ];
    for (i, (schema, position, tag)) in cases.into_iter().enumerate() {
        let id = (i + 1) as f64;
        let payload = combat::bolt(&WbEncode {
            schema, id, generation: 1., tick: 1., expiry_tick: 2., launch_tick: 1.,
            execution_tick: 1., position_array: true, direction_array: true, offset_array: true,
            position: position.to_vec(), direction: vec![0., 0., 1.], offset: vec![0.; 3],
            life: 1., speed: 20., merge: 1., ..Default::default()
        }).unwrap();
        journal.emit_raw(id, 1., 1., schema, &payload, Default::default());
        let event = &journal.cosmetics()[i];
        assert_eq!(event.bytes[14], tag);

        exchange.input = vec![id, 1., 1., schema, 1.];
        exchange.input.extend(pairs);
        exchange.input.push(0.);
        exchange.payload = payload;
        exchange.run(0, &bounds).unwrap();
        assert_eq!(exchange.bytes, event.bytes);
        assert_eq!(exchange.run(0, &PositionBounds::LEGACY), Err("snapshot bounds do not match admitted map"));
        let inspected = inspect_bytes(net_op::COSMETIC, &event.bytes, &pairs, &bounds).unwrap();
        assert_eq!(&inspected[..4], &[id, 1., 1., schema]);
        bundle.u16(event.bytes.len() as f64).unwrap();
        bundle.raw(&event.bytes);
    }
    let inspected = inspect_bytes(net_op::COSMETICS, &bundle.0, &pairs, &bounds).unwrap();
    assert_eq!(inspected[0], cases.len() as f64);
    receiver.receive_with_bounds(&own_packet(1, 0, Some(&bundle.0)), &bounds).unwrap();
    let batch = receiver.capture_publication();
    let records = batch["records"].as_array().unwrap();
    assert_eq!(records.len(), cases.len());
    for (record, (schema, expected, tag)) in records.iter().zip(cases) {
        let payload = serde_json::from_value::<Vec<u8>>(record["payload"].clone()).unwrap();
        let mut view = cosmetics::Compact::default();
        cosmetics::decode_bolt_into(schema, 1., &payload, &mut view).unwrap();
        let actual = view.view.position.unwrap();
        if tag == 0 {
            assert_eq!(actual, expected);
        } else {
            assert_position(actual, expected, bounds);
        }
    }
}

#[test]
fn recorded_capture_encoding_and_inspection_share_the_admitted_box() {
    use super::module::{wire_capture_in, wire_create_in, wire_encode_in, wire_held_in};
    use crate::gen::{WireBounds, WireCapture, WireEncode, WireOutput};

    let bounds = space_bounds();
    let mut world = crate::world::owner::World::new();
    let runtime = wire_create_in(&mut world, &[-8000., -8000., -8000., 16000., 16000., 16000.]);
    assert_ne!(runtime, 0);
    let position = [7050., 1397., -524.];
    let mut capture = WireCapture { epoch: 1., tick: 1., records: vec![WireRecord {
        kind: WireRecordKind::Item, id: 1., position: Some(position), ..Default::default()
    }], ..Default::default() };
    let result = |world: &crate::world::owner::World| {
        let (row, heap) = world.sim.calls.args();
        WireOutput::take(row, heap)
    };
    world.sim.calls.result(WireCapture::WORDS, |row, heap| capture.put(row, heap));
    wire_capture_in(&mut world, runtime).unwrap();
    assert!(result(&world).error.is_some());

    capture.bounds = WireBounds { x_min: -8000., y_min: -8000., z_min: -8000.,
        x_extent: 16000., y_extent: 16000., z_extent: 16000. };
    world.sim.calls.result(WireCapture::WORDS, |row, heap| capture.put(row, heap));
    wire_capture_in(&mut world, runtime).unwrap();
    assert_eq!(result(&world).error, None);
    let current = world.wire.as_ref().unwrap().runtimes.get(runtime).unwrap().latest().unwrap().clone();
    let codes = bounds.encode(position);
    assert_eq!(current.slots[0].entry.pos, codes);
    wire_held_in(&mut world, runtime, 1., 1., 0., 1.).unwrap();
    let retained = result(&world);
    let encode = WireEncode { current, baseline: None, interval: 1. };
    world.sim.calls.result(WireEncode::WORDS, |row, heap| encode.put(row, heap));
    wire_encode_in(&mut world, 0).unwrap();
    let encoded = result(&world);
    assert_eq!(encoded.error, None);
    assert_eq!(encoded.parts, retained.parts);
    let inspected = inspect_bytes(net_op::ENTITIES, &encoded.parts[0].bytes, &[0.; 5], &bounds).unwrap();
    // The position group follows the header, world count, identity and group descriptor.
    assert_eq!(&inspected[19..25], &[0., 4., 0., codes[0], codes[1], codes[2]]);

    // Inspection receives the same box and the repeated X, Z, Y pairs.
    let mut words = vec![4., -8000., 16000., -8000., 16000., -8000., 16000.];
    words.extend(codes);
    words.extend([0.; 5]);
    let mut values = [0.; 16];
    let mut output = Output { values: &mut values, at: 0 };
    read::run(net_op::RECORD, &mut Words::new(&words), &mut output, &bounds).unwrap();
    assert_eq!(&output.values[..3], &bounds.decode(codes));
    assert_eq!(read::run(net_op::RECORD, &mut Words::new(&words), &mut output, &PositionBounds::LEGACY),
        Err("snapshot bounds do not match admitted map"));
}

#[test]
fn corrupt_narrow_deltas_wrap_in_the_baseline_and_ignore_reserved_mode_bits() {
    let bounds = PositionBounds::LEGACY;
    let strings = Strings::default();
    let mut runtime = Runtime::default();
    runtime.reserve_world(0, 0).unwrap();
    let mut receiver = Session::new();
    receiver.reserve_receive();
    let mut old_codes = [0_i32, 65535, 42];
    let mut item = WireRecord { kind: WireRecordKind::Item, id: 7., generation: 1., ..Default::default() };
    for tick in 1..=3u32 {
        let movement = f64::from(tick - 1);
        item.position = Some(bounds.decode([movement, 65535. - movement, 42.]));
        runtime.capture_canonical(f64::from(tick), 1., std::slice::from_ref(&item), std::iter::empty(), &strings).unwrap();
        let baseline = tick - 1;
        let mut packet = runtime.encode(f64::from(tick), 1., f64::from(baseline), 1.).unwrap()[0].clone();
        if tick == 1 {
            // Bits 1 through 6 of an absolute position were ignored.
            assert_eq!(packet[39], 1);
            packet[39] |= 0x7e;
        } else {
            assert_eq!(&packet[27..], &[0, 1, 255, 0]);
            let corrupt = if tick == 2 { [-10_i8, 127, -128] } else { [20, -127, 127] };
            packet[27] = 0xfe;
            for axis in 0..3 {
                packet[28 + axis] = corrupt[axis] as u8;
                old_codes[axis] = i32::from((old_codes[axis] + i32::from(corrupt[axis])) as u16);
            }
        }
        receiver.receive_with_bounds(&packet, &bounds).unwrap();
        receiver.receive_with_bounds(&own_packet(tick, baseline, None), &bounds).unwrap();
        assert_eq!(receiver.header[net_header::ACK_TICK], f64::from(tick));
        let held = receiver.capture(f64::from(tick));
        let actual: [f64; 3] = serde_json::from_value(held["snapshot"]["slots"][0][1]["pos"].clone()).unwrap();
        assert_eq!(actual, old_codes.map(f64::from));
    }
}

#[test]
fn asymmetric_wide_box_saturates_each_world_axis_at_its_own_faces() {
    use crate::gen::WireRecordState;
    use WireRecordKind as Kind;

    let bounds = PositionBounds::from_map_extents([-7936., -448., -2936.], [7936., 1088., 4936.]).unwrap();
    let lo = [-8000., -512., -3000.];
    let hi = [8000., 1152., 5000.];
    let fixtures = [
        (Kind::Soldier, net_kind::SOLDIER, [-8131., -207.125, 4950.5]),
        (Kind::Vehicle, net_kind::VEHICLE, [8153., 937.1, -134.25]),
        (Kind::Droideka, net_kind::DROIDEKA, [144.75, -617., -974.2]),
        (Kind::Remote, net_kind::REMOTE, [-525.4, 1413., 67.1]),
        (Kind::Item, net_kind::ITEM, [3101.5, -2.5, -3300.]),
        (Kind::Item, net_kind::ITEM, [-1024., 27.5, 5210.]),
    ];
    let rows: Vec<_> = fixtures.iter().enumerate().map(|(i, &(kind, _, position))| WireRecord {
        kind, id: (i + 1) as f64, generation: 1., position: Some(position), health: Some(100.),
        pitch: Some(0.), yaw: Some(0.), quaternion: Some([0., 0., 0., 1.]), active: true,
        state: WireRecordState::F64(0.), ..Default::default()
    }).collect();
    let mut runtime = Runtime::with_bounds(bounds);
    runtime.reserve_world(0, 0).unwrap();
    runtime.capture_canonical(1., 1., &rows, std::iter::empty(), &Strings::default()).unwrap();
    let mut receiver = Session::new();
    receiver.reserve_receive();
    for part in runtime.encode(1., 1., 0., 1.).unwrap() {
        receiver.receive_with_bounds(part, &bounds).unwrap();
    }
    receiver.receive_with_bounds(&own_packet(1, 0, None), &bounds).unwrap();
    for (i, (_, kind, position)) in fixtures.into_iter().enumerate() {
        let id = (i + 1) as f64;
        let lease = receiver.lookup(kind, id, &bounds).unwrap();
        let record = receiver.capture_record(lease);
        let decoded: [f64; 3] = serde_json::from_value(record["position"].clone()).unwrap();
        let entry = &runtime.latest().unwrap().slots.iter().find(|row| row.entry.entity_id == id).unwrap().entry;
        for axis in 0..3 {
            let clamped = position[axis].clamp(lo[axis], hi[axis]);
            let span = hi[axis] - lo[axis];
            let code = ((clamped - lo[axis]) / span * 16_777_215.).round();
            assert_eq!(entry.pos[axis], code);
            assert!((decoded[axis] - clamped).abs() <= span / 16_777_215. / 2.);
            if clamped != position[axis] { assert_eq!(decoded[axis], clamped); }
        }
    }
}

#[test]
fn wide_receiver_checks_absolute_and_delta_width_before_using_the_codes() {
    let bounds = space_bounds();
    let mut runtime = Runtime::with_bounds(bounds);
    runtime.reserve_world(0, 0).unwrap();
    let mut receiver = Session::new();
    receiver.reserve_receive();
    for tick in 1..=2u32 {
        let item = WireRecord { kind: WireRecordKind::Item, id: 1., generation: 1.,
            position: Some([-8000. + f64::from(tick - 1) * 0.01, 8000., 0.]), ..Default::default() };
        runtime.capture_canonical(f64::from(tick), 1., &[item], std::iter::empty(), &Strings::default()).unwrap();
        let baseline = tick - 1;
        let part = &runtime.encode(f64::from(tick), 1., f64::from(baseline), 1.).unwrap()[0];
        let mut corrupt = part.clone();
        let mode = if tick == 1 { 39 } else { 27 };
        assert_ne!(corrupt[mode] & 2, 0);
        corrupt[mode] &= !2;
        assert_eq!(receiver.receive_with_bounds(&corrupt, &bounds), Err("snapshot position width does not match map"));
        if tick == 2 {
            for (axis, diff) in [(0, -1_i8), (1, 1)] {
                corrupt.copy_from_slice(part);
                corrupt[mode + 1 + axis] = diff as u8;
                assert_eq!(receiver.receive_with_bounds(&corrupt, &bounds), Err("snapshot position delta out of range"));
            }
        }
        receiver.receive_with_bounds(part, &bounds).unwrap();
        receiver.receive_with_bounds(&own_packet(tick, baseline, None), &bounds).unwrap();
        assert_eq!(receiver.header[net_header::ACK_TICK], f64::from(tick));
    }
}
