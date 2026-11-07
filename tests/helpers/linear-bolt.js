import { boltFade } from "../../src/runtime/linear-bolt.js";

// What a replicated straight bolt shows at a displayed tick, from its launch
// sample alone: how far it has flown and whether it is drawn at all. This is
// the behaviour the ordnance presentation implements (linearBoltFrame in
// rust/bfe-presentation/src/ordnance.rs), stated again here so a test of the
// network path can say which bolts a page draws without drawing them.
export function linearBoltFrame(source, tick) {
  if (!Number.isFinite(source.networkFlightTick) || !Number.isFinite(tick)) return null;
  const elapsed = (tick - source.networkFlightTick) / 60;
  const fade = boltFade(source.def, elapsed - source.life);
  if (source.flown + source.speed * elapsed < 0 || !(fade > 0)) return null;
  return { flown: source.flown + source.speed * Math.min(elapsed, source.life), ended: elapsed >= source.life, fade };
}
