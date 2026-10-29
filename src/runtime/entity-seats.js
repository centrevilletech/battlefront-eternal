import { SeatLease, unseatBody } from '../seats.js';

// Installs authoritative leases using the same seats and view controllers as simulation.
export class EntitySeats {
  constructor(game) { this.game = game; }
  setLease(turret, vehicle, value, assign, index = 0) {
    const holder = turret ?? vehicle.seats[index];
    const old = holder.lease;
    if (old) old.released = true;
    if (old?.unit?.seat === old) { old.unit.seat = null; if (!value) unseatBody(old.unit); }
    if (!value) { assign(null); return; }
    const [id, human, role, generation] = value;
    const unit = this.game.units.byId(id);
    const lease = new SeatLease({ unit, human: Boolean(human), team: unit?.team ?? 0, vehicle, turret, index, role });
    lease.generation = generation;
    if (!unit) { assign(null); return; }
    assign(lease);
    // Seated, as the world seated him (seats.js seatBody). A switch to
    // another seat arrives as his old seat emptied, which unseated the body
    // above, then the new one taken; the world's seated flags never changed,
    // so no state brings them back.
    unit.seat = lease;
    unit.targetable = false;
    if (unit.combat) unit.combat.mounted = true;
    if (human && vehicle) this.game.driver.lease = lease;
  }

  setDriver(index) {
    const g = this.game, driver = g.driver;
    if (!driver) return;
    const v = index >= 0 ? g.vehicles.vehicles[index] : null;
    if (driver.vehicle === v && (v || !driver.lease && !g.playerUnit?.seat?.vehicle)) return;
    if (!v && driver.release) { driver.release(); return; }
    if (driver.vehicle) {
      driver.vehicle.driver = null;
      if (driver.hiddenHull) driver.hideHull?.(null);
    }
    driver.vehicle = v;
    driver.cameraSet = false;
    if (v) { v.driver = driver.soldier; v.humanDriver = driver; }
    else driver.lease = null;
  }

  setGunner(index) {
    const g = this.game, gunner = g.gunner;
    if (!gunner) return;
    gunner.turret = index >= 0 ? g.live.buildings[index] : null;
  }

}
