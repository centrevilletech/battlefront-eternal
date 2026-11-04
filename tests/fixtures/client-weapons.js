import { Arsenal } from '../../src/weapons.js';
import { prepareMelee } from '../../src/heroes.js';
import { PlayerCombat } from '../../src/combat.js';
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
