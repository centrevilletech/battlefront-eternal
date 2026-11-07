import {sceneProperty} from "./scene-changes.js";
import * as THREE from "three";
import { programLayouts } from "../render-programs.js";
import { ownedProgram, bindProgram } from "./programs.js";
// Numeric kinds as exported by the generated program table.
const KIND_NAMES = {0:"World",1:"Skinned",3:"Plain",4:"Effect",9:"Motion"};
function acceptsKind(name, kinds) {
  if (!Array.isArray(kinds)) return KIND_NAMES[kinds] === name;
  for (const kind of kinds) if (KIND_NAMES[kind] === name) return true;
  return false;
}

// Uniform and raster-state shell for particle batches. Their source and
// compilation belong exclusively to the renderer's effects catalog.
export function ownedEffectMaterial(key, parameters) {
  const material = new THREE.Material();
  material.forceSinglePass = true;
  Object.assign(material, parameters);
  bindProgram(material, key);
  sceneProperty(material,"visible",parameters.visible ?? true);
  return material;
}

// The selector is a closed catalog of captured variants. Missing side, quality
// or authored defines stop registration instead of selecting a nearby shader.
export function ownedEffectProgram(core, material, kind, side = material.side, shadows = material.userData?.bfeNoShadows ? false : core.shadowsOn()) {
  const base = material.userData?.bfeProgram;
  if (typeof base !== "string" || !base.startsWith("effects/")) throw Error(`Missing effects shader key for ${material.name || material.type}`);
  // Selection scans the whole catalog, so remember the result per material and
  // per side/shadow/kind slot; the draw path then costs one array read.
  const cache = core.effectMaterialPrograms ||= new WeakMap();
  let slots = cache.get(material);
  if (!slots) { slots = []; cache.set(material, slots); }
  const expected = Array.isArray(kind) ? kind[0] : kind;
  const slot = side + (shadows ? 3 : 0) + 6 * expected, csm = Boolean(core.graphics.csm);
  const previous = slots[slot];
  if (previous && previous.base === base && previous.version === material.version && previous.premultipliedAlpha === material.premultipliedAlpha && previous.csm === csm) {
    if (!acceptsKind(previous.program.kind, kind)) throw Error(`${previous.program.key}: incompatible shader kind`);
    return previous.program;
  }
  // A base key with its own layout is used as is. Otherwise the catalog row whose
  // recorded selector matches this material's side, shadows and defines wins.
  let key = base;
  if (!programLayouts[key] || (programLayouts[key].selector && programLayouts[key].material === base)) {
    key = Object.keys(programLayouts).find((key) => {
      const spec = programLayouts[key], select = spec.selector;
      return acceptsKind(spec.kind, kind) && spec.material === base && select && select.side === side && select.shadows === shadows
        && (select.vertexColors === undefined || select.vertexColors === Boolean(material.vertexColors))
        && (select.premultipliedAlpha === undefined || select.premultipliedAlpha === material.premultipliedAlpha)
        && (select.csmChunks === undefined || select.csmChunks === Boolean(core.graphics.csm))
        && Object.keys(material.defines || {}).length === Object.keys(select.defines).length
        && Object.entries(select.defines).every(([name, value]) => String(material.defines?.[name]) === String(value));
    });
  } else if (shadows && programLayouts[key].shadowTwin) key = programLayouts[key].shadowTwin;
  if (!key) throw Error(`Missing shader variant for ${base}, side ${side}, shadows ${core.shadowsOn()}`);
  if (!acceptsKind(programLayouts[key].kind, kind)) throw Error(`${key}: incompatible shader kind`);
  const program = ownedProgram(core, key, null);
  slots[slot] = { base, version: material.version, premultipliedAlpha: material.premultipliedAlpha, csm, program };
  return program;
}

// Basic meshes and sprites retain their page-owned color/texture properties;
// their expanded programs are catalogued alongside the handwritten effects.
export function ownedBasicEffect(shape, parameters) {
  const material = ownedEffectMaterial('effects/' + shape, parameters);
  material.color = new THREE.Color(parameters.color ?? 0xffffff);
  material.map = parameters.map ?? null;
  material.rotation = parameters.rotation ?? 0;
  material.sizeAttenuation = parameters.sizeAttenuation ?? true;
  material.forceSinglePass = false;
  material.userData.bfeShape = shape === 'sprite' ? 1 : 0;
  return material;
}

// The effects programs the current settings can draw: a shadowed twin when
// shadow maps are on, its unshadowed base when they are off, and any program
// that has no twin; cascade-chunk variants follow the cascade setting. Anything
// a later setting needs is linked when the settings change (EffectsPass).
export function effectProgramKeys(core) {
  const shadows = core.shadowsOn(), csm = Boolean(core.graphics.csm), keys = [];
  for (const [key, layout] of Object.entries(programLayouts)) {
    if (!key.startsWith("effects/")) continue;
    const twin = layout.shadowOf !== undefined || layout.shadowTwin !== undefined;
    if (twin && (layout.shadowOf !== undefined) !== shadows) continue;
    const select = layout.selector;
    if (select && select.csmChunks !== undefined && select.csmChunks !== csm) continue;
    keys.push(key);
  }
  return keys;
}

// Link those programs one at a time with `settle()` awaited between them, so the
// driver's link waits fall on the loading screen and not in one long stall.
// A program already made (a later warmup, a restored context) costs nothing.
export async function warmEffectPrograms(core, settle) {
  for (const key of effectProgramKeys(core)) {
    if (core.ownedPrograms?.has(key)) continue;
    ownedProgram(core, key, null);
    await settle();
  }
}
