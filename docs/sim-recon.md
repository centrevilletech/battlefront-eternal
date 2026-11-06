# Authoritative recon service

`recon::Authority` implements the authoritative recon lifecycle over generated `Store.recon_context` and
`Store.recon_droid` slabs. It is a native service for the unit coordinator;
every headless battle runs that coordinator. Page prediction continues using
its private JS recon replay.

Prepare numeric droid, weapon-level and strike definitions once at load with
`DroidDef::read`, `WeaponLevel::read` and `StrikeDef::read`. Resolve ordnance,
explosion and weapon handles against their owning stores. Reserve query and
shot scratch maxima with `reserve_scratch` before simulation starts. `World`
implements native collision, water, F1 fire and immediate explosions. Its
`fired` operation appends the remote-shot presentation notification after fire.
These operations must never call JavaScript or reenter the WASM module.

Authority construction also reserves the largest prepared loadout per admitted
slot. First launch consumes that buffer; reusing a dead slot recovers its old
weapon storage and resets every weapon in place. The coordinator must reserve
the generated droid and immutable owner slabs before admitting launches.

Attach a context using the world's stable numeric owner id and the shared
qualified Unit key. The caller supplies that owner's independent initial RNG
seed. Rebind on a new life; existing droids and salvos keep the launching key.
Weapon indices are scoped to that captured life, so a new arsenal cannot
reacquire a previous life's droid through an equal numeric index.

`World.capture_owner` creates the generated immutable owner row at launch and
publishes its creation notice before a later projectile or damage notice refers
to it. Each droid retains this handle. Fire and explosion services receive that
captured owner, including after a context rebind or while a dead droid owns a
detached strike. They must not recover attribution from the current Unit life.

Reserve admission before consuming a remote weapon's shot. Launch commits the
reservation; failure must cancel it. Network identities use the first reusable
slot, with a generation that advances only on commit. A dead droid with a salvo
keeps its slot until the detached strike finishes. Cancelled reservation tokens
cannot act on a later reservation of the same slot.

Call `step` for active units at their existing pre-combat remote point. Freeze
and consume the relevant control edges at that observation point, preserving
the use/weapon-switch short circuit and zoom before fire. Mouse samples enter
through `look`; first-person mode lives on the context row. After all units and
vehicles/seats, call `finish`, then separation. Tick deduplication prevents a
context from advancing twice; `finish` also advances retired/dead owners. Native
fire and explosion calls see committed rows and may apply depth-first blasts.
Static-only casts implement blast cover; moving hulls do not shield droids.

Changed reference lists expose touched contexts and droids for sparse mirror
publication. Clear them after publication. A changed context whose row is gone
was retired. Publishing mirrors must not advance cameras or recompute flight.
The unit coordinator supplies the phase entry, command boundary, native world
services, sparse mirror binding and event delivery.

`recon_authority` runs without game content. Its whole-phase golden test
checks 240 updates each over ground, water, crash and triangle geometry, with
first-person/chase views, firing, countdown, owner death, rebinding, detached
strikes and blast. The synthetic fixture preserves the original little-endian
scalar observations as a word count and bit-sensitive hash per tick. Separate
fixtures check admission, tick deduplication, old-life weapon exclusion, nested
explosion order, static cover and warmed allocation counts. Live battle hashes
and authority observations are checked separately against recorded scalar observations.
