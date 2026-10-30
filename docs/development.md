# Development and playtesting

Developer setup, controls and implementation notes for Battlefront Eternal.
For the project overview and roadmap, see the [README](../README.md).

This repository is the engine and the notes. The retail game stays on disk. ISOs, extracts, and asset dumps are gitignored under `data/game/` and `extract/`, and they are never pushed.

You need Node.js 24 with npm, Python 3, and your own copy of the game.

```bash
npm ci
npm test                                # unit tests; no game files needed
python3 tools/extract.py bes2           # one map, read from your copy of the game
python3 tools/zero_shell.py --catalog   # Instant Action's map catalog (shell.json)
npm run dev
```

`zero_shell.py --catalog` writes `public/common/shell/shell.json` and marks the maps that are extracted, so run it again after extracting more. `python3 tools/prepare.py` does all of the preparation in one run, as the desktop app does on its first launch: the catalog, every army and map, the interface, and the optional upscaling and Enhanced sound steps (a step whose tools are missing is skipped and reported).

The extractor reads the game as Steam installs it, copied whole into `data/game/install/`, and writes `public/` (both gitignored). It takes a map name, a side, and the units to export: `python3 tools/extract.py nab2 --side cis` exports the droid army for Theed (each side is extracted once; they share `public/common/`). The archives sit where the game keeps them, under `GameData/Data/_LVL_PC`: `bes2` is `BES/bes2.lvl`. It writes four folders:

- `public/maps/<map>/`: the map;
- `public/sides/<side>/`: the side's playable units from `SIDE/<side>.lvl` (every class its battles hand out, or those named with `--unit`), each with its body, first-person arms, and weapons;
- `public/common/`: the soldier animations and effects from `Common/common.lvl`, and the HUD's fonts and text from `core.lvl`;
- `public/audio/`: the battle's sound, its sound sub-level's samples and settings and the streams (music, voice-over, ambience) its mission opens.

Set `BFE_GAME_DIR` to read a Steam installation in place (the folder holding `GameData`, see `tools/zero_paths.py`). The Electron client is covered in `docs/desktop.md`.

`docs/formats.md` describes what it reads. `docs/audio.md` covers the sound: the Original (2004) and Enhanced (2026) modes, speaker and headphone setup, and the optional upscaling.

Classic battles run with `?map=nab2&unit=rep_inf_clone_trooper&ai=classic`.
Add `&tier=easy|medium|hard` (default Medium), `&heroes=1`, and an optional
`&seed=1`. The pause menu's OPTIONS → GAME OPTIONS offer the same options;
RESTART MISSION applies AI, difficulty, or hero changes. Friendly fire
changes immediately. AI remains off by default until selected.
F1 orders at ease, F2 follow, F3 move out, and F4 hold, matching the
original PC bindings. Bots use the mission's rosters, navigation, combat,
vehicles/turrets and class jobs. The HUD shows units, target relations, hits,
kill messages and score changes. Implementation contracts and known fidelity
limits are in [docs/ai.md](ai.md).

Gameplay now uses one fixed 60 Hz clock, with rendering independent of it.
Graphics settings offer Performance, Balanced and High quality presets, an
optional frame cap, and adaptive resolution when GPU timing is supported.
Rendering defaults to the display's refresh rate. Performance targets 30 FPS
on weaker hardware; this is a qualification target, not a measured minimum PC.
The battle runs in a local server worker
(`src/runtime/local-server-worker.js`), off the page's thread: the page
sends the player's input as commands, draws the world from the worker's
packets, and shows the player's own soldier, hull, mounted guns, recon droid
and shots ahead of them, as an online client does.
Browser checks step the worker frame by frame through
`tests/battle-page.mjs` (`game.lockstep`); world logic without a page runs
under Node through `tools/headless-world.mjs`.

The timing/threading implementation status, audit and migration gates are in
[docs/runtime-architecture.md](runtime-architecture.md). For a live CPU
and input-timing capture, add `&profileRuntime=1`, reset with
`__bf.runtimeProfile.reset()` after loading, then inspect
`__bf.runtimeProfile.report()` after reproducing a hitch. These timings end at
render submission; they do not measure input-to-photon latency.

`npm run dev` opens the main menu. Use Instant Action or a direct `?map=` URL to launch an extracted battle. `?map=` loads another extracted map, `?side=` another side, and `?unit=` another unit (`?map=bes2&unit=rep_inf_clone_trooper` is Bespin: Cloud City with a clone trooper; `?side=cis&unit=cis_inf_battledroid` is a super battle droid). Each map plays the battle of the era the URL asks for: `?era=gcw` (or a Galactic Civil War side, `?side=imp`) is its Galactic Civil War battle, `?era=cw` or nothing its Clone Wars one, and `?mission=nab2a` names one outright (docs/shell.md, `src/mission.js`). The Republic plays the clone trooper, ARC trooper, pilot, sharpshooter, and jet trooper; the CIS the super battle droid, the B1 assault, pilot, and assassin droids (and the battles' `_inf`, `_av`, `_pilot`, and `_sniper` versions), and the droideka (`?side=cis&unit=cis_inf_droideka`). The command posts start as the map sets them. Standing in a post's capture zone neutralizes and captures it. Your team spawns at its own posts, and holding every post starts a 20-second victory countdown. Click to capture the mouse. The keys are the 2004 PC defaults:

- W and S (or Up and Down) move, A/D or Left/Right strafe. The mouse aims.
- Space jumps. With only A or D held, it dives to that side instead. In the air, Space again lights a jet trooper's jetpack (he hovers, flies where he looks with W, and burns fuel shown under the health bar), and once more cuts it.
- C or Ctrl crouches and X goes prone.
- The left mouse button fires and the right one throws a grenade. F or wheel-up selects the next primary weapon, wheel-down the previous one, G switches the secondary weapon, R or Enter reloads, and Z (or the middle button) zooms. Holding the trigger charges the weapons that charge (the mortar, the arc caster).
- The droideka: E folds it into a ball and unfolds it, the strafe keys turn it, and the right button raises and lowers its shield.
- The recon droid: the right button with the remote weapon throws it and puts you in control. WASD flies it, the mouse aims its blaster (left button), and the right button calls the orbital strike. E or F hands control back; while it lives, the right button takes it again and R blows it up.
- E gets into the vehicle or turret in front of you, and out again. Vehicles use WASD and the arrow keys to drive or steer, the mouse to aim, and the mouse buttons for primary and secondary weapons. Space brakes; F changes seats; R reloads. Aircraft take off and land with Space, throttle with W and S, steer with the mouse and roll with A and D (see [docs/vehicles.md](vehicles.md)). Q switches between chase and forward views.
- Q (or mouse button 4) switches to first person and back.
- M shows the whole map and the backquote key hides the stats line. As in the 2004 game, the spawn screen picks the command post you spawn at; no key moves you between posts.
- Open `/?noreload=1` for the main menu, then SINGLEPLAYER → INSTANT ACTION to build and launch a playlist. Direct `?map=` links still enter a battle. The browser development build cannot play online: LAN and ONLINE work in the desktop app ([docs/desktop.md](desktop.md)). See [docs/shell.md](shell.md).
- T opens global chat (yellow), Y opens team chat (green); Enter sends and Esc cancels. Server announcements are blue. Single-player messages echo locally.
- Esc pauses: GAME PAUSED, with OPTIONS (the CONTROL OPTIONS page lists every key, by INFANTRY, VEHICLE and STARFIGHTER), RESPAWN, RESTART MISSION and QUIT. The menus are described in [docs/menus.md](menus.md).

Vehicles are loaded from each map's own spawn records and mission factions by
the normal extractor. Hover tanks, speeders, wheeled Hailfires, animated walkers,
command walkers, and aircraft share the vehicle system. Command-post ownership
selects the faction on respawn; neutral posts wait, and occupied pads delay
respawning. The importer preserves each map's original vehicle roster and pad
locations. Maps without vehicle pads, such as Bespin: Cloud City, remain
infantry maps; Theed uses its four original tank pads.

Vehicles have their native models, weapon loadouts, health and movement settings,
moving collision, engine/fire sounds, map icons, pilot repairs, recharge stations,
mines and blast damage, destruction, and respawning. Beam weapons persist while
applying damage over time. Snowspeeder tow cables attach to enemy command walkers;
three circuits trip the target, while reversing unwinds the cable. Vehicle first
person currently uses a clear forward camera above the hull. Driving and flight
use a bounded 120 Hz simulation with interpolated rendering, shared collision
BVHs, sleeping parked hulls, and skinned geometry batched by material.
Vehicle HUDs stay visible with hull health, speed, seat, and flight status,
including fallback reticles for weapons without retail aiming art. Steering
and chase cameras ease into turns; repulsor craft bob subtly without moving
their collision hulls. Ground vehicle speed blur can be disabled in graphics
settings; aboard a craft nothing blurs.
Creature mounts retain their per-vertex skin joints. Riders follow the animated
saddle at the pelvis, play their riding clips, and blend through idle and movement.
Creature run cadence follows the clip's recorded stride travel, reverses when
backing up, and uses foot-area terrain support without repulsor clearance.
Walker animation reference records no longer dilute the animated root pose,
keeping the feet planted through idle breathing and on smooth slopes.

To refresh vehicles for maps extracted before this change, run
`python3 tools/zero_vehicle.py --all-extracted`. To extract a new map, use the
normal command, for example `python3 tools/extract.py geo1 --side rep` or
`python3 tools/extract.py hot1 --side imp`. The map extractor also distinguishes
detailed terrain indices from low-detail strips, fixing holes under vehicle
pads on Geonosis and Hoth; those maps need a full re-extraction when upgrading.

Rhen Var: Citadel loads with
`?map=rhn2&side=rep&unit=rep_inf_clone_trooper&ai=classic` after
`python3 tools/extract.py rhn2 --side rep`. Its Clone Wars battle uses seven
command posts, the original navigation graph, and emplaced turrets, with no
vehicle pads. The CIS roster must also be extracted for both armies to field
(`python3 tools/extract.py rhn2 --side cis` on a fresh installation).
The sky importer resolves mixed-case model references; re-extract Citadel if
an older import shows a black sky.

Kamino: Tipoca City loads with `?map=kam1` after
`python3 tools/extract.py kam1 --side rep`. Spawn paths follow each command
post's authored reference, including Kamino's `CP2SpawnPath` naming. Kamino
retains its native infantry layout without vehicle spawns.
Its native precipitation and ocean settings are imported too: falling rain
respects building cover, and scrolling water/foam textures and wind-driven waves
extend into the far scene. Ocean geometry and rain instances have fixed budgets.
Graphics → Rain effects switches between Classic and Enhanced (the default).
Enhanced uses layered fine rain, wind gusts, soft intersections, low drifting
mist, and surface rings and spray. Rain remains outside building cover. Spawn
placement uses the authored height directly, avoiding the underside of bridges.
Graphics → Ocean and sky also defaults to Enhanced: large rolling swells,
crest foam, wind-blown spray, sea haze, moving volumetric storm clouds, and
intermittent branching lightning that illuminates the clouds, water and scene.
Classic retains the imported ocean textures and original sky. These options
apply to native ocean maps; inland water retains its existing quality settings.
On every map with ships in its sky, Enhanced also flies every class of the
battle's era past the view distance's far plane, with smooth fades; Classic
flies the original's (docs/environment-effects.md, "The sky's flyers").

Vehicle checks: `node --test tests/vehicles.test.js`,
`python3 -m unittest discover -s tests -p 'test_vehicle*.py'`, and
`node tests/vehicles-browser.mjs` with Vite and extracted Theed, Geonosis, Hoth,
and Bespin. Set `PLAYWRIGHT_MODULE` if Playwright is installed outside this project.
The optional retail audit checks all 28 standard map/era combinations;
`node --test tests/vehicle-assets.test.js` checks the extracted vehicle rigs,
controllers, collision, and weapon seats together.

The options (Esc → OPTIONS) cover:

- GAME: friendly fire, the Classic AI's mode, difficulty and heroes, the stats line; gameplay advances at a fixed 60 Hz, independently of display refresh;
- VIDEO: the presets, frame pacing, render scale and upscaling, antialiasing, shadows, texture filtering, view distance, bump mapping, the map's soft-focus blur, and each Classic or Enhanced effect;
- AUDIO: the volumes, Original or Enhanced sound, the output layout and dynamics, and the speaker test;
- CONTROL: mouse sensitivity on the game's own 0.5 to 10 slider, invert Y, and the key list.

HUD intake can generate faithful 4× lossless WebP atlases when Pillow is
installed in the extraction environment. To upgrade an existing extraction,
run `python3 tools/upscale_hud.py`; native bitmaps remain the fallback. Traced
HUD/font vectors are no longer used. See `docs/hud-vectors.md` for details.

Hanging banners now bend subtly in the wind with fixed upper edges and moving
shadows. Water quality is selectable in graphics settings: Low preserves the
original look, while High and Ultra add more detailed surface lighting,
reflections of nearby geometry, underwater scattering, and small shore splashes.

They are saved in the browser. URL options override them for a visit:

- `?spawn=CP3` picks a starting command post.
- `?scale=2` renders at twice the window's pixel density.
- `?sens=` sets the mouse sensitivity slider and `?invert=1` inverts the mouse.
- `?shadows=off` turns shadows off. Old `?rate=` values are ignored; gameplay stays at 60 Hz.

The mouse reads raw counts where the browser allows it, like the original's DirectInput.

Theed's flower beds and wall ivy retain their original layouts. Tree detail
offers Original backdrops or Enhanced nearby canopies built from the game's
leaf textures, with depth, subtle motion, and a transition to distant cards.
Leaf cutouts write depth so trees and holograms occlude each other correctly.

The renderer lights the map the way the game did, from the map's own sky settings: per-vertex sun and ambient in gamma space, baked interiors, per-pixel bump mapping, and terrain layers. Distant buildings dissolve into a far scene of low-detail models, as in the original. "Original" view distance matches the game's longest setting; the longer settings are the port's own. Shadows are cascaded shadow maps.

Each unit carries its own loadout from the game, and the weapons behave as the original game has them:

- the rifles, pistols, and launchers, the jet trooper's EMP launcher (its rockets fly as models with smoke trails), and the grenades;
- the super battle droid's wrist blaster, its shrapnel (three bolts in a fixed spread), and its rockets;
- the Dark Trooper's blast cannon and the Rebel pilot's blaster cannon (five bolts a press, each with its own share of the spread, drawn where they fly);
- the sniper rifles (2x and 8x zoom, and a kick that lifts the aim), the B1 pilot's mortar (a tap lobs a shell, a full charge a bouncing grenade), and the clone pilot's arc caster (charged lightning);
- mines and power-up dispensers: mines stick where they land and glow, power-ups heal and resupply whoever picks them up, and the dead may drop one;
- the fusion cutter, which builds the turrets on their pads and repairs or rebuilds turrets and recharge droids;
- the recon droid, which hovers, shoots, and calls in an orbital strike;
- the droideka's twin blasters, which fire in turn, and its shield, which soaks up damage until it runs dry.

The droideka walks only as fast as its walking clips carry it, rolls up into a ball that reaches 13 m/s, and dies in a string of blasts.

**Graphics → Droideka effects** selects Classic or Enhanced. Classic restores the native shield's scrolling cloud texture, edge falloff and quick expanding pop. Enhanced adds a bright energy rim, fine transparent cell detail, an activation sweep and expanding damage ripples, plus an outdoor dust trail while rolling. Both use the corrected folded-shell axle and smoothly sampled roll pose.

**Graphics → Jetpack effects** keeps the original flames in Classic; Enhanced adds twin hot exhaust jets, smoke, embers and local light.

Firing rates, clips, reloads, heat, spread, charge levels, bolts, grenade bounces and fuses, explosions, muzzle flashes, and the particle effects come from the game's weapon classes and effect files. Damage follows each class's health type, so EMP hurts droids twice as much as clones. Droids blow apart into pieces as often as the game has them do. Your own grenades can kill you, which costs a reinforcement.

First person uses the game's own arms rig and first-person weapon models, its clips and the rules for picking them, its eye position, and the camera motions from its scripts: the head bob, dive rolls, lying down, getting up, and the landing dip. Zooming in first person shows the weapon's scope.

The HUD is laid out as the original game lays it out: the angled ammo and health panels cut from the game's HUD textures, the reinforcement meter, the reticle, the capture timer, the round minimap, the message feed, and the damage indicator, all in the game's bitmap fonts and localized text.

Health and ammo droids heal and resupply anyone standing by them. The gun turrets start as build pads, marked by a hologram that only pilots see; built, they can be manned. Blasts and fire destroy both: a turret goes back to its pad and rebuilds itself in two minutes, and a droid leaves a wreck for a pilot to rebuild.

The soldier runs on the game's own data and rules:

- Speeds, acceleration, and the camera sections come from the trooper's class properties.
- The animation is the decoded soldier clip set.
- Collision is the map's own collision meshes, shapes, and terrain.
- Movement, jumping, diving, the soldier's collision body and contact handling, mouse look, and the chase camera follow the 2004 game (see `docs/formats.md`).

The original game, its code, and its assets remain the property of their rights holders. Nothing in this repo is a copy of them.
