import { contentPath } from './helpers/content.js';
import { rustOrdnance } from './helpers/rust-simulation.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';

function target(id = 1, x = 0, z = 40) {
  return { id, generation: 1, alive: true, health: 100, team: 2, kind: 6,
    position: new THREE.Vector3(x, 0, z), sphere: { radius: .1 },
    aimPoint(out) { return out.copy(this.position); } };
}
function context(list, collision = { cast: () => null }) {
  return { units: { all: () => list, byId: id => list.find(u => u.id === id), relation: (a, b) => a === b ? 1 : -1 },
    collision, origin: new THREE.Vector3(), dir: new THREE.Vector3(0, 0, 1), viewer: { id: 0, team: 1 } };
}

test('guided missiles track moving targets and never adopt a recycled unit', () => {
  const t = target(), ctx = context([t]);
  const ord = rustOrdnance({ scene: new THREE.Scene(), collision: ctx.collision,
    model: () => null, effects: {}, lights: { add() {} } });
  ord.units = {...ctx.units,raycast:()=>null};
  const def = { base: 'missile', Velocity: 40, LifeSpan: 8, TurnRate: 2, ScaleTime: .1 };
  const guided = ord.fire(def, ctx.origin, ctx.dir, null, null, null, null, { targetId: t.id, targetGeneration: t.generation });
  const free = ord.fire(def, ctx.origin, ctx.dir, null);
  t.position.x = 10; ord.update(.1,false);
  assert.ok(guided.velocity.x > 0); assert.equal(free.velocity.x, 0);
  const last = guided.desired.clone(); t.generation++; t.position.x = -30;
  ord.update(.1,false); assert.equal(guided.targetId, -1); assert.deepEqual(guided.desired, last);
});


for (const side of ['all', 'rep', 'cis', 'imp']) {
  const file = contentPath(`sides/${side}/units.json`);
  test(`retail ${side} assault launcher retains its own projectile schedule`, { skip: !fs.existsSync(file) }, () => {
    const data = JSON.parse(fs.readFileSync(file));
    const def = Object.values(data.units).flatMap(u => u.weapons || []).find(w => w.class === `${side}_weap_inf_torpedo_launcher`);
    assert.ok(def);
    assert.equal(def.SalvoCount,side==='all'?2:1);
    assert.equal(def.InitialSalvoDelay||0,side==='all'?.25:0);
    if(side==='all')assert.equal(def.SalvoDelay,.25);
  });
}
