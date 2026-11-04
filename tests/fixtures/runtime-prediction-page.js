// Runs in the real page for browser and desktop correctness checks.
export async function exercisePredictionPage(native) {
  const b = __bf, c = b.client, sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const owner = b.predictionSim, handles = [];
  let nativeActorSteps = 0;
  if (owner) {
    const bind = owner.actors.bind;
    owner.actors.bind = (...args) => {
      const session = bind(...args), step = session.step;
      session.step = (...values) => { const result = step(...values); if (result) nativeActorSteps++; return result; };
      handles.push(session); return session;
    };
  }
  b.graphics.render = () => {};
  b.controls.locked = true;
  let named = 0, shown = false, predicted = false, missiles = 0, badRotation = false;
  b.controls.hold('KeyW', 'Mouse0');
  b.controls.looks.push([0, -100]);
  for (let i = 0; i < 240; i++) {
    if (i % 60 === 0) b.controls.press('Mouse0'); // launcher is single-trigger
    await sleep(25);
    named = Math.max(named, b.units.all().filter(unit => !unit.human && unit.alive && unit.character?.name).length);
    shown ||= Boolean(b.combat.shown?.def.GeometryName && [...b.combat.held.values()].some(held => held.group.visible));
    if (c.predicted) {
      const saved = new DataView(c.saved.buffer, c.saved.byteOffset, c.saved.byteLength), p = b.soldier.position;
      // Compact Soldier schema: four header bytes, six scalar fields, then XYZ.
      predicted ||= Math.hypot(p.x - saved.getFloat64(52, true), p.y - saved.getFloat64(60, true), p.z - saved.getFloat64(68, true)) > 1e-4;
    }
    for (const round of c.entity.combat.dynamic.objects.values()) {
      if (round.type !== 'round') continue;
      if (!round.mesh) continue;
      missiles++;
      badRotation ||= !round.quaternion?.isQuaternion || !round.mesh.quaternion.equals(round.quaternion);
    }
  }
  b.controls.clear();
  b.pause();
  const restored = !c.predicted;
  await c.host.call('suspend');
  await sleep(100);
  b.resume();
  await c.host.call('start');
  await sleep(500);
  const weaponSession = c.weapons?.nativeSession, correctionCalls = owner?.corrections.calls ?? 0;
  const reconcileCalls = owner?.reconcile.calls ?? 0;
  const nativeWeapon = !!weaponSession?.handle, shared = owner ? owner.bridge.x === b.collision.rays.x : false;
  // Close from inside replay, where its temporary arsenal/body are installed.
  const authority = c.weapons.saved?.arsenal ?? b.combat.arsenal, step = c.weapons.step;
  let closedDuringReplay = false;
  c.weapons.step = function (...args) { const result = step.apply(this, args); closedDuringReplay = true; c.host.dispose(); return result; };
  b.controls.hold('KeyW');
  for (let i = 0; i < 120 && !c.host.closed; i++) await sleep(25);
  c.host.dispose(); await sleep(100);
  const disposed = !c.ready && !c.running && !c.predicted && !c.predictionBodyPending && b.combat.arsenal === authority && (!owner || owner.disposed && !owner.corrections.handle && !owner.reconcile.handle && !weaponSession?.handle && handles.every(session => !session.handle));
  return { named, shown, predicted, missiles, badRotation, restored, tick: b.worldClock.tick,
    native, nativeActorSteps, nativeWeapon, correctionCalls, reconcileCalls, shared, disposed, closedDuringReplay, hasOwner: !!owner };
}
