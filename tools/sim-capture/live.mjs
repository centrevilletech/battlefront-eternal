// LiveObjects owns traversal and target choice. Lifecycle changes remain
// calls to their targets, whose Building/Trap/Vehicle methods have their
// own native replay gates. Callback entry geometry is checked before
// applying scratch-vector changes from nested lifecycle queries.
const xyz = p => [p.x, p.y, p.z];
const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
export const liveTarget = t => ({ self: t, alive: Boolean(t.alive), state: t.state ?? null, healthType: t.healthType ?? null,
  health: t.health ?? NaN, maxHealth: t.maxHealth ?? NaN, team: t.team ?? NaN, body: t.bodies?.[t.state] ?? null,
  gunner: Boolean(t.gunner), position: xyz(t.position), matrix: t.matrix?.toArray() ?? identity });
const amounts = ['BuildingHealth', 'VehicleHealth', 'BuildingRebuild', 'BuildingBuild'];
const scales = ['PersonScale', 'AnimalScale', 'DroidScale', 'VehicleScale', 'BuildingScale'];
export const liveDefinition = d => ({ repair: amounts.map(k => d?.[k]), targetVehicle: d?.TargetVehicle, targetNeutral: d?.TargetNeutral,
  targetEnemy: d?.TargetEnemy, targetFriendly: d?.TargetFriendly, range: d?.LockOnRange, angle: d?.LockOnAngle,
  shieldScale: d?.ShieldScale, damage: d?.Damage, inner: d?.DamageRadiusInner, outer: d?.DamageRadiusOuter, scales: scales.map(k => d?.[k]) });
const prepared = new WeakSet(), entered = new WeakSet(), contexts = [], frames = new WeakMap();
const queryState = live => ({ center: xyz(live.center), toward: xyz(live.toward) });
const effects = new Map();
function effect(name, run) {
  const live = contexts.at(-1)?.live, before = live && queryState(live);
  try { return run(); } finally { if (live) effects.set(name, { before, after: queryState(live), targets: contexts.at(-1).targets.map(liveTarget) }); }
}
const foreign = { teamScale(fn, target) { return effect('teamScale', () => fn(target)); }, may(fn, target) { return effect('may', () => fn(target)); } };
let hitOwner = -1;
const methodTarget = (method, args) => method === 'struck' ? args[0]?.owner : method === 'lockOn' ? args[3] : method === 'repair' || method === 'repairs' ? args[1] : null;
const definition = (method, args) => method === 'blast' ? args[0] : method === 'struck' ? args[1] : method === 'lockOn' ? args[2] : method === 'repair' || method === 'repairs' ? args[0] : null;
function isBuilding(value) {
  for (let p = value; p; p = Object.getPrototypeOf(p)) if (p.constructor?.name === 'Building') return true;
  return false;
}
function prepare(game) {
  if (prepared.has(game)) return;
  prepared.add(game);
  const live = game.live;
  const targets = [...new Set([...live.buildings, ...(live.vehicleTargets ?? [])])];
  for (const [index, target] of targets.entries()) for (const method of ['damage', 'setHealth']) {
    const base = target[method]; if (typeof base !== 'function') continue;
    target[method] = function (...args) { return effect(`${method}:${index}`, () => base.apply(this, args)); };
  }
  for (const [method, index, callback] of [['blast', 3, 'teamScale'], ['struck', 3, 'teamScale'], ['enterable', 2, 'may']]) {
    const base = live[method];
    live[method] = function (...args) {
      const fn = args[index];
      if (typeof fn === 'function') args[index] = target => foreign[callback](fn, target);
      return base.apply(this, args);
    };
  }
  if (live.collision) {
    const base = live.collision.cast;
    live.collision.cast = function (...args) {
      const result = base.apply(this, args);
      hitOwner = contexts.at(-1)?.targets.indexOf(result?.body?.owner) ?? -1;
      return result;
    };
  }
}
function boundaries(game) {
  prepare(game);
  const out = [['live.teamScale', foreign, 'teamScale', () => effects.get('teamScale')], ['live.may', foreign, 'may', () => effects.get('may')]];
  if (game.live.collision) out.push(['live.cast', game.live.collision, 'cast', () => ({ owner: hitOwner })]);
  const targets = [...new Set([...game.live.buildings, ...(game.live.vehicleTargets ?? [])])];
  for (const [index, target] of targets.entries()) for (const method of ['sphere', 'blastSphere', 'damage', 'setHealth']) {
    if (typeof target[method] !== 'function') continue;
    out.push([`live.${method}:${index}`, target, method, () => ({ ...effects.get(`${method}:${index}`), targets: contexts.at(-1).targets.map(liveTarget) })]);
  }
  return out;
}
export function liveState(method, game, args, live) {
  let frame = frames.get(args);
  if (!entered.has(args)) {
    entered.add(args);
    const known = [...new Set([...live.buildings, ...(live.vehicleTargets ?? [])])], one = methodTarget(method, args);
    const targets = one && !known.includes(one) ? [...known, one] : known;
    frame = { targets, one, live };
    frames.set(args, frame); contexts.push(frame);
  } else if (contexts.pop() !== frame) throw Error('live capture frame order');
  return { center: xyz(live.center), toward: xyz(live.toward), hit: live.hit, playerTeam: live.playerTeam ?? NaN, collision: Boolean(live.collision),
    targets: frame.targets.map(liveTarget), buildings: live.buildings.map(t => frame.targets.indexOf(t)),
    vehicles: (live.vehicleTargets ?? []).map(t => frame.targets.indexOf(t)), turrets: live.turrets.map(t => frame.targets.indexOf(t)),
    one: frame.one ? frame.targets.indexOf(frame.one) : -1, definition: liveDefinition(definition(method, args)),
    building: method !== 'struck' || isBuilding(frame.one) };
}
export const liveObjects = { frames: ['blast', 'struck', 'lockOn', 'repairs', 'repair', 'enterable'].map(method => ({
  method: `LiveObjects.${method}`, target: game => game.live, boundaries,
  state: (game, args, cap, self) => liveState(method, game, args, self),
})) };
