// Invented mounted weapon definitions shared by native phase fixtures.
export const weaponDefinition = (name, channel = 0, extra = {}) => ({
  class: name, base: 'cannon', channel, RoundsPerClip: 8, ammo: 5, ShotDelay: 0.1, ReloadTime: 0.23,
  InitialSalvoDelay: 0, SalvoCount: 1, ShotsPerSalvo: 1, SalvoDelay: 0,
  YawSpread: 1.7, PitchSpread: 1.3, StandStillSpread: 0.4, CrouchStillSpread: 0.2, ProneStillSpread: 0.1,
  StandMoveSpread: 0.8, CrouchMoveSpread: 0.6, ProneMoveSpread: 0.5, StandAimSpread: 0.1, CrouchAimSpread: 0.05, ProneAimSpread: 0,
  SpreadPerShot: 0.2, SpreadThreshold: 0.3, SpreadRecoverRate: 0.4, SpreadLimit: 2.1,
  HeatPerShot: 0.07, HeatRecoverRate: 0.12, HeatThreshold: 0.3,
  ordnance: { base: 'bolt', Velocity: 100, LifeSpan: 1 }, ...extra,
});
