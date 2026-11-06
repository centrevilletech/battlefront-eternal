// Interface models keep authored values on CPU resource shells. Their program
// bodies and finite skin/colour variants belong to the Rust source catalog.
import * as THREE from "three";

export function surfaceMaterial(program, values = {}) {
  const material = new THREE.Material();
  material.uniforms = values.uniforms || {};
  material.userData.interfaceProgram = program;
  material.forceSinglePass = true; // ShaderMaterial's original double-sided draw
  for (const name in values) if (name !== "uniforms") material[name] = values[name];
  return material;
}

// SpawnStage/HoloMap's unlit projector pieces retain BasicMaterial's value
// vocabulary so their colour/opacity animation needs no per-frame adapter.
export function interfaceBasic(values = {}) {
  const material = surfaceMaterial("basic", values);
  material.color = new THREE.Color(values.color ?? 0xffffff);
  material.map = values.map || null;
  material.forceSinglePass = false; // BasicMaterial renders back then front
  return material;
}

export function interfacePoints(values) {
  const material = interfaceBasic(values);
  material.userData.interfaceProgram = "points";
  return material;
}
