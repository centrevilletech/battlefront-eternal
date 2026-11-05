// Family manifests declare layouts before a program is linked. Neither source
// nor reflection is borrowed from three. A refused variant leaves its pass to
// three; there is no nearest-variant substitution.
import { shaderSources } from "../render-shaders.js";

export function ownedProgram(core, material, attributes) {
  const key = material.userData.bfeProgram;
  const cache = core.ownedPrograms ||= new Map();
  if (cache.has(key)) return cache.get(key);
  const source = shaderSources[key];
  let entry = null;
  const start = performance.now();
  try {
    if (!source) throw Error(`unknown shader variant: ${key}`);
    const layout = source.attributes.map(([n, l]) => `${n}:${l}`).join(",");
    if (attributes !== layout) throw Error(`attribute layout for ${key}: ${attributes}`);
    entry = { id: core.rust.add_owned_program(key), uniforms: source.uniforms, attributes: null };
  } catch (error) {
    core.fail(error);
  }
  (core.shaderPrograms ||= []).push({ key, owned: true, hash: source?.hash, ms: performance.now() - start, accepted: !!entry });
  cache.set(key, entry);
  return entry;
}
