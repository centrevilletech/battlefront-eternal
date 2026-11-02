// Combat definitions made at run time from content ones (a tank's missile
// given guidance, weapon-targeting.js) are not content. The network's combat
// registry is built from content alone, so every peer numbers it the same;
// it names such a definition by the content definition it was made from,
// whose look (model, trail, impact) it shares.
const sources = new WeakMap();

export function derivedFrom(derived, source) {
  if (source && typeof source === "object") sources.set(derived, source);
  return derived;
}

export function combatSource(def) {
  return sources.get(def);
}
