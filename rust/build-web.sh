#!/bin/sh
# Builds the Rust battle renderer (bfe-render) for the page into src/rust-pkg,
# which src/rust-render/index.js loads. Needs the wasm32-unknown-unknown
# target and wasm-bindgen-cli 0.2.130 (the version Cargo.toml pins).
set -e
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown -p bfe-render
wasm-bindgen --target web --no-typescript --out-dir ../src/rust-pkg target/wasm32-unknown-unknown/release/bfe_render.wasm
