// Runs in the real page for browser and desktop correctness checks.
// The runner explicitly installs client-prediction-diagnostics before entry.
export async function exercisePredictionPage() {
  const b = __bf, c = b.client, sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const owner = b.predictionSession, start = { ...owner.counters };
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
  const shared = owner.bridge.x === b.collision.rays.x;
  // Close from a presentation callback while the page's replay is active.
  const authority = c.weapons.saved?.arsenal ?? b.combat.arsenal, present = owner.present;
  let closedDuringReplay = false;
  owner.present = function(unit, presentation, onShot) {
    const before = presentation.beforeStep;
    presentation.beforeStep = function() { before.call(this); closedDuringReplay = true; c.host.dispose(); };
    try { return present.call(this, unit, presentation, onShot); }
    finally { presentation.beforeStep = before; }
  };
  b.controls.hold('KeyW');
  for (let i = 0; i < 120 && !c.host.closed; i++) await sleep(25);
  c.host.dispose(); await sleep(100);
  const disposed = !c.ready && !c.running && !c.predicted && !c.predictionBodyPending && b.combat.arsenal === authority && owner.disposed && !owner.handle;
  return { named, shown, predicted, missiles, badRotation, restored, tick: b.worldClock.tick,
    nativeTicks: owner.counters.ticks - start.ticks, calls: owner.counters.calls - start.calls,
    shared, disposed, closedDuringReplay, hasOwner: Boolean(owner) };
}
