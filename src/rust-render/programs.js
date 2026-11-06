// Family manifests declare layouts before a program is linked. Neither source
// nor reflection is borrowed from three. Invalid variants stop registration;
// there is no compiled-source or nearest-variant substitution.
import { programLayouts } from "../render-programs.js";

// A material's catalog key and its family are settled once, when it is made;
// no pass inspects key strings per object per frame.
export function bindProgram(material, key) {
  material.userData.bfeProgram = key;
  if (key.startsWith("effects/")) material.userData.bfeEffect = true;
  return material;
}

export const ownsEffectSource = (material) => material?.userData?.bfeEffect === true;

export function ownedProgram(core, key, attributes) {
  const cache = core.ownedPrograms ||= new Map();
  if (cache.has(key)) return cache.get(key);
  const source = programLayouts[key];
  let entry;
  const start = performance.now();
  try {
    if (!source) throw Error(`unknown shader variant: ${key}`);
    const layout = source.attributes.map(([n, l]) => `${n}:${l}`).join(",");
    if (attributes !== null && attributes !== layout) throw Error(`attribute layout for ${key}: ${attributes}`);
    entry = { key, kind: source.kind, id: core.rust.add_owned_program(key), uniforms: source.uniforms, attributes: source.attributes, names: new Set(source.uniforms.map(u => u.name)) };
  } catch (error) {
    core.fail(Error(`${key}: ${String(error)}`, { cause: error }));
  }
  (core.shaderPrograms ||= []).push({ key, hash: source?.hash, ms: performance.now() - start, accepted: true });
  cache.set(key, entry);
  return entry;
}

// Geometry passes consume the declared locations and active input names, without
// consulting a linked browser program. Registration errors remain fatal.
export function ownedGeometryProgram(core, key) {
  const layout = programLayouts[key];
  if (!layout) throw Error(`unknown shader variant: ${key}`);
  const entry = ownedProgram(core, key, layout.attributes.map(([n, l]) => `${n}:${l}`).join(","));
  entry.attributes = layout.attributes;
  entry.names ||= new Set(layout.uniforms.map(u => u.name));
  return entry;
}
