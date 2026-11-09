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

## Verification

```sh
~/bfe-work/tools/rbuild.sh cargo test --locked -p bfe-render
```
