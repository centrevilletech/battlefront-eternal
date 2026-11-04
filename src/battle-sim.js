// Battle rules and end-of-match tallies on the shared simulation instance.
// The module owns the calculation; the bridge preserves the Characters
// and nested vectors/records the AI, scoreboard and network already hold.
import { bridgeFor } from './sim/runtime.js';
import { KIND_BATTLE, KIND_CHARACTER, KIND_COMMAND_POSTS, BrNumberMirror, putBrInput, syncBrNumber } from './sim/gen/index.js';
import { observe } from './battle-stats.js';
import { MobilePost } from './mobile-posts.js';

export function attachBattleSim(instance, battle) {
  const bridge = bridgeFor(instance, { classes: { Post: battle.posts.posts[0]?.constructor, MobilePost } }), x = bridge.x;
  const input = { words: x.battle_rules_words(0), put: putBrInput };
  const output = { Mirror: BrNumberMirror, sync: syncBrNumber };
  const feed = () => {
    bridge.write(KIND_CHARACTER, battle.characters, true);
    bridge.write(KIND_BATTLE, [battle], true);
    bridge.write(KIND_COMMAND_POSTS, [battle.posts], true);
  };
  const sync = () => {
    bridge.sync(KIND_BATTLE, battle);
    bridge.sync(KIND_CHARACTER);
  };
  bridge.register({
    battleQuery({ op, a, b, unit, character }) {
      const out = { number: 0, optionalNumber: null, flag: false, values: [], seen: null };
      if (op === 'relation') out.number = battle.relation(a, b);
      else if (op === 'unitAlive') out.flag = Boolean(unit.alive);
      else if (op === 'living') out.optionalNumber = battle.living(a);
      else if (op === 'owned') out.values = battle.posts.owned(a).map(post => post.desc.valueBleed ?? undefined);
      else if (op === 'statsActive') out.flag = Boolean(battle.statsActive(character));
      else if (op === 'observe') {
        const seen = observe(character);
        if (seen) out.seen = { ...seen, position: seen.position ? [seen.position.x, seen.position.y, seen.position.z] : null };
      }
      return out;
    },
    battleOver({ value }) {
      sync();
      try { battle.onOver(value); } finally { feed(); }
    },
  });

  function run(op, args = {}) {
    feed();
    bridge.callArgs(input, { op, dt: 0, team: 0, count: 1, relation: 0, ...args, victim: args.victim ?? null, killer: args.killer ?? null });
    const out = bridge.callResult(output, x.battle_rules_run());
    sync();
    return out.value;
  }
  const methods = {
    update(dt) { run('update', { dt }); },
    spend(team, count = 1) { run('spend', { team, count }); },
    credit(victim, killer = null, relation = 0) { run('credit', { victim, killer, relation }); },
    headshot(killer) { run('headshot', { killer }); },
    vehicleDestroyed(killer) { run('vehicleDestroyed', { killer }); },
    point(killer) { run('point', { killer }); },
    postDestroyed(killer, relation) { run('postDestroyed', { killer, relation }); },
    score(team) { return run('score', { team }); },
    updateTeam(team, dt) { run('updateTeam', { team, dt }); },
    updateLocals(team) { run('updateLocals', { team }); },
  };
  Object.defineProperties(battle, Object.fromEntries(Object.entries(methods).map(([name, value]) => [name, { value, configurable: true }])));
  return { bridge, source: battle, feed, sync };
}
