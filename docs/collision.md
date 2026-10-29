# Collision

How soldier and world collision is imported from the retail archives and
how the soldier controller moves over it. The collision buffers are written by
`tools/extract.py` into the ignored map extracts (`collision.bin`,
`collision_index.bin`, `collision_poly.bin`, the terrain buffers and, for live
objects, `props_collision.bin`) and queried by `src/collision.js`.

## Case-insensitive lookup

Collision resource lookup is case-insensitive, matching the original game.
The importer canonicalizes placed objects to their ODF class spelling, but
collision resource names often use lowercase. Looked up literally, names such
as Yavin Arena's `yav2_bldg_Arch_stairs` and `yav2_bldg_Temple`, and the
stairs, bridges and buildings on Hoth and both Rhen Var maps, would be
dropped, and soldiers would meet incomplete world collision rather than a
step-height limit. Mesh and primitive lookup therefore folds case while
preserving first-archive priority; explicit soldier primitive names and
collision skeleton lookups also ignore case. Visible near/far geometry uses
the same name matching so that collision has its corresponding visible
surfaces.

An installation whose ignored map buffers predate a change to these rules
needs its normal asset intake to regenerate them. No movement constants or
map-specific collision exceptions are involved.

`python3 tests/test_collision_export.py` checks synthetic case/priority
fixtures and, when retail archives are available, every placed collision
model type across all 16 maps. `node --test tests/stair-traversal.test.js`
walks all four Yavin Arena arch staircases uphill/downhill at three offsets,
checks the temple approach in both directions, and samples clear interiors
of walkable inclined collision faces throughout every available map. The
interior sweep excludes walls, edges, obstructed headroom, and steep sliding
surfaces; it complements the full Yavin routes and existing Theed/Bespin
movement tests rather than claiming to traverse every possible map route.

## Doorways, vehicle-only boxes and stepping

Some placed objects carry broad boxes that only vehicles collide with. The
original ODF property hash `0xc3f25cbb` is `VehicleCollisionOnly`; the
extractor decodes that property and respects its named exclusions, so those
boxes do not block infantry. Detailed collision meshes and ordinary
primitives remain, so columns, walls, floors, and stairs stay solid. Treated
as infantry collision, five broad vehicle-only boxes would seal the
pedestrian openings of Theed's open `nab2_bldg_canal_home` arcade leading to
the rotunda bridges, in all three of its rotated placements; the same
metadata covers Theed's vehicle-only lamp, planter, and fountain primitives.
This is an extraction-layer rule, not a map-specific hole cut into
collision.

Soldiers step onto a tread up to 0.45 m high and follow descending steps up
to 0.25 m without entering a fall. Both body spheres are checked through the
upward and forward movement before a step is accepted. Low ceilings, tall
walls, enabled live props, and map boundaries still block movement. Grounded
rolls share this support; jumping and prone movement do not get the walking
step assist. An exactly coincident sphere/face contact resolves using the
face normal instead of abandoning the remaining contacts.

The collision exporter ear-clips concave polygons. A triangle fan would fill
regions outside concave source faces; the Bespin chamber-exit model contains
three such faces. Convex polygons keep their fan order.

The Theed arcade at approximately (-168, -49, -28) is traversable standing
and prone, and the bridge beyond retains floor support. All three arcade
placements pass at three lateral offsets in both postures, and rays still hit
their flanking columns. The separate open Theed arch at approximately
(-168, -50, 14.56) is traversable in both directions, including five-metre
lateral offsets. The arch at (-168, -50, 100) contains a visibly closed gate
and a separate authored invisible barrier; those remain solid.

Run `node tests/collision.mjs` and `python3 tests/test_collision_export.py`.
The JavaScript checks cover low platforms, a tall wall, blocked headroom,
high-speed thin-wall collision, and toggled live-body clearance, including a
synthetic 0.4 m riser the controller walks across. With local retail assets
present, they also cross the three arcade variants standing and prone at
three offsets, verify their pillar collision, cross the open Theed arch at
three lateral offsets in both directions, preserve the closed gate, and cross
all three placed Bespin switchback ramps uphill and downhill at three lateral
offsets. The Python checks cover concave openings, winding, and projection
planes. The retail ramp tests are regression coverage.

## Movement continuity

A walkable sloping face also pushes the lower body sphere slightly backward.
The step helper therefore requires an obstructing riser or tread edge: if it
mistook this ordinary contact for a stair riser, it would sample the floor
ahead and lift the soldier 20 to 30 cm, and the next tick would push the
soldier down again. Sphere separation still clears the physical geometry,
while the actual supporting face supplies the movement normal. This prevents
rounded tread edges from becoming tiny launch ramps. The drive axes form an
orthonormal ground tangent basis, and downhill floor following accounts for
the sphere's height above a sloping polygon. Grounded rolls use the same
support rules.

The controller exposes cumulative `groundAdjustment` for the presentation
layer: only artificial height corrections while grounded contribute; jumping
and gravity retain their own trajectory. Spawning resets the counter. This
lets actor and camera presentation soften a real step without moving the
collision body or delaying input.

`node --test tests/movement-continuity.test.js` checks continuous and diagonal
slopes, turning, a ballistic jump, and, with local assets, both directions on
the real Theed bridge staircase and Bespin ramp. Four additional real-map
cases roll uphill/downhill, verify the full roll duration, and require
unbroken ground support. Wall, low-ceiling, platform, and arcade tests are in
`tests/collision.mjs`.

At 60 simulation ticks/s, the four real-map staircase routes (Theed and
Bespin, uphill and downhill) keep ground support on every tick, and the
largest vertical movement of the collision body in one tick is 5.3 to 6.2 cm,
measured before camera and presentation smoothing. A step helper that
mistook sloped contacts for risers moved the body about 30 cm in one tick on
the uphill routes, and the Bespin uphill route lost ground support for 56
ticks. The four roll cases retain ground support throughout and show no
alternating up/down correction.

## Terrain export window

The exported terrain collision covers the bounding box of the placed objects
plus 64 m, and also every path point (spawn, patrol and boundary paths). A
soldier past the window would have no ground and fall without end; on
Yavin's temple map (no `bnd_` boundary, so the whole height field is
playable) the ground beyond x 328 or z 152 lies past the objects' box plus
64 m. A map without a boundary therefore exports the whole height field.
Maps with a boundary keep a window, since their boundary turns soldiers back
before its edge.

## Collision filed under the model a class draws

The archives name `coll` meshes and `prim` shapes for the model, and a class
loads the model its `GeometryName` names, so the exporter files collision
under the model the class draws, not under the placed class's name. Looking
it up under the placed class's name finds nothing where the two differ, which
let soldiers walk through most trees on Yavin Temple and fall through the
raised dock platforms on Kashyyyk Docks. Temple places `yav_prop_leaftree` 64
times; its seven trunk cylinders are filed under `yav_prop_leaftree1`. Docks
places `kas2_bldg_platform_1.msh` 10 times and `kas2_bldg_platform_roof.msh`
4 times; their decks are the `coll` meshes of `kas2_bldg_platform_1` and
`kas2_bldg_platform_roof`. The visible geometry already fell back to the
GeometryName, so a literal lookup would draw trees and platforms with nothing
to stand against. The live objects need the same lookup: without it Temple's
seven tower turrets (`yav_bldg_watchtower`) and Bespin's eight AAA turrets
(`kam_prop_antiaircraft`) had no collision, nor did any
`com_item_vehiclerecharge` (its model is spelled `vehicalrecharge`), any
`com_bldg_major_controlzone`, Geonosis's Techno Union command posts, or the
Tusken barriers and campfires on Dune Sea: 166 placed objects on 12 of the
16 maps.

`collision_model` in `tools/extract.py` files each placed class's collision
under its GeometryName (its own name when it has none, or when the class is
unknown, as for painted File meshes), for the static world, the live objects,
and the vehicle-only blockers (`tools/zero_vehicle.py`). The class's own
collision flags (SoldierCollision, VehicleCollisionOnly) still apply. The
buffer formats are unchanged: an older extract loads and plays as before,
only without that collision. The map import version (`maps` in
`tools/content-version.json`, read by `tools/prepare.py`) is raised whenever
the map stages change, so the desktop app's intake runs them again and reuses
the sides, common assets, audio, and movies.

`tools/audit-collision.py` checks an extract against the archives:

    BFE_GAME_DIR=<install> python3 tools/audit-collision.py --content <game data> [--maps yav1 kas2]

For every object a map places (its world instances and tree-line props), it
computes the collision the extractor should have written, through the
extractor's own placement, and looks for those vertices in `collision.bin`
or, for live objects, `props_collision.bin`. It lists the objects with
retail collision but none in the extract, by class and model, and apart, the
classes without soldier collision in the archives (Theed's tall trees,
Hoth's cables and rafters), which are not gaps. It exits 1 when a map has a
gap. An extract made before the GeometryName lookup reports the 166 objects
above; a current extract reports none on any map.

`python3 tests/test_collision_export.py -k GeometryName` covers the lookup,
the static and live exports, and the audit's comparison on synthetic
fixtures. `node --test tests/collision-import.test.js` walks a soldier into
a trunk cylinder and across a raised deck built from the exporter's own
polygon layout, runs those Python checks, and, with an extract present
(`BFE_CONTENT_DIR`, or `public/`), probes three Docks platforms and three
Temple trunks in the real buffers.
