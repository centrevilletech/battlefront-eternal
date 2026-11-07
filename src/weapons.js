import {bindWeaponPolicyLevels} from "./weapon-policy.js";
// Weapon and loadout resources read by packet publication, HUD and models.
// Rust combat::weapons owns firing, reload, heat, charge and salvo clocks;
// ClientPrediction uses that same core for every own-player controller.
// Constructors resolve immutable class levels once. Read-only spread remains
// the HUD reticle's authored angular extent, independent of controller ticks.

// A weapon's charge in six bits, for what other players are shown of a
// soldier online: 0 while it isn't charging, else the held share of its
// MaxPressedTime in 1..63. applyChargeStep puts it back on the weapon a
// remote soldier holds, the rest of his arsenal not charging.
export const CHARGE_STEPS = 63;
export function chargeStep(weapon) {
  if (!weapon?.charging) return 0;
  const most = weapon.levels?.[0].MaxPressedTime || 0;
  return most > 0 ? Math.max(1, Math.min(CHARGE_STEPS, Math.round((weapon.held / most) * CHARGE_STEPS))) : 0;
}
export function applyChargeStep(arsenal, weapon, step) {
  for (const other of arsenal?.weapons || []) {
    if (other === weapon) continue;
    other.charging = false;
    other.held = 0;
  }
  if (!weapon) return;
  const most = weapon.levels[0].MaxPressedTime || 0;
  weapon.charging = step > 0 && most > 0;
  weapon.held = weapon.charging ? (step / CHARGE_STEPS) * most : 0;
}

export class Weapon {
  constructor(def, { player = true, random = null } = {}) {
    // The class as it is at each charge level.
    this.levels = [def, ...(def.charges || []).map((level) => ({ ...def, ...level }))];
    bindWeaponPolicyLevels(this.levels);
    this.def = def;
    this.level = 0; // which of its levels `def` is (the store's number for it)
    this.name = def.class;
    this.channel = def.channel;
    // Thrown: grenades, dispensers' mines and power-ups, and remote droids.
    this.thrown = def.base === "grenade" || def.base === "dispenser" || def.base === "remote";
    this.admitShot = null; // optional authority admission before ammo/spread
    this.deployed = false; // a remote weapon's droid is out: no other throw
    this.repair = def.base === "repair";
    this.firing = false; // a repair weapon at work
    this.timer = 0; // s into its tick
    this.bank = def.AnimationBank || "rifle";
    this.rounds = def.RoundsPerClip;
    this.bottomless = !(def.RoundsPerClip >= 1);
    this.perShot = this.bottomless ? 0 : 1 / (def.RoundsPerClip + 0.0001);
    this.clip = 1;
    this.reserve = def.ammo > 0 ? def.ammo - 1 : Infinity;
    this.heat = 0;
    this.overheated = false;
    this.floor = player ? -(def.SpreadThreshold || 0) : 0;
    this.kick = this.floor;
    // Authored spread modifiers still feed HUD and native registration.
    this.random = random; // () => [0, 1), or null for the legacy spread stream
    this.spreadScale = 1; // x SpreadPerShot per salvo
    this.sniperScale = 0;
    this.yawSpreadOverride = null; // degrees in place of YawSpread, or null
    this.wait = 0; // seconds until the trigger may fire again; below 0 while catching up
    this.reloadLeft = 0; // seconds of reload left, 0 when not reloading
    this.throwTime = -1; // seconds into a grenade throw, -1 when not throwing
    this.released = false; // the grenade has left the hand
    this.hidden = false; // the thrown thing's model is gone from the hand
    this.charging = false;
    this.held = 0; // seconds the trigger has been held, charging
    this.charge = 1; // the last shot's charge, 0..1
    // s: the weapon timer's limit as the original game keeps it: 1 when new,
    // ShotDelay from a shot, ReloadTime from a reload, MaxPressedTime from a
    // charge's start. A bot's charge holds for a draw of it (weapons-ai.js).
    this.limit = 1;
    this.pattern = 0; // the shot pattern's next entry
    this.salvosLeft = 0;
    this.salvoWait = 0;
    this.salvoRandom = null; // the fire cycle's seeded draws (a press's), or null
    // That generator's state as numbers (the store's): its seed, or null, and
    // how many numbers it has given.
    this.salvoSeed = null;
    this.salvoCount = 0;
    this.lockTargetId = -1;
    this.lockGeneration = -1;
    this.lockElapsed = 0;
    this.lockComplete = false;
    this.melee = null; // a melee weapon's swing (heroes.js prepareMelee)
  }

  // What the store keeps of its authority's admission and its generator:
  // whether it has them (rust/sim/schema/combat.mjs).
  get admits() {
    return Boolean(this.admitShot);
  }

  get hasRandom() {
    return Boolean(this.random);
  }

  // The thrown thing has left the hand and, with HideOnFire, its model is
  // gone from it until the weapon has reloaded.
  get handEmpty() {
    return this.hidden;
  }

  // Back to the class's own level.
  baseLevel() {
    this.def = this.levels[0];
    this.level = 0;
  }

  get reloading() {
    return this.reloadLeft > 0;
  }

  // Busy: in the middle of something the channel waits on.
  get busy() {
    return this.throwTime >= 0 || this.firing || this.salvosLeft > 0;
  }

  // Rounds in the clip, and in reserve, for the HUD.
  get loaded() {
    return this.bottomless ? Infinity : Math.round(this.clip * this.rounds);
  }

  get spare() {
    return this.reserve === Infinity ? Infinity : Math.round(this.reserve * Math.max(this.rounds, 1));
  }

  get canShoot() {
    return this.bottomless || this.clip >= this.perShot - 1e-6;
  }

  // Out of ammo: not even one shot left between the clip and the reserve.
  get empty() {
    return !this.canShoot && this.clip + this.reserve < this.perShot - 1e-6;
  }

  deselect() {
    this.clearLock();
    this.reloadLeft = 0;
    this.charging = false;
    this.held = 0;
    if (!this.busy) this.baseLevel();
  }

  clearLock() {
    this.lockTargetId = -1;
    this.lockGeneration = -1;
    this.lockElapsed = 0;
    this.lockComplete = false;
  }

  // The spread to pick a shot's offsets from, in degrees per axis.
  spread(stick, posture, zoomed) {
    const d = this.def;
    const name = posture === "crouch" ? "Crouch" : posture === "prone" ? "Prone" : "Stand";
    const still = zoomed ? d[`${name}AimSpread`] ?? d[`${name}StillSpread`] : d[`${name}StillSpread`];
    const move = d[`${name}MoveSpread`] ?? 0;
    const pose = (still ?? 0) + (move - (still ?? 0)) * Math.min(stick, 1);
    const kick = Math.max(this.kick, 0);
    const yaw = this.yawSpreadOverride ?? (d.YawSpread || 0);
    return [kick + pose + yaw, kick + pose + (d.PitchSpread || 0)];
  }


}

export class Arsenal {
  constructor(loadout, options) {
    this.weapons = loadout.map((def) => new Weapon(def, options));
    this.channels = [0, 1].map((c) => this.weapons.filter((w) => w.channel === c));
    this.selected = [0, 0];
  }

  current(channel) {
    return this.channels[channel][this.selected[channel]] || null;
  }

  // Select one of the arsenal's weapons in its channel (the AI's weapon
  // choice). Like next(), refused while the channel's weapon is busy.
  select(weapon) {
    const list = this.channels[weapon?.channel] || [];
    const i = list.indexOf(weapon);
    if (i < 0) return false;
    const old = this.current(weapon.channel);
    if (old === weapon) return true;
    if (old?.busy) return false;
    old?.deselect();
    this.selected[weapon.channel] = i;
    return true;
  }

}
