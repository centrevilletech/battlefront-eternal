// End-of-run coverage is separate from update freshness. A received old life
// must not conceal a current life that never reached a particular viewer.
const ages = values => {
  values.sort((a, b) => a - b);
  const at = fraction => values[Math.min(values.length - 1, Math.floor(values.length * fraction))] ?? null;
  return {count: values.length, p50: at(.5), p95: at(.95), max: values.at(-1) ?? null};
};
export function actorCoverage(roster, observations) {
  if (!roster || !Array.isArray(roster.actors)) return {available: false};
  const identity = actor => `${actor.kind}:${actor.id}:${actor.generation}`;
  const allAges = [];
  const clients = observations.map(client => {
    const seen = new Map(client.observedActorLives);
    const expected = roster.actors.filter(actor => !(actor.kind === 'soldier' && actor.id === client.ownId));
    const missing = expected.filter(actor => !seen.has(identity(actor)));
    const receivedAges = expected.map(actor => seen.get(identity(actor))).filter(Number.isFinite);
    allAges.push(...receivedAges);
    return {index: client.index, expected: expected.length, received: expected.length - missing.length, missing,
      receivedUpdateWallAgeMs: ages(receivedAges)};
  });
  return {
    available: true,
    referenceTick: roster.tick,
    meaning: 'Authoritative live actor census at end-of-run metrics collection, compared with exact actor generations observed during this connection. Own soldier excluded. Receipt collection follows the census; this is a coverage checkpoint, not continuous coverage proof. Update ages measure browser wall time since a newer sample arrived for these lives, not capture-to-delivery latency. Missing lives are counted separately, never as zero age.',
    receivedUpdateWallAgeMs: ages(allAges),
    clients,
  };
}
