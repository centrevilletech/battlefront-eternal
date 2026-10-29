// A Rebel bot in a snowspeeder's rear seat while the player flies the
// speeder round an AT-AT, or past it (the player holds the pilot's seat;
// the hull is placed on its course each tick, so the flight model does not
// fly it off): does the gunner see the walker, turn the tow gun to it and
// fire the harpoon, and does the cable hold?
//
//   node tools/tow-gunner-scenario.mjs [--radius 35] [--height 8]
//     [--speed 25] [--seconds 20] [--seed 3] [--direction 1|-1] [--pass]
//     [--trace]
//
// --pass flies straight passes 300 m long beside the walker, --radius off
// its side, back and forth, instead of the circle.
import * as THREE from 'three';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const radius = Number(opt('radius', 35));
const height = Number(opt('height', 8));
const speed = Number(opt('speed', 25));
const seconds = Number(opt('seconds', 20));
const seed = Number(opt('seed', 3));
const direction = Number(opt('direction', 1)); // 1 counter-clockwise (walker on the left), -1 clockwise
const trace = argv.includes('--trace');
const pass = argv.includes('--pass');
const { headlessGame, idleCommand } = await import('./headless-world.mjs');

const game = await headlessGame(`map=hot1&side=all&ai=classic&seed=${seed}&heroes=0`);
await game.ready;
game.begin?.();
const step = () => game.step(idleCommand(game));
const system = game.vehicles;
// Until the bots have spawned, with a speeder still on its pad.
const ready = () => game.units.all().some((u) => u.alive && u.kind === 0 && u.character && !u.seat && u !== game.playerUnit && u.team === 2);
for (let i = 0; i < 1800 && !ready(); i++) step();
const speeder = system.vehicles.find((v) => v.alive && /snowspeeder/.test(v.name) && !v.occupied);
const walker = system.vehicles.find((v) => v.alive && v.base === 'commandwalker');
if (!speeder || !walker) throw new Error(`no free snowspeeder (${Boolean(speeder)}) or AT-AT (${Boolean(walker)})`);
const UP = new THREE.Vector3(0, 1, 0);
const a0 = Math.atan2(speeder.position.x - walker.position.x, speeder.position.z - walker.position.z);
let a = a0;
let along = -150, way = 1;
const place = () => {
  const c = walker.position;
  let yaw;
  if (pass) {
    // Beside the walker, along its heading, |radius| off its side.
    const f = { x: Math.sin(walker.yaw), z: Math.cos(walker.yaw) }, side = { x: f.z * direction, z: -f.x * direction };
    speeder.position.set(c.x + side.x * radius + f.x * along, c.y + height, c.z + side.z * radius + f.z * along);
    yaw = walker.yaw + (way > 0 ? 0 : Math.PI);
  } else {
    speeder.position.set(c.x + Math.sin(a) * radius, c.y + height, c.z + Math.cos(a) * radius);
    // Along the circle: (cos a, -sin a) turning one way, its opposite the other.
    yaw = direction > 0 ? a + Math.PI / 2 : a - Math.PI / 2;
  }
  speeder.yaw = yaw;
  speeder.quaternion.setFromAxisAngle(UP, yaw);
  speeder.flightPreviousQ?.copy(speeder.quaternion); speeder.flightTickQ?.copy(speeder.quaternion);
  speeder.flightState = 'Flying'; speeder.grounded = false; speeder.sleeping = false;
  speeder.velocity.set(Math.sin(yaw) * speed, 0, Math.cos(yaw) * speed);
  speeder.airspeed = speed; speeder.throttle = .5;
  speeder.previous.copy(speeder.position);
  speeder.syncBody();
};
// The player at the controls.
game.driver.release?.();
game.soldier.spawn(speeder.position.x, speeder.position.y + 2, speeder.position.z, 0);
if (!game.driver.enter(speeder) || speeder.seatIndex !== 0) throw new Error(`the player did not take the pilot's seat (${speeder.seatIndex})`);
// A Rebel bot in the rear seat.
const bot = game.units.all().find((u) => u.alive && u.kind === 0 && u.team === speeder.controllingTeam && u.character && !u.seat && u !== game.playerUnit);
if (!bot || !system.board(bot, speeder, 1)) throw new Error('no bot boarded the rear seat');
const lease = speeder.seats[1].lease;
console.log(`speeder #${system.vehicles.indexOf(speeder)} team ${speeder.controllingTeam}, walker #${system.vehicles.indexOf(walker)} team ${walker.team}; gunner ${bot.className}; ${pass ? 'passes' : 'circle'} r=${radius} h=${height} v=${speed} ${direction > 0 ? 'ccw/left' : 'cw/right'}`);
const deg = (r) => ((((r * 180) / Math.PI) % 360) + 540) % 360 - 180;
const stats = { shots: {}, harpoons: 0, held: 0, seconds: 0, target: {}, agent: {}, onWalker: 0, inArc: 0 };
const launchTow = system.launchTow.bind(system);
system.launchTow = (...args) => {
  const ok = launchTow(...args);
  if (args[0] !== speeder) return ok;
  stats.harpoons++;
  if (ok) stats.held++;
  console.log(`  t=${(n / 60).toFixed(2)} harpoon ${ok ? `HELD leg ${speeder.towLeg}` : 'missed'} at ${speeder.position.distanceTo(walker.position).toFixed(1)} m`);
  return ok;
};
const onFired = system.onFired;
system.onFired = (v, seat, shot, ...rest) => {
  if (v === speeder && seat === 1) stats.shots[shot.weapon?.def?.class ?? '?'] = (stats.shots[shot.weapon?.def?.class ?? '?'] ?? 0) + 1;
  return onFired?.(v, seat, shot, ...rest);
};
const towed = { wound: 0, tripped: false };
const releaseTow = system.releaseTow.bind(system);
system.releaseTow = (source, reason) => {
  if (source === speeder && source.towTarget >= 0) console.log(`  t=${(n / 60).toFixed(2)} cable let go: ${reason} (wound ${source.towWinding.toFixed(2)} rad)`);
  if (reason === 'tripped') towed.tripped = true;
  return releaseTow(source, reason);
};
let n = 0;
const total = seconds * 60;
const w = direction * speed / radius; // rad/s round the walker
for (n = 1; n <= total && walker.alive; n++) {
  a += w / 60;
  along += way * speed / 60;
  if (Math.abs(along) > 150) way = -way;
  place();
  step();
  if (n % 30) continue;
  const controller = lease.controllable?.controller;
  const agent = controller?.agent;
  const target = agent?.tracker?.target?.unit ?? agent?.tracker?.target ?? null;
  const p = controller?.perception;
  const seat = speeder.seats[1];
  const toWalker = Math.atan2(walker.position.x - speeder.position.x, walker.position.z - speeder.position.z);
  const rel = deg(seat.aimYaw - speeder.yaw), want = deg(toWalker - speeder.yaw), err = deg(seat.aimYaw - toWalker);
  const name = target ? (target.className ?? target.vehicle?.name ?? target.name) : 'none';
  if (n % 60 === 0) {
    stats.seconds++;
    stats.target[name] = (stats.target[name] ?? 0) + 1;
    const key = `${agent?.constructor.name ?? '-'}/${agent?.state ?? '-'}${agent?.patterns && agent.state === 4 ? `/p${agent.patterns.state}` : ''}`;
    stats.agent[key] = (stats.agent[key] ?? 0) + 1;
    if (Math.abs(err) < 10) stats.onWalker++;
    if (want >= 80 || want <= -80) stats.inArc++;
  }
  if (trace) {
    const fire = p?.fire;
    const ch = fire?.channel ?? 0;
    const weapon = lease.controllable?.combat?.arsenal?.current?.(ch)?.def?.class ?? '-';
    console.log(`t=${(n / 60).toFixed(1)} ${agent?.constructor.name}/${agent?.state}${agent?.patterns && agent.state === 4 ? `/p${agent.patterns.state}` : ''} scan=${p?.lastScanCount ?? '-'} tgt=${name} w=${weapon} gun=${rel.toFixed(0)} walkerAt=${want.toFixed(0)} err=${err.toFixed(0)} buttons=${fire?.buttons.map((b) => b.count).join('/')} aiming=${lease.input.aiming} tow=${speeder.towTarget} wound=${speeder.towWinding.toFixed(2)}`);
  }
}
console.log(JSON.stringify({ ...stats, walkerAlive: walker.alive, tripped: towed.tripped }));
process.exit(0);
