import { captureWeaponState, decodeWeaponState, applyValidatedWeaponState } from './network-weapon-state.js';

// Capture and restore the authoritative mirror around one Rust mounted replay.
export class MountedWeaponPrediction {
  constructor(session) { this.session = session; this.saved = []; this.tick = 0; }
  restore() {
    for (const entry of this.saved) {
      applyValidatedWeaponState(entry);
      entry.owner.barrel = entry.barrel;
    }
    this.saved.length = 0;
    this.tick = 0;
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
      this.tick = this.session.mounted(unit, this.saved, pending, from, held);
      return this.tick;
    } catch (error) { this.restore(); throw error; }
  }
}
