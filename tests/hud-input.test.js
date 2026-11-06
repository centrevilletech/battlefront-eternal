import test from 'node:test';
import assert from 'node:assert/strict';
import { HudInput, MAX_UNITS, MAX_POSTS, MAX_TEAMS, STATES, RINGS, TEAMS, UNITS } from '../src/hud-input.js';
import { BattleHud } from '../src/hud.js';
import { HudFrame as H, HudState as S, HudRing as R, HudCommand as C, HudUnit as U, HudTeam as T } from '../src/hud-layout.js';

// Exercise the real page boundary without WebGL. Native phase tests verify the
// same immediate events and retained rows against HUD state and geometry.
function fixture() {
  let text = 0, events = 0;
  const memory = new Float64Array(RINGS + 128 * R.words);
  const core = { hudModelInput: memory, views() {}, addView() {},
    rust: { hud_model_input_len: () => memory.length, hud_model_text: () => ++text,
      hud_model_event() { events++; }, hud_model_observe() {}, hud_model_asset() {} },
  };
  const hud = Object.create(BattleHud.prototype);
  Object.assign(hud, { strings: {}, texture: () => null, graphics: { rust: core }, unitContext: {}, unitSource: [], layer: null });
  hud.input = new HudInput(hud, null);
  return { hud, input: hud.input, memory, events: () => events };
}

test('loading previews retain state and real events never wait for publication', () => {
  const { hud, input, memory, events } = fixture();
  for (let i = 0; i < 100_000; i++) {
    hud.setSecondBar('fuel', i / 100_000);
    hud.setHull('hull', i / 100_000);
    hud.setSupply(true, false);
    hud.stats({ kills: i, deaths: 0, captures: 0 });
    hud.message('loading');
    hud.killMessage({ killer: { human: true, name: 'A', team: 1 }, victim: { human: true, name: 'B', team: 2 } });
    hud.hit('enemy');
  }
  assert.equal(events(), 300_000);
  assert.equal(memory[H.states], 0, 'no frame has published');
  assert.equal(input.f[STATES + C.second * S.words + 1], 0.99999);
  input.publish();
  assert.equal(memory[STATES + C.stats * S.words], 99_999);
  assert.equal(memory[STATES + C.second * S.words + 1], 0.99999);
  assert.equal(input.f[H.states], 0);
});

test('unit definitions parse once and team tables replace per-unit callbacks', () => {
  const { hud, input } = fixture();
  let parses = 0;
  const number = { valueOf() { parses++; return 12; } };
  const def = { MapScale: number, ScanningRange: number, TransmitRange: number };
  const units = Array.from({ length: 800 }, (_, i) => ({ classDef: def, position: { x: i, y: 0, z: 0 }, team: i % 3, alive: true }));
  const relations = new Float64Array(MAX_TEAMS * MAX_TEAMS), locals = new Uint8Array(MAX_TEAMS);
  relations[MAX_TEAMS + 2] = -1; locals[2] = 1;
  Object.assign(hud.unitContext, { viewerTeam: 1, relations, locals, tickets: { 2: 9 }, living: { 2: 7 } });
  hud.unitSource = units;
  for (let update = 0; update < 100; update++) input.units(true);
  assert.equal(parses, 3);
  assert.equal(input.f[TEAMS + 2 * T.words + T.relation], -1);
  assert.equal(input.f[TEAMS + 2 * T.words + T.locals], 1);
  assert.equal(input.f[TEAMS + 2 * T.words + T.tickets], 9);
  assert.equal(input.f[TEAMS + 2 * T.words + T.living], 7);
  assert.equal(input.f[UNITS + U.scale], 12);
});

test('entity lifetime churn and oversized lists clip without exhausting future frames', () => {
  const { hud, input } = fixture();
  const def = {}, position = { x: 0, y: 0, z: 0 };
  const old = { def, position, alive: true };
  hud.unitSource = [old]; input.units(true);
  const generation = input.f[UNITS + U.generation];
  for (let batch = 0; batch < 12; batch++) {
    hud.unitSource = Array.from({ length: MAX_UNITS + 100 }, () => ({ def, position, alive: true }));
    for (let update = 0; update < 3; update++) input.units(true);
    assert.equal(input.rowCount, MAX_UNITS);
  }
  hud.unitSource = [old];
  for (let update = 0; update < 3; update++) input.units(true);
  assert.equal(input.rowCount, 1);
  assert.notEqual(input.f[UNITS + U.generation], generation);
  hud.posts = { relation: () => 'neutral', posts: Array.from({ length: MAX_POSTS + 100 }, () => ({ position, counts: new Map(Array.from({ length: MAX_TEAMS + 10 }, (_, i) => [i, 1])) })) };
  input.posts();
  assert.equal(input.f[H.posts], MAX_POSTS);
  input.publish();
});


test('excess cold art skips missing descriptors instead of failing a frame', () => {
  const { input } = fixture();
  for (let i = 0; i < 5000; i++) input.asset(`extra-${i}`);
  assert.equal(input.assets.length, 4096);
  assert.equal(input.asset('extra-4999'), 0);
  input.publish();
});


test('transition feedback observes changed samples while repeated state stays local', () => {
  const { input, hud } = fixture(), samples = [];
  input.core.rust.hud_model_observe = (kind, value) => samples.push([kind, value]);
  hud.setPlayer(null, 0.35);
  hud.setPlayer(null, 1);
  hud.setPlayer(null, 0.35);
  for (let i = 0; i < 1000; i++) { hud.setPlayer(null, 0.35); hud.setSecondBar('fuel', i / 1000); }
  assert.deepEqual(samples, [[C.health, 0.35], [C.health, 1], [C.health, 0.35]]);
  assert.equal(input.f[STATES + C.health * S.words], 0.35);
});
