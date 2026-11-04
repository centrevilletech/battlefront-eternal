use bfe_sim::gen::{WireDelta, WireEncode, WireOwn, WireWorld};
use bfe_sim::wire::codec;
#[test]
fn javascript_snapshot_bytes() {
    let Ok(path) = std::env::var("BFE_WIRE_TWIN") else {
        eprintln!("BFE_WIRE_TWIN selects the generated wire twin corpus");
        return;
    };
    let data = std::fs::read(path).unwrap();
    let mut at = 0;
    let mut count = 0;
    fn u32(data: &[u8], at: &mut usize) -> usize {
        let n = u32::from_le_bytes(data[*at..*at + 4].try_into().unwrap()) as usize;
        *at += 4;
        n
    }
    while at < data.len() {
        let op = u32(&data, &mut at);
        let words = u32(&data, &mut at);
        let block: Vec<f64> = data[at..at + words * 8]
            .chunks_exact(8)
            .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
            .collect();
        at += words * 8;
        let row = &block[5..block[2] as usize];
        let heap = &block[block[2] as usize..];
        let actual = match op {
            0 => {
                let a = WireEncode::take(row, heap);
                codec::encode_entities(&a.current, a.baseline.as_ref(), a.interval).unwrap()
            }
            1 => {
                let a = WireDelta::take(row, heap);
                vec![codec::word_delta(&a.current, a.baseline.as_deref()).unwrap()]
            }
            2 => {
                let a = WireWorld::take(row, heap);
                vec![
                    codec::world_section(&a.current, a.baseline.as_deref().unwrap_or(&[])).unwrap(),
                ]
            }
            3 => {
                let a = WireOwn::take(row, heap);
                vec![codec::own_datagram(&a).unwrap()]
            }
            _ => panic!("unknown wire case"),
        };
        let parts = u32(&data, &mut at);
        assert_eq!(actual.len(), parts, "case {count} op {op}");
        for part in actual {
            let len = u32(&data, &mut at);
            assert_eq!(part, &data[at..at + len], "case {count} op {op}");
            at += len;
        }
        count += 1;
    }
    assert!(count >= 2450, "incomplete wire corpus: {count}");
    eprintln!("{count} native snapshot cases match JavaScript bytes");
}
#[test]
fn word_delta_threshold_padding_and_absolute_limit() {
    let baseline = vec![0; 25];
    let mut value = baseline.clone();
    value[24] = 9;
    assert_eq!(
        codec::word_delta(&value, Some(&baseline)).unwrap(),
        [1, 25, 0, 4, 0, 8, 9, 0, 0, 0, 0, 0, 0, 0]
    );
    assert!(codec::word_delta(&vec![0; 65536], None).is_err());
    assert_eq!(codec::word_delta(&[], Some(&[])).unwrap(), [0, 0, 0]);
}
