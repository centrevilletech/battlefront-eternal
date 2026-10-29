// Keep replay work bounded without making outbound input wait for snapshots.
// A locally simulated checkpoint continues through a downstream blackout. A
// fresh authoritative state replaces it; a snapshot older than the retained
// replay window cannot rewind it using an incomplete command history.
export class PredictionWindow {
  constructor(limit = 120) { this.limit = limit; this.reset(); }
  reset() { this.checkpoint = null; }
  select({ source, identity, epoch, generation, authorityTick, pending, held }) {
    const c = this.checkpoint;
    const sameLife = c && c.source === source && c.identity === identity && c.epoch === epoch && c.generation === generation;
    const checkpoint = sameLife && (authorityTick === c.authorityTick || authorityTick < c.tick - this.limit) ? c : null;
    if (!checkpoint) this.reset();
    return {
      checkpoint, from: checkpoint?.tick ?? authorityTick,
      held: checkpoint?.held ?? held,
      commands: checkpoint ? pending.filter(command => command.sequence > checkpoint.sequence) : pending,
    };
  }
  save({ source, identity, epoch, generation, authorityTick, tick, commands, held, state }) {
    const last = commands.at(-1);
    this.checkpoint = { source, identity, epoch, generation, authorityTick, tick, state,
      sequence: last?.sequence ?? this.checkpoint?.sequence ?? 0,
      held: last?.down ?? held };
  }
}

// Initial network assignment can replace the mission's local player ID while
// reusing the exact Soldier object and generation number. It is a different
// prediction identity, not a same-life correction to the provisional body.
export function resetPredictionIdentity(client) {
  client.predictionWindow.reset(); client.vehicleWindow.reset();
  client.smoother.reset(); client.vehicleSmoother.reset();
  client.predictedTick = 0; client.vehiclePrediction = null;
  client.held = []; client.heldGeneration = 0;
}

export function retainPredictionCommands(commands, tick, generation, epoch, limit = 120) {
  return commands.filter(command => command.generation === generation && command.epoch === epoch && command.tick > tick - limit).slice(-limit);
}
