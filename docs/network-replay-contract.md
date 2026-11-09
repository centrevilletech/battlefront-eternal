# Authoritative Network Replay Contract

Deterministic simulation replay requires the authoritative server and client
reconciliation loop to process player inputs in a reproducible order, apply
snapshots at a well-defined tick boundary, and isolate non-deterministic
presentation work from authoritative state.

---

## 1. Input Ordering & Tick Assignment

1. **Authoritative Tick Clock**
   - The server advances an integer `simTick` at a fixed timestep (`SIM_DT = 1/60s`).
   - Every client input command (`ClientInputPacket`) carries:
     - `clientTick`: the client's local prediction tick when the input was sampled.
     - `ackSnapshotTick`: the last authoritative snapshot tick the client had received.
     - `seq`: monotonically increasing per-client input sequence number.
     - Quantized control state: `moveX`, `moveZ`, `aimYaw`, `aimPitch`, `buttons` bitmask (`JUMP`, `CROUCH`, `PRONE`, `SPRINT`, `FIRE`, `SECONDARY`, `RELOAD`, `USE`, `SWITCH_WEAPON`, `ENTER_VEHICLE`, `SPOT`), `selectedWeaponSlot`, `requestedSpawnPointId`, `requestedClassIndex`.

2. **Server Input Scheduling**
   - Incoming client inputs are queued per slot (`0 .. maxPlayers - 1`) in `rust/sim` (`world::owner`).
   - At the start of authoritative `simTick`, for each active human slot:
     - If an input packet scheduled for `simTick` is available, consume it and record its `seq` as `lastProcessedInputSeq[slot]`.
     - If multiple packets arrived late, consume in ascending `seq` order up to the current tick window; discard packets older than `lastProcessedInputSeq[slot]`.
     - If no new packet arrived for `simTick` (jitter/drop up to `MAX_INPUT_HOLD_TICKS = 3`), repeat the last movement/orientation command with edge-triggered action bits (`FIRE_PRESSED`, `RELOAD`, `USE`, `ENTER_VEHICLE`, `JUMP_PRESSED`) cleared.
   - **Canonical Processing Order**: Player slots are stepped in ascending `slotIndex` order (`0 .. maxPlayers - 1`) before AI agents (`0 .. aiCount - 1`), vehicles (`0 .. vehicleCount - 1`), projectiles (`0 .. projectileCount - 1`), and command posts (`0 .. cpCount - 1`).

---

## 2. Authoritative Snapshot Schema (`WorldSnapshot`)

Every authoritative snapshot emitted at `simTick` contains:

- `simTick`: `u32` authoritative simulation tick.
- `baselineTick`: `u32` reference tick (`0` for full snapshot, `> 0` for delta snapshot).
- `stateHash`: `u32` FNV-1a deterministic state checksum over quantized authoritative fields.
- `matchState`:
  - `phase`: `u8` (`WARMUP`, `ACTIVE`, `VICTORY`, `DEFEAT`)
  - `tickets`: `[u16, u16]`
  - `bleedTimer`: `u16` (millisecond/centisecond quantized)
- `commandPosts[]` (ordered by `cpId`):
  - `ownerTeam`: `i8`
  - `controlProgress`: `i16` (quantized `[-1000, 1000]`)
  - `capturingTeam`: `i8`
- `players[]` (ordered by `slotIndex`):
  - `slotIndex`: `u8`, `team`: `u8`, `classIndex`: `u8`, `alive`: `bool`
  - `lastProcessedInputSeq`: `u32`
  - `pos`: `[i32, i32, i32]` (millimeter quantized `round(v * 1000)`)
  - `vel`: `[i16, i16, i16]` (cm/s quantized `round(v * 100)`)
  - `yaw`: `i16`, `pitch`: `i16` (0.001 rad quantized)
  - `stance`: `u8`, `jetpackFuel`: `u16`
  - `hp`: `u16`, `shield`: `u16`
  - `vehicleId`: `i16`, `seatIndex`: `i8`
  - `weaponSlot`: `u8`, `ammoInClip`: `u16`, `ammoReserve`: `u16`, `heat`: `u16`, `reloadingTimer`: `u16`
- `vehicles[]` (ordered by `vehicleId`):
  - `vehicleId`: `u16`, `alive`: `bool`, `hp`: `u16`, `respawnTimer`: `u16`
  - `pos`: `[i32, i32, i32]`, `quat`: `[i16, i16, i16, i16]`, `vel`: `[i16, i16, i16]`
  - `aimYaw`: `i16`, `aimPitch`: `i16`
  - `occupants`: `[i8; MAX_SEATS]`
- `projectiles[]` (active authoritative ordnance, ordered by `netId`):
  - `netId`: `u32`, `typeId`: `u8`, `ownerSlot`: `i16`, `team`: `u8`
  - `pos`: `[i32, i32, i32]`, `vel`: `[i16, i16, i16]`, `ttl`: `u16`
- `events[]` (reliable combat/gameplay events emitted since `baselineTick`):
  - `HIT`, `KILL`, `EXPLOSION`, `VEHICLE_DESTROYED`, `CP_CAPTURED`, `SPAWN`, `PICKUP`

---

## 3. Client Prediction & Rollback Reconciliation

1. **Ring Buffer**:
   - Client maintains a circular buffer of the last `128` predicted ticks (`{ tick, inputSeq, input, predictedPlayerState }`) in `rust/sim` (`client` and `prediction`).

2. **Snapshot Application & Rollback**:
   - Upon receiving `WorldSnapshot` for `serverTick`:
     1. Apply authoritative remote entities (`commandPosts`, remote `players`, `vehicles`, `projectiles`, `matchState`).
     2. Look up local player's predicted state at `lastProcessedInputSeq`.
     3. Compare authoritative local player position/velocity/stance/state against the predicted record at `lastProcessedInputSeq`.
     4. If position error `> 0.01m` (`10mm`), velocity error `> 0.05m/s`, or discrete state (`stance`, `alive`, `vehicleId`, `weaponSlot`) differs:
        - Snap local player simulation state to authoritative state at `serverTick`.
        - Re-simulate all buffered local inputs with `seq > lastProcessedInputSeq` through the shared movement & weapon step function (`rust/sim`).
        - Accumulate visual smoothing offset (`visualErrorOffset`) so the camera/mesh interpolates smoothly over `50–100ms` without affecting simulation coordinates.

---

## 4. Replay Log & Verification Harness

A deterministic network replay log (`NetworkReplayLog`) records:
- `seed`: `u32` PRNG seed
- `mapId`: `string`
- `tickRate`: `60`
- `inputsByTick`: array of `{ tick, slotIndex, seq, input }`
- `checkpoints`: periodic `{ tick, stateHash }`

**Invariant**: Running the authoritative world loop twice with the same `NetworkReplayLog`, or running a client reconciliation loop against the emitted snapshots, must produce identical `stateHash` values at every tick (`cargo test --locked -p sim`).
