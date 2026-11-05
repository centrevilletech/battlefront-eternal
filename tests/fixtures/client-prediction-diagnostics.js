// Explicit test wiring. The normal session contains neither these tallies nor
// branches to maintain them. This function is also serializable into a page.
export function instrumentClientPrediction(session) {
  const inPage = session === undefined;
  if (inPage) session = globalThis.__bf.predictionSession;
  if (session.counters) return inPage ? undefined : session;
  const counters = { frames: 0, calls: 0, ticks: 0, wordsIn: 0, wordsOut: 0 };
  const transfers = { headerIn: 0, actorIn: 0, correctionIn: 0, weaponsIn: 0, flagsIn: 0,
    commandsIn: 0, lookIn: 0, crowdIn: 0, headerOut: 0, poseOut: 0, beforeOut: 0,
    correctionOut: 0, weaponsOut: 0, shotsOut: 0 };
  const predict = session.predict, mounted = session.mounted;
  function record() {
    const f = session.frame, r = session.result, sizes = session.layout;
    const correctionIn = Math.ceil(f.correctionBytes / sizes.correctionBytesPerWord);
    const commandsIn = f.commandsCount * sizes.command;
    const crowdIn = f.crowdCount * sizes.crowd + f.mountsCount * sizes.mount;
    const wordsOut = r.shotsAt + r.shotsCount * (sizes.shot + r.actorWords);
    counters.frames++; counters.calls++; counters.ticks += r.ticks;
    counters.wordsIn += session.inputWords; counters.wordsOut += wordsOut;
    transfers.headerIn += sizes.frame; transfers.actorIn += f.actorWords;
    transfers.correctionIn += correctionIn; transfers.weaponsIn += f.weaponsWords;
    transfers.flagsIn += f.flagsCount; transfers.commandsIn += commandsIn; transfers.crowdIn += crowdIn;
    transfers.lookIn += session.inputWords - sizes.frame - f.actorWords - correctionIn - f.weaponsWords - f.flagsCount - commandsIn - crowdIn;
    transfers.headerOut += f.mode === 'mounted' ? r.weaponsAt : r.actorAt;
    transfers.poseOut += r.actorWords; transfers.beforeOut += r.ticks ? r.actorWords : 0;
    transfers.correctionOut += Math.ceil(r.correctionBytes / sizes.correctionBytesPerWord);
    transfers.weaponsOut += r.weaponsWords; transfers.shotsOut += r.shotsCount * (sizes.shot + r.actorWords);
  }
  session.predict = function (...args) { const value = predict.apply(this, args); record(); return value; };
  session.mounted = function (...args) { const value = mounted.apply(this, args); record(); return value; };
  session.counters = counters; session.transfers = transfers;
  return inPage ? undefined : session;
}
