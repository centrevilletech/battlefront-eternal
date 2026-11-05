import test from 'node:test';
import bailoutGolden from './fixtures/f2-bailout-golden.json' with { type: 'json' };
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { mountedFleet } from './fixtures/f2-mounted.js';
import { weaponDefinition } from './fixtures/prediction-weapon-cases.js';
import { mountedCases, mountedScenario } from './fixtures/f2-mounted-scenarios.js';

// Recorded from the last switched engine over invented geometry and weapons.
// These cover the live scheduler without depending on game files or a JS twin.
const golden = JSON.parse(readFileSync(new URL('./fixtures/f2-mounted-golden.json', import.meta.url)));
for (const name of mountedCases) test(`native mounted phase keeps the ${name} golden`, async () => {
  assert.deepEqual(await mountedScenario(mountedFleet, name), golden[name]);
});

// A new warning used to intern its text on the first high-altitude tick.
// Reach that transition after warming the actual scheduler, not a text helper.
test('warm F2 flight reaches a new warning without allocating native storage', async () => {
  const f = await mountedFleet(weaponDefinition('fixture-flight-warning'),
    { flying: true, tow: true, network: true, environment: { ceiling: 15 } });
  const keys = { forward: 1, right: 0, stickX: 0, stickY: 0 };
  f.crew.intent.flight = keys;
  let ceiling = false;
  try {
    for (let tick = 0; tick < 600; tick++) {
      if (tick === 200) keys.stickY = .2;
      f.vs.update(1 / 60);
      if (tick > 60) assert.equal(f.vs.f2.x.f2_allocations(), 0, `flight allocation at ${tick}`);
      if (tick >= 200 && f.v.flightWarning === 'Flight ceiling') ceiling = true;
    }
    assert.ok(ceiling, `the warmed pilot reaches a previously unused warning: ${f.v.position.toArray()} ${f.v.flightState}`);
  } finally { f.vs.dispose(); }
});

test("F2 resets only the respawning hull's mounted weapons", async () => {
  const f = await mountedFleet(weaponDefinition('fixture-mounted-respawn'), { other: true });
  const life = f.target.aiTarget.generation, sourceLife = f.v.aiTarget.generation;
  const freshClip = f.target.seats[0].weapons[0].state.clip;
  try {
    for (let tick = 0; tick < 90; tick++) {
      if (tick === 7) f.target.damage(Infinity);
      // A foreign modifier must survive another hull's birth publication.
      if (tick === 13) { f.v.seats[0].weapons[0].state.spreadScale = .25; f.v.seats[0].weapons[0].state.channel = 1; }
      f.crew.intent.buttons[0] = 1; f.vs.update(1 / 60);
    }
    assert.equal(f.target.aiTarget.generation, life + 1);
    assert.equal(f.v.aiTarget.generation, sourceLife);
    assert.equal(f.target.seats[0].weapons[0].state.clip, freshClip);
    assert.equal(f.v.seats[0].weapons[0].state.spreadScale, .25);
    assert.equal(f.v.seats[0].weapons[0].state.channel, 1);
    assert.ok(f.shots.length > 2);
  } finally { f.vs.dispose(); }
});

// The pre-removal synthetic flight records include the exit body and the
// abandoned craft's complete descent, without any game files.
test('native pilot bailout keeps the recorded air body and abandoned descent', async () => {
  const f = await mountedFleet(weaponDefinition('fixture-bailout'), { flying: true, network: true, tow: true });
  try {
    const accepted = f.vs.unboard(f.crew), s = f.crew.soldier;
    const exit = { accepted, position: s.position.toArray(), velocity: s.velocity.toArray(), grounded: s.grounded, air: s.air, held: Boolean(f.crew.seat) };
    let ticks = 0; while (ticks < 600 && f.v.alive) { f.vs.update(1 / 60); ticks++; }
    assert.deepEqual({ exit, ticks, alive: f.v.alive, health: f.v.health }, bailoutGolden);
  } finally { f.vs.dispose(); }
});

// Vehicle.emitShot announces the cable before the shot listener. Both become
// reliable network events, so matching final cable state alone misses this.
test('a native tow journals attachment before its fired hook exactly once', async () => {
  const def = weaponDefinition('fixture-tow-order', 0, { ordnance: { class: 'fixture-order-cable', base: 'towcable', CableLength: 100 }, LockOnRange: 80 });
  const f = await mountedFleet(def, { tow: true });
  const order = [];
  f.vs.world.sound = name => { if (name === 'towAttach') order.push(name); };
  f.vs.onFired = () => order.push('shot');
  try {
    f.crew.intent.buttons[0] = 1;
    f.vs.update(1 / 60);
    assert.deepEqual(order, ['towAttach', 'shot']);
    assert.ok(f.v.towTarget >= 0);
  } finally { f.vs.dispose(); }
});
