//! The released 2004 receiver's behaviour at the edge of its field bank, kept
//! exactly: 170 keyframes of 192 fields of 139 bytes (28 parts each, one own
//! datagram, no loss) fill the bank, and the released receiver completes 85
//! of them, misses from the 86th on and stays at tick 255. Only an admitted
//! 2005 match's receiver retires old baselines to make room and completes
//! every one.
use bfe_wide::Wide;
use bfe_presentation::{network::Session, network_layout::net_header as h};
use bfe_sim::gen::{WireField, WireSnapshot};
use bfe_sim::wire::{buffered::Encoder, codec::{own_datagram_into, OwnParts, Writer}, PositionBounds};

fn stream(swbf2: bool) -> (u32, Option<u32>, f64, u64, usize, Wide) {
    let mut encoder = Encoder::new(192);
    let mut receiver = Session::new();
    receiver.reserve_receive();
    if swbf2 { receiver.admit_swbf2(); }
    let (mut complete, mut first_miss, mut parts, mut digest) = (0, None, 0, Wide::ZERO);
    let bounds = [-2048., 4096., -2048., 4096., -256., 1024.];
    for i in 1..=170u32 {
        let tick = f64::from(i * 3);
        let world = (0..192u32).map(|j| WireField { schema: 7., id: f64::from(90_000 + j), generation: 1.,
            payload: (0..139u32).map(|k| (i + j + k) as u8).collect() }).collect();
        let snapshot = WireSnapshot { tick, epoch: 1., slots: vec![], world };
        let packets = encoder.encode(&snapshot, None, 3., PositionBounds::LEGACY.width()).unwrap();
        parts = packets.len();
        for packet in packets {
            for &b in packet { digest = digest.rotate_left(9).xor(Wide::from_u32(u32::from(b))); }
            receiver.receive(packet, &bounds).unwrap();
        }
        let mut own = Writer::default();
        own_datagram_into(&mut own, OwnParts { epoch: 1., tick, interval: 3., baseline: 0., ack: 0., second: false, count: 1.,
            correction: None, summaries: None, cosmetics: None }).unwrap();
        receiver.receive(&own.0, &bounds).unwrap();
        if receiver.header[h::ACK_TICK] == tick && receiver.header[h::COMPLETE] != 0. { complete += 1; } else { first_miss.get_or_insert(i); }
    }
    (complete, first_miss, receiver.header[h::ACK_TICK], receiver.missed, parts, digest)
}

#[test]
fn a_2004_receiver_fills_its_bank_as_released() {
    // The released build's figures for the same stream.
    assert_eq!(stream(false), (85, Some(86), 255., 2371, 28, Wide::new(0xa4bc_c06c, 0xb795_0452)));
}

#[test]
fn a_2005_receiver_retires_old_baselines_instead() {
    let (complete, first_miss, ack, missed, parts, _) = stream(true);
    assert_eq!((complete, first_miss, ack, missed, parts), (170, None, 510., 0, 28));
}
