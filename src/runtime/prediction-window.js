// Page-owned input retention and visual identity reset. Native prediction
// owns continuation eligibility and checkpoints; this file never replays ticks.
// Initial network assignment can replace the mission's local player ID while
// reusing the exact Soldier object and generation number. It is a different
// prediction identity, not a same-life correction to the provisional body.
export function resetPredictionIdentity(client) {
  client.smoother.reset(); client.vehicleSmoother.reset();
  client.predictedTick = 0; client.vehiclePrediction = null;
  client.held = []; client.heldGeneration = 0;
}

export function retainPredictionCommands(commands, tick, generation, epoch, limit = 120) {
  return commands.filter(command => command.generation === generation && command.epoch === epoch && command.tick > tick - limit).slice(-limit);
}
