// Shared immutable weapon classes for authoritative combat adapters.
// A JavaScript value as words for the module's content (module.rs
// `read_js`): a weapon class inside a unit class is its index.
export function valueWords(v, strings, out, weapons = null, depth = 0) {
  if (v === undefined) out.push(0);
  else if (v === null) out.push(1);
  else if (v === false) out.push(2);
  else if (v === true) out.push(3);
  else if (typeof v === "number") out.push(4, v);
  else if (typeof v === "string") out.push(5, strings.id(v));
  else if (weapons?.has(v)) out.push(8, weapons.get(v));
  else if (Array.isArray(v) && depth < 12) {
    out.push(6, v.length);
    for (const x of v) valueWords(x, strings, out, weapons, depth + 1);
  } else if (typeof v === "object" && depth < 12) {
    const keys = Object.keys(v);
    out.push(7, keys.length);
    for (const k of keys) {
      out.push(strings.id(k));
      valueWords(v[k], strings, out, weapons, depth + 1);
    }
  } else out.push(9);
  return out;
}

const registries = new WeakMap();
export function weaponClasses(bridge) {
  let sent = registries.get(bridge);
  if (!sent) { sent = new Map(); registries.set(bridge, sent); }
  return sent;
}
export function registerWeaponClass(bridge, def) {
  const sent = weaponClasses(bridge);
  if (sent.has(def)) return sent.get(def);
  const id = bridge.defs.weapon.id(def);
  const words = valueWords(def, bridge.strings, []), pointer = bridge.x.sim_call_room(words.length, 0);
  bridge.views(); bridge.f.set(words, pointer / 8 + 5);
  bridge.x.cmb_weapon(id); sent.set(def, id);
  return id;
}
