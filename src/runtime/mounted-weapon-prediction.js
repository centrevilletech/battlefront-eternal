import { copyPredictionWeapon } from './prediction-arsenal.js';

// MountedWeaponPrediction.predict saves the wire checkpoint's exact field set.
// Authority and the drawn weapon keep their identities; only mutable timers,
// ammunition, heat and barrel cursors are borrowed across the native frame.
export class MountedWeaponPrediction {
  constructor(session) {
    // An array-like count retains property storage when the active list is
    // empty; shrinking a JS Array to zero can discard its backing elements.
    this.session = session; this.saved = { length: 0 }; this.pool = []; this.tick = 0;
    this.turretEntry = { state: null, owner: null }; this.turretEntries = [this.turretEntry];
  }
  restore() {
    for (let i = 0; i < this.saved.length; i++) {
      const entry = this.saved[i];
      copyPredictionWeapon(entry.state, entry.weapon);
      entry.owner.barrel = entry.barrel;
      entry.weapon = null; entry.owner = null; this.saved[i] = null;
    }
    this.saved.length = 0; this.tick = 0;
  }
  dispose() { this.restore(); this.turretEntry.state = null; this.turretEntry.owner = null; }
  predict(unit, pending, from, held, hull = null, epoch = 0, correction = null, correctionSchema = 0) {
    this.restore();
    const lease = unit?.seat;
    if (!lease || lease.released || !unit.alive || unit.combat?.dead) return from;
    const vehicle = lease.vehicle;
    let entries, count;
    if (vehicle) { entries = vehicle.seats[lease.index].weapons; count = entries.length; }
    else {
      const turret = lease.turret;
      this.turretEntry.state = turret.weapon; this.turretEntry.owner = turret;
      entries = this.turretEntries; count = Number(Boolean(turret.weapon));
    }
    // Kept Vehicle.fireSeat quirk: authority's takeoff/landing state freezes
    // weapon timers even when private hull replay predicts a flight transition.
    const frozen = vehicle?.flying && vehicle.flightState !== 'Flying';
    if (frozen && !hull) return from;
    if (frozen) count = 0;
    for (let i = 0; i < count; i++) {
      const entry = entries[i], weapon = entry.state, owner = entry.owner || entry;
      let saved = this.pool[i];
      if (!saved) saved = this.pool[i] = { weapon: null, owner: null, barrel: 0,
        advancesBarrel: false, towHeld: false, state: { levels: null, def: null } };
      saved.weapon = weapon; saved.owner = owner; saved.barrel = owner.barrel;
      saved.advancesBarrel = !vehicle || Boolean(entry.muzzles?.length);
      // A tow gun with a cable takes no trigger. Snapshot this fact once,
      // rather than asking its eligibility again for each pending tick.
      saved.towHeld = Boolean(vehicle?.towHeld?.(entry));
      saved.state.levels = weapon.levels; copyPredictionWeapon(weapon, saved.state);
      this.saved[i] = saved; this.saved.length++;
    }
    try {
      this.tick = this.session.mounted(unit, this.saved, pending, from, held, hull, epoch, correction, correctionSchema);
      return this.tick;
    } catch (error) { this.restore(); throw error; }
  }
}
