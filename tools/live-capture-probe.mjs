// Deterministic queries against the map's actual buildings, collision and
// extracted repair/ordnance classes. Scan, capture and plain runs all get
// the same stimulus; no query implementation is replaced.
import assert from 'node:assert/strict';
import * as THREE from 'three';
let game, repair, round, explosion, changed = 0, locked = 0, casts = 0;
export function battle({ game: value }) {
  game = value;
  const weapons = Object.values(game.factory.sides).flatMap(side => Object.values(side.units).flatMap(def => def.weapons ?? []));
  repair = weapons.find(w => w.BuildingHealth !== undefined || w.BuildingRebuild !== undefined);
  round = weapons.find(w => w.ordnance?.Damage > 0)?.ordnance;
  explosion = game.live.buildings.find(b => typeof b.explosion === 'object' && b.explosion?.Damage > 0)?.explosion;
  assert.ok(repair && round, 'the loaded classes include a repair tool and a damaging round');
}
export function ticked(tick) {
  if (tick < 10 || tick > 130 || tick % 3) return;
  const live = game.live, targets = live.buildings.filter(b => Number.isFinite(b.maxHealth) && b.constructor.name !== 'Trap');
  if (tick === 12) for (const b of targets) { b.setHealth(b.maxHealth); b.setHealth(b.maxHealth * .5); }
  const b = targets[(tick / 3) % targets.length];
  const eye = b.position.clone().add(new THREE.Vector3(0, 1, -4));
  const direction = b.center(new THREE.Vector3()).sub(eye).normalize();
  live.repairs(repair, b);
  changed += Number(live.repair(repair, b));
  locked += Number(Boolean(live.lockOn(eye, direction, repair, tick % 2 ? b : null, tick % 2 === 0)));
  live.enterable(eye, direction, t => t.team === 0);
  if (b.bodies[b.state]) { live.struck(b.bodies[b.state], round, game.playerUnit, t => t.team === 0 ? .5 : 1, direction); casts++; }
  if (explosion && tick === 72) live.blast(explosion, b.position, game.playerUnit, t => t.team === 0 ? .5 : 1);
}
export function ended() {
  assert.ok(changed && locked && casts, 'real repair, lock and collision-hit paths ran');
  console.error(JSON.stringify({ liveProbe: { changed, locked, casts, explosion: Boolean(explosion) } }));
}
