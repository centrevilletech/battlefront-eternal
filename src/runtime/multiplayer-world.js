import { Character } from '../battle.js';
import { InputMailbox } from './transport.js';
import { humanCommands } from './commands.js';
import { F4Connections } from '../sim/wasm/f4-connections.js';
import { createMultiplayerRoster } from './multiplayer-roster.js';
import { PlayersBridge, LaneView } from './players-bridge.js';

// A dedicated match owns one simulation and one independently stamped input
// mailbox per authenticated connection. The connections' queues, their
// tick (grace, stale input, choices, controls, seats) and the facts the AI
// and unit phases read of them are native (rust/sim/src/players); this host
// admits connections and packets, and mirrors what a tick changed.
export function createMultiplayerWorld(game, { maxPlayers = 64, armySize = 32 } = {}) {
  // No single simulation camera scopes crowd separation online: every pair
  // is separated everywhere, and clients predict the same push.
  if (game.separationScope) game.separationScope.global = true;
  // AI scheduling uses the authoritative players' views (written natively
  // by the players' phase), never a client camera: the host supplies none.
  if (game.ai?.worldData) { game.ai.worldData.networkViews = true; game.ai.worldData.cameras = []; }
  const players = new Map(), byHandle = new Map();
  const inputMailbox = () => new InputMailbox(1, 256, game.simBridge);
  const native = new PlayersBridge(game.simBridge);
  game.serverWorld = true;
  const remoteWorld = game.createNetworkRemotes?.(maxPlayers);
  game.ordnance.hitHistory = game.unitsSim.history;
  // The dedicated world's viewpoint remains a camera facade, never an extra
  // player. Both its body flags and spawn records are cleared natively.
  game.unitsSim.parkViewpoint(game.playerUnit);
  game.unitsSim.nativePlayers = true;
  game.playerUnit.undraw();
  const connections=new F4Connections(game.tailOwner);
  const roster = createMultiplayerRoster(game, armySize);
  // A network player's weapons draw what no press seeds (a held trigger's
  // repeats, a charge's release, a throw, a melee swing) from the world's
  // network-weapons stream; a press's own fire cycle keeps its spread seed.
  const networkWeapons = game.randomStream?.('network-weapons') ?? null;
  // A squad command's notice under the player's body (the page's HUD).
  const squadLabels = ['moveout', 'hold', 'follow', 'atease'];
  game.unitsSim.eventHandlers.playerSquad = e => game.simEvents.emit('presentation', { event: { kind: 'squad', ownerId: e.unit, ownerGeneration: e.life, label: squadLabels[e.label] } });
  function spawn(p,className=p.className,postName=null) {
    return connections.command('spawn',p,className,postName);
  }
  function unspawn(p) {
    connections.command('unspawn',p);p.unit.actor.root.visible=false;
  }
  function characterHandle(p) { return p.spectator ? null : game.tailOwner.entries.get(p.character).handle; }
  // The presses each body fired this tick, on the host's own unit object
  // before its shot records are delivered (Unit.networkShots, read by the
  // presentation records' fireId); the retained lists allocate nothing.
  function mirrorShots() {
    for (const s of native.shots()) {
      const p = byHandle.get(s.handle), u = p?.unit; if (!u) continue;
      const list = (p.shotList ??= []), entries = (p.shotEntries ??= [{ button: 0, id: 0 }, { button: 1, id: 0 }]);
      list.length = 0;
      if (s.fire0 !== null) { entries[0].id = s.fire0; list.push(entries[0]); }
      if (s.fire1 !== null) { entries[1].id = s.fire1; list.push(entries[1]); }
      u.networkShot = null; u.networkShots = list;
    }
  }
  game.beforeWorldFinish = mirrorShots;
  // What a tick changed of a connection: its body, class, team and spawn
  // screen, or its retirement at the end of its grace.
  function mirror() {
    for (const view of native.changed()) {
      const p = byHandle.get(view.handle); if (!p) continue;
      if (view.deleted) { p.mailbox.retired(); byHandle.delete(view.handle); players.delete(p.id); continue; }
      p.mailbox.synchronize();
      p.unit = view.unit ?? p.unit; p.team = view.team; if (view.className) p.className = view.className;
      const s = p.spawn; s.phase = view.phase; s.className = view.className; s.post = view.post; s.spawnedOnce = view.spawnedOnce; s.deadTime = view.deadTime; s.ended = view.ended;
      // The host's copy of his body keeps the intent his controls wrote
      // (applyNetworkControls' fields; a stop also blanks his aim and exit).
      const n = view.intent, i = p.unit?.intent;
      if (n && i) {
        i.forward = n.forward; i.right = n.right; i.turn = n.turn; i.pitch = n.pitch;
        i.jump = n.jump; i.crouch = n.crouch; i.prone = n.prone; i.use = n.use; i.reload = n.reload;
        i.nextPrimary = n.nextPrimary; i.previousPrimary = n.previousPrimary; i.nextSecondary = n.nextSecondary; i.zoom = n.zoom;
        i.buttons[0] = n.buttons[0]; i.buttons[1] = n.buttons[1]; i.brake = n.brake; i.up = n.up; i.down = n.down;
        if (view.stopped) { i.aim = null; i.exit = false; }
      }
    }
  }
  return {
    players, roster, spawn,
    // spawn: false joins at the spawn screen (the original's Character starts
    // with no unit); teamSelect: whether his team choice is honoured (the
    // original's TEAMS: PLAYER SELECT) or the roster's assignment stands.
    async addPlayer(id, { team, className, name = 'Player', spectator = false, reliableFire = false, separateActions = false, spawn: autoSpawn = true, teamSelect = true } = {}) {
      const existing = players.get(id);
      if (existing) {
        if (!existing.disconnectedAt) throw new Error('player already joined');
        const at = performance.now();
        existing.disconnectedAt = null; existing.mailbox.dispose(); existing.mailbox = inputMailbox(); existing.reliableFire = reliableFire; existing.separateActions = separateActions;
        native.relink(existing.handle, { mailbox: existing.mailbox.handle, reliableFire, separateActions, at });
        existing.fire = new LaneView(native, existing.handle, 1); existing.actions = new LaneView(native, existing.handle, 0);
        if (existing.unit) existing.unit.f3.advanceLife(existing.unit);
        return this.player(id);
      }
      if (players.size >= maxPlayers) throw new Error('server full');
      team = team === 1 || team === 2 ? team : roster.counts().sort((a,b)=>a.humans-b.humans)[0].team;
      className ||= game.spawnPort.roster(team)[0]?.className;
      if (!className || !game.spawnPort.roster(team).some(x => x.className === className)) throw new Error('invalid player class');
      await game.factory.prepare(className);
      // Admission is outside the unit phase. Reserve the new connection's
      // potential cold body before its native lifecycle can acquire it.
      const character = spectator ? new Character(-1,team,true) : roster.reserve(team,id,name);
      if (spectator) character.name = String(name).slice(0,32);
      if(spectator)character.hero=false;
      const p = { id,team,className,character,spectator,reliableFire,separateActions,teamSelect:teamSelect!==false,ordinal:players.size+1, mailbox:inputMailbox(),unit:null,handle:0,disconnectedAt:null,
        spawn:{phase:'unspawned',className,post:null,spawnedOnce:false,deadTime:0,pending:null,ended:false,auto:null} };
      const reconKey = spectator ? null : remoteWorld?.admitKey(id) ?? null;
      if(!spectator) {
        for(const team of [1,2]) for(const entry of game.spawnPort.roster(team)) {
          await game.factory.prepare(entry.className);game.unitsSim.construction.prepare(entry.className,1);
        }
        game.unitsSim.construction.bindPlayer(character,null,{combatRandom:game.randomStream('gameplay'),weaponRandom:networkWeapons,
          reconKey,reconSeed:remoteWorld?.randomFor(id).seed??0});
        connections.command('admit',p,className);
      }
      players.set(id,p);
      if (!spectator && !spawn(p)) { p.mailbox.dispose(); players.delete(id); roster.release(character); throw new Error('no eligible spawn'); }
      if (!spectator && !autoSpawn) unspawn(p);
      p.handle = native.create({ mailbox: p.mailbox.handle, character: characterHandle(p), spectator, reliableFire, separateActions, teamSelect: p.teamSelect,
        autoRespawn: false, ordinal: p.ordinal, team, reconKey, at: performance.now() });
      byHandle.set(p.handle, p);
      // The reliable lanes and the local world's respawn policy live natively.
      p.fire = new LaneView(native, p.handle, 1); p.actions = new LaneView(native, p.handle, 0);
      let autoRespawn = false;
      Object.defineProperty(p, 'autoRespawn', { get: () => autoRespawn, set(value) { autoRespawn = Boolean(value); native.autoRespawn(p.handle, autoRespawn); } });
      // The stale-input clock lives natively: set when his last input arrived, read whether it stilled him.
      Object.defineProperty(p, 'lastInputAt', { set(at) { native.input(p.handle, at, NaN, true); } });
      Object.defineProperty(p, 'inputsStale', { get: () => native.lanes(p.handle, 0, false).stale });
      return this.player(id);
    },
    player(id) { const p=players.get(id); return p ? { unitId:p.unit?.id ?? game.playerUnit.id, playerId:p.unit?.id ?? game.playerUnit.id,generation:p.unit?.generation ?? 0, team:p.team,className:p.className,spectator:p.spectator,teamSelect:p.teamSelect!==false,tick:game.worldClock.tick } : null; },
    removePlayer(id) {
      const p=players.get(id); if(!p)return;
      p.disconnectedAt = game.worldClock.tick + 1;
      native.disconnect(p.handle, p.disconnectedAt);
      native.stop(p.handle);
      p.mailbox.synchronize();
      // His droid flies on unpiloted, as it does after his death.
      remoteWorld?.exit(p.id);
      // His seat is vacated now, not when the grace period ends: a hull
      // held by an absent driver never sleeps, and bots may take the seat.
      // His body stays where it rode.
      const u=p.unit;
      if(u?.seat?.vehicle)game.vehicles.unboard(u,{force:true,placement:false});
      else if(u?.seat?.turret)game.turretSeats?.leave(u,{placement:false});
    },
    // Players holding a live connection (players also keeps disconnected
    // ones through their reconnect grace period).
    connectedCount() { let n=0; for(const p of players.values()) if(!p.disconnectedAt) n++; return n; },
    clearInput(id) {
      const p=players.get(id);if(p){native.stop(p.handle);p.mailbox.synchronize();}
    },
    fire(id,edge) {
      const p=players.get(id);if(!p || !p.reliableFire || p.spectator || p.disconnectedAt || !p.unit?.alive)return false;
      if(!edge || !Number.isSafeInteger(edge.id) || !Number.isSafeInteger(edge.tick) || !Number.isSafeInteger(edge.generation) || typeof edge.pressed!=='boolean')return false;
      return native.fire(p.handle,edge,game.worldClock.tick,performance.now());
    },
    actions(id,message) {
      const p=players.get(id);if(!p || !p.separateActions || p.spectator || p.disconnectedAt)return false;
      if(!message || !Number.isSafeInteger(message.id) || !Number.isSafeInteger(message.tick) || !Number.isSafeInteger(message.generation))return false;
      if(!Array.isArray(message.actions) || message.actions.length>64 ||
        !message.actions.every(action=>typeof action==='string' && action.length<=64 && !['firePrimary','fireSecondary'].includes(action)))throw Error('invalid reliable actions');
      // The spawn screen's choices: validated here, applied natively.
      let humans, humanValid = true;
      try { humans = humanCommands(message.human).map(command => ({ command, valid: true })); } catch { humans = []; humanValid = false; }
      const entry={id:message.id,tick:message.tick,generation:message.generation,actions:message.actions,human:[]};
      return native.actions(p.handle,message.epoch,entry,humans,humanValid,game.worldClock.tick,performance.now());
    },
    input(id,command) {
      const p=players.get(id); if(!p || p.spectator || p.disconnectedAt)return false;
      if (!command || Math.abs(command.tick-game.worldClock.tick)>120) return false;
      // This client's input lead: how far ahead of the server tick it stamps
      // its commands, bounded (the native lane keeps its running max).
      const lead=command.tick-game.worldClock.tick;
      const accepted=p.mailbox.push(p.separateActions ? {...command,actions:[],human:undefined} : command);
      native.input(p.handle,performance.now(),lead,accepted);
      return accepted;
    },
    // One tick. at: the host's wall time for it (ms, performance.now()'s),
    // the step's one clock input: half a second of it without input stops a
    // player (the native stop).
    step(at = performance.now()) {
      const tick=game.worldClock.tick+1;
      game.step({epoch:1,sequence:tick,tick,generation:game.playerUnit.generation,down:[],actions:[],look:[],at});
      mirror();
      return tick;
    },
  };
}
