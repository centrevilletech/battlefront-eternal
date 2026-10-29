import { fireWeapon, quietWeapon, updateWeapon, weaponSlot } from "./weapons.js";

// Every other soldier's sounds, as the 2004 game plays them for every
// soldier alike (docs/formats.md, "Sound"); the player's own are
// PlayerSounds' (player.js), by the same rules. A bot's come from combat.js
// ("actorFire", "actorHurt", "actorDeath"; a world worker sends them on,
// src/runtime/replica.js); another player's online from his rounds'
// launches and his replicated health and death
// (src/runtime/network-entity-game.js).
//
// - Weapons: each of his weapons keeps its voices (weapons.js): FireSound
//   once a fire cycle (FireSoundStop cutting the one before off), the fusion
//   cutter's FireLoopSound while it works, ReloadSound through a reload, the
//   OverheatSound loop while it is too hot. His charge is SceneSounds'.
//   Online only his shots are known (his weapons' states aren't sent).
// - His voice: HurtSound when a hit takes more than 2% of the health it
//   leaves him and he lives, DeathSound when it kills him, on one voice of
//   his that each restarts. Nothing else holds it back: the sound's own
//   PlayInterval (0 for the stock lines) and PlayProbability (0.85 for a
//   wound) are all.
//
// Every one is 3D at his feet and follows him. None starts out of earshot
// (engine.js: a far battle takes no voices); the voice pool keeps the
// loudest 32 of what plays.

const HURT_SHARE = 0.02; // a hit over 2% of the health it leaves hurts
const OTHERS = { charge: false }; // their charges are SceneSounds'

// A soldier's voice: hurt or killed, on the one voice he has, which each
// sound cuts off. Returns the voice it now holds.
export function soldierVoice(audio, voice, className, amount, health, dead, at) {
  if (!dead && !(amount > HURT_SHARE * health)) return voice;
  voice?.stop(0.02);
  return audio.playClass(className, dead ? "DeathSound" : "HurtSound", { ...at, category: "voice" });
}

export class SoldierSounds {
  constructor(audio) {
    this.audio = audio;
    this.weapons = new WeakMap(); // weapon -> its voices
    this.voices = new WeakMap(); // combat -> his voice
    this.places = new WeakMap(); // combat -> { follow } at his feet
  }

  get game() {
    return this.audio.game;
  }

  // At his feet, following him.
  at(combat) {
    let at = this.places.get(combat);
    if (!at) {
      const position = combat.soldier.position;
      this.places.set(combat, (at = { follow: () => position }));
    }
    return at;
  }

  slot(weapon) {
    let s = this.weapons.get(weapon);
    if (!s) this.weapons.set(weapon, (s = weaponSlot()));
    return s;
  }

  // A round (or a salvo) leaves one of his weapons, or a repair tick lands.
  fire(combat, shot) {
    if (!shot?.weapon?.def || !combat?.soldier) return;
    fireWeapon(this.audio, this.slot(shot.weapon), shot, this.at(combat));
  }

  // He is hurt, or killed (amount 0 when only the death is known).
  damage(combat, amount, dead) {
    const className = combat?.owner?.className;
    if (!className || !combat.soldier) return;
    const voice = soldierVoice(this.audio, this.voices.get(combat), className, amount, combat.health, dead, this.at(combat));
    this.voices.set(combat, voice);
  }

  // Each frame, his weapons' states: a reload, an overheat, the fire loop
  // ending. A body dead or off the field has its loops cut.
  update() {
    const own = this.game.combat;
    for (const unit of this.game.units?.all?.() || []) {
      const combat = unit.combat;
      if (!combat?.arsenal || !combat.soldier || combat === own) continue;
      const at = unit.removed || combat.dead ? null : this.at(combat);
      for (const weapon of combat.arsenal.weapons) {
        if (at) updateWeapon(this.audio, this.slot(weapon), weapon, at, OTHERS);
        else {
          const s = this.weapons.get(weapon);
          if (s?.v0) quietWeapon(s);
        }
      }
    }
  }
}
