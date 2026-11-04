// Recoil moves aim in the authoritative combat tick, after Soldier.tick. Replay
// the same method against private recoil history: never step weapons, consume
// ammunition, emit shots, or mutate the authoritative combat object.
export class InfantryPrediction {
  constructor(soldier, combat, kick = combat.kick, { native = null } = {}) {
    this.soldier = soldier;
    this.kick = { ...kick };
    this.updateKick = combat.updateKick;
    this.nativeSession = native?.bind(soldier) ?? null;
  }
  step(input, dt) {
    if (this.nativeSession?.step(this, input, dt)) return;
    this.soldier.tick(input, dt);
    this.updateKick(dt);
  }
  dispose() { this.nativeSession?.dispose(); }
}
