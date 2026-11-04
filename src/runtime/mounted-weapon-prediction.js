import { CommandControls } from './commands.js';
import { replayTicks } from './reconcile.js';
import { captureWeaponState, decodeWeaponState, applyValidatedWeaponState } from './network-weapon-state.js';

const aim = Object.freeze({ stick: 0, posture: 'stand', zoomed: false });
const cosmeticRandom = () => .5;

// Replay eligibility through the shared Weapon rules. Returned shots are never
// emitted: damage, projectiles, sounds, lock acquisition and admission stay on
// the authority. Restore before applying a correction or changing the lease.
export class MountedWeaponPrediction {
  constructor({ native = null } = {}) { this.saved = []; this.controls = new CommandControls(); this.tick = 0; this.native = native; this.nativeSession = null; }
  restore() {
    for (const entry of this.saved) {
      applyValidatedWeaponState(entry);
      entry.owner.barrel = entry.barrel;
    }
    this.saved.length = 0;
    this.tick = 0;
    this.nativeSession?.dispose(); this.nativeSession = null;
  }
  dispose() { this.restore(); }
  predict(unit, pending, from, held, view) {
    this.restore();
    const lease = unit?.seat;
    if (!lease || lease.released || !unit.alive || unit.combat?.dead) return from;
    const vehicle = lease.vehicle;
    const entries = vehicle ? vehicle.seats[lease.index]?.weapons || []
      : lease.turret?.weapon ? [{ state: lease.turret.weapon, owner: lease.turret }] : [];
    // Shared Vehicle.fireSeat freezes weapons during takeoff/landing.
    if (vehicle?.flying && vehicle.flightState !== 'Flying') return from;
    for (const entry of entries) {
      const weapon = entry.state, owner = entry.owner || entry;
      // A tow gun whose cable holds takes no trigger (Vehicle.towHeld).
      this.saved.push({ weapon, owner, barrel: owner.barrel, advancesBarrel: !vehicle || Boolean(entry.muzzles?.length), towHeld: Boolean(vehicle?.towHeld?.(entry)), state: decodeWeaponState(captureWeaponState(weapon)) });
    }
    try {
      if (this.native) this.nativeSession = this.native.createMounted(this.saved);
      this.tick = replayTicks(pending, from, unit.generation, held, (_tick, command) => {
        this.controls.down.clear();
        for (const key of command.down) this.controls.down.add(key);
        for (let channel = 0; channel < this.saved.length; channel++) {
          const { weapon, owner, advancesBarrel, towHeld } = this.saved[channel];
          const action = channel ? 'fireSecondary' : 'firePrimary';
          const pressed = !towHeld && command.actions.includes(action);
          const random = weapon.random, admission = weapon.admitShot;
          try {
            weapon.random = cosmeticRandom;
            weapon.admitShot = null;
            const held = !towHeld && (pressed || this.controls.held(action)), reload = command.actions.includes('reload');
            if (this.nativeSession) this.nativeSession.mounted(this.saved[channel], pressed, held, reload);
            else {
              const shots = weapon.update(1 / 60, true, { pressed, held }, reload, aim);
              if (advancesBarrel) owner.barrel += shots.length;
            }
          } finally { weapon.random = random; weapon.admitShot = admission; }
        }
      }, view);
      return this.tick;
    } catch (error) { this.restore(); throw error; }
  }
}
