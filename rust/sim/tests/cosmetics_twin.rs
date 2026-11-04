use bfe_sim::wire::cosmetics::Session;
use std::collections::BTreeMap;

struct Reader {
    data: Vec<u8>,
    at: usize,
}
impl Reader {
    fn u32(&mut self) -> u32 {
        let n = u32::from_le_bytes(self.data[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        n
    }
    fn bytes(&mut self) -> Vec<u8> {
        let n = self.u32() as usize;
        let out = self.data[self.at..self.at + n].to_vec();
        self.at += n;
        out
    }
    fn words(&mut self) -> Vec<f64> {
        let n = self.u32() as usize;
        let out = self.data[self.at..self.at + n * 8]
            .chunks_exact(8)
            .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
            .collect();
        self.at += n * 8;
        out
    }
}
#[test]
fn javascript_cosmetic_bytes_and_ordered_delivery_state() {
    let Ok(path) = std::env::var("BFE_COSMETICS_TWIN") else {
        eprintln!("BFE_COSMETICS_TWIN selects the generated cosmetic corpus");
        return;
    };
    let mut r = Reader {
        data: std::fs::read(path).unwrap(),
        at: 0,
    };
    assert_eq!(r.u32(), 0x31434642);
    let mut sessions = BTreeMap::<u32, Session>::new();
    let mut count = 0;
    while r.at < r.data.len() {
        let id = r.u32();
        let op = r.u32();
        let input = r.words();
        let payload = r.bytes();
        let error = r.bytes();
        let words = r.words();
        let bytes = r.bytes();
        let session = sessions.entry(id).or_default();
        session.input = input;
        session.payload = payload;
        let actual = session.run(op).err().unwrap_or("");
        assert_eq!(
            actual.as_bytes(),
            error,
            "case {count}, session {id}, operation {op}"
        );
        assert_eq!(
            session.bytes, bytes,
            "case {count}, session {id}, operation {op} bytes"
        );
        assert_eq!(
            session.words.len(),
            words.len(),
            "case {count}, session {id}, operation {op} word count"
        );
        for (i, (a, b)) in session.words.iter().zip(words).enumerate() {
            assert!(
                jsrt::same_value(*a, b),
                "case {count}, session {id}, operation {op} word {i}: {a:?} != {b:?}"
            );
        }
        count += 1;
    }
    assert!(count >= 5000, "incomplete cosmetic corpus: {count}");
    eprintln!("{count} native cosmetic operations match JS-verified bytes and state");
}
