import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { PlayerCombat } from '../src/combat.js';
import { SoldierCamera } from '../src/camera.js';

for (const posture of ['stand', 'crouch', 'prone']) for (const zoomStep of [1, 2]) {
  test(`scope and fired direction agree from third person: ${posture}, zoom ${zoomStep}`, () => {
    const combat = Object.create(PlayerCombat.prototype);
    const soldier = { position: new THREE.Vector3(), aimYaw: .7, aimPitch: -.1, posture, air: null, jet: { recharge: 0 } };
    const camera = new THREE.PerspectiveCamera(60, 16 / 9, .05, 2000);
    const weapon = { def: { ZoomMin: 2, ZoomMax: 8, ZoomFirstPerson: 1, ScopeTexture: 'scope' } };
    const def = { cameras: { STAND: { TrackCenter: [0, 1.8, 0], TrackOffset: [0, .5, 4], TiltValue: 5 } } };
    let reticle;
    Object.assign(combat, { soldier, camera, def, firstPerson: false, health: 100, maxHealth: 100, zoomStep,
      collision: { cast: () => null }, arsenal: { current: () => weapon },
      hud: { setVehicle() {}, setPlayer() {}, setSecondBar() {}, setScope() {}, setReticle(y) { reticle = y; } } });
    for (const key of ['origin','pivot','aimDir','left','up','worldPoint','screen','shotDir','target','eyePoint','from']) combat[key] = new THREE.Vector3();
    combat.aim();
    const eye = combat.eye(soldier.position, new THREE.Vector3());
    const view = new SoldierCamera(camera, combat.collision, def, { responsive: true });
    assert.equal(combat.usesEyeView, true);
    view.update(1, soldier.position, soldier.aimYaw, soldier.aimPitch, posture, combat.zoom, combat.usesEyeView, eye);
    camera.updateMatrixWorld(); combat.updateHud(null, soldier);
    // Prone has a small authored lateral eye offset, but never vertical drift.
    assert.ok(Math.abs(reticle - .5) < 1e-8, `reticle y=${reticle}`);
    const forward = camera.getWorldDirection(new THREE.Vector3());
    assert.ok(forward.dot(combat.shotDir) > 1 - 1e-10);
    combat.zoomStep = 0;
    assert.equal(combat.usesEyeView, false, 'unzoom restores the chosen third-person view');
  });
}
