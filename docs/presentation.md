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

In a Battlefront II battle the visual loader opens the selected mission's
particle library during scene admission. It binds its model buffers, textures
and placed emitters to the existing particle renderer. Both games submit one instance of each prepared particle batch to the battle framebuffer,
using a separate loading frame for each vertex layout, texture and blend. The
window keeps drawing the loading screen while, in a Battlefront II battle, a shared graphics context specializes the
particle programs; a 2004 battle draws its batches on the window's own context and opens no second one. A fence completes the display context’s final batch before play. If the
driver rejects a warm draw, ordinary particle drawing initializes it in play.
The reserved instance buffers retain their capacity as the live count changes.
A Battlefront II streak joins consecutive particle positions from the same
emitter. A turn bends the strip. A parked socket whose particles have no
motion of their own uses no contrail slots or particle budget.
Each mission particle batch has room for the full 4,096-particle pool. The
renderer overflow counters include `particleRows` when a producer rejects a row.
When sockets exceed the available handles, each ship receives a proportional
share. Ships keep their extra socket while eligible. The camera still sets
the emission rates each frame.
Particle clocks advance
on display time. The headless world does not read these files.

Battlefront II ordnance resolves a separate group for each published contact
surface. Its generic contact effect fills empty surface bindings. A launch
can play the firing weapon's authored muzzle and discharge groups. Contacts
and force activations without a published event can be presented by their
source class, property, tick, serial, position and surface normal. Vehicle
observations and building health summaries drive exhaust and damage sockets.
Recorded system poses use the same display-time interpolation as vehicle rows.

An infantry weapon's mesh flash uses its flash color and duration and follows
the posed barrel. The Battlefront II effect manifest identifies its shared mesh.
When the actor has no posed gun, the launch position, muzzle offset and firing
direction place the flash in the world.

Placed Battlefront II light beams taper between the socket's two authored
ellipses. The camera-facing flare and the fading beam share the existing
additive vertex-color program. Each placement uses its authored draw distance.
Continuous recorded activations can carry a release tick so already emitted
particles finish their tracks when the source stops.

The native view keeps the Battlefront II mission effects beside the battle
content used to assign network IDs. An observed mounted shot can name the
firing weapon and its ordnance slot.
The view draws its bolt on the display clock using the same Classic or
Enhanced style as a replicated shot. A recorded terminal supplies its tick,
contact point, surface and optional normal. The visual styles do not occupy
network definition IDs.
The battle names a saber contact's effect itself and sends it as its named-effect
event, so a local battle and every online client draw it alike, at the contact's
point and tick: a blade on a body shows the attacking weapon's `HitEffect`, a
thrown blade on a body the thrown ordnance's `ImpactEffect`, and a guard that stops
a blade or a round, or turns a round aside, its own weapon's `DeflectEffect`.
A blade whose combo plays its explosion (a landing slam) shows that
explosion's `Effect`, or its `WaterEffect` below the water line, once,
upright, 0.2 m above where the battle put the wielder, and flashes the
explosion class's light. Power effects (push and pull rings, lightning,
smolders, landing slams) are drawn from the battle's published `power`
events, so an Instant Action client and a server's clients draw the same.
A burn (a flamethrower's) is published four times a second with its effect
and the seconds it has left; the client puts one copy of the effect on each
`SmolderBone` the weapon names and keeps it on the body after its death when
`SmolderVanishDeath` is 0.

A Battlefront II bolt class with `FadeLightColor` changes colour in flight:
from `DamageTransitionDelay` for `DamageTransitionPeriod` seconds its glow
moves from `LaserGlowColor` to `EndLaserGlowColor` and its light from
`LightColor` to `EndLightColor` by the same measure. The draw gives each bolt one
of 32 prepared steps by its display age; the wire is unchanged. An explosion
class's `LightColor`, `LightRadius` and `LightDuration` flash a light where
and when the explosion goes off, once per published explosion.

A Battlefront II blurred particle samples the scene behind its textured mask.
Visible blur batches use one color copy of the current view, with the strength
and sampling level selected by the emitter. Its authored lifetime, size and
orientation still run through the shared CPU particle player.

Battlefront II muzzle presets select the mesh radius, flash duration and light
color. Weapons can override these and the mesh tint. Posed guns and observed
fire points resolve the same settings and fade on the fractional display tick.

Battlefront II can display up to 4,096 particles. Combat uses the pool first
each frame. Exhaust, contrails and damage effects can use the smaller of the
remaining capacity and three quarters of the pool. These continuous sources
have their own bounded handles, emitters and commands. A simultaneous combat
burst can immediately reclaim particle capacity from existing trails.

A socket's charge is its mean burst rate times particle lifetime, including
child emissions, with a one-eighth margin. Under pressure every eligible socket
gets a share of the birth rate. The remaining share goes to craft inside the
drawn camera's frustum in distance order, followed by those outside. An
eight-percent exit margin and a 180 ms rate response keep existing trails
continuous as the camera turns. Particles already in flight age in display
seconds. The separate limit of 1,023 mounted handles applies after thinning.

A fixed-step presenter can submit saber body contact, a guard block, a
deflection, a surface impact, a class explosion or a weapon fire point using
the published class hash, event serial, tick and world point. The mission
resolves the weapon or ordnance slot. Optional contact normals orient the
burst. A sustained power stops on the release tick for its activation serial.
Exhaust takes a life-qualified pose and observed thrust. Damage mounts take
the affected component's pose and hull fraction. Retiring the life releases
both sets of sources. A slot's generation advances modulo 2³². Differences
from 1 through 2³¹−1 are newer. Older or exactly half-range observations cannot
replace a live body, even across separate engine and hull publications. The
most recent retirements are retained for the receiver's fixed record window;
a new battle epoch clears this history.

A complete ship endpoint uses `ship_state` once per fixed step. Use the
independent engine and damage endpoints when publishing those streams
separately. A terminating bolt contact uses `weapon_contact`; a destroyed
craft or component uses `body_destroyed`. For an armed stationary charge,
`arm_charge` starts its trail. Use the same activation serial on disarm or
when `trigger_charge` both ends the trail and starts the class explosion.

Exhaust follows the thrust observation. A contrail can continue above its
authored minimum speed while the ship coasts.

A Battlefront II map can also enable camera-relative space dust. Travel
distance drives emission from a plane ahead of the camera. The shared CPU
streak program draws the configured image. Speed sets streak length, and
a camera cut clears the field.

In local Battlefront II space battles each simulated weapon launch has a
presentation serial, a muzzle position and a weapon class. The space
collision step supplies the contact position and its normal when available.
The drawn flight ends at that position, inside the tick when it lies between two
steps: the fastest rounds cross hundreds of metres in one. A contact plays its
surface effect and the explosion its round names, including one that does no
damage. Fading rounds use the sniper trace, which has room for every round a
frame can draw.
Fighter exhaust follows the mounted model and its throttle or boost input.
Each hull and each mounted round is one `ship_state` body per fixed step at
its published end point, retired on the fixed step its life ends. Rocket
trails follow the two published positions on the display clock.
Hull damage and destruction select the admitted class's own effect bindings.
When the mission reports a critical system killed, its placed building selects
the destroyed model and emits the class's destruction effect. Interior systems
use the same observation as exterior systems. An animated ship keeps the
selected model on its moving placement.

The mission supplies its normal infantry population. A developer can set
`BFE_DEV_SWBF2_ASSAULT_BOTS` to select a bot count per team for a local match.

The local Space Assault start also boards two opposing starfighters with
scripted stand-in pilots. Two native soldiers from the opposing roster start at
their pads and board through the regular life-qualified seat entry. They fly a
short circuit and make firing passes using ordinary seat inputs. Their hulls can be locked,
hit and destroyed through the space simulation. While acquiring or
holding a fighter lock, the shared HUD marker follows that target's current
life.

On foot, Use at a gun's interior terminal seats its operator. A terminal or a
linked gun that has a team admits only that team. The space missions leave
both neutral, and the rounds belong to the operator's side.
The turret bindings aim and fire the linked
gun through the chair and gun receipt. The camera follows its interpolated yaw and pitch. Use releases the
chair at the entry position.

For the gunship's guided-rocket station, set `BFE_DEV_SWBF2_GUNNER=1`
before starting the developer battle. Use boards the first gunner seat
within reach. A scripted friendly pilot takes the pilot station and flies
the transport while the player aims, fires and steers the rocket.
