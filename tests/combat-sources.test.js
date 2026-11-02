import test from "node:test";
import assert from "node:assert/strict";
import { createCombatRegistry } from "../src/runtime/network-combat-registry.js";
import { vehicleMissileDefinition } from "../src/weapon-targeting.js";

test("a tank missile given guidance at run time goes by its content definition", () => {
  const ordnance = { base: "bullet", Velocity: 40, LifeSpan: 7, GeometryName: "missile" };
  const weapon = { class: "rep_weap_hover_fightertank_missile", base: "cannon", ordnance };
  const registry = createCombatRegistry({ weapons: { tank: weapon } });
  const paths = [...registry.paths];
  const derived = vehicleMissileDefinition(weapon);
  assert.notEqual(derived.ordnance, ordnance);
  assert.equal(derived.ordnance.base, "missile");
  assert.equal(registry.definitionId(derived.ordnance), registry.definitionId(ordnance));
  // Content only: every peer numbers the registry the same as before.
  assert.deepEqual(registry.paths, paths);
  assert.throws(() => registry.definitionId({ base: "missile", Velocity: 1, LifeSpan: 1 }), /absent/);
});
