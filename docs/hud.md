# Battle HUD

`bfe-render::hud_model::HudModel` owns the battle HUD's display state and
geometry. `bfe-render::interface` supplies retained quad batches, transforms,
bitmap fonts and stack number labels. Native hosts use both modules directly.
The `bfe-render::hud` pass consumes the resulting vertices and ordered
texture/mask draw records with its Rust-owned program.

The client writes scalar game facts into a reusable generated input block. Its
schema is `rust/sim/schema/hud.json`; `cargo xtask schema` generates the
standalone layouts in `rust/sim` and `hud_model/layout.rs`. Weapon,
entity, objective, objective-count, team and state rows use numeric asset/text handles.
Fonts and their glyph pages are registered once. The block length is checked
when the host attaches. Input publication writes changed scalar values only.

`hud_model_update` consumes latest-value state rows and advances visual clocks
and radar discovery. `hud_model_build` consumes state received since that update
and generates the drawn groups without advancing time. Health, secondary/hull
bars, supply, enemy health, stats, banners and squad state overwrite retained
rows instead of adding one command per preview call. Changed health/hull and
stat samples also enter `hud_model_observe`: their intermediate losses/direction
are transition feedback even if the final bar value repeats. Unchanged samples
stay local; observations are consumed immediately without storage. Messages, kills, hits,
damage, shots and map toggles enter `hud_model_event` immediately, including
during loading. There is no event queue whose capacity depends on frame rate.
The native feed keeps the original newest five lines; hits accumulate and radar
shot windows update in place.
The host supplies weapon lock and objective progress; the HUD never advances
simulation or chooses gameplay targets.

Radar eligibility, living counts, sensors, shot flashes and discovery decay use
authoritative positions. Map markers use the current visual positions and hull
quaternions at draw time. A visual interpolation cannot reveal an enemy outside
its authoritative sensor range. Numeric identity and generation retain discovery
within a life and reset it on reuse. Host slots are reclaimed after an absent
update, so entity lifetime churn cannot exhaust a battle-long ID counter.
Definitions cache parsed map/sensor numbers at first observation, and team
relationship, locals, tickets and living tables are supplied once per update.
Oversized source/objective lists clip in source order; art and quad exhaustion
skip excess drawing rather than abort a frame. The radar retains its original
450-candidate limit. Buffers are allocated before frames; native
fixtures cover the complete update/build path with an allocation counter.

Battle HUD output is copied directly between Rust model and render buffers.
The model lives outside the GPU renderer, preserving clocks, messages,
fonts and radar state during context reconstruction. Restored resource handles
keep their logical IDs and force a full GPU upload before the next HUD draw.

## Battlefront II definitions

The installed `ingame.lvl` supplies the transform tree and one element tree per split-screen player count. The single-player viewport describes bars, anchors, map templates, text, equipment bindings and script events. The three-player tree contains only a mask. Split-screen rendering is pending. [Interface formats](formats-2005-interface.md)

## Verification

```sh
cargo test --locked -p bfe-render
```

## Battlefront II battle screens

The developer example selects the 2005 battle screens for a recorded map. Local Space Assault also selects these screens when the player boards a fighter. The scoreboard examples use the supplied recording. The 2005 interface reads its viewport, colour ramps, equipment art and localized captions from the prepared family root. The selected look supplies the fonts and panel colours. A 2005 battle deploys through the spawn launcher both games share ([spawn-screen.md](spawn-screen.md)); these screens draw nothing while the player chooses.

Each draw borrows a battle observation from the recording. Health, stamina, map contacts, target bounds and equipment energy interpolate between adjacent 60 Hz samples. Deadlines and message expiry use simulation ticks. The observation supplies team scores, victory conditions, hero health and message audiences. A roster hero uses the ordinary health bar. A hero awarded during play uses the hero meter. Developers can drive the interface with a recorded sequence while the 2005 family is unavailable in the player menu.

The local Battlefront II scoreboard keeps the connected player’s profile name and team through death and redeployment. That player appears on the opening score page. Bots use their current class names. Hero Assault scores each character by the points its mission adds to the killer’s team during the death callback. Team changes outside a death callback have no personal recipient.

A pilot sees hull, throttle, boost and the two current vehicle weapons. A fixed gun uses its own hull, equipment and target. Lock acquisition fills under the projected bracket. The Space Assault display groups the mission's ordered system icons by team, showing hull, optional shields and destruction. When a pilot can land or take off, the vehicle observation supplies the action text. The screen adds the current primary starfighter binding in the selected language. Clearing that binding leaves the action text visible.

The `battle_screens` example accepts a prepared root, a JSON observation sequence, a look (`bf1`, `bf2` or `eternal`), a locale, a framebuffer size and an output directory. It uses the saved Battlefront II bindings for both the displayed key and the recorded device input, with a fixed interpolation fraction. After a binding edit, call `Presentation::apply_bindings` with the updated interface settings before drawing again. Its recording adapter borrows mission rosters and observations from the input document. On Linux it writes the GPU framebuffer directly to PNG.

In a vehicle the system icons are accompanied by the selected team's system names and hull percentages. Targeting a friendly ship switches the readout to that team. Destroyed components remain in their mission slots at zero hull. Mission objectives wrap at the current font's measured width. Only whole system rows that fit between the objectives, timers and lower target instruments are shown.

The small map stays centred on the player and turns with their heading. Terrain, contacts and the north mark turn together under the upright field-of-view cone. Up to 128 contacts inside the circle are drawn. Headings interpolate along the shortest arc. An equipment name appears on a change of slot and fades on the viewport's schedule. An empty round count uses the unlimited-ammunition mesh. The recording can supply an image for equipment without a dedicated HUD icon. The example uploads that image. A model silhouette must already have been projected when the recording was prepared.

During play the reticle, target, gauges, radar, system grid and scores have first claim on the overlay batch. Up to four objectives share twelve text rows. Each visible objective gets a line before any continuation uses the draw budget. Up to three timers follow, with the countdown reserved before the label. The named system readout and the four most recent visible notices share the remaining space. Labels shrink to a nine-pixel minimum before shortening. Narrow strokes align with device pixels in small windows. Scoreboard captions are bounded to 32 glyphs, with space reserved for the actions and visible rows. The draw reports the number of omitted glyph layers and quads when it reaches a presentation budget.

### Hero Assault observations

After a completed simulation tick, call `BattleView::for_hero_assault` with the tick, phase, local player and team, map caption and texture, both team records and the admitted spawn choices. Attach the borrowed contacts, active objectives, selected timer, unexpired notices and scoreboard rows. Publish these together so every display frame sees one complete observation.

`Body::from_infantry` takes two fixed-tick samples, health and stamina limits, the hero role, selected slots and reticle. Interpolation begins anew on a different match, slab lease or gameplay generation, or across a missing tick. Keep the equipment change tick until the selected weapon changes. A recorded result can coexist with the last living body. Use the authority's deployment phase to decide when to open spawn selection.

A runnable example assembles spawn-phase, play and result views from plain values, including an audience-specific message, a kill, a signed stamina value and an active timer. Run `cargo run -p bfe-battle-ui --example hero_battle_view` in the Rust workspace. [Example source](../rust/bfe-battle-ui/examples/hero_battle_view.rs)

### Local Space Assault instruments

The developer Space Assault start opens the selected family's prepared battle
screen at admission. A pilot or transport gunner sees the occupied craft's
hull, airspeed and boost energy, the selected equipment on each seat channel,
and any acquired target. Heat and finite clips come from the mounted weapon.
The radar places living fighters on the prepared world chart using its
map-bounds region. Without a chart it uses the playable bounds. The
mission supplies team points, their visibility and the target score. Gauges and
projected target bounds interpolate consecutive observations of the same life.
The pilot's landing permission displays the current starfighter binding over
an eligible pad. Each marked critical system keeps its place in the mission's
registration order for its team. The current building life supplies its hull
percentage, shield and destroyed state. When the system grid shares the
radar's corner, it moves below the chart so the pilot can still see contacts.
Leaving the seat removes the instrument panel.
