//! Native construction against the installed MeshBVH's packed root words
//! and indirect order, including the 16-bit boundary and depth limit.
use bfe_content::{
    bvh::{build, Positions},
    json, Value,
};
use std::{path::Path, process::Command};

fn get<'a>(value: &'a Value, name: &str) -> &'a Value {
    value.get(name).unwrap()
}
fn bytes(value: &Value) -> Vec<u8> {
    let hex = value.as_str().unwrap();
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}
fn words(value: &Value) -> Vec<u32> {
    bytes(value)
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

#[test]
fn native_builder_matches_javascript_root_bytes_and_indirect_order() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let oracle = Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
        .arg(root.join("tools/bvh-build-twin.mjs"))
        .current_dir(&root)
        .output()
        .expect("run BVH oracle");
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let mut count = 0;
    for line in std::str::from_utf8(&oracle.stdout).unwrap().lines() {
        let value = json::parse(line).unwrap();
        let label = get(&value, "label").as_str().unwrap();
        let source = bytes(get(&value, "positions"));
        let index = words(get(&value, "indices"));
        let tree = if get(&value, "wide").as_bool().unwrap() {
            let positions = source
                .chunks_exact(8)
                .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>();
            build(Positions::F64(&positions), &index)
        } else {
            let positions = source
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>();
            build(Positions::F32(&positions), &index)
        };
        let expected = get(&value, "roots")
            .as_array()
            .unwrap()
            .iter()
            .map(words)
            .collect::<Vec<_>>();
        assert_eq!(tree.roots, expected, "{label}: root words");
        assert_eq!(
            tree.indirect,
            words(get(&value, "indirect")),
            "{label}: indirect"
        );
        count += 1;
    }
    assert_eq!(count, 107);
    eprintln!("BVH: {count} native packed-tree cases identical to JavaScript");
}
