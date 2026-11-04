// Exercise real mounted weapons and a replacement life in the same map
// for plain, captured and native runs. Actions occur inside the world step
// so method captures see the firing calls and their actual RNG streams.
import assert from 'node:assert/strict';

let game, server, turret, hull, seat, replaced = false;
let turretShots = 0, vehicleShots = 0;
export function battle(context) {
  ({ game, server } = context);
  const crews = game.turretSeats, fleet = game.vehicles;
  const turretFired = crews.onFired, vehicleFired = fleet.onFired;
  crews.onFired = (...args) => { if (args[0] === turret) turretShots++; return turretFired?.(...args); };
  fleet.onFired = (...args) => { if (args[0] === hull) vehicleShots++; return vehicleFired?.(...args); };
  const step = context.world.step;
  context.world.step = function (...args) { act(game.worldClock.tick + 1); return step.apply(this, args); };
}
function press(input, tick) {
  input.held[0] = input.held[1] = true;
  input.pressed[0] = input.pressed[1] = tick % 30 === 0;
  input.reload = tick % 100 === 0;
}
function act(tick) {
  const human = server.actor.unit, crews = game.turretSeats, fleet = game.vehicles;
  if (tick === 30) {
    turret = crews.turrets.find(t => t.weapon);
    assert.ok(turret, 'the map has a real armed turret');
    turret.setHealth(turret.maxHealth);
    if (turret.lease) crews.vacate(turret.lease, { placement: false });
    assert.ok(crews.man(human, turret), 'the real network human mans it');
  }
  if (tick >= 40 && tick < 180) {
    assert.equal(human.seat?.turret, turret);
    press(human.seat.input, tick); crews.drive(human.seat, 1 / 60);
  }
  if (tick === 180) assert.ok(crews.leave(human, { placement: false }));
  if (tick === 220) {
    hull = fleet.vehicles.find(v => v.alive && !v.flying && v.seats.some((s, i) => s.weapons.length && fleet.eligibleSeat(human, v, i) === i));
    assert.ok(hull, 'the map has an available real ground weapon');
    seat = hull.seats.findIndex((s, i) => s.weapons.length && fleet.eligibleSeat(human, hull, i) === i);
    assert.ok(fleet.board(human, hull, seat));
  }
  if (tick >= 240 && tick < 460 || tick >= 560 && tick < 780) {
    assert.equal(human.seat?.vehicle, hull);
    press(human.seat.input, tick); hull.fireSeat(seat, 1 / 60);
  }
  if (tick === 480) fleet.vacate(human.seat, { placement: false });
  if (tick === 500) {
    const previous = hull.seats[seat].weapons.map(w => w.state);
    hull.spawn(hull.team);
    replaced = hull.seats[seat].weapons.every((w, i) => w.state !== previous[i]);
    assert.ok(replaced, 'the real spawn created replacement Weapon instances');
  }
  if (tick === 540) assert.ok(fleet.board(human, hull, seat));
}
export function ended() {
  assert.ok(turretShots > 0 && vehicleShots > 0 && replaced, 'both real holders fired across the vehicle replacement life');
  console.error(JSON.stringify({ ordinaryHolderProbe: { turretShots, vehicleShots, replaced } }));
}
