// A soldier's weapons, following the 2004 game.
//
// Each weapon keeps its own clip (a share of a full clip), reserve (in clips),
// heat, and kick, and keeps updating while it isn't selected.
//
// - Heat: a shot at heat 1 or more overheats the weapon; it cools at
//   HeatRecoverRate and comes back once it is down to HeatThreshold.
// - Firing: a trigger starts SalvoCount salvos after InitialSalvoDelay,
//   spaced by SalvoDelay, with ShotsPerSalvo rounds in each. A held trigger
//   repeats after ShotDelay (the remainder carries over). TriggerSingle
//   weapons fire once per press, and a press inside the ShotDelay lockout is
//   dropped. Grenades leave the hand InitialSalvoDelay after the press and
//   are busy for ShotDelay more.
// - Ammo: WeaponAmmo is clips, counting the loaded one (0: unlimited); a
//   RoundsPerClip under 1 is a bottomless clip. A reload puts the partial clip
//   back in the reserve and takes up to a full one after ReloadTime. Empty
//   clips reload by themselves; R reloads a clip that isn't full. Switching
//   away resets the reload.
// - Spread, in degrees: each shot is off by S x (rand - rand) in yaw and
//   pitch, where S is the base spread plus the posture's spread plus the kick.
//   The kick grows SpreadPerShot per salvo up to SpreadLimit and recovers at
//   SpreadRecoverRate; for a player it starts at, and never drops below,
//   -SpreadThreshold, so the first shots of a burst are free of it. Every
//   round of a salvo draws its own offsets (once per round), so
//   the blast cannon's five bolts fan out across the spread.
// - A press's draws: a trigger carrying a `seed` (spreadSeed: the soldier,
//   his life, the channel and the press's id) draws its fire cycle's offsets
//   from a generator seeded by it. The authority and the page predicting a
//   player's rounds both know the press, so a predicted salvo flies where the
//   authority's does.
// - Shot patterns: a weapon with ShotPatternPitchYaw (the super battle
//   droid's shrapnel, the bowcaster's charged fans) sends each round of a
//   salvo along the next entry, in degrees (positive pitch down), plus the
//   spread. Each charge level has its own list (the bowcaster's 5-bolt fan
//   is 0, -0.7, +0.7, -1.4 and +1.4 degrees of yaw); one that lists none has
//   no pattern. The entry index belongs to the weapon and wraps at the
//   level's count.
// - Charging (MaxPressedTime above 0): holding the trigger charges, up to
//   MaxPressedTime, and letting go fires with charge = held time /
//   MaxPressedTime; a grenade (the mortar) leaves the hand then. The charge
//   climbs through the class's charge levels (each NextCharge), and the shot
//   uses the level it reached (the mortar's tap is a shell, its full charge
//   a sticky grenade; the arc caster heats up more). Weapons that don't
//   charge fire at charge 1. Switching away drops a charge.
// - Repair weapons (the fusion cutter) tick instead of firing: while the
//   trigger is held, the weapon isn't overheated, and it has a target, a tick
//   on the press and one every ShotDelay after; an interval under way always
//   runs out before the weapon stops. The caller applies each tick.
//
// A soldier carries two channels, primary and secondary, each with a weapon
// selected; switching is instant. Only one channel fires at a time: while the
// secondary is busy, or pressed first, the primary trigger counts as released.
//
// A bot's weapons (src/ai/classic/weapons-ai.js) are these same weapons with
// the AI's settings, all of which leave a player's weapons as they are:
// { player: false } keeps the kick from dropping below zero; `random` (an
// option, or the field) is the generator the spread draws come from, the
// shared AI generator in the 2004 game, and with it a shot draws its pitch
// pair before its yaw pair as the 2004 game does (without it, Math.random
// as before); `spreadScale` multiplies SpreadPerShot, once per salvo (the
// side's curve B; 1 for a player); `yawSpreadOverride`, when not null,
// stands in for YawSpread (a zoomed single-shot weapon's extra AI yaw, in
// degrees); `sniperScale` holds the side's curve S for that. Arsenal.select()
// picks a weapon in its channel directly, for the AI's weapon choice.

const DEG = Math.PI / 180;
const EPSILON = 1e-6; // s; a press landing on the end of the lockout counts

// A press's spread seed (see the top of the file): the same 32 bits wherever
// the soldier's id and life, the channel and the press's id are known.
export function spreadSeed(unit, channel, press) {
  let h = 0x811c9dc5;
  for (const n of [unit?.id ?? 0, unit?.generation ?? 0, channel, press]) {
    h = Math.imul(h ^ (n >>> 0), 0x01000193);
    h ^= h >>> 15;
  }
  return h >>> 0;
}

// Uniform draws in [0, 1) from a seed (mulberry32): integer arithmetic, so
// a world worker, a server under Node and a page draw the same numbers.
export function seededRandom(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

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
    this.def = def;
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
    // AI settings (see the top of the file); the defaults are a player's.
    this.random = random; // () => [0, 1), or null for Math.random
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
    this.lockTargetId = -1;
    this.lockGeneration = -1;
    this.lockElapsed = 0;
    this.lockComplete = false;
  }

  // The thrown thing has left the hand and, with HideOnFire, its model is
  // gone from it until the weapon has reloaded.
  get handEmpty() {
    return this.hidden;
  }

  // Back to the class's own level.
  baseLevel() {
    this.def = this.levels[0];
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

  // Launcher state 4. Range fields on a cannon
  // describe target selection; they do not turn it into a launcher.
  updateLock(dt, target) {
    if (this.def.base !== 'launcher' || this.def.ordnance?.base !== 'missile' || !target?.alive) {
      this.clearLock();
      return;
    }
    if (this.lockTargetId !== target.id || this.lockGeneration !== (target.generation ?? 0)) {
      this.clearLock();
      this.lockTargetId = target.id;
      this.lockGeneration = target.generation ?? 0;
    }
    if (this.busy || this.reloading || this.wait > EPSILON) return;
    const duration = this.def.LockTime > 0 ? this.def.LockTime : this.def.MaxPressedTime || 0;
    this.lockElapsed = Math.min(duration, this.lockElapsed + dt);
    this.lockComplete = duration > 0 && this.lockElapsed >= duration - EPSILON;
  }

  startReload() {
    if (this.bottomless || this.clip >= 1 || this.reloading) return false;
    if (this.clip + this.reserve < this.perShot - 1e-6) return false;
    this.reserve += this.clip;
    this.clip = 0;
    this.reloadLeft = Math.max(this.def.ReloadTime, 0.01);
    this.limit = this.reloadLeft;
    return true;
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

  // One frame. trigger: { held, pressed }; returns the shots fired, each with
  // its yaw and pitch offsets in radians.
  update(dt, selected, trigger, reload, aim) {
    const d = this.def;
    if (this.heat >= 1) this.overheated = true;
    this.heat = Math.max(this.heat - (d.HeatRecoverRate || 0) * dt, 0);
    if (this.overheated && this.heat <= (d.HeatThreshold ?? 1)) this.overheated = false;
    this.kick = Math.max(this.kick - (d.SpreadRecoverRate || 0) * dt, this.floor);
    this.wait -= dt;
    const shots = [];
    if (!selected) {
      this.clearLock();
      this.salvosLeft = 0;
      if (this.wait < 0) this.wait = 0;
      this.firing = false;
      return shots;
    }

    if (this.salvosLeft > 0) {
      this.salvoWait -= dt;
      this.salvoStep(shots, aim);
      return shots;
    }

    if (this.repair) {
      const can = trigger.held && !this.overheated && Boolean(aim.repairTarget);
      if (this.firing) this.timer += dt;
      else if (can) {
        this.firing = true;
        this.timer = 0;
        shots.push({ weapon: this, repair: true });
      }
      while (this.firing && this.timer >= d.ShotDelay) {
        if (can) {
          this.timer -= d.ShotDelay;
          shots.push({ weapon: this, repair: true });
        } else this.firing = false;
      }
      return shots;
    }

    if (this.throwTime >= 0) {
      this.throwTime += dt;
      this.throwStep(shots, aim);
      return shots;
    }

    if (this.reloading) {
      this.reloadLeft -= dt;
      if (this.reloadLeft <= 0) {
        this.reloadLeft = 0;
        const take = Math.min(1, this.reserve);
        this.clip = take;
        this.reserve -= take;
        this.hidden = false;
      }
      if (this.wait < 0) this.wait = 0;
      return shots;
    }
    if (!this.canShoot || (reload && this.clip < 1 && this.reserve >= this.perShot)) {
      this.startReload();
      if (this.wait < 0) this.wait = 0;
      return shots;
    }

    const single = d.TriggerSingle > 0;
    const most = this.levels[0].MaxPressedTime || 0;
    if (most > 0) {
      if (this.charging) {
        if (trigger.held) {
          this.held = Math.min(this.held + dt, most);
          this.climb(most);
          return shots;
        }
        // Let go: fire at the charge reached.
        this.charging = false;
        this.charge = this.held / most;
        this.held = 0;
        if (this.thrown) this.startThrow(shots, aim);
        else {
          this.startBurst(shots, aim);
          this.wait = this.def.ShotDelay;
        }
        return shots;
      }
      if (!this.busy) this.baseLevel();
      if (trigger.held && this.wait <= EPSILON && !this.overheated) {
        this.charging = true;
        this.limit = most;
        this.held = Math.min(dt, most);
        this.climb(most);
      }
      if (this.wait < 0) this.wait = 0;
      return shots;
    }
    if (this.thrown) {
      if (trigger.pressed && this.wait <= EPSILON && !this.deployed) {
        this.charge = 1;
        this.startThrow(shots, aim);
      }
      return shots;
    }
    if (this.overheated) {
      if (this.wait < 0) this.wait = 0;
      return shots;
    }
    if (single) {
      if (trigger.pressed && this.wait <= EPSILON) {
        this.startBurst(shots, aim, trigger.seed);
        // A press that reached the authority late (trigger.late, s: its edge
        // ran after the tick it was stamped for) is locked out from when it
        // was pressed, as the shooter's page counted it. Counting from its
        // arrival dropped his next on-time press as inside the lockout,
        // after his page had drawn the round and played its sound.
        this.wait = d.ShotDelay - Math.min(trigger.late || 0, (d.ShotDelay || 0) * 0.5);
      }
    } else if (trigger.held) {
      if (trigger.pressed && this.wait < 0) this.wait = 0;
      // Only the press's own cycle has its seed; held repeats draw as before.
      let seed = trigger.pressed ? trigger.seed : null;
      while (this.wait <= EPSILON && this.canShoot) {
        this.startBurst(shots, aim, seed);
        seed = null;
        this.wait += Math.max(d.ShotDelay || 0, EPSILON);
        if (this.salvosLeft > 0 || !(d.ShotDelay > 0)) break;
      }
    }
    if (this.wait < 0) this.wait = 0;
    return shots;
  }

  // Cannon firing loop: a trigger starts a sequence; each
  // salvo emits ShotsPerSalvo rounds and spends ammunition per round.
  // seed: the press's spread seed, when the trigger has one.
  startBurst(shots, aim, seed = null) {
    this.salvoRandom = seed == null ? null : seededRandom(seed);
    this.salvosLeft = Math.max(1, Math.floor(this.def.SalvoCount || 1));
    this.salvoWait = Math.max(0, this.def.InitialSalvoDelay || 0);
    this.limit = this.def.ShotDelay || 0;
    this.salvoStep(shots, aim, false);
  }

  salvoStep(shots, aim, cooldown = true) {
    while (this.salvosLeft > 0 && this.salvoWait <= EPSILON) {
      if (!this.canShoot) { this.salvosLeft = 0; break; }
      this.shoot(shots, aim);
      this.salvosLeft--;
      this.salvoWait += Math.max(0, this.def.SalvoDelay || 0);
    }
    if (this.salvosLeft === 0) {
      if (cooldown) this.wait = Math.max(this.wait, this.salvoWait + (this.def.ShotDelay || 0));
      if (this.def.base === 'launcher' && this.def.ClearLockOnFire !== 0) {
        this.lockElapsed = 0;
        this.lockComplete = false;
      }
    }
  }

  // Up through the charge levels the held time has passed.
  climb(most) {
    let level = this.levels.indexOf(this.def);
    while (level + 1 < this.levels.length && this.levels[level + 1].at < this.held / most) level++;
    this.def = this.levels[Math.max(level, 0)];
  }

  // A throw: the grenade leaves the hand InitialSalvoDelay in (at once for
  // none), and the weapon is busy for ShotDelay more.
  startThrow(shots, aim) {
    this.throwTime = 0;
    this.released = false;
    this.wait = this.def.InitialSalvoDelay + this.def.ShotDelay;
    this.limit = this.def.ShotDelay || 0;
    this.throwStep(shots, aim);
  }

  throwStep(shots, aim) {
    const d = this.def;
    if (!this.released && this.throwTime >= d.InitialSalvoDelay) {
      const before = shots.length;
      this.shoot(shots, aim);
      if (this.admitShot && shots.length === before) {
        this.throwTime = -1; this.wait = 0; this.hidden = false;
        this.baseLevel(); return;
      }
      this.released = true;
      this.hidden = d.HideOnFire !== 0;
    }
    if (this.throwTime >= d.InitialSalvoDelay + d.ShotDelay) {
      this.throwTime = -1;
      if (this.canShoot) this.hidden = false;
      this.baseLevel();
    }
  }

  // A salvo: spend the ammo, heat up, pick each shot's offsets, then kick.
  shoot(shots, aim) {
    const d = this.def;
    const count = Math.max(d.ShotsPerSalvo || 1, 1);
    const [yaw, pitch] = this.spread(aim.stick, aim.posture, aim.zoomed);
    // The level's own pattern: data imported before the extractor restarted
    // the list at each level (2026-10-12) holds the earlier levels' entries
    // first, so the last ShotPatternCount are this level's.
    const pattern = d.ShotPatternPitchYaw;
    const entries = pattern?.length ? Math.min(d.ShotPatternCount || pattern.length, pattern.length) : 0;
    const first = entries ? pattern.length - entries : 0;
    const guidance = { targetId: this.lockComplete ? this.lockTargetId : -1, targetGeneration: this.lockGeneration };
    const before = shots.length;
    for (let i = 0; i < count && this.canShoot; i++) {
      const admission = this.admitShot ? this.admitShot(this) : true;
      if (admission === false) break;
      const admitted = admission === true ? null : { launchAdmission: admission };
      if (!this.bottomless) this.clip = Math.max(this.clip - this.perShot, 0);
      this.heat += d.HeatPerShot || 0;
      let [down, aside] = [0, 0];
      if (entries) {
        [down, aside] = pattern[first + (this.pattern % entries)];
        this.pattern = (this.pattern + 1) % entries;
      }
      const r = this.salvoRandom || this.random;
      if (r) {
        // A press's seeded draws or the AI's generator, in the original game's
        // order: pitch, then yaw.
        const tipped = r() - r();
        const turned = r() - r();
        shots.push({ weapon: this, yaw: (aside + yaw * turned) * DEG, pitch: (-down + pitch * tipped) * DEG, charge: this.charge, ...guidance, ...admitted });
        continue;
      }
      shots.push({
        weapon: this,
        yaw: (aside + yaw * (Math.random() - Math.random())) * DEG,
        pitch: (-down + pitch * (Math.random() - Math.random())) * DEG,
        charge: this.charge,
        ...guidance,
        ...admitted,
      });
    }
    if (this.admitShot && shots.length === before) return;
    this.kick = Math.min(this.kick + (d.SpreadPerShot || 0) * this.spreadScale, d.SpreadLimit ?? Infinity);
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

  // The next weapon on a channel. Returns false when there is no other.
  next(channel, direction = 1) {
    const list = this.channels[channel];
    if (list.length < 2) return false;
    const old = this.current(channel);
    if (old?.busy) return false;
    old?.deselect();
    this.selected[channel] = (this.selected[channel] + direction + list.length) % list.length;
    return true;
  }

  // input: { primary: {held, pressed}, secondary: {held, pressed}, reload };
  // aim: { stick, posture, zoomed, blocked }. Returns the shots fired.
  update(dt, input, aim) {
    const primary = this.current(0);
    const secondary = this.current(1);
    const none = { held: false, pressed: false };
    const blocked = aim.blocked;
    const secondBusy = secondary?.busy || (input.secondary.pressed && secondary && !secondary.empty);
    const shots = [];
    for (const weapon of this.weapons) {
      let trigger = none;
      if (!blocked && weapon === secondary) trigger = input.secondary;
      if (!blocked && weapon === primary && !secondBusy) trigger = input.primary;
      const selected = weapon === primary || weapon === secondary;
      shots.push(...weapon.update(dt, selected, trigger, weapon === primary && input.reload, aim));
    }
    return shots;
  }
}
