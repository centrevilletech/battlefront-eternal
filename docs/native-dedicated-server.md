# Native Dedicated Server (`bfeserver`)

`bfeserver` (`rust/bfe-server`) is the standalone Rust dedicated game server replacing the Node `server-native` entrypoint (`server-native/src/cli.js`). It links `bfe-transport`, `bfe-content`, `bfe-sim`, and `bfe-online` into a single native binary with zero Node.js, Electron, or V8 runtime dependencies.

## Process & Thread Architecture

`bfeserver` separates real-time UDP transport I/O, authoritative 60 Hz world simulation, operator HTTP/SQLite administration, LAN discovery, and master-server signaling onto dedicated OS threads communicating over lock-free/bounded `crossbeam_channel` queues:

1. **UDP Transport Thread (`rust/bfe-server/src/udp_server.rs` — `UdpGameServer`)**
   - Binds the dual-stack/IPv4 UDP game socket and drives `bfe_transport::server::ServerEndpoint` (`src/net/endpoint.js` `UdpEndpoint` in server mode).
   - Performs 3-packet `BFE-UDP/1` handshakes (`INITIATION` -> `RESPONSE` -> `CONFIRM` -> `HANDSHAKE_DONE`), stateless HMAC `RETRY` cookie challenges under load, per-prefix handshake rate limiting (`MAX_BUCKETS = 4096`), and zero-allocation steady-state packet encryption/decryption.
2. **Dedicated 60 Hz World Thread (`rust/bfe-server/src/world_worker.rs` — `NativeWorldHost`, `rust/bfe-server/src/local_server.rs` — `LocalDedicatedServer::world_loop`)**
   - Owns the authoritative `bfe_sim::abi::context::Context` and runs the 60 Hz simulation and snapshot publication loop (`TICK_DURATION = 16,666,667` ns).
   - Receives `WorldCmd` messages (`Join`, `Leave`, `Start`, `Resync`, `Input`, `EdgeAck`, `PushRecordedPublication`) from the UDP transport thread and emits `WorldEvent::Publication` blocks back to the UDP thread for fanout.
3. **Operator SQLite & Admin HTTP Thread (`rust/bfe-server/src/operator.rs` — `OperatorService`)**
   - Uses `rusqlite` (`bundled` feature) to open or initialize `<stateDir>/operator.sqlite` with the exact `server-native/src/operator.js` schema (`bans`, `scores`, `audit` tables in `WAL` mode).
   - Serves `/healthz`, `/v1/status`, `/v1/players`, `/v1/bans`, `/v1/scores`, `/v1/audit`, `/v1/kick`, and `/v1/rotate`.
4. **LAN Discovery Responder (`rust/bfe-server/src/lan.rs` — `LanResponder`)**
   - Listens on UDP port `18452` (unicast + `ff02::1` IPv6 link-local multicast) for `BFE-LAN/1` queries and replies with server status.
5. **Master Server & WebSocket Signaling Client (`rust/bfe-server/src/online_server.rs` — `OnlineServer`)**
   - Registers with the master directory (`/v1/servers`), maintains the signaling WebSocket, verifies Ed25519-pinned global ban lists (`rust/bfe-server/src/bans.rs`), and schedules UDP hole-punch probes (`PUNCH_SCHEDULE_MS`).

## Why `WORLD_SLICE_MS = 4` ms Does Not Apply on the Dedicated 60 Hz World Thread

In the Node server (`server-native/src/game-session.js` `GameSession.drainSlice()`), `WORLD_SLICE_MS = 4` ms caps how long `drainSlice()` may spend advancing overdue 60 Hz ticks in a single `setImmediate` turn. That cooperative yield budget exists only because the Node world worker runs on a single-threaded JavaScript event loop where `MessagePort` IPC (`input`, `ack`, `start`, `resync`, `join`, `leave`) and timer callbacks share the same thread as `world.step()`; running multiple catch-up ticks uninterrupted for >4 ms would starve `MessagePort` delivery inside the worker.

In `bfeserver`, `NativeWorldHost` runs on its own dedicated OS thread (`bfe-world-worker` in `local_server.rs`) while `UdpGameServer` runs on a separate OS thread (`bfe-udp-server`). Incoming UDP datagrams, `BFE-UDP/1` handshakes, ACKs, and `BNI1`/`BNA1` frames are received and decrypted by `UdpGameServer` concurrently while `NativeWorldHost::step()` is executing, and are queued into the non-blocking `cmd_tx` channel drained at the top of every tick iteration. Yielding back to a user-space loop every 4 ms on a dedicated simulation thread would only add context-switch jitter without helping network I/O.

Late ticks on the dedicated 60 Hz world thread are bounded at three levels:

1. **Outer Catch-Up Horizon Clamp (`4 * TICK_DURATION` = `66.67` ms in `LocalDedicatedServer::world_loop`)**
   - Before each `NativeWorldHost::step()`, `world_loop` checks `Instant::now().saturating_duration_since(next_tick)`.
   - If the thread falls behind by more than `4 * TICK_DURATION` (`66,666,668` ns), `world_loop` clamps `next_tick` forward to `now - 4 * TICK_DURATION` (`dropped_ticks = (behind - 4 * TICK_DURATION) / TICK_DURATION + 1`) and records `dropped_ticks` into `NativeWorldHost::record_dropped_ticks()`, which increments `WireWorldFacts::DROPPED_TICKS` so clients observe the exact skipped tick count in `input-ack.dropped`.
   - Overdue ticks within the `<= 4` tick catch-up window execute back-to-back without sleeping (`Instant::now() >= next_tick`) until `next_tick` catches up to `Instant::now()`.
2. **Inner Native `WorldClock` Step Cap (`sim/src/world/clock.rs` `Clock::advance`)**
   - Inside `Context::world_step_run()`, the simulation's `WorldClock` enforces its configured `max_steps` per call (`steps > max_steps` increments `clock.dropped` and advances the accumulator past the unsimulated gap), guaranteeing a single `step()` call never loops unboundedly even if passed a large wall-clock `now_ms` jump.
3. **Publication Cadence & Per-Client Bandwidth/Staleness Bounds (`sim/src/wire/publish_module.rs` `Publisher`)**
   - `wire_publish` emits snapshots only on the configured `snapshot_stride` (`3` ticks = 20 Hz) and bounds per-client `BNS1` entity payloads via `wire_budget_run` (`max_entities`, byte budget, and baseline ACK staleness fallback), keeping per-tick serialization work bounded regardless of catch-up state.

## Host-Qualification Gates (`rust/bfe-server/tests/server_tests.rs`)

The host-qualification test suite ports `tools/host-qualification.mjs` (`lifecycle.mjs`, `service.mjs`, and `net-probe-shaping.mjs`) into `rust/bfe-server/tests/server_tests.rs`:

- `host_qualification_lifecycle_sequential_battles_isolation_and_stress_ticks`:
  - Runs 24 sequential battle lifecycles (join -> 120 ticks -> leave -> rejoin -> 120 ticks -> drop) and verifies `0` leaked `Context` handles (`Context::world_replay_live_handles()`).
  - Verifies concurrent world isolation across two simultaneous `NativeWorldHost` instances on separate threads.
  - Steps 12,000 consecutive ticks (200 simulated seconds at 60 Hz) with zero scratch buffer reallocation.
- `sixty_four_concurrent_clients_join_reconnect_and_stream_snapshots_within_memory_budget`:
  - Connects 64 concurrent `Endpoint` clients across two full connection/reconnect waves (with every 8th client withholding `BNA1` ACKs), verifying `welcome`, `input-ack`, `BNS1`, and `BNO1` delivery and confirming peak RSS (`~128.4 MiB` in a `1 GiB` Linux cgroup v2) stays well below the `1 GiB` ceiling.
- `host_qualification_network_shaping_jitter_loss_reorder_and_blackout_recovery`:
  - Drives 16 concurrent sessions under 5% packet loss, 50 ms +/- 15 ms jitter/reordering, and a 3.0-second 100% packet-loss blackout, verifying 100% session survival and post-blackout `Lane::Events` + `Lane::State` recovery.
