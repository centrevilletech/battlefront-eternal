# bfe-client

The game: one window and OpenGL context (`winit`, `glutin`), one sound
stream (`cpal`), the front end, Instant Action on a local 60 Hz world
thread, and online play against `bfeserver`. See the
[native client guide](../../docs/native-client.md) and the
[architecture overview](../../docs/architecture.md).

```bash
cd rust
cargo run --release --locked -p bfe-client              # the game
cargo run --release --locked -p bfe-client -- --help    # its options
```

- `src/app/`: the window, the state machine (startup, front end, loading,
  battle), the Slint screens' models, settings, first run and scripted runs.
- `src/local/`: the local world thread that runs single player battles.
- `src/online/`: the receiver, replicas, prediction and presentation that
  every battle (local or online) draws through, and the online session.
- `src/scene.rs`, `src/render_bridge.rs` and the `*_host.rs` files: the
  battle's drawing over `bfe-render`.
- `src/audio.rs`, `src/sound.rs`: the sound device and the battle's sound.
- `src/bin/render-smoke.rs`: the software-OpenGL renderer smoke check ([renderer-smoke.md](../../docs/renderer-smoke.md)).
