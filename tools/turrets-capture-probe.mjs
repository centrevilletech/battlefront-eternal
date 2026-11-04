// Exercise the actual placed turret classes and model hierarchies. The
// identical stimulus is used for plain, scan and captured battles.
import assert from 'node:assert/strict';
import * as THREE from 'three';
let game, calls = 0;
export function battle({ game: value }) { game = value; assert.ok(game.live.turrets.length, 'the map has turrets'); }
export function ticked(tick) {
  if (tick < 5 || tick > 120 || tick % 3) return;
  for (const t of game.live.turrets) {
    const point = new THREE.Vector3(), direction = new THREE.Vector3();
    t.steer(1 / 60, tick % 2 ? 1 : -1, .25, 1);
    t.holdYaw(tick / 10); t.viewDirection(direction); t.present();
    const target = t.position.clone().add(new THREE.Vector3(3, 2, 40));
    if (t.gun) t.gunWant(target, {});
    t.aimGun(1 / 60, t.gun ? target : null); t.pose(1 / 60, t.gun ? target : null);
    t.muzzle(point, direction, true); t.fireDirection(point, target, direction, direction);
    t.eyePoint(point); t.aimOrigin(point, Boolean(tick % 2)); t.seatPoint(point); t.seatTurn(new THREE.Quaternion()); t.seatYaw(); calls++;
  }
}
export function ended() { assert.ok(calls); console.error(JSON.stringify({ turretProbe: { turrets: game.live.turrets.length, batches: calls } })); }
