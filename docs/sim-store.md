# Canonical Simulation Store (`rust/sim`)

## Architecture

`rust/sim` owns the authoritative battle simulation state in a schema-driven Structure-of-Arrays (SoA) `Store` (`rust/sim/schema/*.json`). Every authoritative subsystem; units, vehicles, ordnance, command posts, live objects, recon droids, AI controllers, collision, movement, and wire serialization; reads and mutates the canonical Rust `Store` directly.

### Schema-Driven Store Layout

The declarative JSON schemas under `rust/sim/schema/` define the tables and columns of the simulation store:
- `units.json`: Unit transforms, velocities, vitals, stance, weapon slots, animations, AI state, and player slot bindings.
- `vehicles.json`: Vehicle transforms, velocities, chassis state, seats, weapon mounts, and respawn timers.
- `ordnance.json`: Projectiles, grenades, mines, detpacks, and beams.
- `command-posts.json`: Command post ownership, capture progress, spawn regions, and team bleed state.
- `live-objects.json`: Destructible props, turrets, health/ammo droids, and repair state.
- `combat.json`, `recon.json`, `ai.json`, `battle.json`, `world.json`: Combat damage records, recon droid state, AI perception/planner tables, match rules, and world phase state.

`cargo xtask schema` (`rust/xtask`) validates the JSON schemas and generates the typed Rust store accessors in `rust/sim`.

### Single-Source-of-Truth Invariants

1. **Zero Parallel Mirrors**: The Rust `Store` is the single authoritative representation of every entity in a battle. Neither the local battle thread in `bfe-client` nor the authoritative loop in `bfe-server` maintains a secondary object mirror of simulation entities.
2. **Deterministic Snapshot & Replay**: `rust/sim` computes deterministic state digests (`world_hash` and `world_deep_hash`) and encodes wire snapshots directly from the `Store` columns.
3. **Read-Only Presentation Views**: `rust/bfe-presentation` and `rust/bfe-render` sample read-only published views and interpolated poses produced by `rust/sim` without mutating simulation state.
