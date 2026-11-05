// Family manifests declare layouts before a program is linked. Neither source
// nor reflection is borrowed from three. Invalid variants stop registration;
// there is no compiled-source or nearest-variant substitution.
import { programLayouts } from "../render-programs.js";

export function ownedProgram(core, key, attributes) {
  const cache = core.ownedPrograms ||= new Map();
  if (cache.has(key)) return cache.get(key);
  const source = programLayouts[key];
  let entry;
  const start = performance.now();
  try {
    if (!source) throw Error(`unknown shader variant: ${key}`);
    const layout = source.attributes.map(([n, l]) => `${n}:${l}`).join(",");
    if (attributes !== layout) throw Error(`attribute layout for ${key}: ${attributes}`);
    entry = { id: core.rust.add_owned_program(key), uniforms: source.uniforms, attributes: null };
  } catch (error) {
    core.fail(Error(`${key}: ${String(error)}`, { cause: error }));
  }
  (core.shaderPrograms ||= []).push({ key, hash: source?.hash, ms: performance.now() - start, accepted: true });
  cache.set(key, entry);
  return entry;
}
