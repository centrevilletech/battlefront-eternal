// Exercise the actual map's repair pads, turret deaths and supply pulses.
// Use this plugin in scan, capture and plain runs, so their fingerprints
// still prove that recording leaves the battle unchanged.
import assert from 'node:assert/strict';
let game, built = 0, killed = 0, served = 0;
export function battle({ game: value }) { game = value; }
export function ticked(tick) {
  const buildings = game.live.buildings.filter(b => b.constructor.name !== 'Trap');
  if (tick === 10 || tick === 90) {
    for (const b of buildings) {
      b.setHealth(b.maxHealth);
      assert.ok(b.alive);
      built++;
    }
  }
  if (tick === 30 || tick === 110) {
    const unit = [...game.units.all()].find(u => u.alive && u.supply);
    if (unit) for (const station of game.live.stations) {
      unit.soldier.position.copy(station.position);
      unit.supply.health = unit.supply.maxHealth * 0.5;
      for (const weapon of unit.supply.weapons) if (!weapon.bottomless) weapon.reserve = 0;
      station.update(station.delay, [unit.supply], game.vehicles?.vehicles ?? []);
      if (station.servedHealth || station.servedAmmo) served++;
    }
  }
  if (tick === 50) {
    for (const b of buildings.filter(b => b.constructor.name !== 'ObjectivePost')) {
      b.damage(b.maxHealth + 1, game.playerUnit);
      killed++;
    }
  }
}
export function ended() {
  assert.ok(built && killed, 'real buildings were built and destroyed');
  if (game.live.stations.length) assert.ok(served, 'a real station served a soldier');
  console.error(JSON.stringify({ propsProbe: { built, killed, served } }));
}
