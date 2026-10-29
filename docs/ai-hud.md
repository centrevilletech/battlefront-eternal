# Classic AI HUD

`src/hud-units.js` implements unit visibility, relation colours, reticle
selection, hit feedback and kill-message selection. `src/hud.js` draws them
through the existing raster HUD batch. No DOM labels, overhead health bars
or damage numbers are added.

## Runtime API

Call `hud.setUnits(registry, context)` before `hud.update`. The registry may
also be an array. Context has:

- `viewer`: the human soldier; `viewerTeam` defaults to its team.
- `controllable`: the current vehicle, turret or remote droid, otherwise the
  viewer. Its position and `ScanningRange` control radar sensors.
- `relation(a, b)`: registry relationship, +1 friendly / 0 neutral / -1 enemy.
- `easy`: whether the difficulty is Easy.
- `tickets(team)`: remaining reinforcements.
- `objects`: optional additional vehicle/turret objects, deduplicated against
  the registry. `classDef.settings` and `def.settings` are supported.
- `living(team)`, `sensors`: optional overrides for non-hero live counts and
  sensor circles (`{position: Vector3, radius}`); normally computed locally.

`hud.update(dt, posts, player, battle)` advances all presentation timers and
the radar. Its existing `player.position/yaw` specify the map centre and
orientation. Call once per simulation interval, including frames with no
shot events; rendering does not advance timers.

`hud.noteShot(unit, weaponOrDefinition)` records each actual shot, including
mounted weapons against their vehicle/turret registry entry. Each shot's
flash falls from white during its `ShotDelay`, with a one-frame minimum;
the strongest active flash wins. The shot immediately reveals the icon so
short shot windows survive a slower render frame. Visibility memory is
discarded when a pooled unit's generation changes.

`findReticleTarget({origin, dir, units, weapon, viewer, relation, los, range})`
returns null or `{unit, point, lockable, score, distance}`. `weapon` accepts
a Weapon or its definition. `los(origin, point, viewer, candidate)` must
test terrain/buildings and ignore the endpoints' own geometry; the caller
supplies the world's collision adapter. Direction is normalized locally.
No unit or input vector is mutated. `range` overrides the definition range.

After projection/selection each frame:

- `hud.setReticleTarget(relationOrNull, markerOrNull)`: relation can be the
  numeric registry value or `friendly/enemy/neutral/locals`. The optional
  lock marker is `{x, y, progress}` in screen anchors (0..1), with lock
  progress 0..1 supplied by the caller. It draws only for `LockOffAngle > 0`.
- `hud.enemyHealth(enemyOrNull)`: a living enemy refreshes a health/icon
  snapshot. Null stops refreshing; the last snapshot fades over two seconds.
  Call only for hostile selections. A recycled unit cannot change the faded
  snapshot.
- `hud.hit(victimRelation)`: once per successful player-inflicted damage
  event, including blasts and mounted shots; adds 0.5 s to that relation's
  hit-ring slot. This is feedback, not hit detection.
- `hud.killMessage({killer, victim, relationToViewer})`: once at death. Feed
  units, not weak refs or Characters, and normalize the legacy `"player"`
  attacker to the player's unit. Use `radarRelation` for yellow local-team
  messages. Localization falls back to built-in English strings.
- `hud.stats({kills, teamkills, deaths, captures})`: supply the human
  Character's cumulative stats every update. The initial sample is quiet;
  later changes show a three-second popup. Kills display `kills-2*teamkills`.
- `hud.squadCommand(kind)`: after a command is accepted. Display indices
  0/1/2/3 mean MOVE OUT/HOLD POSITION/FOLLOW ME/AT EASE; string aliases are
  `moveout`, `hold`, `follow`, `atease`. These indices differ from the AI's
  command IDs, so callers should use the strings.

The new preloaded common textures are `troop_icon`, `hit_indicator`,
`stats_kills`, `stats_deaths`, and `stats_cpp`. Vehicle/health icons are
resolved by the ordinary texture callback. The teamkill localization key is
`game.infomsg.teamkill`. Extracted files remain ignored; no retail data is
included here. Missing stat icons use compact K/D/CP text.

## Behavior

Radar icons use 6 times MapScale HUD units, face with their unit, and hide
beyond the 125 m minimap range; only posts pin to the edge. The large map
does not rotate. Up to 450 eligible units and 64 sensors are considered.
Directional art points up, while world +Z maps down, so its battlefield
rotation is `PI - yaw`; the minimap also applies the map's rotation.
Vehicle markers follow the hull rather than the driver's weapon aim.
Aircraft headings project the simulation quaternion's +Z nose onto the
map, retaining the last horizontal heading while the nose is vertical.
The same orientation rules apply to fallback vehicles and the spawn map.
Seated soldiers, removed pool entries and unspawned units are hidden;
corpses blend halfway toward (64,64,64). Friendly, enemy, neutral and local
colours are (32,223,32), (223,32,32), (192,192,192), (223,223,32).

Visibility checks friendship, reinforcements no greater than live non-hero
soldiers, then Easy difficulty. Otherwise a sensor or a shot reveals the
unit, with alpha falling by 32/255 per second. Sensors require strictly
inside a scan circle; a same-team transmitter must reach the controlled
object before it contributes a circle. The map draws posts, units, post
flashes, labels, then the player.

The reticle considers up to 300 live candidates in range, allowing
off-axis distance `d <= r + t*LockOnAngleRadians`. It ranks `16*d*d+t`, with
valid lockable targets taking priority. Target relation/category masks
reuse the Classic weapon module. An ineligible lock target may still colour
the reticle. Friendly is green, hostile (including locals) red, everything
else blue; weapon dimming is retained. A lock marker has a circle and a
diamond shrinking from 32 to 8 units. No target name is displayed.

Hit slots decay by one second per second. Their intensities are
`e/f/n=min(10*time,1)`, `m=max(e,f,n)`, and the ring RGBA is
`(32+192*e/m,32+192*f/m,32+192*n/m,128*m)`; zero intensity draws nothing.

The feed is limited to deaths of humans or kills by humans/heroes. It
distinguishes uncredited death, suicide, same-team kill and ordinary kill.
AI Character names persist through death; five messages last ten seconds.
Enemy health lasts two seconds after losing selection. A stat row's icon
and number flash yellow when it increases or red when it decreases:
independent one-second timers hold the colour for half a second, then blend
back over the remaining half to the popup's resting white at alpha 64. Its
layout is in [scoreboard.md](scoreboard.md#the-stat-popup). Squad commands
show all four labels, selected full opacity and others half, fading over two
seconds.

Capture credit is broader than capture contribution. At a successful
capture the game revisits the same CaptureRegion query (64 entries maximum)
and awards one capture to each living entity with a Controllable and a
Character. That award loop does not repeat the team, CapturePosts or
seated-state gates used to count contributors. The award updates the current
team's 16-bit counter, and the popup reads the same counters. The simulation
owns the credit; the HUD only observes the Character counter.

## Provisional details

- Older side extracts lack MapTexture/MapScale. Only absent soldier fields
  fall back to `troop_icon`, scale 1.4 (droideka 1.6); an explicit empty
  MapTexture stays hidden. Fresh extraction should supply the game's own
  fields.
- The original game's reticle range depends on the last parsed OrdnanceName
  or LockOnRange. `ReticleRange` can carry that exact result; old extracts use
  projectile Velocity times LifeSpan, then LockOnRange. This gives the
  documented 120 m rifle range, but parse-order exceptions require new data.
- Target `box.half.length()` supplies the original game's half-diagonal when
  available; current unit adapters expose an enclosing sphere instead.
- ShotDelay approximates the original game's weapon shot-state window.
- Post-label size and the geometric lock-marker art are presentation
  approximations. The message stack moves up 76 HUD units
  while squad commands show, keeping simultaneous overlays legible.
  Lock progress is supplied by the integrator;
  HUD rendering does not establish projectile guidance or weapon locks.

## Validation

`timeout 40s node --max-old-space-size=1024 --test tests/hud-units.test.js`
covers relation/death/shot colour, visibility ordering and fades, strict
sensor boundaries and transmitter reach, pool generations, mounted/removed
units, non-hero counts, hit-ring mixing, cone ordering, LOS, target masks,
range overrides, localized kill gating, stat penalties, enemy snapshots and
map clipping/orientation/draw order. Fixtures are synthetic.

`tests/ai-hud-browser.mjs` uses real Classic actors and the `src/game.js`
integration: enemy selection, hidden/revealed radar, real combat damage/death
and human credit, mixed hit feedback, stat and command fades, and an actual
neutral post capture with two contributors plus a non-contributing hostile
hero. The latter checks the separate capture-credit loop and 16-bit wrapping.
It saves `feedback.png`, `faded.png`, `capture.png` and `report.json` under
`AI_TEST_ARTIFACTS` (default: a `bf-ai-hud` folder in the temp directory). Set
`PLAYWRIGHT_MODULE` and `AI_TEST_URL` as for the ordinary AI browser test. Bot
movement is held for deterministic actor placement; the shot event invokes the
game's real adapter in `src/game.js`, while the damage and capture checks run
their actual simulation methods.

The fixture runs on Theed with Medium Classic AI. In the capture check the
counters go from `[0, 0, 65535]` to `[1, 1, 0]` for the player, the allied
contributor and the hostile non-contributor respectively (the last by 16-bit
wrapping), and the HUD shows the player's new capture.

## Contract changes

The HUD methods above change no registry or simulation contract.
`src/game.js` owns event dispatch and reticle collision/projection.
