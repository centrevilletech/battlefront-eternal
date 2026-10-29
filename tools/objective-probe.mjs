// Headless check of the destructible command posts on one map:
// node tools/objective-probe.mjs <map> [ticks] [--destroy]
// Prints their health every 30 s, how many bots have each as their sight
// candidate, and who damaged them (docs/objectives.md).
import { headlessGame, idleCommand } from './headless-world.mjs';
const [map = 'hot1', ticksArg = '600', ...flags] = process.argv.slice(2);
const side = { hot1: "imp", geo1: "rep", end1: "imp" }[map] || "rep";
const seed = process.env.SEED || 1;
const game = await headlessGame(`map=${map}&side=${side}&ai=classic&seed=${seed}`);
const objectives = game.live?.objectives || [];
const report = () => objectives.map((b) => ({ post: b.desc.post, team: b.team, health: Math.round(b.health), state: b.state, active: b.post?.active, registered: Boolean(b.aiTarget?.id) }));
console.log('objectives', JSON.stringify(report()));
// Who hurts them, by attacker class.
const tally = new Map();
for (const b of objectives) {
  const damage = b.damage.bind(b);
  b.damage = (amount, attacker) => {
    const who = attacker === 'player' ? 'player' : attacker?.className || attacker?.vehicle?.name || attacker?.name || String(attacker);
    const key = `${b.desc.post}<-${who}`;
    tally.set(key, (tally.get(key) || 0) + amount);
    return damage(amount, attacker);
  };
}
const watching = () => {
  const controllers = game.battle.characters.map((c) => c.controller).filter(Boolean);
  // Per objective: bots sighting it, bots sent to it, and how near those are.
  return objectives.map((b) => {
    const sent = game.battle.characters.filter((c) => c.objective === b.post && c.unit?.alive);
    const near = sent.map((c) => Math.round(c.unit.position.distanceTo(b.position))).sort((x, y) => x - y);
    return [b.desc.post, [...controllers].filter((c) => c.perception?.candidate?.unit === b.aiTarget).length, sent.length, near.slice(0, 6)];
  });
};
const step = (n) => { for (let i = 0; i < n; i++) game.step(idleCommand(game)); };
if (flags.includes('--destroy')) {
  // Bots in by then; three of them put inside the kill region, if it has
  // one, before the first objective is destroyed.
  step(900);
  const target = objectives[0];
  const region = target.post?.desc.killRegion;
  const placed = [];
  if (region) {
    const [x, y, z] = region.matrix.slice(12, 15);
    for (const unit of [...game.units.all()].filter((u) => u.soldier && u.alive && !u.seat && u !== game.playerUnit).slice(0, 3)) {
      unit.soldier.spawn(x + placed.length * 2, y + 2, z, 0);
      placed.push(unit);
    }
  }
  target.damage(target.health + 1, null);
  step(2);
  console.log('destroyed', JSON.stringify(report()), 'in the kill region', placed.length, 'killed', placed.filter((u) => !u.alive).length,
    'score', game.battle.score(target.post.team));
}
for (let done = 0; done < Number(ticksArg); done += 1800) { const n = Math.min(1800, Number(ticksArg) - done); step(n); console.log('t', (done + n) / 60, JSON.stringify(report().map((r) => r.health)), JSON.stringify(watching())); }
console.log('damage by attacker', JSON.stringify(Object.fromEntries([...tally].map(([k, v]) => [k, Math.round(v)]))));
console.log('after', ticksArg, 'ticks', JSON.stringify(report()), 'tickets', JSON.stringify(game.battle.reinforcements));
process.exit(0);
