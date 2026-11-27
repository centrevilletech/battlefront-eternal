//! A crowded 2005 battle's shared snapshots, sent loss-free to a freshly
//! reserved receiver that always acknowledges its last completed snapshot, with
//! a keyframe every 40 snapshots. Every snapshot must complete, including when
//! every world field changes in every snapshot: a full field bank retires old
//! baselines instead of stalling.
mod crowded;
use bfe_presentation::{network::Session, network_layout::net_header as h};
use bfe_sim::gen::WireSnapshot;
use bfe_sim::wire::{bounds::Capacity, buffered::Encoder, codec::{own_datagram_into, OwnParts, Writer}};

fn stream(fields: usize, changing: bool) -> (u32, u64, u32) {
    let bounds = crowded::bounds();
    let mut encoder = Encoder::with_capacity(512, Capacity::FULL);
    let mut receiver = Session::new();
    receiver.reserve_receive();
    receiver.admit_swbf2();
    let mut acknowledged: Option<WireSnapshot> = None;
    let mut complete = 0;
    for i in 1..=170u32 {
        let snapshot = crowded::snapshot(bounds, i, fields, changing);
        let tick = snapshot.tick;
        let base = if i % 40 == 0 { None } else { acknowledged.as_ref() };
        let shared = encoder.encode(&snapshot, base, 3., bounds.width()).unwrap();
        let mut own = Writer::default();
        own_datagram_into(&mut own, OwnParts { epoch: 1., tick, interval: 3., baseline: base.map_or(0., |b| b.tick), ack: 0.,
            second: false, count: 1., correction: None, summaries: None, cosmetics: None }).unwrap();
        for packet in shared.iter().map(Vec::as_slice).chain(std::iter::once(own.0.as_slice())) {
            receiver.receive_with_bounds(packet, &bounds).unwrap();
        }
        if receiver.header[h::ACK_TICK] == tick && receiver.header[h::COMPLETE] != 0. {
            complete += 1;
            acknowledged = Some(snapshot);
        }
    }
    (complete, receiver.missed, receiver.retired)
}

#[test]
fn a_crowded_loss_free_stream_never_stalls() {
    // 600 fields take more than the released 32 parts a keyframe: a 2005
    // battle's snapshots may take up to 64, and every receiver reads them.
    for (fields, changing) in [(64, false), (286, false), (286, true), (600, true)] {
        let (complete, missed, retired) = stream(fields, changing);
        println!("crowded receiver fields={fields} changing={changing} complete={complete}/170 missed={missed} retired={retired}");
        assert_eq!((complete, missed), (170, 0), "fields={fields} changing={changing}");
    }
}
