# Classic heroes and melee

The existing population service allocates heroes and preserves their class
on respawn. Hero content remains an ordinary soldier class; `character.hero`
sets `unit.hero`. The stock classes are Mace, Dooku, Luke, snow Luke and
Vader. Snow Luke is a content variant. No score meter, timed possession or
force powers are added.

## Gameplay

`src/heroes.js` supplies the shared combat implementation with:

- `regenerate(combat, dt)`: living actors recover authored `AddHealth` per
  second, clamped to `MaxHealth`. Stock heroes have both values set to
  1,000,000. Damage remains ordinary finite health subtraction. Dead actors
  cannot regenerate, including after force-death damage. Capture and boarding
  use their authored `CapturePosts=0` and `NoEnterVehicles=1`.
- `prepareMelee(arsenal)`: installs `MeleeSwing` on melee weapons while
  retaining their common Arsenal selection, input and AI interfaces. Melee
  has unlimited ammo and never reloads. A press starts a swing; presses
  during a swing and holding do not restart it. Swing entry spends an update
  setting its clock to zero. Hit fires once when `old <= hit < old + dt`;
  completion occurs strictly after duration. The stock swing lasts 0.7 s
  with a hit at 0.5 s. Its selected swing index chooses shoot/shoot2/shoot3.
- `meleeStrike(combat)`: up to ten local eligible objects within the box
  around origin + aim, each tested by its own strict-under-2-metre ray.
  Successful contacts deliver 1000 damage with unit type scales of 1 through
  the existing projectile damage callbacks, preserving credit and friendly
  fire settings. Several objects can be struck. There is no added team
  filter or world-occlusion rule in the swing itself. Soldiers use their
  existing hit shapes; vehicles and buildings use their collision BVHs.
- `attachBlade(...)`: core and glow under the existing held hilt, using its
  authored fire point and native local tip displacement `(0, 0.8, 0.5)`.
  Body visibility, pooling and animation thus also control blade visibility.

`PlayerCombat` runs these rules for human and bot inputs. Native melee asks
for a button press independently of the generic weapon's TriggerSingle
property. The port's `weaponAI` adapter routes melee fire requests through
its existing single-shot input consumer so continuous AI pursuit supplies
new presses. The exact cadence of this adapter is a port integration choice;
physical swing and hit timing follow the original game's conditions.

The existing extracted animation sets already include sabre actions and the
sabre → tool → pistol → rifle fallback. No shared animation pack is replaced.

## Defense compatibility

`tools/zero_weapons.py` records `canDeflectAuthored` separately from effective
`CanDeflect`. Absent values use `bf1-cloned-melee-cleared-v1`: effective zero,
as for all five stock sabers in the original game. Explicit values are read
the way the original game reads integer properties (leading integer prefix),
so true/false are zero.

Stock heroes consequently take normal projectile and melee damage; their
large finite health and regeneration make ordinary weapons ineffective.
`tryMeleeBlock` supports an explicitly enabled melee block with no facing or
swing-state gate. Generic projectile reflection and the block pose/sounds
are not implemented; custom CanDeflect=1 classes therefore have only melee
blocking support. No stock class in the selected profile needs that missing
projectile behavior.

## Content extraction

`playable_units(side)` now includes mission hero classes and all faction
fallback hero variants. The player menu continues to use ordinary mission
rosters. `extract_side` includes the saber hilt, core/glow textures, hit and
deflect effects, plus hero body rigs. It writes model bounds and AISizeType
for navigation. `SwingTime` records retain their order, capped at three;
missing/negative hit times default to half duration. The stock malformed
Imperial numeric property `7.5.0` reads as 7.5 through numeric-prefix parsing.

Side-only refresh (preserves map and common outputs):

```sh
PYTHONPATH=tools python3 - <<'PY'
from extract import extract_side, playable_units, shared_archives
shared = shared_archives()
for side in ('rep', 'cis', 'all', 'imp'):
    extract_side(side, playable_units(side), shared)
PY
```

All retail-derived output remains under ignored `public/sides`. No binaries
or retail tables are tracked.

## Remaining presentation limits

- The blade core blend is additive as an approximation of the original
  game's core blend mode. Glow uses fixed width 0.16 rather than native
  per-frame endpoint flicker. Gameplay never consumes cosmetic random draws.
- Dooku/Vader addon cape properties are exported, but animated capes are not
  attached. This does not change body collision or AI.
- Swing/hit sounds and defense poses still require audio/animation wiring.
  Swing events already include the authored sound name for that integration.
- The melee broadphase approximates the native object's axis-aligned bounds
  using the port target's bounding sphere against the query box. The exact
  per-object ray and ten-candidate cap remain in place.

## Verification

`tests/ai-heroes.test.js` covers finite regeneration and irreversible death,
strict swing timing, multiple victims, range and candidate caps, seeded
controller input through real `IntentControls` and `PlayerCombat` for repeated
swings, effective defense, blade geometry, and all five extracted stock
classes when local assets exist. `tests/test_ai_heroes_export.py` uses synthetic
properties to cover ordered swings and the defense compatibility parser.

`tests/ai-heroes-browser.mjs` runs the headless world under Node
(`tools/headless-world.mjs`) on locally extracted content (`public/` or
`BFE_CONTENT_DIR`); the map must name a hero in its teams (the test loads
`nab2`). It checks Heroes Off versus On roster size, both real hero spawns,
their exclusion from the battle's capture contributors, boarding rejection,
repeated controller-to-combat melee, damage events, kill credit, finite
regeneration and forced death. The controlled run records five 1000-damage
hits in four seconds for each hero, then one credited kill. It writes a JSON
report outside the repo, into the directory named by `AI_TEST_ARTIFACTS`.
