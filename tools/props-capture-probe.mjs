// Exercise the actual map's repair pads, turret deaths and supply pulses.
// Use this plugin in scan, capture and plain runs, so their fingerprints
// still prove that recording leaves the battle unchanged.
import assert from 'node:assert/strict';
let game, built = 0, killed = 0, served = 0, crew = null, crewKilled = false, objectives = 0;
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
    if (crew) { crewKilled = !crew.alive; assert.ok(crewKilled, 'turret death killed its real occupant'); }
  }
  if (tick === 40 && game.live.turrets.length) {
    const turret = game.live.turrets.find(t => t.alive && !t.gunner);
    const unit = game.units.all().find(u => u.soldier && game.turretSeats.canMan(u, turret));
    if (unit && game.turretSeats.man(unit, turret)) crew = unit;
  }
  if (tick === 70) {
    for (const b of game.live.objectives) { b.damage(b.maxHealth + 1, game.playerUnit); objectives++; }
  }
}
export function ended() {
  assert.ok(built && killed, 'real buildings were built and destroyed');
  if (game.live.stations.length) assert.ok(served, 'a real station served a soldier');
  console.error(JSON.stringify({ propsProbe: { built, killed, served, crewKilled, objectives } }));
}
