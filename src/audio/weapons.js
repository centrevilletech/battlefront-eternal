import { soundRef } from "./data.js";
import { classNumber, prop } from "./props.js";

// A weapon's voices, as the 2004 game keeps them for every soldier's
// weapons alike (docs/formats.md, "Sound"): the player's (player.js) and
// everyone else's (soldiers.js). Three each, all following the owner's
// origin: one for the state sound (charge, overheat, reload, the fusion
// cutter's loop), one for firing and the empty click, one for movement.
// Starting a sound on a voice that is still playing lets the old one
// finish; only these stop theirs: a reload when it completes, the charge on
// release, the overheat when it ends, the fire loop when the firing stops,
// and a FireSound marked FireSoundStop when the next starts.

const NONE = Object.freeze({});

export function weaponSlot() {
  return { state: { reloading: false, overheated: false, charging: false }, v0: null, v1: null, v2: null };
}

// A sound's own Pitch.
export function soundPitch(audio, value) {
  const ref = soundRef(Array.isArray(value) ? value[0] : value);
  const sound = ref && audio.data.sound(ref.name);
  return sound ? prop(sound, "Pitch") : 1;
}

// The state voice, from the weapon's state against the last look. at: the
// owner's place ({ follow }). charge: whether the charge sounds here too
// (other soldiers' charges are SceneSounds').
export function updateWeapon(audio, s, weapon, at, { charge = true } = {}) {
  const was = s.state;
  const cls = weapon.def.class;
  const sounds = audio.data.classSounds(cls) || NONE;
  // Reload: starts on the state voice, cut off when the reload is done.
  if (weapon.reloading && !was.reloading) s.v0 = audio.playClass(cls, "ReloadSound", at);
  if (!weapon.reloading && was.reloading && s.v0?.kind === "reload") s.v0.stop(0.02);
  if (weapon.reloading && s.v0) s.v0.kind = "reload";
  // Overheat: a loop while overheated, rising in pitch as it cools.
  if (weapon.overheated && !was.overheated) {
    s.v0 = audio.playClass(cls, "OverheatSound", { ...at, loop: true });
    if (s.v0) s.v0.kind = "overheat";
  }
  if (s.v0?.kind === "overheat") {
    if (!weapon.overheated) {
      s.v0.stop(0.05);
      s.v0 = null;
    } else {
      const base = soundPitch(audio, sounds.OverheatSound);
      const low = classNumber(sounds.OverheatSoundPitch, 1);
      const t = Math.min((weapon.def.HeatThreshold ?? 1) / Math.max(weapon.heat, 1e-6), 1);
      s.v0.setAbsolutePitch?.(base * (low + (1 - low) * t));
    }
  }
  // Charging: the charge sound's pitch follows the charge, held between
  // 0.2 and 0.4 (the original game's clamp).
  if (charge) {
    if (weapon.charging && !was.charging) {
      const base = soundPitch(audio, sounds.ChargeSound);
      const low = classNumber(sounds.ChargeSoundPitch, 1);
      s.v0 = audio.playClass(cls, "ChargeSound", at);
      if (s.v0) {
        s.v0.kind = "charge";
        s.v0.setAbsolutePitch?.(Math.min(Math.max(base * low, 0.2), 4));
      }
    } else if (weapon.charging && s.v0?.kind === "charge") {
      const base = soundPitch(audio, sounds.ChargeSound);
      const low = classNumber(sounds.ChargeSoundPitch, 1);
      const f = weapon.held / (weapon.levels[0].MaxPressedTime || 1);
      s.v0.setAbsolutePitch?.(Math.min(Math.max(base * (low + (1 - low) * f), 0.2), 0.4));
    }
    if (!weapon.charging && was.charging && s.v0?.kind === "charge") {
      s.v0.stop(0.02);
      s.v0 = null;
    }
  }
  // The fusion cutter's loop: restarted each tick, stopped when it stops.
  if (!weapon.firing && s.v0?.kind === "loop") {
    s.v0.stop(0.05);
    s.v0 = null;
  }
  was.reloading = weapon.reloading;
  was.overheated = weapon.overheated;
  was.charging = weapon.charging;
}

// A round (or a salvo) leaves the weapon, or a repair tick lands: once a
// frame per weapon (a salvo's rounds go together).
export function fireWeapon(audio, s, shot, at) {
  const weapon = shot.weapon;
  const now = audio.engine.time;
  if (s.lastFire === now) return;
  s.lastFire = now;
  const cls = weapon.def.class;
  const sounds = audio.data.classSounds(cls) || {};
  if (shot.repair) {
    // FireLoopSound, restarted each cycle.
    if (sounds.FireLoopSound) {
      s.v0?.stop(0.02);
      s.v0 = audio.playClass(cls, "FireLoopSound", { ...at, loop: true });
      if (s.v0) s.v0.kind = "loop";
    }
    return;
  }
  if (Number(sounds.FireSoundStop) && s.v1) s.v1.stop(0.01);
  s.v1 = audio.playClass(cls, "FireSound", at);
}

// A weapon's loops end with the life that held it (a death, a body taken
// off the field, another body for the player).
export function quietWeapon(s) {
  if (s.v0?.kind !== "loop" && s.v0?.kind !== "overheat") return;
  s.v0.stop(0.05);
  s.v0 = null;
}
