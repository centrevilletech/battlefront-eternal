// Immutable, pooled hitbox samples; never move live physics back in time.
// The retained window is 48 ticks (800 ms), but a shot may only rewind as
// far as its own client's pipeline justifies. How far back a client sees is
// the round trip (the press travels to the server while the bodies it saw
// had already come from it) plus its input lead past that, plus its playout
// delay, which can trail its target by up to the playout clock's snap before
// it jumps (playout.js), plus 2 ticks. The lead measured at the server is
// only the part past the round trip (the stamp follows the server clock as
// estimated one way behind it), so the round trip is allowed for here, up to
// ONLINE_TRANSIT_MAX_TICKS. Without it a shot at 60 ms or more was clamped
// to a later pose than the shooter saw, and a head shot on a moving target
// landed behind him.
export const MAX_INPUT_LEAD_TICKS = 24;
export const ONLINE_PLAYOUT_MAX_TICKS = 10;
export const ONLINE_PLAYOUT_SNAP_TICKS = 12;
export const ONLINE_TRANSIT_MAX_TICKS = 18; // 300 ms round trip
