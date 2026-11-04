// Deterministic stimulus for the map's real creature/door, repeated in
// scan, capture and plain runs. No animation or damage implementation is
// replaced. A soldier is brought to the actual trigger and grab node.
import assert from 'node:assert/strict';
let game, prop, unit, grabbed = false, swallowed = false, carried = false;
export function battle({ game: value }) {
  game = value;
  prop = game.live.animated.find(p => p.grabs.length && p.pose);
  assert.ok(prop, 'map has a creature with a real grab clip');
}
export function ticked(tick) {
  if (tick < 10 || swallowed) return;
  if (!unit) unit = game.units.all().find(u => u.alive && u.soldier && !u.walker && !u.seat && !u.held);
  assert.ok(unit, 'living soldier for creature probe');
  if (!grabbed) {
    const trigger = prop.triggers[0], grab = prop.grabs.find(g => g.slot === trigger.slot);
    if (prop.loop || prop.slot < 0) unit.soldier.position.copy(trigger.point);
    else { prop.rig.root.updateMatrixWorld(true); grab.node.getWorldPosition(unit.soldier.position); }
    unit.soldier.position.y -= 1.048;
    unit.soldier.velocity.set(0, 0, 0);
    prop.step(1 / 60, [unit.position], [unit], tick);
    grabbed = Boolean(prop.victim?.unit === unit);
  }
  if (grabbed) {
    carried ||= Boolean(unit.held?.frame);
    if (unit.removed) swallowed = true;
  }
}
export function ended() {
  assert.ok(grabbed && carried && swallowed, 'actual creature grabbed, posed, killed and removed its soldier');
  console.error(JSON.stringify({ animatedProbe: { grabbed, carried, swallowed } }));
}
