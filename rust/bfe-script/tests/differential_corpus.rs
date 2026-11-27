//! Differential corpus: 240 seeded standalone Lua 5.0 programs and fixed cases written
//! by `tests/lua/corpus/generate.py` (sources committed beside it), compiled by a
//! reference Lua 5.0.3 `luac` with the 2005 game's chunk settings and packed with the
//! reference interpreter's stdout by `tests/lua/corpus/pack.py`, which also records a
//! hash of each source. Every program must print exactly the reference output, and
//! every committed source must be the one that was packed.

use bfe_wide::{Fnv, Wide};
use std::collections::BTreeMap;

#[path = "common/execution_records.rs"]
mod execution_records;

/// Programs in the corpus: 240 seeded and the fixed cases.
const PROGRAMS: usize = 241;

use bfe_script::Vm;

fn corpus_dir() -> String {
    format!("{}/tests/lua/corpus", env!("CARGO_MANIFEST_DIR"))
}

fn read_u32(data: &[u8], pos: &mut usize) -> usize {
    let v = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap()) as usize;
    *pos += 4;
    v
}

/// FNV-1a over `data` (as `pack.py` computes it).
fn fnv1a64(data: &[u8]) -> Wide {
    let mut h = Fnv::new();
    h.bytes(data);
    h.finish()
}

struct Case {
    name: String,
    luac: Vec<u8>,
    expected: Vec<u8>,
    source_hash: Wide,
}

fn load_corpus() -> Vec<Case> {
    let path = format!("{}/corpus.bin", corpus_dir());
    let data =
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read corpus {path}: {e}"));
    assert!(data.len() >= 8 && &data[..4] == b"BFE2", "bad corpus magic");
    let mut pos = 4usize;
    let count = read_u32(&data, &mut pos);
    let mut cases = Vec::with_capacity(count);
    for _ in 0..count {
        let nlen = read_u32(&data, &mut pos);
        let clen = read_u32(&data, &mut pos);
        let elen = read_u32(&data, &mut pos);
        let source_hash = Wide::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
        pos += 8;
        let name = String::from_utf8(data[pos..pos + nlen].to_vec()).unwrap();
        pos += nlen;
        let luac = data[pos..pos + clen].to_vec();
        pos += clen;
        let expected = data[pos..pos + elen].to_vec();
        pos += elen;
        cases.push(Case {
            name,
            luac,
            expected,
            source_hash,
        });
    }
    assert_eq!(pos, data.len(), "trailing bytes in differential corpus");
    cases
}

/// The bundle holds exactly the committed sources, each as it was packed: a source
/// edited (or added, or removed) without repacking fails here.
#[test]
fn corpus_bundle_matches_the_committed_sources() {
    let packed: BTreeMap<String, Wide> = load_corpus()
        .into_iter()
        .map(|c| (c.name, c.source_hash))
        .collect();
    let sources: BTreeMap<String, Wide> = std::fs::read_dir(corpus_dir())
        .unwrap()
        .filter_map(|e| {
            let entry = e.unwrap();
            let name = entry.file_name().into_string().unwrap();
            let stem = name.strip_suffix(".lua")?;
            stem.starts_with('c').then(|| {
                let text = std::fs::read(entry.path()).unwrap();
                (stem.to_string(), fnv1a64(&text))
            })
        })
        .collect();
    assert_eq!(
        packed, sources,
        "corpus.bin must be packed from exactly the committed sources, as committed"
    );
}

#[test]
fn differential_corpus_matches_reference_lua503_byte_for_byte() {
    let cases = load_corpus();
    assert_eq!(cases.len(), PROGRAMS);

    let mut failures = Vec::new();
    for Case {
        name,
        luac,
        expected,
        ..
    } in &cases
    {
        let mut vm: Vm = Vm::new_with_stdlib();
        vm.capture_output();
        if let Err(err) = vm.run_chunk(&mut (), luac) {
            failures.push(format!("{name}: VM error: {err}"));
            continue;
        }
        let actual = vm.take_output();
        if actual != *expected {
            failures.push(format!(
                "{name}:\n  expected: {:?}\n  actual:   {:?}",
                String::from_utf8_lossy(expected),
                String::from_utf8_lossy(&actual)
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} differential corpus programs failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
