# Snapshot & Delta Encoding Design (`rust/sim`)

## 1. Overview & Bandwidth Targets

The authoritative simulation runs at 60 Hz (`SIM_DT = 1/60s`) and broadcasts snapshots at 20–30 Hz (every 2–3 ticks).
For a 32-player match with 64 AI bots, 12 vehicles, 8 command posts, and active ordnance:
- **Uncompressed JSON full snapshot**: ~35–50 KB/snapshot (700–1500 KB/s per client; unviable for 32 clients).
- **Quantized Delta Snapshot (`rust/sim`, `wire` module)**:
  - Steady-state delta snapshot (20 Hz): **~800 – 1,800 bytes** (~16–36 KB/s per client).
  - Full keyframe snapshot (on join / baseline reset): **~4,500 – 6,500 bytes**.

---

## 2. Quantization Rules

All authoritative state fields are quantized to fixed-point integers before hashing and serialization so that:
1. `stateHash` is 100% deterministic across architectures and matches between encoder and decoder.
2. Small floating-point jitter below perceptual thresholds does not trigger unnecessary entity delta updates.

| Field Category | Precision | Quantization Formula | Wire Type |
|---|---|---|---|
| Position (`x, y, z`) | `1 mm` (`0.001 m`) | `Math.round(v * 1000)` | `i32` |
| Velocity (`vx, vy, vz`) | `1 cm/s` (`0.01 m/s`) | `clamp(Math.round(v * 100), -32768, 32767)` | `i16` |
| Angles (`yaw, pitch`) | `0.001 rad` (~`0.057°`) | `clamp(Math.round(v * 1000), -32768, 32767)` | `i16` |
| Unit Quaternion (`qx, qy, qz, qw`) | `1 / 32767` | `clamp(Math.round(v * 32767), -32767, 32767)` | `i16` |
| Normalized Progress (`controlProgress`) | `0.001` (`0.1%`) | `clamp(Math.round(v * 1000), -1000, 1000)` | `i16` |
| Health / Shield / Fuel / Heat | `0.1` units | `clamp(Math.round(v * 10), 0, 65535)` | `u16` |
| Timers (`bleed`, `reload`, `respawn`) | `1 ms` | `clamp(Math.round(v * 1000), 0, 65535)` | `u16` |

---

## 3. Baseline & Delta Compression Protocol

1. **Per-Client Baseline Tracking (`BaselineTracker` in `rust/sim`)**:
   - The server stores a ring buffer of recent quantized `WorldSnapshot` keyframes (e.g., last 32 emitted snapshots, covering ~1.0–1.6 seconds at 20–30 Hz).
   - Each `ClientInputPacket` carries `ackSnapshotTick`: the latest authoritative `simTick` the client has received and applied.
   - When encoding a packet for client $C$ at `simTick`:
     - Look up `baseline = ringBuffer.get(C.ackSnapshotTick)`.
     - If found, encode a **Delta Snapshot** (`baselineTick = C.ackSnapshotTick`) containing only entities that were added, removed, or mutated relative to `baseline`.
     - If `C.ackSnapshotTick == 0` or the baseline has aged out of the ring buffer, encode a **Full Snapshot** (`baselineTick = 0`).

2. **Entity-Level & Field-Mask Delta Encoding**:
   - **Match State**: Included only if `phase`, `tickets[0]`, `tickets[1]`, or `bleedTimer` changed since `baseline`.
   - **Command Posts**: Sparse array of `[cpId, ownerTeam, capturingTeam, controlProgress]` for posts whose state changed.
   - **Players / Bots (`units`)**:
     - Each unit has a stable `unitId` (`0 .. maxUnits - 1`).
     - A 1-byte `changeMask` indicates which field groups changed since `baseline`:
       - `0x01`: Identity/Meta (`team`, `classIndex`, `alive`, `isBot`, `vehicleId`, `seatIndex`)
       - `0x02`: Transform (`pos`, `vel`)
       - `0x04`: Orientation & Stance (`yaw`, `pitch`, `stance`, `jetpackFuel`)
       - `0x08`: Vitals (`hp`, `shield`)
       - `0x10`: Weapon & Ammo (`weaponSlot`, `ammoInClip`, `ammoReserve`, `heat`, `reloadingTimer`)
       - `0x20`: Input Ack (`lastProcessedInputSeq`; human slots only)
     - Removed/despawned unit IDs are listed in `removedUnits[]`.
   - **Vehicles**:
     - 1-byte `changeMask`:
       - `0x01`: Vitals & Respawn (`alive`, `hp`, `respawnTimer`)
       - `0x02`: Transform (`pos`, `quat`, `vel`)
       - `0x04`: Turret Aim (`aimYaw`, `aimPitch`)
       - `0x08`: Occupants (`occupants[]`)
   - **Projectiles**:
     - Newly spawned or trajectory-corrected projectiles (`spawned[]`) and destroyed/expired projectile IDs (`removed[]`).
   - **Gameplay Events**:
     - Reliable event log slice for ticks `(baselineTick, simTick]`, deduplicated by `eventSeq`.

---

## 4. Client Reconstruction & Verification

When the client receives a delta packet `(simTick, baselineTick, stateHash, delta)`:
1. Look up local baseline snapshot at `baselineTick`.
2. Reconstruct full quantized `WorldSnapshot` for `simTick` by cloning `baseline` and applying `delta` masks and removals.
3. Compute `computedHash = computeSnapshotHash(reconstructedSnapshot)`.
4. Assert `computedHash === packet.stateHash`.
   - If the hash matches, commit `reconstructedSnapshot` into the client's baseline ring buffer and advance `ackSnapshotTick = simTick`.
   - If a hash mismatch ever occurs (e.g., corrupted state), drop the packet and set `ackSnapshotTick = 0` to request a full keyframe on the next server broadcast.
