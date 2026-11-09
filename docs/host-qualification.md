# Host qualification

`bfeserver` (`rust/bfe-server`) is the native dedicated server binary. It runs
the authoritative 60 Hz world simulation (`rust/sim`), the encrypted UDP
transport (`rust/bfe-transport`), master server registration and signaling
(`rust/bfe-online`), and local operator administration without a display server
or graphics context.

## Building and verifying `bfeserver`

Build and test the native server crate and its host qualification suite from
`rust/`:

```sh
cargo build --manifest-path rust/Cargo.toml --release --locked --bin bfeserver
cargo test --manifest-path rust/Cargo.toml --locked -p bfe-server
```

## Qualification gates

Server qualification exercises `bfeserver` across four areas:

1. **Lifecycle and battle rotation:** Repeat sequential and rejoin battles across
   `nab2/rep`, `tat1/rep`, `bes1/rep`, and `hot1/imp`, plus a 12,000-tick
   tick-driven stress battle, verifying clean world disposal, zero live handle
   leaks across rotations, and bounded RSS growth.
2. **Service capacity and cgroup bounds:** Run 32 and 64 authenticated synthetic
   clients in one battle, and concurrent server instances under a disposable
   1 GiB cgroup v2 (`python3 tools/host-qualification/cgroup-selftest.py`),
   keeping synthetic clients outside the service cgroup so client memory never
   masks server admission or keepalive behavior. Rotation retains the server
   identity and UDP endpoint; slow clients retain transport while withholding
   snapshot ACKs to verify bounded send queues.
3. **Network impairment and recovery:** Inside a disposable Linux network
   namespace (`ip`/`tc`), apply 50 +/- 15 ms delay and 5% kernel loss in each
   direction, followed by a 3-second total blackout and recovery, verifying
   authenticated reconnects and continuous 60 Hz authority advancement.
4. **Exact-tree parity:** Run `cargo xtask check` and `cargo xtask matrix`
   against the frozen 61-cell reference hashes (`tests/fixtures/world-hashes.json`)
   and frozen publication cases (`tests/fixtures/f6-publications.jsonl`).

## Acceptance matrix

| Job | Required observations |
| --- | --- |
| Q1-1..3 | Disjoint standing manifest shards; 61 cases and 189,000 ticks total, three exact frozen hashes (`stateHash`, `deepDigestHash`, `publicationHash`) |
| Q2 | All frozen publication JSONL cases, exact bytes and output order, ACK/edge/reliable/fault fields; boundary and baseline fixtures |
| Q3-map | Four maps, 24 sequential/rejoin battles each, two-owner isolation, 12,000 stress ticks, wall soak, double close, clean shutdown |
| Q4 | 32 clients in one battle, then 64 each in one/two/four battles, real UDP, rotation, fresh-receiver reconnect and held ACKs inside the 1 GiB service cgroup |
| Q5 | Four maps, fresh pinned process per role, 3,000 warm and 6,000 measured ticks; step and publication microsecond budgets and zero steady-state allocations |
| Q6-map | Authenticated `bfeserver`, four `bfe-client` peers, 60 seconds, 50 +/- 15 ms each direction, 5% loss, 3-second blackout, reconnect |
| Q7 | Desktop `HostController` (`rust/bfe-desktop`) spawns detached `bfeserver`, accepts authenticated clients, and performs clean keep-running / attach / stop lifecycle |
| Q8 | Protocol, simulation build and content-hash mismatch rejection before session or spawn admission |
| Q9 | Full workspace test suite (`cargo test --locked`) and packaged server archive check (`build/native/check-packages.py`) |
