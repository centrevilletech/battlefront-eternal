// Page physics fixtures step the same private hull entry points as
// runtime/vehicle-prediction.js. The required world phase is tested separately
// by native goldens; this helper never boards a crew or executes a weapon.
export function stepPrediction(system, dt, v = system.vehicles[0], input = {}) {
  system.time += dt;
  system.predictionAccumulator = (system.predictionAccumulator ?? 0) + dt;
  while (system.predictionAccumulator >= 1 / 60 - 1e-10) {
    v.captureTick();
    if (v.flying) v.tick(1 / 60, input);
    else { v.tick(1 / 120, input); v.tick(1 / 120, input); }
    v.walk(1 / 60);
    v.age += 1 / 60;
    v.render(1, 0);
    system.predictionAccumulator -= 1 / 60;
  }
}
