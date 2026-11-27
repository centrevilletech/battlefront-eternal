//! Ignored regression check for 2004 gameplay fingerprints and released
//! visual/placement behavior against prepared content (`BFE_CONTENT_DIR`).
//! Kept separate from synthetic unit tests so default `cargo test` runs with
//! zero game files.
use bfe_content::files::DirFiles;
use bfe_content_native::{
    fingerprint,
    loader::{Blend, Loader},
    placed, GameFamily,
};
use std::collections::BTreeMap;
use std::path::Path;

#[test]
#[ignore]
fn swbf1_fingerprints() {
    let root = std::env::var("BFE_CONTENT_DIR").expect("BFE_CONTENT_DIR");
    let files = DirFiles::new(&root, bfe_family::GameFamily::Swbf1);
    let selections = [
        "?map=nab2&side=rep",
        "?map=tat2&side=all",
        "?map=hot1&side=imp",
        "?map=geo1&side=cis",
    ];
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(path) = std::env::var("BFE_FINGERPRINT_BASELINE") {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read BFE_FINGERPRINT_BASELINE {path}: {e}"));
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 3 && parts[0] == "fingerprint" {
                expected.insert(parts[1].to_string(), parts[2].to_string());
            }
        }
    }
    let loader = Loader::detect(Path::new(&root));
    for query in selections {
        let digest_a = fingerprint::hash(&files, query)
            .unwrap_or_else(|e| panic!("fingerprint failed for {query}: {e}"));
        let digest_b = fingerprint::hash(&files, query)
            .unwrap_or_else(|e| panic!("repeat fingerprint failed for {query}: {e}"));
        let digest_family = fingerprint::hash_for_family(&files, GameFamily::Swbf1, query)
            .unwrap_or_else(|e| panic!("family fingerprint failed for {query}: {e}"));
        assert_eq!(digest_a, digest_b, "non-deterministic fingerprint for {query}");
        assert_eq!(digest_a, digest_family, "Swbf1 family fingerprint drift for {query}");
        assert!(
            digest_a.starts_with("bfe-content-sha256-") && digest_a.len() == 19 + 64,
            "unexpected fingerprint format for {query}: {digest_a}"
        );
        if let Some(want) = expected.get(query) {
            assert_eq!(&digest_a, want, "2004 fingerprint drift for {query}");
        }
        println!("fingerprint {query} {digest_a}");
    }

    // Verify Loader::load_for_family(Swbf1, ...) matches Loader::load(...) exactly on nab2.
    let direct = loader.load("nab2", "?map=nab2&side=rep").unwrap();
    let family = loader
        .load_for_family(GameFamily::Swbf1, "nab2", "?map=nab2&side=rep")
        .unwrap();
    assert_eq!(direct.world.len(), family.world.len());
    assert_eq!(direct.models.len(), family.models.len());
    assert_eq!(direct.actors.len(), family.actors.len());
    assert_eq!(direct.units.len(), family.units.len());
    assert_eq!(direct.world[0].material.blend, family.world[0].material.blend);
    assert_eq!(direct.world[0].material.flags, family.world[0].material.flags);
    assert_eq!(
        direct.models.iter().filter(|m| m.material.bump.is_some()).count(),
        0,
        "2004 prop models must not populate material.bump"
    );
    let p_direct = placed::admit(&direct).unwrap();
    let p_family = placed::admit(&family).unwrap();
    assert_eq!(p_direct.parts.len(), p_family.parts.len());
    assert_eq!(p_direct.deferred, p_family.deferred);
    assert_eq!(direct.world[0].material.blend, Blend::Opaque);
}
