# Presentation Crate (`rust/bfe-presentation`)

`rust/bfe-presentation` turns the simulation's authoritative world state and the
render clock into the poses, camera views, ragdoll transitions, first-person
offsets, and unit skinning matrices that `bfe-render` draws each frame.

---

## Responsibilities

| Module | Role |
|---|---|
| `clock` (`PresentationClock`) | Advances the render clock, computes the interpolation alpha between the previous and current simulation ticks, and smooths remote entity snapshots. |
| `unit_pose` | Samples upper- and lower-body animation clips from the loaded animation bank (`rust/bfe-assets`), blends walk/aim/Action states, applies procedural torso and head aim offsets, and produces per-bone world and skinning matrices (`palette_4x3`). |
| `ragdoll` | Steps procedural death ragdolls and explosion impulses against terrain height and collision normals. |
| `first_person` | Computes first-person weapon bob, sway, recoil kick, and camera eye offsets from the local unit's movement and weapon state. |
| `vehicle_view` | Computes vehicle third-person and gunner camera transforms, hull tilt interpolation, and turret mount poses. |
| `world_present` | Gathers visible units, vehicles, ordnance, command posts, and effects for the current render frame into contiguous draw batches consumed by `bfe-render`. |

---

## Determinism

Transcendental operations in the pose pipeline route through `v8math`, so
`x86_64` and `aarch64` produce identical results without a runtime interpreter.
