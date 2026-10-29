import { InfantryPrediction } from './infantry-prediction.js';
import { infantryFireBlocked } from '../combat-input.js';

const quiet = () => {};

// Shared movement and recoil, with the same post-movement shield edge as
// PlayerCombat.update. Never replay weapons, damage, or death presentation.
export class DroidekaPrediction extends InfantryPrediction {
  constructor(soldier, combat, kick = combat.kick) {
    super(soldier, combat, kick);
    this.shieldEquipped = combat.arsenal.current(1)?.def.base === 'shield';
    this.gunUnderwater = combat.gunUnderwater;
  }
  step(input, dt, shieldPressed = false) {
    const s = this.soldier;
    const blast = s.onDeathBlast, exploded = s.onExploded;
    s.onDeathBlast = s.onExploded = quiet;
    try { s.tick(input, dt); }
    finally { s.onDeathBlast = blast; s.onExploded = exploded; }
    if (this.shieldEquipped && shieldPressed &&
        !infantryFireBlocked(s, this.gunUnderwater, Math.max(Math.abs(input.forward || 0), Math.abs(input.right || 0)))) {
      s.toggleShield();
    }
    this.updateKick(dt);
  }
}
