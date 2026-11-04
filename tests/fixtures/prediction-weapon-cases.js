import { Arsenal, Weapon } from '../../src/weapons.js';
import { prepareMelee } from '../../src/heroes.js';
import { PlayerCombat } from '../../src/combat.js';
import { WeaponPrediction } from '../../src/runtime/weapon-prediction.js';
import { MountedWeaponPrediction } from '../../src/runtime/mounted-weapon-prediction.js';
import { captureWeaponState } from '../../src/runtime/network-weapon-state.js';

export const weaponDefinition = (name, channel = 0, extra = {}) => ({
  class: name, base: 'cannon', channel, RoundsPerClip: 8, ammo: 5, ShotDelay: 0.1, ReloadTime: 0.23,
  InitialSalvoDelay: 0, SalvoCount: 1, ShotsPerSalvo: 1, SalvoDelay: 0,
  YawSpread: 1.7, PitchSpread: 1.3, StandStillSpread: 0.4, CrouchStillSpread: 0.2, ProneStillSpread: 0.1,
  StandMoveSpread: 0.8, CrouchMoveSpread: 0.6, ProneMoveSpread: 0.5, StandAimSpread: 0.1, CrouchAimSpread: 0.05, ProneAimSpread: 0,
  SpreadPerShot: 0.2, SpreadThreshold: 0.3, SpreadRecoverRate: 0.4, SpreadLimit: 2.1,
  HeatPerShot: 0.07, HeatRecoverRate: 0.12, HeatThreshold: 0.3,
  ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 1 }, ...extra,
});
const forbiddenRandom = () => { throw Error('authority random stream used by prediction'); };
const forbiddenAdmission = () => { throw Error('authority admission called by prediction'); };
const prepare = definitions => {
  const arsenal = prepareMelee(new Arsenal(definitions, { random: forbiddenRandom }));
  for (const weapon of arsenal.weapons) weapon.admitShot = forbiddenAdmission;
  return arsenal;
};
export function createWeaponUnit(definitions) {
  const soldier = { posture: 'stand', aimYaw: -0, aimPitch: 0.2, air: null, dive: 0, lock: 0, rolled: false };
  const combat = Object.create(PlayerCombat.prototype);
  Object.assign(combat, { arsenal: prepare(structuredClone(definitions)), soldier, zoomStep: 0, dead: false, gunUnderwater: false });
  return { id: 17, generation: 3, alive: true, soldier, combat };
}
export function weaponObservation(weapon) {
  return { name: weapon.name, bytes: Array.from(captureWeaponState(weapon)), level: weapon.level,
    salvoSeed: weapon.salvoSeed, salvoCount: weapon.salvoCount, seeded: !!weapon.salvoRandom,
    melee: weapon.melee?.captureState() ?? null };
}
export function arsenalObservation(arsenal, zoomStep) {
  return { selected: arsenal.selected.slice(), zoomStep, weapons: arsenal.weapons.map(weaponObservation) };
}
export function shotObservation(shot, arsenal) {
  return { ...Object.fromEntries(Object.entries(shot).filter(([key]) => key !== 'weapon')),
    weapon: arsenal.weapons.indexOf(shot.weapon), name: shot.weapon.name, level: shot.weapon.levels.indexOf(shot.weapon.def) };
}
const command = (tick, actions = [], down = []) => ({ tick, sequence: tick, generation: 3, down, actions, look: [] });
const begin = (tick = 0, checkpoint = null, epoch = 2) => ({ op: 'begin', tick, checkpoint, epoch });
const step = (tick, actions = [], down = [], extra = {}) => ({ op: 'step', tick, command: command(tick, actions, down), ...extra });
const repeated = (count, make) => Array.from({ length: count }, (_, i) => make(i + 1));

export function weaponPredictionCases() {
  const automatic = repeated(200, tick => {
    const actions = [];
    if (tick % 19 === 1) actions.push('firePrimary');
    if (tick % 31 === 2) actions.push('fireSecondary');
    if ([41, 101, 161].includes(tick)) actions.push('nextPrimary');
    if ([61, 121, 181].includes(tick)) actions.push('previousPrimary');
    if (tick % 43 === 7) actions.push('reload');
    if (tick % 47 === 9) actions.push('zoom', 'zoom');
    return step(tick, actions, tick % 39 < 27 ? ['Mouse0'] : tick % 4 ? ['Mouse2'] : [],
      { presses: [tick % 19 === 1 ? tick * 7 : null, tick % 31 === 2 ? 0xffffffff - tick : null] });
  });
  const charge = repeated(180, tick => step(tick,
    [...(tick % 30 === 1 ? ['firePrimary'] : []), ...(tick % 47 === 5 ? ['fireSecondary'] : []), ...(tick === 113 ? ['nextPrimary'] : [])],
    tick % 30 < 17 ? ['Mouse0'] : tick % 47 >= 5 && tick % 47 < 10 ? ['Mouse2'] : [], { presses: [tick + 35, tick + 97] }));
  const blocked = repeated(180, tick => step(tick, ['firePrimary', ...(tick % 7 === 0 ? ['zoom'] : [])], ['Mouse0', ...(tick % 4 ? ['KeyW'] : [])], {
    soldier: { posture: ['stand', 'crouch', 'prone'][Math.floor(tick / 9) % 3], air: [null, 'jump', 'fall', 'thrown', 'jet'][Math.floor(tick / 11) % 5],
      dive: tick % 37 === 1 ? -1 : 0, lock: tick % 41 === 2 ? 0.001 : 0, rolled: tick % 43 === 3 },
    combat: { gunUnderwater: tick % 23 === 0 }, presses: [tick % 4 ? tick : false, null],
  }));
  const burstSteps = repeated(36, tick => step(tick, tick === 1 || tick === 23 ? ['firePrimary'] : [], ['Mouse0'], { presses: [tick === 1 ? 9123 : tick === 23 ? 478 : null, null] }));
  const melee = repeated(180, tick => step(tick,
    [...(tick % 31 === 1 ? ['firePrimary', 'fireSecondary'] : []), ...(tick % 43 === 2 ? ['nextSecondary'] : [])], tick % 8 ? ['Mouse0', 'Mouse2'] : [], { presses: [tick * 101, tick * 311] }));
  return [
    { name: 'automatic-reload-selection', definitions: [weaponDefinition('rifle'), weaponDefinition('pistol', 0, { TriggerSingle: 1, ZoomMin: 2, ZoomMax: 6 }), weaponDefinition('secondary', 1)],
      initial: [{ index: 0, fields: { clip: 0, reserve: 2, reloadLeft: 2 / 60 } }], operations: [begin(), ...automatic, { op: 'restore' }] },
    { name: 'charge-levels-throws-patterns', definitions: [weaponDefinition('charged', 0, { MaxPressedTime: 0.4, ShotsPerSalvo: 2,
      charges: [{ at: 0.25, ShotsPerSalvo: 3, ShotPatternPitchYaw: [[1, -2], [0, 0], [-1, 2]] },
        { at: 0.75, ShotsPerSalvo: 5, ShotPatternCount: 2, ShotPatternPitchYaw: [[9, 9], [0.2, -0.3], [-0.2, 0.3]] }] }),
      weaponDefinition('single', 0, { TriggerSingle: 1, ShotDelay: 0.031 }),
      weaponDefinition('thrown', 1, { base: 'grenade', MaxPressedTime: 0.18, InitialSalvoDelay: 0.05, ShotDelay: 0.09, HideOnFire: 1 })],
      operations: [begin(), ...charge, { op: 'restore' }] },
    { name: 'blocking-zoom-and-posture', definitions: [weaponDefinition('scoped', 0, { ZoomMin: 2, ZoomMax: 8, TriggerSingle: 1, ShotDelay: 0.02 }), weaponDefinition('tool', 1, { base: 'repair', ShotDelay: 0.04 })],
      initial: [{ index: 1, fields: { firing: true, timer: 0.005, heat: 0.4 } }], operations: [begin(), ...blocked, { op: 'restore' }] },
    { name: 'checkpoint-keeps-current-salvo-generator', definitions: [weaponDefinition('burst', 0, { InitialSalvoDelay: 0.025, SalvoCount: 5, SalvoDelay: 0.04, ShotsPerSalvo: 3, RoundsPerClip: 80 })],
      operations: [begin(), ...burstSteps.slice(0, 4), { op: 'checkpoint', key: 'mid' }, ...burstSteps.slice(4, 14), { op: 'restore' }, begin(4, 'mid'),
        ...burstSteps.slice(4, 14), { op: 'restore' }, { op: 'unit', generation: 4 }, begin(4, 'mid'), ...burstSteps.slice(4), { op: 'restore' }] },
    { name: 'melee-channel-order-and-inactive-timeline', definitions: [weaponDefinition('cannon'), weaponDefinition('blade', 1, { base: 'melee', swings: [
      { duration: 0.16, hit: 0.03, sound: 'swing-a' }, { duration: 0.13, hit: 0.06, sound: null },
      { duration: 0.22, hit: 0.1, sound: 'swing-c' }, { duration: 100, hit: 50, sound: 'unused' }] }), weaponDefinition('tool', 1, { base: 'repair' })],
      initial: [{ index: 1, melee: { time: 0.01, index: 0, active: true }, fields: { firing: true } }], operations: [begin(), ...melee, { op: 'restore' }] },
    { name: 'deployed-and-shield-suppression', definitions: [weaponDefinition('remote', 0, { base: 'remote', droid: { base: 'droid' }, InitialSalvoDelay: 0.03 }),
      weaponDefinition('shield', 1, { base: 'shield' }), weaponDefinition('ordinary')], initial: [{ index: 0, fields: { deployed: true, heat: 0.7, wait: 0.2 } }],
      operations: [begin(), ...repeated(90, tick => step(tick, ['firePrimary', 'fireSecondary', ...(tick === 47 ? ['nextPrimary'] : [])], ['Mouse0', 'Mouse2'])),
        { op: 'restore' }, { op: 'source' }, begin(90), ...repeated(25, i => step(90 + i, ['firePrimary'], ['Mouse0'])), { op: 'restore' }] },
  ];
}

export function runWeaponPredictionCase(scenario, { native = null, onShot = null, after = null } = {}) {
  const unit = createWeaponUnit(scenario.definitions), prediction = new WeaponPrediction({ native }), checkpoints = new Map(), observations = [];
  let authority = unit.combat.arsenal;
  for (const patch of scenario.initial || []) { Object.assign(authority.weapons[patch.index], patch.fields); if (patch.melee) authority.weapons[patch.index].melee.restoreState(patch.melee); }
  for (let at = 0; at < scenario.operations.length; at++) {
    const operation = scenario.operations[at], emitted = []; let shots = [];
    if (operation.op === 'begin') prediction.begin(unit, { tick: operation.tick, epoch: operation.epoch, checkpoint: checkpoints.get(operation.checkpoint) ?? null });
    else if (operation.op === 'step') {
      Object.assign(unit.soldier, operation.soldier); Object.assign(unit.combat, operation.combat);
      shots = prediction.step(operation.command, operation.tick, (shot, command, tick) => {
        emitted.push({ shot: shotObservation(shot, prediction.arsenal), command: structuredClone(command), tick });
        onShot?.({ prediction, unit, shot, command, tick });
      }, operation.presses).map(shot => shotObservation(shot, prediction.arsenal));
    } else if (operation.op === 'checkpoint') checkpoints.set(operation.key, prediction.checkpoint());
    else if (operation.op === 'restore') prediction.restore();
    else if (operation.op === 'unit') { if (operation.generation !== undefined) unit.generation = operation.generation; if (operation.id !== undefined) unit.id = operation.id; }
    else if (operation.op === 'source') { authority = prepare(structuredClone(scenario.definitions)); unit.combat.arsenal = authority; }
    else throw Error(`unknown fixture operation ${operation.op}`);
    const observation = { operation: operation.op, shots, emitted, active: unit.combat.arsenal !== authority,
      state: arsenalObservation(unit.combat.arsenal, unit.combat.zoomStep), authority: arsenalObservation(authority, 0),
      private: prediction.arsenal ? arsenalObservation(prediction.arsenal, prediction.combat.zoomStep) : null,
      presentedThrough: prediction.presentedThrough };
    observations.push(observation); after?.({ prediction, unit, observation, operation, at });
  }
  return { prediction, unit, authority, observations, checkpoints };
}

export function mountedPredictionCases() {
  const definitions = [weaponDefinition('seat-primary', 0, { RoundsPerClip: 6, HeatPerShot: 0.3, HeatRecoverRate: 0.4, HeatThreshold: 0.2 }),
    weaponDefinition('seat-secondary', 1, { TriggerSingle: 1, ShotsPerSalvo: 3, ShotPatternPitchYaw: [[1, -1], [2, 0], [1, 1]] })];
  const pending = repeated(120, i => ({ ...command(i + Math.floor(i / 7),
    [...(i % 23 === 1 ? ['firePrimary'] : []), ...(i % 31 === 2 ? ['fireSecondary'] : []), ...(i % 35 === 3 ? ['reload'] : [])],
    i % 39 < 27 ? ['Mouse0'] : ['Mouse2']), generation: i % 17 === 0 ? 2 : 3, sent: i % 29 === 0 ? false : true,
    mouseScale: 0.5, look: [], worldLook: [] }));
  const base = { definitions, pending, from: 0, held: [], view: { firstPerson: true }, entries: [{ weapon: 0, owner: 0, muzzles: 2 }, { weapon: 1, owner: 1, muzzles: 1 }] };
  return [
    { ...base, name: 'vehicle-heat-reload-barrels' },
    { ...base, name: 'turret-barrel', entries: [{ weapon: 0, owner: 0, muzzles: 0 }], turret: true },
    { ...base, name: 'shared-weapon-and-owner', entries: [{ weapon: 0, owner: 0, muzzles: 2 }, { weapon: 0, owner: 0, muzzles: 1 }, { weapon: 1, owner: 0, muzzles: 1 }] },
    { ...base, name: 'missing-muzzles-and-tow', entries: [{ weapon: 0, owner: 0, muzzles: 0 }, { weapon: 1, owner: 1, muzzles: 1, tow: true }] },
    { ...base, name: 'late-collapsed-and-unsent', from: 120, held: ['Mouse2'] },
    { ...base, name: 'flying', flying: true, flightState: 'Flying' },
    { ...base, name: 'takeoff-freezes', flying: true, flightState: 'TakingOff', from: 7 },
    { ...base, name: 'landing-freezes', flying: true, flightState: 'Landing', from: 7 },
    { ...base, name: 'released-lease', released: true },
    { ...base, name: 'dead-combat', dead: true },
    { ...base, name: 'dead-unit', alive: false },
    { ...base, name: 'empty-pending', pending: [] },
    { ...base, name: 'empty-seat', entries: [] },
    { ...base, name: 'wrong-generation', pending: pending.map(c => ({ ...c, generation: 9 })) },
  ];
}

export function runMountedPredictionCase(scenario, { native = null, ownerRead = null, ownerWrite = null, towCall = null, afterPredict = null } = {}) {
  const weapons = structuredClone(scenario.definitions).map(d => new Weapon(d, { random: forbiddenRandom }));
  for (const weapon of weapons) weapon.admitShot = forbiddenAdmission;
  const owners = Array.from({ length: Math.max(...scenario.entries.map(e => e.owner)) + 1 }, (_, index) => {
    let barrel = index * 3;
    return { get barrel() { ownerRead?.(index, barrel); return barrel; }, set barrel(value) { barrel = value; ownerWrite?.(index, value); } };
  });
  const entries = scenario.entries.map(e => ({ state: weapons[e.weapon], owner: owners[e.owner], muzzles: Array.from({ length: e.muzzles }, () => ({})) }));
  const vehicle = { flying: !!scenario.flying, flightState: scenario.flightState, seats: [{ weapons: entries }], towHeld(entry) {
    const i = entries.indexOf(entry); towCall?.(i); return !!scenario.entries[i].tow;
  } };
  const unit = { generation: 3, alive: scenario.alive !== false, combat: { dead: !!scenario.dead }, seat: scenario.turret
    ? { turret: Object.assign(owners[0], { weapon: weapons[0] }) } : { index: 0, vehicle, released: !!scenario.released } };
  const prediction = new MountedWeaponPrediction({ native }), observe = () => ({ weapons: weapons.map(weaponObservation), barrels: owners.map(o => o.barrel), tick: prediction.tick });
  const before = observe(), ticks = [], observations = [];
  for (let i = 0; i < 2; i++) { ticks.push(prediction.predict(unit, scenario.pending, scenario.from, scenario.held, scenario.view)); observations.push(observe()); afterPredict?.(prediction); }
  unit.seat = null; prediction.restore(); const restored = observe();
  return { prediction, unit, weapons, owners, before, ticks, observations, restored };
}
