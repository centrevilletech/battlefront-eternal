fn main() {
    napi_build::setup();
    // The staging helper binds the addon to the exact engine payload. Bare
    // cargo builds remain loadable for fixtures, but cannot pass a bundle.
    for name in ["BFE_ENGINE_BUILD", "BFE_SCHEMA_FINGERPRINT"] {
        println!("cargo:rerun-if-env-changed={name}");
        let value = std::env::var(name).unwrap_or_else(|_| "unbundled".into());
        println!("cargo:rustc-env={name}={value}");
    }
}
