// Object publication and allocation ports for the native connection driver.
// A step context retains exactly the player, command and body references
// selected by that invocation; a nested step gets its own context.
export function createNetworkStepHost({ game, players, hitHistory, roster, remoteWorld, stopInput, spawn, traceHuman, controls, updateViews }) {
  return {
    state({ p, u, choice, choiceBody }) {
      const kind = choiceBody ? choice.kind : null;
      return { disconnected: Boolean(p?.disconnectedAt), disconnectedAt: p?.disconnectedAt ?? 0,
        spectator: Boolean(p?.spectator), stale: Boolean(p?.inputsStale), lastInputAt: p?.lastInputAt ?? 0,
        separateActions: Boolean(p?.separateActions), alive: Boolean(u?.alive),
        removed: Boolean(u?.removed), deadTime: u ? u.deadTime : 0, autoRespawn: Boolean(p?.autoRespawn),
        spawnedOnce: Boolean(p?.spawn.spawnedOnce), teamSelect: p?.teamSelect !== false,
        choice: ['suicide', 'team', 'spawn'].includes(kind) ? kind : 'other',
        choiceTeam: choiceBody && typeof choice.team === 'number' ? choice.team : NaN };
    },
    call(op, context, value) {
      const { p, u, command, choice } = context;
      switch (op) {
        case 'capture': hitHistory.capture(game.worldClock.tick); break;
        case 'players':
          context.players = players.values()[Symbol.iterator]();
          context.playerNext = context.players.next;
          break;
        case 'nextPlayer': {
          context.playerBody = false;
          const next = Reflect.apply(context.playerNext, context.players, []);
          if (next.done) { context.players = null; context.p = context.u = null; return false; }
          context.p = next.value; context.u = null; context.playerBody = true;
          return true;
        }
        case 'ended': p.spawn.ended = game.battle.gameOver; break;
        case 'release': p.character.unit = p.unit; roster.release(p.character); break;
        case 'retire': remoteWorld?.retire(p.id); break;
        case 'delete': players.delete(p.id); break;
        case 'stop': p.inputsStale = true; stopInput(p); break;
        case 'command': context.u = p.unit; context.command = p.mailbox.take(value, context.u.generation); break;
        case 'actions': {
          const discrete = p.actions.take(value, u.generation);
          command.actions = discrete.actions; command.human = discrete.human;
          break;
        }
        case 'accept': p.controls.accept(command); break;
        case 'choices':
          context.choices = (command.human || [])[Symbol.iterator]();
          context.choiceNext = context.choices.next;
          break;
        case 'nextChoice': {
          context.choiceBody = false;
          const next = Reflect.apply(context.choiceNext, context.choices, []);
          if (next.done) { context.choices = context.choice = null; return false; }
          context.choice = next.value; context.spawned = null; context.choiceBody = true;
          return true;
        }
        case 'suicide': u.combat.damage(Infinity); break;
        case 'switchTeam': return roster.switchTeam(p.character, choice.team);
        case 'team': p.team = choice.team; u.team = u.combat.team = p.team; break;
        case 'currentAlive': return p.unit.alive;
        case 'spawn': context.spawned = spawn(p, choice.className, choice.post); break;
        case 'trace': traceHuman({ stage: 'decision', tick: value, choice, spawned: context.spawned, team: p.team,
          unit: { id: p.unit.id, generation: p.unit.generation, className: p.unit.className, alive: p.unit.alive, deadTime: p.unit.deadTime }, phase: p.spawn.phase }); break;
        case 'unit': context.u = p.unit; break;
        case 'deadTime':
          u.f3.deadTime(u,value);
          break;
        case 'autoSpawn': spawn(p, p.className, p.spawn.post); break;
        case 'deadPhase': p.spawn.phase = ['unspawned', 'awaiting', 'dying'][value]; p.spawn.deadTime = u.deadTime; break;
        case 'controls': controls(p, u, command, value); break;
        case 'views': updateViews(); break;
        case 'step': game.step({ epoch: 1, sequence: value, tick: value, generation: game.playerUnit.generation, down: [], actions: [], look: [] }); break;
        default: throw Error(`unknown network phase ${op}`);
      }
      return false;
    },
  };
}
