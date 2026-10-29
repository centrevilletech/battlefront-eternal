# Vehicles and the unified flight model

Every aircraft uses the same flight model, regardless of the map's source game.
Each craft flies as its own ODF has it in the 2004 game: its speeds, how fast
it turns and pitches, how it leans into a turn and levels its wings, its lift
off a pad and its landing (see [Movement](#movement)) and its pilot's chase
camera and fields of view (see [Camera](#camera)). A/D roll, boost, tricks,
the landing prompt and the combat assists are the port's own tuning, shared
by class.
Imported meshes, seats, hardpoints, weapons and collision geometry remain
assets.

## Flight model

Aircraft integrate at 60 Hz and render interpolated position and quaternion;
ground vehicles keep their own implementation. Projected heading, pitch and
bank fields are read-only telemetry for shared seat and AI services. There is
one flight path: no handling selector, and unknown saved settings are
discarded when a profile loads.

## Module layout

The flight modules are under `src/flight/`:

| Module | Responsibility |
| --- | --- |
| `config.js` | Four class configurations; shared live tuning |
| `debug.js` | First-built telemetry, axes/path/cone/ray/camera gizmos, tuning panel |
| `model.js` | Quaternion dynamics, states, tricks, swept contacts and boundaries |
| `controls.js` | Independent starfighter bindings, gamepad and rebind capture |
| `camera.js` | Rolling chase/cockpit cameras, clearance and view blend |
| `targeting.js` | Explicit lock, hysteresis, assists and missile navigation |
| `ai.js` | Virtual pilot controller through the same model |
| `gear.js` | Landing gear, gear doors and S-foils from each craft's takeoff clip |
| `spawn.js` | Friendly landed attack transport spawn points |
| `pilotless.js` | A craft in flight that nobody flies |

`Vehicles` owns the fixed-step model and weapon/seat integration. `game.js`
connects input, profile settings, debug and transport spawn selection. The
battle HUD (`src/hud.js`) draws a craft's seat as it draws any vehicle's; see
[The HUD aboard a craft](#the-hud-aboard-a-craft).
`ordnance.js` advances flight missiles. The entity protocol carries the flight
state and quaternion (`src/runtime/network-entity-flight.js`); presentation
interpolation stays on the rendering page.

## Controls and behavior

Mouse X/Y drive a clamped, recentering virtual stick. The stick turns and
pitches the craft about its own axes; the craft turns flat, its frame level
and its drawn hull leaning into the turn. Starfighter sensitivity,
invert-Y and response curve are independent from infantry settings. The controls
menu rebinds flight keys, mouse buttons and seat-wheel directions. Pointer lock
uses the game's existing play-session pointer capture.

| Action | Default |
| --- | --- |
| Roll | A / D |
| Throttle / brake; release returns to cruise | W / S |
| Boost | Shift + W |
| Barrel roll / loop | Alt or Right Ctrl + direction / double tap |
| Immelmann | Caps Lock |
| Hold roll lock | Z or middle mouse |
| Take off / land | Space |
| Target lock / release | Q or Mouse 4 |
| Primary / secondary | Left / right mouse |
| Cockpit / chase | V; profile remembers choice |
| Enter / exit | E; in flight E bails out |
| Next / previous seat | 1 / 2 or wheel |

Gamepad: left stick yaw/pitch, right stick X roll and Y throttle/brake,
right-stick click boost, button 0 trick. Normal difficulty enables small-cone
auto-target assistance, shot correction and sticky input; Elite disables them.

The states are `Landed → TakingOff → Flying → Landing → Landed`, and
`Destroyed`. Space works as in the 2004 game's flyer update (docs/formats.md
"Flyer takeoff and landing") and refuses nothing: landed, the craft lifts
off; flying, it starts down wherever it is, at any height or speed; late in a
takeoff (progress past 0.75) it goes back down, and late in a landing (under
0.25) back up. Only a space map keeps its zones: there a landing starts only
inside one (`landingZones`, the port's).

The takeoff is the 2004 game's lift (see [Movement](#movement)): straight up
to TakeoffHeight over the pad, then over to flight. A hull resting on a
sloping pad or ground has corners below its surface; the lift's sweeps look
past what it rests in (`restingTop`), so it is not mistaken for something
overhead, and past what lies inside the box the parked hull fills: a pad's
structure it already overlaps (on Bespin's gunship pad `_16` it reaches into
a corner of the box at mid-height) does not hold it down, as the original
lifts it out. Under a roof the lift slides along it on its way over to
flight; anything else in its way holds it there, TAKEOFF PATH BLOCKED, until
the way clears (a bot's crew leave a craft held over its pad; nobody aboard,
it sets back down).

The landing is the lift run back: the craft goes out of flight over its
LandingTime while it sinks at LandingSpeed and comes to rest on the ground
under its middle (gear on it, else 0.15 m over it), on ground flatter than
0.9375 (about 20 degrees). Going along, its whole hull meets what it runs
into; going down, what lies under its wings does not hold it up (the 2004
game's craft meet the ground with the spheres about the fuselage their ODF
names), nor does what it will rest in, but its fuselage stops on what it
comes down on top of (LANDING PATH BLOCKED). On a steeper slope or over water
(the surface holds a flyer's origin up) it stays on its way down until the
pilot lifts off again; a hull it comes down on sends it back up.

World metadata accepts `flight: {space, ceiling, bounds: {min, max},
landingZones: [{min, max, floorY}]}`. Without explicit bounds the engine derives
the horizontal extent from map collision geometry. No source-game selector
participates in the model. Space omits world-up leveling.

## Tuning

Speeds are metres/second, angular rates radians/second, camera offsets metres.
A craft's speeds, handling, lift and camera are its own ODF's (see
[Movement](#movement) and [Camera](#camera)). The port's own tuning, shared by
class, has its mutable source of truth in `src/flight/config.js`. Add
`?flightDebug` to show live controls and diagnostic geometry; the controls'
commands carry tuning changes to the simulation.

| Class | A/D roll |
| --- | ---: |
| Multi-purpose fighter | 2.8 |
| Scout fighter | 3.4 |
| Bomber | 1.8 |
| Attack transport | 1.3 |

Shared values: stick return 3.5/s, response exponent 1.35. Boost holds
MaxSpeed at twice the craft's Acceleraton (it never passes MaxSpeed) and costs
38% of the turn and pitch rates. The landing prompt (TO LAND, and where a bot
presses the key) shows over ground within 18 m under the hull flatter than
0.9375, where a landing soon settles; the key lands anywhere. Power capacity 100, boost
drain 25/s, recharge 12/s; tricks cost 30, shared cooldown 1 s, input window
.3 s. Barrel roll .6 s with 12 m lateral displacement; loop 1.8 s; Immelmann
1.5 s. The view blends .25 s between chase and cockpit.

Lock range 1600 m, acquisition/retention cones 15°/25°, dwell 1 s,
LOS grace .3 s. Missiles use proportional navigation gain 3.5, speed 180 m/s,
turn cap 1.1 rad/s, lifetime 8 s, seeker half-angle 15°. A powered roll exceeding
8 rad/s inside 300 m breaks seeker tracking; tracking loss is irreversible.
This is the port's own counterplay tuning.

## Movement

Every vehicle's speeds come from its class's ODF, read the way the 2004 game
reads them (`src/vehicle-motion.js`). The page, the world worker, the dedicated
server and desktop host, and a client's prediction copies all build hulls
through it, so a vehicle is as fast online as offline, for the player and for
the AI. A key the ODF leaves unset takes the original's class default, and a
key authored after a later seat's section still belongs to the class (the
AT-ST's AnimationSpeed). The extractor keeps keys it can't name as their FNV-1a
hash (`MinSpeed` is `fbe2eb62`, `MidSpeed` `838bda20`); the runtime reads
either.

- **Flyers** (after the original's flyer update; the speeds and rates are per
  second of the update's own time step, so in m/s and rad/s as written). The
  stick sets a target speed: MidSpeed released, MaxSpeed held forward,
  MinSpeed held back, in proportion in between. The speed along the nose moves
  to it at a constant Acceleraton (m/s²) from the speed the craft is flying at
  along its nose, so a bump slows it and so does the takeoff. The port's W/S
  throttle and release to cruise drive this range. A MinSpeed below zero flies
  backward: the LAAT/i and the MAF hover at a MidSpeed of 0.
  - *Turning.* The mouse asks for a turn at TurnRate and a pitch at TurnRate
    too (the original gives a player on the mouse TurnRate for both; a
    bot's stick pitches at PitchRate). Each rate follows the stick at its
    filter, TurnFilter and PitchFilter: an update moves it the filter times
    the time step of the way. The craft turns about its own axes and its
    frame keeps its wings level, rolling back at LevelFilter (the roll rate is
    LevelFilter times the height of its side axis), so a turn is flat: the
    snowspeeder holds a 26.7 m circle at cruise (40 m/s at 1.5 rad/s) with
    the stick hard over, 13.3 m at MinSpeed. The drawn hull leans into the
    turn by BankAngle times the turn rate times the speed over MaxSpeed (38°
    for the snowspeeder turning hard at cruise, 69° at full speed), eased at
    BankFilter;
    the frame the craft flies by, which the chase camera follows, does not
    lean. A/D roll the frame (the port's; the 2004 game has no roll key) and
    the wings level again when let go; space maps don't level.
  - *The lift.* Space starts it: the craft rises at TakeoffSpeed, an eighth of
    it on its pad and all of it TakeoffHeight up. Once its origin is
    TakeoffHeight above where it rests (LandedHeight) it goes over to flight
    in TakeoffTime, its climb fading and its speed along the nose growing; it
    leaves slow (about 5 m/s) and speeds up at Acceleraton. The stick turns
    it as much as it has risen and pitches it as much as it has gone over
    (the original's own pitch leveling during the lift, added each update,
    is taken at the port's 60 Hz); its nose and wings level meanwhile. The
    snowspeeder rises 4 m and flies 2.2 s after Space, at cruise at 3.3 s.
  - *The landing* is the lift run back: its progress runs out
    over LandingTime, its speed along the nose fading with it, while it
    sinks at LandingSpeed, an eighth of it at the ground and all of it
    TakeoffHeight up; the stick turns and pitches it as in the lift. An
    X-wing (LandingTime 2, LandingSpeed 5) with the throttle released
    glides on about 50 m and sinks 5 m in its first 2 s, then comes
    straight down at 5 m/s.
  - *Defaults* for keys a class leaves unset: MinSpeed 10, MidSpeed 20,
    MaxSpeed 30, Acceleraton 5, TurnRate 2 (the original raises any lower
    TurnRate to 1.5, so every 2004 fighter turns at 1.5 rad/s), TurnFilter 10,
    PitchRate 1, PitchFilter 10, BankAngle .5, BankFilter 2, LevelFilter 3,
    TakeoffHeight 5, TakeoffTime 5, TakeoffSpeed 3, LandedHeight 0,
    LandingTime 5, LandingSpeed 3. The turn
    and pitch keys are the body section's own; a gunner's section has its
    own.
  - *Not ported:* StrafeSpeed and StrafeRollAngle (A/D slide and lean the
    craft sideways in the 2004 game; no fighter sets them, the LAAT/i and the
    MAF slide at 15 m/s; A/D stay the port's roll), ThrustPitchAngle (the
    nose leaning with the throttle; no 2004 flyer sets it), a gamepad's turn
    ramp (its yaw grows from PitchRate to TurnRate over a second of the stick
    held over), and the pull-up near the floor and ceiling (the port
    recovers from its flight ceiling and bounds instead).
- **Hovers** (and the Hailfire): the speed along the hull moves toward
  ForwardSpeed (ReverseSpeed backing up) and across it toward StrafeSpeed, at
  Acceleration with the stick held and Deceleration released. The hover's key
  is spelled `Acceleration`; flyers and walkers use `Acceleraton`. Defaults:
  ForwardSpeed 20, ReverseSpeed 10, StrafeSpeed 15, Acceleration 10,
  Deceleration 5.
- **Walkers and animals** (AT-AT, AT-TE, AT-ST, spider walker, tauntaun,
  kaadu): no speed key moves them. The hull follows DummyRoot's travel in the
  walk clips, each clip lasting its frame count at 30 fps over AnimationSpeed,
  so the pace is the two steps' travel over their frames at 30 fps, times
  AnimationSpeed. Backward plays the same steps in reverse at the same pace.
  The walker class never reads MaxStrafeSpeed: A/D turn a walker. MaxSpeed
  only tells the AI what to expect. How fast a walker starts and stops (its
  Acceleraton, and for an animal its gallop reached in 1/Acceleraton s) is
  the port's.

| Vehicle | Top speed | Released | Held back | Rate (m/s²) |
| --- | ---: | ---: | ---: | --- |
| X-wing, Geonosian fighter | 82 | 50 | 20 | 30 |
| Snowspeeder, TIE fighter, droid starfighter | 72 | 40 | 20 | 30 |
| Jedi starfighter | 72 | 45 | 20 | 30 |
| Cloud car | 72 | 20 | 20 | 8 |
| Y-wing | 60 | 35 | 20 | 15 |
| TIE bomber | 50 | 30 | 20 | 10 |
| LAAT/i gunship, MAF | 45 | 0 | -15 | 20 |
| Speeder bikes | 40 | 0 | 12 back | 30 / 40 |
| Hailfire | 30 | 0 | 16 back | 8 / 10 |
| STAP | 20 | 0 | 8 back, 4 aside | 8 / 10 |
| Gian speeder | 16 | 0 | 14 back, 8 aside | 5.5 / 9 |
| Skiff | 16 | 0 | 12 back, 4 aside | 5 / 8 |
| AAT, fighter tank | 12 | 0 | 10 back, 4.5 aside | 5.25 or 5.5 / 7 |
| Tauntaun, kaadu | 19.8, 19.5 | 0 | the same back | |
| AT-ST | 6.5 | 0 | the same back | |
| Spider walker | 3.0 | 0 | the same back | |
| AT-TE | 2.8 | 0 | the same back | |
| AT-AT | 2.2 | 0 | the same back | |

The flyers' handling (TurnRate as flown; the lift's TakeoffHeight /
TakeoffSpeed / TakeoffTime):

| Craft | Turn rate | TurnFilter | PitchRate (bot) | PitchFilter | BankAngle | BankFilter | LevelFilter | Lift | LandedHeight |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| Snowspeeder | 1.5 | 5.75 | .8 | 2.75 | .8 | 4.5 | 5 | 2.5 / 3 / 1 | .5 |
| X-wing | 1.5 | 5.75 | 1 | 2.9 | .7 | 2.5 | 2 | 2.5 / 3 / 1 | 0 |
| Y-wing | 1.5 | 5.75 | 1 | 1.5 | .7 | 2.5 | 5 | 6 / 5 / 1 | 0 |
| TIE fighter | 1.5 | 5.75 | 1 | 2.9 | .4 | 2 | 2 | 2.5 / 3 / 1 | 0 |
| TIE bomber | 1.5 | 5.75 | 1.75 | 2.75 | .4 | 2.5 | 1 | 1 / 3 / 1 | 0 |
| Droid starfighter | 1.5 | 5.75 | 1 | 2.9 | .9 | 2 | 2 | 2.5 / 3 / 1 | 0 |
| Geonosian fighter | 1.5 | 5.75 | 1 | 2.9 | .8 | 2.5 | 2 | 2.5 / 3 / 1 | 0 |
| Jedi starfighter | 1.5 | 5.75 | 1 | 2.9 | .9 | 2 | 2 | 2.5 / 3 / 1 | 1.1 |
| Cloud car | 1.75 | 2 | 1.75 | 2.75 | .9 | 2.5 | 5 | 2.5 / 3 / 2.5 | 0 |
| LAAT/i gunship | 1.5 | 5.75 | .8 | 1.75 | .55 | 5.5 | 2 | 10 / 5 / 1.5 | 0 |
| MAF | 1.5 | 5.75 | .75 | 1.75 | .5 | 2.5 | 2 | 10 / 5 / 1 | 0 |

The tow cable needs the snowspeeder to circle inside 1.2 x CableLength of the
walker's leg (72 m). With the stick hard over it turns flat at 1.5 rad/s: a
26.7 m circle at cruise, 13.3 m holding S at 20 m/s, a circuit every 4.2 s
either way, so three circuits take 12.6 s.

## Camera

The pilot's chase camera is the 2004 game's, on his section
(`src/flight/camera.js`; the original's flyer camera, through its camera
section). It looks along the craft's nose with the craft's
up, pitched down by TiltValue, and turns toward that at AimTension (1/s); it
rests at TrackCenter in the craft's frame and TrackOffset (right, up, back) in
its own, walls pulling it in, and each of its axes eases there separately at
MoveTension (1/s: the share of the way an update moves it is 1 - e^(-rate x
step); MoveTensionX, Y and Z set one axis, a pair for its two ways). The frame
is the one the craft flies by, so the camera never leans with the drawn hull.
Its field of view is ThirdPersonFOV across, 48° unless the section sets one,
the vertical following the window's shape; it does not change with speed or
boost, and no motion blur is drawn aboard a craft. In the cockpit the eye is
EyePointOffset on the drawn hull, looking along the nose, at FirstPersonFOV.
The port blends a switch between the two over .25 s. The pilot's reticle
marks where the nose points, above the middle of the screen by TiltValue.

| Craft | TrackCenter | TrackOffset | TiltValue | AimTension | MoveTension | FOV, chase / cockpit |
| --- | --- | --- | ---: | ---: | --- | --- |
| Snowspeeder | 0 3 -8 | 0 0 0 | 0 | 10 | 5.5 / 5.5, 2.5 up / 8.5 down, 6.5 | 48 / 60 |
| X-wing | 0 2 -4 | 0 2 6 | 5 | 20 | 4.25 | 48 / 52 |
| Y-wing | 0 2 -4 | 0 2 8 | 0 | 30 | 4.5 | 48 / 55 |
| TIE fighter | 0 8 0 | 0 0 14 | 5 | 20 | 6.5 | 48 / 52 |
| TIE bomber | 0 7 -10 | 0 0 14 | 7 | 20 | 8.5 | 48 / 56 |
| Droid starfighter | 0 2 -2 | 0 2 6 | 0 | 20 | 4.25 | 48 / 45 |
| Geonosian fighter | 0 15 13 | 0 -13 20 | 10 | 20 | 4.25 | 48 / 48 |
| Jedi starfighter | 0 1 -2.75 | 0 2.5 8 | 5 | 8 | 5 | 48 / 65 |
| Cloud car | 0 3.2 -10 | 0 0 0 | 5 | 20 | 8.5 | 48 / 48 |
| LAAT/i gunship | 0 10 -24 | 0 0 0 | 4 | 30 | 4.5 | 55 / 60 |
| MAF | 0 10 -40 | 0 0 0 | 0 | 20 | 4.25 | 48 / 48 |

## The HUD aboard a craft

A craft's seats show the battle HUD (`src/hud.js`) as any vehicle's do, as the
original has it (docs/formats.md, "Seated"): nothing in its HUD checks for a
flyer or for first person. `combat.updateHud` reads the seat the page knows he
holds afresh every frame (`driver.vehicle`, set from the entity records' seat
state, `src/runtime/entity-seats.js`), so leaving a seat by any way takes its
displays with it.

- His own health stays on the top bar; the hull's goes in the panel under it
  with the class's `HealthTexture`, from any of its seats, over a pip per
  seat (his white, anyone else's green). Both fade with the group, as on
  foot.
- The seat's two channels fill the weapons panels; the seat's name
  (`VehiclePosition`: PILOT, MAIN GUNNER, BOMBADIER...) shows on both for
  2 s in place of the weapons' when he takes it. A cannon with no
  `IconTexture` (the TIE fighter's, the Jedi starfighter's) shows no icon.
- Both channels' reticles sit at one anchor: on the screen's centre line,
  at the height of the point 1024 m along the nose (as drawn: the frame the
  chase camera follows, `VehicleDriver.reticleDirection`), so above the
  middle by about `TiltValue` behind the craft and at it in the cockpit.
  Each is coloured by what its own weapon is on (the page targets channel 1
  apart, `updateTargetHud`); one with no art (the Jedi starfighter's
  cannon, the gunship's missiles) is four ticks out to its spread. A
  launcher adds its lock ticks and, locking, the diamond on its target and
  a green bar filling in its panel.
- The minimap turns with the craft's heading and reaches out to 250 m as it
  speeds up.
- Speed, power, altitude and boost have no display; the lock is the
  original's cursor alone; nothing is said on boarding, leaving or
  switching seats.

The port's own, for what its flight model needs, in the place and look of
the original's tool tips (white `gamefont_small` at 80%, centred in the
band from 80% to 95% down the screen; the PC game never shows a tool tip):
landed, the takeoff key and the original's `game.tooltips.flyertakeoff`
(SPACE TO TAKE OFF); low over ground the craft can settle on, the key and TO
LAND (the key lands it anywhere); a lift or descent held up, TAKEOFF PATH
BLOCKED or LANDING PATH BLOCKED while it is; a space map's NO LANDING ZONE
for 2 s; FLIGHT CEILING while the ceiling turns the nose down. All come
from the craft's plain state (`flightState`, `landingReason`,
`flightWarning`); the fade-in time is the HUD's own.

## Seat ownership and destruction

Mounted posing copies aircraft velocity into the soldier. Dismount clears
that velocity, airborne/dive/input state, mounted flags and aim overrides, and
resets interpolation. Release removes the driver, lease and unit seat links
idempotently, including hull destruction without a game callback. Destruction
still kills occupants through the normal combat rules. Respawn is infantry.

A seat change that arrives from the world runs full dismount cleanup. Lease
generations are validated after the state arrives, so old-life leases cannot
remount a respawned player while a valid seat for a new life in the same
packet remains valid. Releasing a seat also clears queued player actions. E in
flight bails out (below).

## Boarding, leaving and seat controls

`src/boarding.js` holds the rules for everyone who sits in a vehicle: the
player (`VehicleDriver`), a network human (`src/runtime/multiplayer-world.js`,
through `Vehicles.board` and his seat lease) and the bots (the Classic AI's
`canEnter` and `boardNow`, through `Vehicles.eligibleSeat` and
`Vehicles.board`).

- What E reaches (`mountInReach`): the nearest hull in reach ahead of him that
  he may board, else the turret he faces. In a jump, fall or jet he waits for
  his feet.
- Who may board (`boardingSeat`, as the original's eligibility test has it,
  docs/ai-vehicles.md): a hull anyone holds, in any seat, takes only that
  crew's friends (team relation above 0). An empty hull of either team takes a
  person. A bot boards only its own team's hulls, and only a free seat. A
  person takes his last seat when it is free, else the first seat that is free
  or a bot holds; the bot moves on to the next free seat or gets out. Nobody
  takes a seat another person holds.
- Seat keys (`switchSeat`): on to the next seat that way that no other person
  holds; a bot there trades places with him. Leaving the pilot's seat lets go
  of the stick.
- Getting out (`mayLeave`, `Vehicles.vacate`, `VehicleDriver.exit`): a person
  may leave at any time. On the ground he is set down at a clear spot beside
  the hull, or stays aboard when boxed in. In flight he bails out beside the
  hull at its height (on the floor below when that is near), at rest and
  falling: the original's seat exit zeroes the soldier's velocity. The
  soldier's MaxFlyHeight push and the usual landing apply; a death region
  below kills him. A bot stays aboard a craft in flight. The last bot aboard
  takes the vacated seat; a person aboard is never moved.
- Killed on his seat (`leaveKilled`, for a bot or a network human through
  `Vehicles.vacate` with reason `killed`, for the player through
  `VehicleDriver.release`): he leaves it at once, where he rode. The original's
  death in a seat runs the seat exit without its search for a spot: the body
  stays at the seat, at rest, among soldiers again, and while the hull lives
  the last bot aboard takes the seat, as after an exit. The port drops the body
  from where it was drawn (under the pelvis, the soles where the lower foot
  was), its death clip under way, so the fall starts on the seat; the player's
  death camera starts as on foot. The original flings a dead rider (9 m/s along
  the round, lifted 0.4 to 0.7) only from a seat whose class sets hash
  `81bce0f9` to 0, which no retail class does, so nobody is flung. The hull
  stays for anyone: a hover with nobody at the controls slows to a stop at its
  Deceleration, as when its driver lets go of the keys.
- A craft nobody flies (`src/flight/pilotless.js`): with anyone still aboard
  it holds its course with the stick let go. Empty, it comes down, its speed
  carried on and gravity bending its path, nose along the path, and is
  destroyed where it strikes the ground, a building, the water or a death
  region; its pad then builds a new one. The 2004 data has no `Eject*` keys,
  so this is the port's choice.
- Seat controls (`seatControl`, `seatInput`, `seatLook`, `seatScheme`): a
  craft's pilot flies it, the driver of anything else drives it, and every
  other seat only aims and fires. The key map (the flight bindings in every
  seat of a craft) and the keys are read for the seat held when they are
  read. The single player step, a network human's intent and the client's
  vehicle replay all build a seat's input with `seatInput`, so a seat change
  earlier in the tick (E, a seat key) cannot leave stale bindings behind.

`BFE_CONTENT_DIR=<game data> node tools/vehicle-matrix.mjs [map ...]` boards
every vehicle class on every imported map, every seat, as the player and as
a network human, through the real command path (E, the seat key, throttle,
takeoff, both triggers, E out), after surveying every craft's pads for a
takeoff. `tests/boarding.test.js` covers the rules.

## Riders in the open

A rider shows on a seat with a pilot animation, and rounds meet him there
unless the section sets hash `2b60e217` to 0 (docs/formats.md "Riders"): the
speeder bikes, the STAP, the skiff's pilot and gunners, the fighter tank's
roof gunner, the gunship's passenger, the combat speeder's crew and the
AT-TE's main gunner, the tauntaun and kaadu riders. The snowspeeder's crew,
the cloud car's, the gunship's ball gunners and the fighters' pilots are out
of reach; the AT-ST, AT-AT, AAT and the other enclosed seats show nobody,
the player included: `VehicleDriver.presentRider` hides his body there every
frame, since each correction from the world shows it again (otherwise it
stands on top of the droid starfighter on Bespin: Platforms).

- What is struck (`Vehicle.exposedSeat`, `SeatLease.exposed`,
  `Unit.updateSpheres`): his body as his seat poses it, from the same clip
  and point in it that `poseRider` draws (`riderSample`), carried down his
  own skeleton at the tick (`src/rider-pose.js`): the head as a sphere (three
  times the damage, as on foot), the torso from the pelvis up to the neck,
  each thigh and shin, and each upper arm and forearm to the middle of the
  hand as capsules (a gunner's arms reach out to the grips well clear of his
  torso, where rounds that visibly hit them passed through). Nothing is
  read from the drawn bones, so a world worker and a dedicated server, which
  pose no bodies, meet him where a client shows him. The original keeps the
  soldier's standing shapes on the seat's node, or for `human_drive` the
  `coll_drive` model's (a body cylinder and a head sphere); those miss the
  STAP's rider, drawn a metre back and up, and float over the animals'
  riders, hence the port's choice.
- Who strikes him: every round the registry's raycast serves (bolts, beams,
  rockets, missiles, shells, the fat ray's sweep), the lag-compensated rounds
  of network humans (`HitHistory`), and melee (the original's swing query
  takes the riders' list), from the player, bots, network humans, turrets and
  hulls. He takes damage and credit as on foot. A blast doesn't hurt a
  soldier in a vehicle (the hull takes it), as in the original. Sight tests
  (`los`, `unitsClear`) still leave everyone seated out.
- What shields him: the nearest hit wins, as in the original's single cast
  over every collision list, so his own hull's ordnance collision shields
  him where it lies in front of him (the combat speeder's canopy, the
  AT-TE's turret, the fighter tank's ring). A riding animal's doesn't: its
  class names no `OrdnanceCollision`, so every primitive it has stops rounds,
  the coarse spheres that keep it and its rider out of walls among them, and
  those swallow the rider on its back. The port lets a round on through the
  animal to him (`Vehicle.shieldsRiders`, `Ordnance.riderBehind`); the animal
  takes what misses him, and his dead body falls through it to the ground.
- A hull's own guns never strike the riders on it (the port's choice: their
  rounds already pass through the hull's own body).

A placed turret's gunner is struck by the same rules, and leaves his gun
the same way when killed there, with one difference the original makes:
a blast hurts him at the gun (docs/live-objects.md "Turret gunners").

`tests/exposed-riders.test.js` covers the shapes, the rounds, the deaths and
the seat; with an extract it fires rifle bolts at every seat of Tatooine's
and Hoth's hulls in the headless world. `tests/turret-gunners.test.js` does
the same for the turrets' gunners.

## Landing gear

The 2004 game reads a flyer's `AnimationName` as the name of an
animation bank and looks up its `takeoff` clip, whose first frame is the craft
landed and last the craft in flight. It poses the model at that first frame
once, when the hull is built, and never again: a 2004 craft shows its gear
down (and an X-wing its S-foils shut) landed and flying alike
(docs/formats.md "Flyer animation"). The port plays the clip by the flight
state instead, so the gear comes up into the hull on takeoff, stays stowed in
flight and comes down on landing.

The stock flyers on the 17 maps (`tools/zero_vehicle.py` exports the clips;
`tests/test_vehicle_retail.py` checks them against the retail archives):

| Craft | Bank | Takeoff clip | First frame (landed) | Last frame (flying) |
| --- | --- | --- | --- | --- |
| Jedi starfighter | `rep_fly_jedifighter` | 80 frames, 2.6 s | nose and side gear down, nose doors open | stowed, doors shut (the model's own pose) |
| X-wing | `all_fly_xwing` | 121 frames, 4 s | gear down, gear doors open, S-foils shut | gear folded, doors shut, each foil 15 degrees open |
| Y-wing | `all_fly_ywing` | 111 frames, 3.7 s | gear down, gear doors open (the model's own pose) | gear folded, doors shut |
| MAF | `cis_fly_maf` | 68 frames, 2.2 s | the gear leg down | the leg in, side flaps 20 degrees down |
| Cloud car | `bes_fly_cloudcar` | 76 frames | moves only bones its model lacks: nothing drawn | |
| Snowspeeder | `all_fly_snowspeeder` | none (one other clip) | modelled pose | |
| TIE fighter, TIE bomber, droid fighter, Geonosian fighter, LAAT/i | none | | modelled pose | |

`src/flight/gear.js` draws the clip at the craft's gear (`gear`, 1 down and 0
stowed; `src/flight/model.js`): held down through the lift, stowed over
`TakeoffTime` once the craft is `TakeoffHeight` above where it rests (as the
original's takeoff progress runs), and down again over `LandingTime` as the
landing's progress runs out. Every takeoff clip also lifts `DummyRoot` by a constant that seats the
original's model on its origin; the port seats a hull by its collision and
leaves that track out.

- **Hosts.** The world poses the clip too, at the craft's own gear: its aim
  tick does (`poseGearAt` in `Vehicle.aimTick`) before the aimers turn and
  the fire points are read, and so does the render pass that `Vehicles.update`
  runs on every host after it. The gear is the craft's flight state there and
  in the pilot's own replay (`src/runtime/vehicle-prediction.js`) and
  correction. A page has every other craft only from its records, which
  carry its flight state but not its gear: the gear follows that state by the
  world's own rule (the same ray down for the lift, 1/`TakeoffTime` stowing,
  the landing's ease over its time), so another player's or a bot's craft
  shows what its pilot sees within a tick (`followFlightState`, called by
  `src/runtime/network-entity-game.js`). Nothing new goes on the wire.
- **Content.** The clips are in each battle's vehicle pack, after every
  walker's so their offsets are unchanged, and the network fingerprint leaves a
  flyer's clips out (`src/runtime/content-hash.js`): a pack prepared before
  them plays with one prepared after. The world reads them too (where a landed
  craft stands, below), but it is the authority on that: a peer without them
  only replays its own craft's landing elsewhere and is corrected.
- **Muzzles.** Rounds leave the fire points where the gear puts them: in
  flight the X-wing's lasers leave its open S-foils' tips (15 degrees out) and
  the MAF's pilot cannons (`hp_cannon1` and `hp_cannon2`, on its side flaps,
  20 degrees down) theirs. The world's and the pilot's page's points agree
  (his predicted bolts leave from his drawn tips): on Bespin: Platforms in
  flight, the X-wing's four laser points and the MAF's two flap cannons are
  0.000 m apart. No other gun of a stock craft hangs from a bone the
  clip moves (the MAF's clip also holds `bone_root`, under its turrets, at
  its own pose). The original's foils never open, so its rounds leave the
  shut foils' tips.
- **Standing on the gear.** A craft with a takeoff clip rests on its gear's
  feet, where it spawns and after a landing: its origin stands the depth of its
  lowest point at the clip's first frame (`gearFoot`, skinned from the class's
  own model batches as a page draws them) over the ground, its collision never
  in it (`standHeight` in `src/flight/model.js`). On Bespin: Platforms every
  one's feet touch the pad. Every other craft rests with its collision 0.5 m
  (`SetAltitude`, unless set) over the ground where it spawns, 0.15 m after a
  landing.
  - The MAF's lowest point is 0.29 m above its origin, so it stands with its
    origin in the pad; its height over the ground (the 1024 m ray down that
    times the lift) starts 0.1 m over its feet instead. A standing craft's
    resting check (what its takeoff and landing sweeps leave out) looks down
    to just under its feet, so the Y-wing and the MAF, whose collision ends
    at their feet, are not kept from landing by the ground they land on.
  - The lift to `TakeoffHeight` is measured from the origin, as the original
    does, so it follows the resting heights: 0.7 m for the X-wing, 2.6 m for
    the Jedi starfighter, 4.7 m for the Y-wing and 10.3 m for the MAF.
  - Every host computes the foot from the same data with additions,
    multiplications and square roots only (no trigonometry), so the world,
    a page's replay of its own craft and every platform agree to the bit
    (the lesson of docs/desktop.md, where Windows' and glibc's `sin`, `cos`
    and `atan2` split the prepared content). The world parks and lands every
    craft; a page only draws where the records put it, and its own craft's
    replay lands it where the world will.

## Acceptance checks and limits

`tests/flight-acceptance.js` runs the actual vehicle/collision/camera modules;
`tests/flight-browser.mjs` runs those scenarios in Chromium with WebGL rendering,
then loads the extracted Bespin Platforms map. Its artifacts go to the folder
named by `FLIGHT_TEST_ARTIFACTS`. These are automated engine checks; subjective
feel and audio quality still need human playtesting. No BF2 space-map assets
are used: hangars, pads and space boundaries use controlled collision fixtures,
not an imported capital-ship playthrough.

| # | Scenario | What it checks |
| --- | --- | --- |
| 1 | Single model | Each craft tops out at its own ODF MaxSpeed and turns and pitches at its own rates under the same class tuning |
| 2 | Hangar takeoff | Under a roof fixture the lift slides along it out to flight, full hull, then speeds up to cruise; a real Bespin fighter also launches unharmed |
| 3 | Cruise/brake/boost | Release returns to cruise, brake respects minimum, boost exhausts the meter |
| 4 | Vertical loop/camera | Full inversion; the camera's largest step stays near .025 rad (about 1.4°), no snap |
| 5 | Banked pull-up | A 90° roll makes local pitch turn horizontally |
| 6 | Flat turn/lean/level | A turn keeps the frame level and its height while the drawn hull leans; after an A/D roll, ground settles to .0005 rad, space keeps .99 |
| 7 | Roll lock | Holds orientation for two seconds, resumes leveling on release |
| 8 | Tricks | All three complete and spend power; Immelmann reverses level, including a smooth exit from a 90° bank |
| 9 | Collisions | A thin wall is hit by the sweep; glancing contact damages without killing, head-on destroys |
| 10 | Boundaries | Ceiling, ground edge and space bounds warn and recover gradually |
| 11 | Landing | Lands at speed on flat ground, a pad and in a space map's zone, and from 120 m up; a space map refuses outside its zones |
| 12 | Lock/missiles | Explicit lock, dwell, LOS grace; at 250 m and full throttle (X-wing speeds), straight flight is hit and barrel and Immelmann evade |
| 13 | Views | .25 s cockpit blend, near plane .03 m, chase pulls inside wall obstruction |
| 14 | Frame rates | Identical position, quaternion and power at 30/60/144 fps |

`npm test` runs the unit tests. `node node_modules/vite/bin/vite.js build --mode desktop --outDir <dir>`
validates the production bundle without copying the local asset library.
The browser runner is `PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs
FLIGHT_TEST_URL=http://127.0.0.1:18612 node tests/flight-browser.mjs` against Vite.

Further checks cover every locally extracted vehicle class (initialization,
movement and every equipped weapon channel); destruction on real Bespin
clearing seat, driver and mounted state and soldier velocity, with infantry
respawn moving normally afterwards; lifecycle order and generation tests;
dumbfire before lock and guidance after lock; gamepad, custom bindings,
power and cooldown; and AI landing from above the landing envelope with full
hull health.

The collision engine exposes rays rather than a spherecast. Hull translation
uses 27 continuous swept samples across its bounds; chase clearance uses five
rays. Very thin geometry between samples and rotation-only wing strikes remain
limitations. Cockpit view hides the exterior because separate cockpit meshes
are not imported. Landing gear is drawn from each craft's takeoff clip (see
[Landing gear](#landing-gear)). Transport spawn
selection is available to friendly human players through the normal spawn UI.

## Seat weapons

Each seat section (`FLYERSECTION` or `WALKERSECTION`, BODY or TURRETn) of a
vehicle class builds its weapons the way the original's seat parser does: a
seat starts at its first weapon slot, `WEAPONSECTION n` selects slot n, and
`WeaponName` fills the current slot. A second `WeaponName` without a section of
its own renames that slot's weapon and its aimers carry on, so the gunship's
`TURRET1` is a single wing laser firing from both wings (barrels taking turns),
not one wing per trigger. A slot's trigger is its channel: the first slot fires
with the left mouse button, the second with the right. No stock vehicle sets
`WeaponChannel` or uses a third slot.

Every seat weapon the original gives is extracted for every vehicle and turret
class across both eras of every stock map; each fires through the E key and the
world's command path (the code the world worker and the server run). The AAT
has two weapons in the original as well: the pilot's LASER BLASTER (side guns)
and the gunner's LASER CANNON (main gun). It has no missile. The fighter tanks
and the Rebel combat speeder carry concussion missiles on the right mouse
button (docs/weapon-targeting.md).

A beam (`OrdnanceBeamClass`) is the one ordnance that reads
`ImpactEffect`: the effect it leaves wherever it touches, a hull or a
soldier as well as the ground (the spider walker's `spiderdroid_ord_exp`, the
gunship's wing and ball lasers, the MAF's and fighter tanks' beam cannons).
Bolts, rounds and grenades that name one ignore it, as the original does.

### Aimers and barrels

An aimer record holds every aimer key between one `NextAimer` and the next, in
whatever order they come: the AT-ST gunner's ODF gives each gun's
`AimerYawLimits` and `AimerPitchLimits` before its `AimerNodeName`, so keys
cannot be hung on the aimer named before them. Under an aimer, barrels take
turns (`BarrelNodeName`, `NextBarrel`, `BarrelRecoil`, `BarrelLength`): each
fires from its own `FirePointName` (`FireNodeName` is the same key), else from
the barrel node itself, `BarrelLength` along it. The barrel step moves to the
next barrel after every round and, past the last, to the next aimer of the
chain, skipping aimers with a `HierarchyLevel` above 0 (parent yaw nodes: the
AT-ST's `cube5`, the spider walker's `turret_y_*`).

- The AT-AT pilot's chin guns are two barrels under `aimer_head`; their tips
  are named by a key (0df83e75, "FirePoint") the original never reads, so
  the rounds leave `barrel_Lcannon` and `barrel_Rcannon`.
- The AT-ST pilot's twin guns fire from `hp_cannon_1` and `hp_cannon_2` in
  turn.
- A barrel that fires kicks back `BarrelRecoil` along its own z, decaying
  over a quarter second; drawn on every page from the shot.
- `FireOutsideLimits` is per aimer (0 unless set): an aimer whose step had
  to be held back at its `AimerYawLimits` or `AimerPitchLimits` fires no
  round (the shot's turn still passes) unless it says 1. An axis
  whose limits are equal is pinned and never counts (the AT-TE main cannon's
  -0 0). The AT-AT chin guns and the spider's laser set it.
- Unchanged and confirmed: the aim point is the camera ray's first hit up to
  512 m (a bot's target's aim point); the reticle sits on the screen's
  centre line at the height of the point 1024 m along the view aim; bolts
  fly straight. BODY `PitchTurnFactor` and `YawTurnFactor` (the AT-AT's 0.5)
  are read by nothing; only a turret seat's are.

### Standing on the ground

A walker with `LegPair` records (the AT-AT and the spider walker two, the AT-ST
one; the AT-TE and the riding animals none) stands on its legs as the
original's leg rays and body update have it (`Vehicle.standOnLegs`): each foot
casts down from `LegRayHitLength` above its `TerrainLeft`/`TerrainRight`
primitive's bottom (16 m for the AT-AT, 5 for the AT-ST, 10 for the spider), a
pair stands at its lower foot (drawn toward the higher once that is over half
the reach up), a four-legged walker pitches 0.01 of its front pair's height
over its back pair's each step (up to 0.2955) and rises to its first two pairs'
mean, the spider rolls toward its pairs' side difference over `WalkerWidth` 22,
and the AT-ST, with one pair, stays level and only rises and falls. Each pair's
side difference (`legSide`) draws that pair's legs toward `leftfoot_up` or
`rightfoot_up`, so on a slope both of an AT-ST's feet reach the snow; a page
works it out for the hulls it draws from the ground under them (`feelLegs`).

Against walls a ground hull is a chain of probe spheres in its collision box,
pushed out of what they meet (`Vehicle.pushWalls`; the port's own, not the
original's). A walker against another hull (a moving body) gives way only as
far as its own probe came toward it that step: the other hull answers for its
own motion. The spheres reach past a walker's body, so a hull stopped by that
body would still sit inside them, and the walker, pushed fully out every step,
would be driven along at the other hull's speed (on Geonosis a hailfire pushed
an AT-TE at 8 m/s). Hovers keep the full push.

Not done: the original's leg blend weight comes from a foot-height table
calibrated at spawn, where the port uses the side value itself; and walkers do
not play `turnl`/`turnr` while turning on the spot.

## The snowspeeder's harpoon and tow cable

The rear seat of `all_fly_snowspeeder` (docs/feature-audit.md "Hoth") turns
the `towcablegun` node aft and fires `all_weap_fly_snowspeeder_reargun`
(base `towcableweapon`: LockOnRange 70, LockTime 0.4, TargetVehicle only, 20
rounds, ShotDelay 3) whose ordnance is a `towcable` with `CableLength 60`
(`tools/zero_weapons.py` exports the hashed key `d9b746b2`).

- The shot leaves the seat's turret node (`Vehicle.muzzleOfSeat`): the
  model has no `hp_fire`, and the hull's nose faced the wrong way.
- `Vehicles.launchTow` casts LockOnRange along the gun. It holds only a leg
  of an enemy command walker: `Vehicle.legAt` takes the hit in the walker's
  frame, refuses anything above the pelvis bones (`legTop`, 12.6 m on the
  AT-AT) and anchors on the nearest leg's column (the `bone_*_thigh`,
  `_calf` and `_foot` bones). A player's harpoon into the body glances off;
  a bot's is re-aimed at the nearest leg (`towAim` aims the bot's shot
  there in the first place).
- The cable's state is plain on the speeder (`towTarget`, `towLocal`, `towLeg`,
  `towAngle`, `towWinding`), so a page showing the world's battle draws it
  (`Vehicles.drawTethers`: the anchor, a point on the legs' circle per quarter
  circuit wound, then the sagging span to the gun). The tick (`updateTethers`)
  winds the angle round the walker; opposite turns unwind it; three complete
  circuits trip the walker (credited to the gunner); the cable snaps past 1.2 x
  CableLength. Nothing between the gun and the walker cuts it: the original's
  cable tests its run only against the walker it holds.
- While the cable holds, the gun takes no trigger (`Vehicle.towHeld`; the
  original's tow-cable weapon answers its fire test no while its cable is
  attached), so a second harpoon cannot let the first go and anchor again with
  the winding back at zero.
- `emitSound("towAttach")` and `"towRelease"` play the sound bank's
  `all_weap_fly_snowspeeder_towcable_attach` and `_release`
  (src/audio/game.js).
- The HUD draws the extra aiming ticks only for a weapon with `LockOffAngle`
  and the target diamond only for a launcher in its acquisition state, as the
  original does. The stock tow weapon has neither, so no range-plus-time marker
  is drawn; see [weapon-targeting.md](weapon-targeting.md).
- The Classic AI flies the original's tow-cable run (pattern 15: an
  ellipse past the walker's legs, then a 25 m circle once `towTarget` says
  the cable holds) and its pilot selects the harpoon for his gunner on the
  pass that puts the walker behind; see docs/ai-vehicles.md "Pattern 15:
  the tow-cable run". A bot gunner rests aft in its arc and fires the
  harpoon only within its reach.

## Checks

`tests/vehicle-assets.test.js` exercises every weapon channel separately on
every locally extracted vehicle class through `CommandControls`, including
release-to-fire weapons, and checks projectile direction and own-hull
exclusion.

`node tests/vehicle-hud-browser.mjs` (Vite with extracted Hoth, Tatooine:
Dune Sea and Bespin: Platforms) boards ten kinds of hull and leaves each by
every way there is (E on the ground and in flight, a seat switch and E, the
hull destroyed, killed on the seat, shot off an open seat, each death
followed by a respawn): seated, the HUD's quads are the hull's; after each,
they are his own soldier's, with no hull icon, no craft prompt and no other
canvas over the game's. `tests/vehicle-hud.test.js` covers the seated
health group, the prompts, the seat names, the reticle colours, the map's
reach and a seat switch on the page.

## Effects and sounds

What a hull shows and plays beyond its model, at Classic:
`src/vehicle-effects.js` (effects), `src/audio/vehicles.js` (sounds) and
`src/vehicle-feet.js` (a walker's footfalls, for both). They run on the page
only and read nothing but what every host gives a hull: whether it is alive,
its health, flight state and velocity, who holds which seat, and the pose
drawn this frame. A battle whose world runs in the local server worker or on
a server therefore looks and sounds the same, and the worker and the server
never draw or play anything. Each module keeps its per-hull state in a
`WeakMap`, never on the `Vehicle`, so a copy of a hull's plain fields from
the world cannot wipe a playing handle
(`tests/vehicle-presentation-replica.test.js`).
A new life of a hull (`aiTarget.generation`) starts both afresh.

### The data

`tools/zero_vehicle.py` `vehicle_fx` reads the whole class into each class's
`fx` in `vehicles.json`. The seat reader (`sections()`) stops at the first
`CHUNKSECTION`, and the flyers keep their thrust, damage and blast keys after
their chunk sections, which is why none of these reached the game before.

| `fx` | From | Notes |
| --- | --- | --- |
| `thrust`, `speed` | `ThrustEffect` (four at most), `ThrustAttachPoint`, `ThrustAttachOffset`, `ThrustEffectMinScale`, `ThrustEffectMaxScale`, `ThrustEffectScaleStart`; `MinSpeed`, `MaxSpeed` (10 and 30 unless set) | The keys after a `ThrustEffect` belong to it |
| `damage` | `DamageStartPercent` (ten at most), `DamageStopPercent`, `DamageEffect`, `DamageEffectScale`, `DamageInheritVelocity`, `DamageAttachPoint`, `DamageAttachOffset` | A record opens at its start percentage; one without an effect is dropped |
| `water`, `wake`, `wakeNodes` | `WaterEffect`, `WakeEffect` (the last of each); the hull's `TerrainCollision` primitives | |
| `feet`, `stomp` | A walker's `TerrainCollision` primitives (bone and point in its frame, in order; `bottom`, where it meets the ground, and `turn`, its turn in that frame); `StompEffect`, `StompDetectionType` (0), `StompThreshold` (0.15), `HydraulicSoundHeight` (1) | `FootstepSound<i>` belongs to the i-th foot. `stomp` is set for every walker with feet (the tauntaun has no `StompEffect`, but its rules time its footsteps) |
| `deathDust` | `DeathDustEffect`, `DeathDustOffset`, `DeathDustDelay` | |
| `explosionOffset` | `ExplosionOffset` | Where the blast goes off, in the hull's frame |
| `chunks` | Each `CHUNKSECTION`'s `Chunk*` keys | `ChunkGeometryName` lower case, as the extras pack files it |
| `dying` (walker) | `DeathAnimationExplosion` (the first four; `07fe74d0` before it was named), `DeathAnimationExplosionTime` (`871eb169`; 0.4 s unless set), `FinalExplosionOffset`; `span`: its `death01` clip's frames at 30 fps over its `AnimationSpeed` | Dropped for a walker without a death clip |

The death blast is `ExplosionName`, else `Explosion`, else `ExplosionDestruct`
(the flyers' name for it after their chunk sections). A walker's
`FinalExplosion` is the class's `finalExplosion`, resolved as the blast is.

`tools/zero_sound.py` also exports the class sound keys `TurningOffSound`,
`WindUpSound`, `WindDownSound`, `HydraulicSound` and `FootstepSound0` to `5`.
Older exports give some of them by hash; `src/audio/vehicles.js` reads both.
In the sound configs a scope that follows another for the same object adds to
it: 46 objects are followed by an empty one, which must not wipe their name
and samples (the AT-ST's and AT-AT's head cannon `FireSound` among them).

### Effects

- Thrust burns from the moment a flyer starts to take off until it has landed
  (`TakingOff`, `Flying`, `Landing`), at its node and offset. Its particles
  are drawn at a scale that rests at `ThrustEffectMinScale` and, once the
  speed passes the point `ThrustEffectScaleStart` of the way from `MinSpeed`
  to `MaxSpeed`, grows in step with it to `ThrustEffectMaxScale` at
  `MaxSpeed`. The original does not stop the growth there, but its flyers
  never go much faster; the port's boost does, so the port stops at the
  maximum. The scale multiplies each particle's drawn size (`Effect.scale`,
  on the CPU and GPU particle paths alike).
- Each damage record burns while the health is at most its start share and
  above its stop share of `MaxHealth`, at its node and offset, drawn at
  `DamageEffectScale`, its particles carrying `DamageInheritVelocity` of the
  hull's velocity on top of their own (`Effect.inherit`). A node the model
  lacks falls back to the hull's root, as the original's attach points do.
- A hover's `WaterEffect` plays on the water's surface under it while it is
  within 4 m of it (the port's height). Its `WakeEffect` would play at its
  `TerrainCollision` primitives, but no retail hover has any, so no wake
  plays in the original either.
- A walker's `StompEffect` plays where a foot comes down, as the
  original's walker foot update plays it: at the foot, level
  with the walker's root, turned as the foot's primitive is (so a planted
  foot's dust rises from the ground, not from inside the foot). See
  [Walkers' feet](#walkers-feet) for when a foot comes down. The effect is
  each map's own: `walkerstomp` and `bigwalkerstomp` are packed into the
  worlds that field walkers, snow on Hoth (`hot1`) and Rhen Var (`rhn1`),
  sand on Tatooine, dark earth on Endor, reddish dust on Geonosis.
- When the hull is destroyed its effects stop, the blast goes off at
  `ExplosionOffset` and its chunks fly (`Debris`, with each one's trail and
  `ChunkSmokeEffect`). A chunk naming a node the model lacks starts at the
  hull's root (the spider walker's six name the model itself). A walker
  with a death animation is dying first (below): its chunks fly when it is
  gone.
- Nearly every hull's chunks name no node: each is modelled in the hull's
  frame, metres from its origin. The port carries each one by its own
  middle (`Debris`, `centre`): it flies out from the hull along the way to
  that middle (tipped up by a quarter of the distance, at its `ChunkSpeed`),
  spins about it, and where it comes to rest it settles over 0.35 s onto
  its broad side, lying 0.15 m into the floor. Carried by the hull's
  origin, which is on the ground under a walker, every piece set off
  straight up, struck the floor in its first frame and lay in its standing
  place: an AT-TE on Rhen Var: Harbor fell, blew up and left a frozen
  outline of itself, two plates hanging 2 to 7 m in the air for 10 s. How
  the original moves a hull's pieces isn't known; this is the port's.
- A walker that played a death animation starts its pieces where its body
  lies: they are carried by its body bone (`bone_root`) from where its
  standing pose has it to where the clip left it (`bodyCarrier`), and none
  starts below the ground it stood on (the AT-TE's body ends 2.5 m lower).
- A dying walker sets off one of its `DeathAnimationExplosion` effects at a
  node of its model, both picked at random, each time another
  `DeathAnimationExplosionTime` has passed on its dying clock, and its
  `DeathDustEffect` rises at `DeathDustOffset` once `DeathDustDelay` has
  passed, if it is still dying then.

### Dying

A destroyed walker (the AT-AT, AT-ST, AT-TE, spider walker, and the
tauntaun and kaadu) plays out its death animation before it is gone, as the
original's walker class does:

1. At 0 health it is dead at once: its crew dies, its damage effects stop
   and `ExplosionName` goes off at `ExplosionOffset`, as for any hull; an
   animal has none and sets off nothing. In the port it is no target from then on,
   nothing boards it and its collision is off. A command vehicle's post goes
   with it then (docs/human-spawn.md, "Command vehicles").
   On the page, a crewman who rode hidden inside it (every seat without a
   pilot animation: the AT-TE's pilot and side gunner, the AT-AT's and
   AT-ST's whole crew) is never drawn again in that life
   (`Vehicles.presentRiders`): he dies with it out of sight. His body still
   dies, falls and is thrown by the blast in the simulation (the AT-TE's
   blast throws its crew 6 m up, flailing over the walker); only its drawing
   is held back. A gunner in the open (the AT-TE's main gunner) dies
   in view as on foot; the blast lifts him about 1.3 m. Once his body is at
   rest and its records stop, it is drawn where it lies
   (`SoldierPresentation`).
2. It stays drawn in its `death01` clip, played at its `AnimationSpeed` (the
   walker class applies it to every one of its clips): 5.8 s for the AT-AT,
   5.37 s for the AT-ST, 4.02 s for the AT-TE, 1.53 s for the tauntaun, 1.7 s
   for the kaadu, 0.1 s for the spider walker, whose clip has 3 frames.
   Meanwhile one of its up to four `DeathAnimationExplosion` effects goes off
   at a random node of its model every `DeathAnimationExplosionTime` (1.5 s
   on the AT-AT, AT-ST and AT-TE, so three, three and two; 0.4 s unless set).
   They are particle effects, not explosion classes: they do no damage. Its
   `DeathDustEffect` clock runs only while it plays, so the AT-TE's
   (5.1 s) and the spider walker's never rise.
   The clip takes over from the pose it was last drawn in alive over its
   first 0.3 s (an eighth of a shorter clip; the port's), so a walker
   destroyed mid-stride doesn't snap its legs into the clip's first frame.
3. When the clip has played, `FinalExplosion` (else `ExplosionName` again:
   the spider walker's blast goes off twice, 0.1 s apart) goes off at
   `FinalExplosionOffset` in its frame, with its damage, and it is gone; its
   chunks fly. Its pad makes the next one only after that (the port's).

The AT-AT's `small_explosion` under the unread key `215ce4d0` is not one of
them.

In the port the simulation keeps the clock: `Vehicle.dying`, the seconds of
the clip left (a plain field), counted down by `Vehicle.dyingTick` in
`Vehicles.update` on every host, which sets off the final blast. Whoever
draws the hull poses it from that clock (`Vehicle.present`, on the page,
whichever host runs the world); `src/vehicle-effects.js` times its
dying explosions and dust from it too, so a slow page or a client shows
the same count. Online the vehicle record says `Dying` and carries the
milliseconds left in its `power`; the client draws the dead hull until its
tombstone (docs/network-replay-contract.md).

### Sounds

- Turning on: someone takes the pilot's seat of a hull that is off or turning
  off. `TurnOnSound` plays and the engine starts.
- Turning off: the pilot's seat is empty and the hull is landed (a flyer still
  in the air keeps its engine). `TurningOffSound` plays (R2's whistle) and,
  `TurnOffTime` later (3 s unless set), `TurnOffSound`; the engine stops.
- A flyer that is on plays `TakeoffSound` (`WindUpSound`) as it starts to take
  off and `LandSound` (`WindDownSound`) as it starts to land.
- The engine, `EngineSound`, is a parameterized sound: each layer's gain and
  pitch is the product of every input bound to it, each through its graph.
  Speed is the speed over `MaxSpeed` (or `ForwardSpeed`), Health the share
  left, and Acceleration the change of speed. `TurnOnTime`, `TurnOffTime`,
  `WindUpTime` and `WindDownTime` count the seconds since the last of those
  sounds; an input the hull's state does not drive is passed as a raw 1,
  which leaves its layers alone, as the original does.
- `HurtSound` when a hit takes more than 2% of the health left;
  `DeathSound` when the hull is destroyed, and its engine stops at once.
- `VehicleCollisionSound` when the speed drops by more than
  `CollisionThreshold` (5 m/s unless set) in a simulation tick (1/60 s;
  a page drawing slowly measures the drop against the time it took),
  outside a flyer's takeoff and landing.
- A walker plays `FootstepSound<i>` where its i-th foot comes down and
  `HydraulicSound` as a foot lifts ([Walkers' feet](#walkers-feet)).
- Turret seats (every seat after the pilot's): `TurretActivateSound` as
  someone takes one and `TurretDeactivateSound` as it empties, as the
  original compares each turret's controller with the last frame's. While
  manned, the aim's turning whirs, with the seat's `TurnRate` and
  `PitchRate` as the top rates. Placed turrets are the same turret
  component in the original and go through the same code
  (docs/live-objects.md, "Turrets").
- An engine, the whirs and the footfalls sound only within the engine
  layers' reach of the camera (plus 10 m); the state keeps running out
  there, and an engine comes back without a `TurnOnSound`.

The 2% hurt share, the collision rule and the 4 m water height are the
port's readings; the original's own rules for them are not known. The
footstep, hydraulic and stomp moments are the original's
([Walkers' feet](#walkers-feet)).

### Walkers' feet

`src/vehicle-feet.js`, after the original's walker foot update (run while the
walker walks or turns; the class keys and their defaults are the original's):

- A foot is where its `TerrainCollision` primitive meets the ground: its
  centre less its up axis (the primitive's Y) times half the height it
  reports (a sphere's diameter, a cylinder's height, a box's second size),
  and its height is that point's over the walker's root.
- It comes down by the class's `StompDetectionType`. 0, the default: as it
  falls through `StompThreshold` (0.15 m unless set; the tauntaun's 0.5)
  above the lowest it has been. 1 (the AT-AT, AT-TE and spider walker):
  when, after falling more than 0.1 m in an update, it falls less. Standing
  still clears type 1's state, so as such a walker sets off every foot
  stomps at once. Coming down plays the stomp effect and
  `FootstepSound<i>`.
- It lifts as it rises past `HydraulicSoundHeight` (1 m unless set; 0.5 on
  the AT-AT and spider walker, 0.2 on the AT-ST), once a rise:
  `HydraulicSound`.

The rules read a foot's travel per update, so they hang on the frame rate,
and the original plays the walk cycle at one pace (`AnimationSpeed`). The
port reads them once per class at 60 updates a second over its walk clips,
the fall measured over any 1/60 s, as the phases of the cycle where each
foot comes down and lifts. A hull's walk phase as the page draws it
passing one of them is the step, at any frame rate and however its records
arrive; a type 1 foot comes down only once it has armed since it last
did. At 60 updates the AT-AT, AT-ST, spider walker, tauntaun and kaadu
step every foot every cycle; the AT-TE's front-left and back-right feet
never fall 0.1 m in 1/60 s (they come within 6%), so only its other two
stomp as it walks (all four as it sets off). At lower frame rates the
original stomps them too.

The walk cycle is drawn on the page by `Vehicle.present` for every hull
(`poseCrew`: the walk cycle, the manned seats, the gear), the one he drives
included: nothing on the page runs `aimTick`, and no record poses his own
hull. His hull's prediction (`src/runtime/vehicle-prediction.js`) advances
its walk cycle with its ticks, as the world's `aimTick` does, and is drawn
between its last two ticks, legs and hull alike. The page also keeps the
hull's `age`, the clock of the idle clip, the repulsor's bob and a rider's
clip, which no record carries.

Not ported: the stomp's camera shake for the people aboard (four class keys
not yet named, `ba714b21`, `8aeb89da`, `3799c3f1` and `4e7dc4e6`; no stock
class sets them, and at their defaults it never shakes), `FootWaterSplashEffect`
(`watersplash_walker` unless set, an effect no level holds) and the water
ripples a foot makes, `HydraulicLowerSound` and `HydraulicLowerHeight` (no
stock class sets the sound), and the start and stop steps and turns on the
spot (the walker class's transition and turn clips, `idle_to_leftfoot`,
`turnl` and so on: the port blends between idle and the walk cycle and
plays no clip for a turn on the spot).

### Water

Walkers and the droideka are the only classes with water damage keys
(`WaterDamageInterval`, `WaterDamageAmount`; the droideka also
`UprightWaterDamageHeight` and `BallWaterDamageHeight`). Every stock walker
sets 1 s and 10: the AT-AT, AT-TE, spider walker and the three AT-STs.
Hovers and flyers have none. `src/vehicle-water.js` follows the original:

- **Wading:** a walker's leg rays meet terrain and buildings, not the water, so
  it walks in on the ground under the surface. The foot update counts a foot
  within 0.3 m of the surface, or under it, as a foot in the water (the splash
  itself, `FootWaterSplashEffect`, is not ported).
- **The test** (in the walker and droideka updates): over a water cell, with
  the surface at or above the origin's height plus a depth, a clock runs down
  from `WaterDamageInterval`. Below zero, every update deals
  `WaterDamageAmount` through an empty damage record (no one credited). Leaving
  the water sets the clock back to the interval; nothing else does, so after
  the first second the amount comes every update, 600 a second at the
  original's 60 updates (the port deals that per second at any step). An AT-ST
  (no MaxHealth: 1000) in deep water lasts under three seconds, an AT-AT
  (30000) under a minute.
- **The depth:** a walker's is its model's bounding sphere radius: 5.5 m for
  the AT-ST, so the water has to reach its cockpit. The droideka's is
  `UprightWaterDamageHeight` standing (1.5) and `BallWaterDamageHeight` rolled
  (0.5); the class defaults are 1 s, 0 damage and 1.5 m.

Water is a terrain layer on Kamino, Kashyyyk: Islands and Docks, Naboo:
Theed, Yavin 4: Temple and Jabba's Palace. Walkers meet it on Kashyyyk:
Docks (the spider walker, the AT-ST) and Yavin 4: Temple (the AT-ST);
droidekas in the Clone Wars battles of all six.
The world applies the damage (`Vehicle.tick`, `src/game.js` for every
droideka); hull and soldier health travel as for any damage, and a client's
own predicted walker never deals itself damage (`vehicle-prediction.js`).
`tests/vehicle-water.test.js` covers the clock, the depths and an AT-ST
wading in and being worn down.

### Not drawn or played yet

- `StompDecal` footprints. The walker's draw adds its `StompDecal`
  (`decal_atat_footprint` on the AT-AT, `decal_atst_footprint` on the AT-ST,
  `decal_tauntaun_footprint` on the tauntaun and kaadu) at a foot that came
  down, `StompDecalSize` across, through the decal manager, which looks the
  name up among its `Decal` classes and adds nothing when it finds none. No
  level of the Steam copy defines one (no `Decal` configuration, and no class
  or texture of those names), so the original draws no footprints either, nor
  the explosions' `decal_explosion`.
- The camera shake the walker class reads for its dying (its keys are not
  named); the dying explosions' following of their node (they play where
  the node is when they go off).
- The vehicle music keys (`AllMusic`, `ImpMusic`, `RepMusic`, `CisMusic`,
  `MusicSpeed`, `MusicDelay`) and the speeder bikes' `BlurEffect`.
- `ChunkSmokeNodeName`: a chunk's smoke plays at the piece's origin.
- `FinAnimation` (`ef563a98`): the flaps' 9-pose clips of the snowspeeder
  (`all_snowspeeder_9pose`), both fighter tanks (`rep_fightertank_9pose`),
  both speeder bikes (`imp_speederbike_9pose`) and the skiff
  (`tat_skiff_9pose`). Each clip is in a small bank named after the hull
  (`all_fly_snowspeeder` in `SIDE/all.lvl`, `rep_hover_fightertank` and
  `imp_hover_speederbike` in `SIDE/imp.lvl` and `rep.lvl`,
  `tat_hover_skiff` in `TAT/tat1.lvl`), which `vehicle_fx`'s walker clip
  export could write the same way. The flyer and hover readers take the
  key; what drives the nine poses at runtime is not known yet.

## Hovers on the ground

The retail hover physics uses the class's `AddSpringBody` spheres. The
exported spheres carry individual travel and lift/pitch/roll factors. Gravity
and drive act before contacts and spring forces; the hull pitches and rolls
with its supports.
Grip depends on support and the hull's up vector. A fighter tank stalls and
slides on steep ground instead of driving up at its level-ground speed.
Without springs, the class uses its altitude lift spring and damping.

Ground correction schema 10 carries tiltRate, rollRate and support along with
position, velocity and orientation. The extractor exports springs and walker
foot bottoms. Rhen Var Harbor has vehicles; Citadel does not.
The Hailfire collision-mesh centre remains a separate parity finding;
walkers' body pitch from their legs is in "Standing on the ground".
