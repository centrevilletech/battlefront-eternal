// World variants are selected from explicit material and scene features. Source
// discovery and nearest-variant substitution are never part of registration.
import { programLayouts } from '../render-programs.js';
import { ownedGeometryProgram } from './programs.js';

const variants = new Map();
for (const [key, layout] of Object.entries(programLayouts)) {
  if (!key.startsWith('world/') || !layout.selector) continue;
  const signature = JSON.stringify(layout.selector);
  if (variants.has(signature)) throw Error(`Ambiguous world variants: ${variants.get(signature)} and ${key}`);
  variants.set(signature, key);
}

export function ownWorldMaterial(material, geometry) {
  material.userData.bfeFabric = Boolean(geometry?.attributes.fabricAnchor);
  material.userData.bfeTree = Boolean(material.uniforms.treeStates);
  material.userData.bfeFamily = 'world';
  material.isShaderMaterial = false;
  material.type = 'Material';
  delete material.vertexShader;
  delete material.fragmentShader;
  delete material.onBeforeCompile;
}

export function ownedWorldProgram(core, material, scene = core.graphics.scene, camera = core.graphics.camera, side = material.side) {
  const entries = core.worldMaterialPrograms ||= new WeakMap();
  const existing = entries.get(material), shadowState=core.graphics.csm, lampLighting=Boolean(core.graphics.settings.get("lampLighting"));
  if (existing && existing.shadowState===shadowState && existing.lampLighting===lampLighting && existing.version === material.version && existing.scene === scene && existing.side === side) return existing.program;
  const defines = Object.entries(material.defines).map(([k,v]) => [k,String(v)]).sort(([a],[b]) => (a < b ? -1 : a > b ? 1 : 0));
  let points = 0, shadows = 0;
  scene.traverseVisible(o => {
    if (!o.layers.test(camera.layers)) return;
    if (o.isPointLight) points++;
    if (o.isDirectionalLight && o.castShadow && core.graphics.shadowsEnabled) shadows++;
  });
  const csm = Boolean(shadowState);
  const lamps = lampLighting && material.defines.DYNAMIC !== undefined && material.defines.NO_POINT === undefined && material.defines.FAR === undefined;
  const signature = JSON.stringify({ defines, side, shadows, points, csm, fabric: Boolean(material.userData.bfeFabric), tree: Boolean(material.userData.bfeTree), map: Boolean(material.map), lamps });
  const key = variants.get(signature);
  if (!key) throw Error(`Unowned world variant: ${signature}`);
  const layout = programLayouts[key], program = ownedGeometryProgram(core, key);
  program.names = new Set(layout.inputs.map(u => u.name.replace(/\[0\]$/, '')));
  program.inputs = layout.inputs;
  program.owned = true;
  program.lamps = layout.lamps;
  program.aliased = Boolean(material.userData.bfeFabric);
  entries.set(material, { shadowState, lampLighting, version: material.version, scene, side, program });
  return program;
}
