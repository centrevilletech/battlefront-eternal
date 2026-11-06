// Explicit test wiring. The normal session contains neither these tallies nor
// branches to maintain them. This function is also serializable into a page.
export function instrumentClientPrediction(session) {
  const inPage = session === undefined;
  if (inPage) session = globalThis.__bf.predictionSession;
  if (session.counters) return inPage ? undefined : session;
  const counters = { frames: 0, calls: 0, ticks: 0, weaponTicks: 0, wordsIn: 0, wordsOut: 0 };
  const transfers = { headerIn: 0, actorIn: 0, correctionIn: 0, weaponsIn: 0, flagsIn: 0,
    commandsIn: 0, lookIn: 0, flightIn: 0, hullIn: 0, hullFactsIn: 0, crowdIn: 0,
    headerOut: 0, poseOut: 0, beforeOut: 0, hullOut: 0, correctionOut: 0, weaponsOut: 0, shotsOut: 0 };
  const predict = session.predict, mounted = session.mounted;
  function record() {
    const f = session.frame, r = session.result, sizes = session.layout, input = session.input;
    const correctionIn = Math.ceil(f.correctionBytes / sizes.correctionBytesPerWord);
    const commandsIn = f.commandsCount * sizes.command;
    const crowdIn = f.crowdCount * sizes.crowd + f.mountsCount * sizes.mount;
    const wordsOut = r.shotsAt + r.shotsCount * (sizes.shot + r.actorWords);
    let lookIn = 0, flightIn = 0, last = f.authorityTick, mine = false;
    // Counts come from generated offsets, including optional flyer command
    // tails. A retained infantry command has no flight tail to transfer.
    for (let i = 0; i < f.commandsCount; i++) {
      const at = f.commandsAt + i * sizes.command, offsets = sizes.commandOffsets;
      lookIn += input[at + offsets.lookCount] * sizes.look;
      if (input[at + offsets.flightAt]) flightIn += sizes.flightCommand;
      if (input[at + offsets.generation] === f.generation) { mine = true; last = Math.max(last, input[at + offsets.tick]); }
    }
    if (mine && last === f.authorityTick) last++;
    // Mounted timers replay from authority even when the hull continues.
    // Keep their tick denominator separate from newly simulated hull ticks.
    const weaponTicks = f.mode === 'actor' ? r.ticks : mine ? last - f.authorityTick : 0;
    const hullIn = f.mode === 'hull' ? session.inputWords - sizes.frame - f.actorWords - correctionIn - f.weaponsWords - f.flagsCount - commandsIn - crowdIn - lookIn - flightIn : 0;
    const h = session.hullFrame;
    const facts = f.mode === 'hull' ? h.membersCount * sizes.hullMember + h.teamsCount + h.teamsCount * h.teamsCount : 0;
    counters.frames++; counters.calls++; counters.ticks += r.ticks; counters.weaponTicks += weaponTicks;
    counters.wordsIn += session.inputWords; counters.wordsOut += wordsOut;
    transfers.headerIn += sizes.frame; transfers.actorIn += f.actorWords;
    transfers.correctionIn += correctionIn; transfers.weaponsIn += f.weaponsWords;
    transfers.flagsIn += f.flagsCount; transfers.commandsIn += commandsIn; transfers.crowdIn += crowdIn;
    transfers.lookIn += lookIn; transfers.flightIn += flightIn;
    transfers.hullIn += hullIn - facts; transfers.hullFactsIn += facts;
    transfers.headerOut += sizes.result; transfers.poseOut += r.actorWords;
    transfers.beforeOut += f.mode === 'actor' && r.ticks ? r.actorWords : 0;
    transfers.hullOut += r.hullWords;
    transfers.correctionOut += Math.ceil(r.correctionBytes / sizes.correctionBytesPerWord);
    transfers.weaponsOut += r.weaponsWords; transfers.shotsOut += r.shotsCount * (sizes.shot + r.actorWords);
  }
  session.predict = function (...args) { const value = predict.apply(this, args); record(); return value; };
  session.mounted = function (...args) { const value = mounted.apply(this, args); record(); return value; };
  session.counters = counters; session.transfers = transfers;
  return inPage ? undefined : session;
}
