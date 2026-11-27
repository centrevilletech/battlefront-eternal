use super::Options;

// The content loader accepts empty binary assets here because these tests
// marshal definitions only; none installs collision geometry or posed rigs.
pub fn context_fixture() -> (bfe_content::files::MemFiles, Options) {
    let mut files = bfe_content::files::MemFiles::default();
    for (path, bytes) in [
        ("maps/synthetic/map.json", r#"{"mission":{"sides":["rep"],"teams":{"1":{"name":"rep","classes":[["rep_inf_a",1]]},"2":{"name":"cis","classes":[]}}}}"#),
        ("common/anims.json", r#"{"clips":{}}"#), ("common/common.json", "{}"), ("common/effects.json", "{}"),
        ("sides/rep/units.json", r#"{"units":{"rep_inf_a":{"base":"soldier","model":"body"}}}"#),
        ("sides/rep/weapons.json", "{}"), ("sides/rep/actors/body.json", r#"{"bones":[],"parts":[]}"#),
    ] { files.insert(path, bytes.as_bytes().to_vec()); }
    for path in ["sides/rep/actors/body.bin", "sides/rep/actors/body_index.bin", "common/anims.bin", "maps/synthetic/collision.bin",
        "maps/synthetic/collision_index.bin", "maps/synthetic/collision_poly.bin", "maps/synthetic/terrain_collision.bin",
        "maps/synthetic/terrain_collision_index.bin", "sides/rep/weapons.bin", "sides/rep/weapons_index.bin", "common/fxmodels.bin", "common/fxmodels_index.bin"] {
        files.insert(path, vec![]);
    }
    (files, Options { map: "synthetic".into(), side: "rep".into(), enhanced: false, seed: 1, heroes: false, unit: None, name: None, bots: Default::default(), ..Default::default() })
}
