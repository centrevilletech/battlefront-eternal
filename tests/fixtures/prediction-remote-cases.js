import assert from 'node:assert/strict';
import { Triangle, Vector3 } from 'three';
import { RemoteDroids } from '../../src/remote.js';
import { Weapon, seededRandom } from '../../src/weapons.js';
import { reconRandom } from '../../src/runtime/network-remote-world.js';
import { captureRemoteReplay } from '../../src/runtime/network-remote-replay.js';
import { captureWeaponState } from '../../src/runtime/network-weapon-state.js';
import { predictRemote, RemotePredictionContext } from '../../src/runtime/network-remote-prediction.js';

export const remoteGun = (extra = {}) => ({ class: 'recon-cannon', base: 'blaster', channel: 0, RoundsPerClip: 11, ammo: 5,
  ShotDelay: .11, ReloadTime: .19, InitialSalvoDelay: 0, SalvoCount: 1, ShotsPerSalvo: 1, SalvoDelay: 0,
  ZoomMin: 2.5, ZoomMax: 4, YawSpread: 1.3, PitchSpread: .7, StandStillSpread: .2, StandAimSpread: .1,
  SpreadThreshold: .25, SpreadPerShot: .03, SpreadLimit: .9, SpreadRecoverRate: .02,
  HeatPerShot: .01, HeatRecoverRate: .03, ordnance: { base: 'bolt', Velocity: 90 }, ...extra });
export const remoteStrike = (extra = {}) => ({ class: 'recon-destruct', base: 'destruct', channel: 1,
  MaxPressedTime: .14, SalvoCount: 4, SalvoDelay: .13, AimDistance: 15, AimElevation: 73, AimAzimuth: -12,
  ScatterDistance: 4, strike: { base: 'bolt', Velocity: 120 }, ...extra });
export const remoteDefinition = (extra = {}) => ({ GeometryName: 'probe', MaxHealth: 70, AddHealth: -.2,
  SetAltitude: 1.75, LiftSpring: 5.25, LiftDamp: 2.125, MaxSpeed: 9, MaxStrafeSpeed: 7, MaxTurnSpeed: 2.25,
  GravityScale: .9, Acceleraton: 70, PitchLimits: [-43, 52], EyePointOffset: [.03, .5, .07],
  TrackCenter: [.1, .55, .02], TrackOffset: [.15, -.25, 1.5], TiltValue: 5,
  ThirdPersonFOV: 51, FirstPersonFOV: 48, CollisionThreshold: [7.5, 2.5], CollisionScale: [10],
  explosion: { Damage: 9 }, weapons: [remoteGun(), remoteStrike()], ...extra });

export function remoteCommands(count = 120, { from = 0, pattern = 'drive', generation = 1 } = {}) {
  return Array.from({ length: count }, (_, i) => {
    const tick = from + i + 1, fire = pattern !== 'idle' && pattern !== 'no-fire';
    const down = [...(pattern === 'idle' ? [] : i % 51 < 39 ? ['KeyW'] : ['KeyS']),
      ...(i % 33 < 11 ? ['KeyD'] : i % 33 < 22 ? ['KeyA'] : []), ...(i % 61 < 9 ? ['ArrowLeft'] : []),
      ...(fire && i % 31 < 19 ? ['Mouse0'] : [])];
    const actions = [...(fire && i % 17 === 0 ? ['firePrimary'] : []), ...(i === 20 || i === 51 ? ['zoom'] : []),
      ...(pattern === 'destruct' && i === 4 ? ['fireSecondary'] : []), ...(pattern === 'exit' && i === 70 ? ['use'] : [])];
    return { tick, epoch: 1, sequence: i + 1, generation, down, actions,
      look: [[i % 5 - 2, i % 7 - 3], [i % 23 === 0 ? -0 : .25, -.125]], mouseScale: i % 11 === 0 ? 3 : .23,
      invertMouse: i % 19 === 0, firstPerson: i % 43 < 17, aspect: i % 29 === 0 ? 2 : 1.5 };
  });
}

function planes(scenario) {
  if (scenario.noFloor) return [];
  const slope = scenario.slope ?? 0, normal = new Vector3(-slope, 1, 0).normalize();
  return [{ normal, point: new Vector3() }, { normal: new Vector3(0, 0, -1), point: new Vector3(0, 0, scenario.wall ?? 7) }];
}
function triangles(scenario) {
  if (scenario.noFloor) return [];
  const wall = scenario.wall ?? 7, slope = scenario.slope ?? 0;
  const a = new Vector3(-100, -100 * slope, -100), b = new Vector3(100, 100 * slope, -100), c = new Vector3(100, 100 * slope, 100), d = new Vector3(-100, -100 * slope, 100);
  return [new Triangle(a, b, c), new Triangle(a, c, d),
    new Triangle(new Vector3(-100, -100, wall), new Vector3(100, -100, wall), new Vector3(100, 100, wall)),
    new Triangle(new Vector3(-100, -100, wall), new Vector3(100, 100, wall), new Vector3(-100, 100, wall))];
}
export function remoteObservation(context, droid) {
  const state = structuredClone(captureRemoteReplay(context, droid));
  return { state, level: droid.arsenal.weapons.map(w => w.level), bytes: droid.arsenal.weapons.map(w => Array.from(captureWeaponState(w))),
    omitted: droid.arsenal.weapons.map(w => ({ salvoSeed: w.salvoSeed, salvoCount: w.salvoCount, random: w.random === context.random,
      randomSeed: w.random?.seed, seeded: !!w.salvoRandom })),
    order: context.droids.map(d => ({ position: d.position.toArray(), dead: d.dead, health: d.health, salvo: !!d.salvo, main: d === droid })),
    strikes: context.strikes.map(d => ({ main: d === droid, dead: d.dead, left: d.salvo?.left, wait: d.salvo?.wait })),
    camera: context.aimCamera ? { position: context.aimCamera.position.toArray(), quaternion: context.aimCamera.quaternion.toArray(),
      fov: context.aimCamera.fov, aspect: context.aimCamera.aspect, projection: context.aimCamera.projectionMatrix.toArray(),
      world: context.aimCamera.matrixWorld.toArray(), inverse: context.aimCamera.matrixWorldInverse.toArray() } : null,
    motion: context.simulationView ? { offset: context.simulationView.motion.offset.toArray(), turn: context.simulationView.motion.turn.toArray() } : null,
    hoverTime: context.hoverTime };
}
const shotState = (shot, droid) => ({ weapon: droid.arsenal.weapons.indexOf(shot.weapon),
  ...Object.fromEntries(Object.entries(shot).filter(([name]) => name !== 'weapon')) });

export function createRemoteFixture(scenario, { onQuery = null, onFired = null, collision: realCollision = null } = {}) {
  const events = [], shots = [], surfaces = planes(scenario), polys = [{ triangles: triangles(scenario) }], hit = { distance: 0, point: new Vector3(), normal: new Vector3(), body: null, terrain: false };
  let effects = 0, firing = 0, viewContext = null, firstAt = null, changedFly = false;
  const record = (name, args, values = {}) => { const event = [name, ...args]; events.push(event); onQuery?.({ name, args, events, context: viewContext, ...values }); return event; };
  const collision = {
    cast(from, direction, far, out = hit) {
      record('cast', [from.toArray(), direction.toArray(), far], { from, direction, out });
      if (viewContext && scenario.mutateAim && far === 512) { from.x += .015625; direction.y += .03125; }
      if (viewContext && scenario.mutateFly && !changedFly && direction.x === 0 && direction.y === -1 && direction.z === 0 && far > 1 && far < 100) {
        changedFly = true; const droid = viewContext.droids[0];
        const patch = { GravityScale: .25, LiftSpring: 1.75, SetAltitude: 9, AddHealth: scenario.replaceDefinition ? -1000 : -.2 };
        if (scenario.replaceDefinition) droid.def = { ...droid.def, ...patch }; else Object.assign(droid.def, patch);
      }
      if (realCollision) return realCollision.cast(from, direction, far, out);
      let nearest = Infinity, picked = null;
      for (const plane of surfaces) {
        const divisor = direction.dot(plane.normal);
        if (Math.abs(divisor) < 1e-12) continue;
        const distance = plane.point.clone().sub(from).dot(plane.normal) / divisor;
        if (distance < 0 || distance > far || distance >= nearest) continue;
        nearest = distance; picked = plane;
      }
      if (!picked) return null;
      out.distance = nearest; out.point.copy(from).addScaledVector(direction, nearest); out.normal.copy(picked.normal);
      if (out.normal.dot(direction) > 0) out.normal.negate();
      out.body = null; out.terrain = picked === surfaces[0]; return out;
    },
    gather(center, radius) { record('gather', [center.toArray(), radius]); return realCollision ? realCollision.gather(center, radius) : polys; },
  };
  const water = scenario.water ? { over(x, z) { record('water-over', [x, z]); return z > -8 && x < 6; },
    get level() { record('water-level', []); return .65; } } : null;
  const marker = new Error('recon callback stopped');
  const base = new RemoteDroids({ scene: { add() { effects++; }, remove() { effects++; } }, camera: null, collision, water,
    lights: { flash() { effects++; } }, model: () => null, firePoints: { probe: [.125, .2, .375] }, fire() { effects++; }, explode() { effects++; },
    onFired(shot, at, droid) {
      viewContext = this; firing++;
      const event = { shot: shotState(shot, droid), at: at.toArray(), direction: this.v[4].toArray(),
        atIsScratch: at === this.v[2], sameOrigin: firstAt === null || firstAt === at, previousOrigin: firstAt?.toArray() ?? null,
        weaponBytes: Array.from(captureWeaponState(shot.weapon)), partial: remoteObservation(this, droid) };
      firstAt ??= at;
      shots.push(event); events.push(['fired', firing, event]);
      if (scenario.mutate && firing === 1) {
        droid.position.x += .0625; droid.velocity.y += .125; droid.health -= .25;
        shot.weapon.wait += .037; this.lookX += 3; this.zoomStep = 1;
        if (this.simulationView) { this.simulationView.motion.offset.y = .125; this.simulationView.motion.turn.x = .03; }
      }
      if (scenario.append && firing === 1) {
        const other = this.createDroid(droid.def, null, new Vector3(-3, 3, -2), new Vector3(.1, 0, -.2), .2, 2);
        other.health = .025; this.droids.push(other);
      }
      if (scenario.replaceWeapon && firing === 1) {
        const replacement = new Weapon(remoteGun({ class: 'replacement', ShotsPerSalvo: 2, RoundsPerClip: 31 }));
        droid.arsenal.weapons[0] = replacement; droid.arsenal.channels[0][0] = replacement; shot.weapon.clip = .777;
      }
      if (scenario.sharedSalvo && firing === 1) {
        this.strike(droid, droid.arsenal.current(1).def);
        const other = this.createDroid(droid.def, null, new Vector3(-3, 3, -2), new Vector3(), 0, 2);
        other.dead = true; other.salvo = droid.salvo; this.droids.push(other); this.strikes.push(other);
      }
      if (scenario.replaceRandom && firing === 1) { this.random = reconRandom(0x765432, 'callback'); this.strike(droid, droid.arsenal.current(1).def); }
      if (scenario.mutateDrain && firing === 1) droid.def.AddHealth = -1000;
      if (scenario.killOnFired && firing === 2) { this.strike(droid, droid.arsenal.current(1).def); this.kill(droid); }
      onFired?.({ context: this, droid, shot, at, event, count: firing, events });
      if (scenario.throwOnFired === firing) throw marker;
    } });
  const owner = { id: 23, generation: 4, team: 1, human: true, alive: true }, source = base.createContext(owner, { random: reconRandom(0x123abc, 'fixture') });
  const definition = remoteDefinition(structuredClone(scenario.definition || {}));
  const droid = source.launch({ def: { SwitchImmediately: 1, droid: definition } }, new Vector3(...(scenario.position || [0, 2, 0])),
    new Vector3(...(scenario.velocity || [0, 0, 0])), scenario.yaw ?? 0, 1);
  droid.networkGeneration = 1; source.firstPerson = false;
  if (scenario.camera !== false) source.updateCamera(1 / 60, false, true);
  else { source.aimCamera = undefined; source.simulationView = null; }
  if (scenario.patch) Object.assign(droid, scenario.patch);
  for (const patch of scenario.weapons || []) {
    const weapon = droid.arsenal.weapons[patch.index]; Object.assign(weapon, patch.fields);
    if (patch.definition !== undefined) weapon.def = weapon.levels[patch.definition];
  }
  if (scenario.strike) { source.strike(droid, droid.arsenal.current(1).def); if (scenario.dead) source.kill(droid); }
  events.length = 0; shots.length = 0; effects = 0;
  return { source, droid, collision, events, shots, marker, effects: () => effects,
    setContext(context) { viewContext = context; } };
}

export function remotePredictionCases() {
  const charged = remoteGun({ MaxPressedTime: .3, RoundsPerClip: 50,
    charges: [{ at: .2, ShotsPerSalvo: 2, ShotPatternPitchYaw: [[1, -2], [-1, 2]] }, { at: .7, ShotsPerSalvo: 4, YawSpread: 3 }] });
  const partial = remoteGun({ ShotsPerSalvo: 3, RoundsPerClip: 90, HeatPerShot: .03 });
  return [
    { name: 'hover-camera-look-and-zoom', ticks: 120, pattern: 'drive' },
    { name: 'slope-water-and-look-ahead', ticks: 120, slope: .125, water: true, velocity: [6, -.5, 1] },
    { name: 'wall-crash-and-death', ticks: 40, position: [0, 2, 6.6], velocity: [0, 0, 18], definition: { MaxHealth: 10 } },
    { name: 'fall-below-world-floor', ticks: 40, position: [0, -499, 0], velocity: [0, -20, 0], noFloor: true },
    { name: 'health-drain-and-filter', ticks: 60, definition: { MaxHealth: .2, AddHealth: -1 } },
    { name: 'strike-keeps-dead-owner-until-finished', ticks: 120, strike: true, dead: true },
    { name: 'destruct-countdown-strike-and-filter', ticks: 120, pattern: 'destruct' },
    { name: 'charge-levels-and-patterns', ticks: 120, definition: { weapons: [charged, remoteStrike()] } },
    { name: 'same-cursor-charge-level-write', ticks: 45, definition: { weapons: [charged, remoteStrike()] },
      weapons: [{ index: 0, definition: 1, fields: { level: 0, charging: true, held: .08 } }] },
    { name: 'restored-outstanding-seeded-salvo', ticks: 80, privateSeed: true,
      weapons: [{ index: 0, fields: { salvosLeft: 3, salvoWait: .01 } }] },
    { name: 'partial-shot-state-and-mutation', ticks: 100, mutate: true, definition: { weapons: [partial, remoteStrike()] } },
    { name: 'queued-shots-keep-replaced-weapon-identity', ticks: 60, replaceWeapon: true, definition: { weapons: [partial, remoteStrike()] } },
    { name: 'shared-salvo-clock-and-owner-filter', ticks: 90, sharedSalvo: true, definition: { weapons: [partial, remoteStrike()] } },
    { name: 'shot-callback-replaces-context-rng', ticks: 90, replaceRandom: true,
      definition: { weapons: [partial, remoteStrike({ SalvoCount: 8, SalvoDelay: .4 })] } },
    { name: 'aim-callback-mutates-shared-vectors', ticks: 60, mutateAim: true },
    { name: 'shot-callback-mutates-same-definition', ticks: 40, mutateDrain: true, definition: { MaxHealth: 12, weapons: [partial, remoteStrike()] } },
    { name: 'downward-cast-mutates-held-definition', ticks: 60, mutateFly: true },
    { name: 'downward-cast-replaces-definition', ticks: 40, mutateFly: true, replaceDefinition: true, definition: { MaxHealth: 12 } },
    { name: 'callback-appends-droid-before-filter-and-strikes', ticks: 100, append: true, killOnFired: true, definition: { weapons: [partial, remoteStrike()] } },
    { name: 'callback-throw-keeps-partial-state', ticks: 40, throwOnFired: 1, mutate: true, definition: { weapons: [partial, remoteStrike()] } },
    { name: 'headless-camera-freshness-and-exit', ticks: 120, camera: false, pattern: 'exit' },
    { name: 'late-gaps-unsent-and-window', ticks: 130, from: 20, gaps: true },
  ];
}

export function runRemotePredictionCase(scenario, { native = null, onQuery = null, onFired = null, before = null, collision = null } = {}) {
  const fixture = createRemoteFixture(scenario, { onQuery, onFired, collision }), { source, droid } = fixture, cache = new RemotePredictionContext();
  const { context, predicted } = cache.acquire(source, droid); fixture.setContext(context);
  if (scenario.privateSeed) {
    const weapon = predicted.arsenal.weapons[0]; weapon.salvoSeed = 987; weapon.salvoCount = 8; weapon.salvoRandom = seededRandom(987);
    for (let i = 0; i < 8; i++) weapon.salvoRandom();
  }
  before?.({ ...fixture, cache, context, predicted });
  fixture.events.length = 0; fixture.shots.length = 0;
  const authority = remoteObservation(source, droid), weapons = droid.arsenal.weapons.slice(), random = source.random;
  const pending = remoteCommands(scenario.ticks, { from: scenario.from || 0, pattern: scenario.pattern || 'drive' });
  if (scenario.gaps) for (let i = 0; i < pending.length; i++) {
    pending[i].tick += Math.floor(i / 7);
    if (i % 17 === 0) pending[i].generation = 9;
    if (i % 19 === 2) pending[i].sent = false;
    if (i % 13 === 1) pending[i].worldLook = [[10001, -10001]];
  }
  let result, error = null;
  try { result = predictRemote(source, droid, pending, scenario.from || 0, 1, ['KeyW'], { aspect: 1.6, mouseScale: .5 }, cache, { native }); }
  catch (caught) { error = caught; }
  assert.deepEqual(remoteObservation(source, droid), authority, 'prediction leaves authority and random state unchanged');
  assert.equal(source.random, random); assert.deepEqual(droid.arsenal.weapons, weapons); assert.equal(fixture.effects(), 0, 'authority effects are suppressed');
  if (scenario.throwOnFired) assert.equal(error, fixture.marker, 'original callback error escapes'); else assert.ifError(error);
  return { ...fixture, cache, result, error, pending,
    observation: remoteObservation(context, predicted), partial: error ? remoteObservation(context, predicted) : null,
    disposed() { return result?.nativeSession?.disposed; } };
}
