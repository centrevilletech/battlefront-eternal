// Every effects program the game can ask for must exist for every shadow
// setting. A missing variant is a battle that stops for the players who run
// that setting, so the catalog is checked as a whole and not by sampling cases.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { programLayouts } from '../src/render-programs.js';
import { effectProgramKeys } from '../src/rust-render/effect-material.js';

const effects = Object.entries(programLayouts).filter(([key]) => key.startsWith('effects/'));

test('every effects selector exists with shadows on and off', () => {
  const seen = new Map();
  for (const [key, layout] of effects) {
    // The first-person view scene has no shadow maps, so its programs exist unshadowed only.
    if (!layout.selector || key.startsWith('effects/view-')) continue;
    const { shadows, ...rest } = layout.selector;
    const signature = JSON.stringify([layout.material, layout.kind, rest]);
    const sides = seen.get(signature) ?? new Set();
    sides.add(shadows);
    seen.set(signature, sides);
    assert.equal(typeof shadows, 'boolean', `${key} declares its shadow setting`);
  }
  for (const [signature, sides] of seen) assert.deepEqual([...sides].sort(), [false, true], `${signature} lacks a shadow setting`);
});

test('every shadowed twin has its base and the base names it', () => {
  for (const [key, layout] of effects) {
    if (layout.shadowOf === undefined) continue;
    const base = programLayouts[layout.shadowOf];
    assert.ok(base, `${key}: base ${layout.shadowOf} exists`);
    assert.equal(base.shadowTwin, key);
    assert.deepEqual(base.attributes, layout.attributes);
    assert.equal(base.kind, layout.kind);
  }
  for (const [key, layout] of effects) if (layout.shadowTwin !== undefined) assert.equal(programLayouts[layout.shadowTwin].shadowOf, key);
});

// A material key written in the source (ownedEffectMaterial('effects/x')) is a
// program key or the material name of selector rows; nothing else resolves.
test('every effects key written in the source resolves to the catalog', () => {
  const materials = new Set(effects.map(([, layout]) => layout.material).filter(Boolean));
  const names = new Set([...effects.map(([key]) => key), ...materials]);
  const missing = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) { if (entry.name !== 'rust-pkg') walk(file); continue; }
      if (!entry.name.endsWith('.js') || entry.name === 'render-programs.js') continue;
      for (const [, key] of fs.readFileSync(file, 'utf8').matchAll(/["'`](effects\/[a-z0-9-]+)["'`]/g)) if (!names.has(key)) missing.push(`${file}: ${key}`);
    }
  };
  walk(new URL('../src', import.meta.url).pathname);
  assert.deepEqual(missing, []);
});

test('the settings link one of each pair and every program that has no twin', () => {
  for (const shadows of [true, false]) for (const csm of [true, false]) {
    const keys = new Set(effectProgramKeys({ shadowsOn: () => shadows, graphics: { csm: csm ? {} : null } }));
    for (const [key, layout] of effects) {
      const select = layout.selector;
      if (select?.csmChunks !== undefined && select.csmChunks !== csm) { assert.ok(!keys.has(key), `${key} belongs to the other cascade setting`); continue; }
      const twin = layout.shadowOf !== undefined, hasTwin = layout.shadowTwin !== undefined;
      if (twin || hasTwin) assert.equal(keys.has(key), twin === shadows, `${key} with shadows ${shadows}`);
      else assert.ok(keys.has(key), `${key} is always linked`);
    }
  }
});
