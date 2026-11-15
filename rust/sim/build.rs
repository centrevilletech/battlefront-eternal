// The store's generated code (`gen`), from rust/sim/schema, into OUT_DIR.
fn main() {
    let schema = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    println!("cargo:rerun-if-changed=schema");
    let source = sim_gen::store_source(&schema).unwrap_or_else(|e| panic!("store schemas: {e}"));
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("gen.rs");
    std::fs::write(out, source).unwrap();
}
