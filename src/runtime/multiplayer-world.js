import { blankSeatIntent } from '../seats.js';
import { MAX_INPUT_LEAD_TICKS, ONLINE_PLAYOUT_MAX_TICKS } from './network-hit-limits.js';
import { networkVehicleInput } from './network-vehicle.js';
import { cycleNetworkSeat } from './network-seat.js';
import { mountInReach, seatScheme } from '../boarding.js';
import { Character } from '../battle.js';
import { CommandControls } from './commands.js';
import { FireCommands } from './fire-commands.js';
import { InputMailbox } from './transport.js';
import { InputActions } from './input-actions.js';
import { F4Connections } from '../f4-connections.js';
import { createMultiplayerRoster } from './multiplayer-roster.js';
import { networkViews } from './network-views.js';

// A disconnected player's body and roster place are held this long in
// simulated seconds (60 s at the 60 Hz world clock), for a reconnect.
const DISCONNECT_GRACE_SECONDS = 60;

// A dedicated match owns one simulation and one independently stamped input
// mailbox per authenticated connection. Network humans use the same Unit,
// Soldier, combat, damage and capture paths as the rest of the battle.
export function createMultiplayerWorld(game, { maxPlayers = 64, armySize = 32 } = {}) {
  // No single simulation camera scopes crowd separation online: every pair
  // is separated everywhere, and clients predict the same push.
  if (game.separationScope) game.separationScope.global = true;
  const players = new Map();
  const inputActions = () => new InputActions();
  const fireCommands = () => new FireCommands();
  const inputMailbox = () => new InputMailbox();
  const commandControls = () => new CommandControls();
  const disconnectGraceTicks = Math.round(DISCONNECT_GRACE_SECONDS * (game.worldClock.hz ?? 60));
  game.serverWorld = true;
  let humanTraceLeft = globalThis.process?.env?.BFE_TRACE_HUMAN === '1' ? 64 : 0;
  const traceHuman = value => { if (humanTraceLeft > 0) { humanTraceLeft--; console.log('network-human-trace', JSON.stringify(value)); } };
  const remoteWorld = game.createNetworkRemotes?.(maxPlayers);
  const hitHistory = game.ordnance.hitHistory = game.unitsSim.history;
  // The dedicated world's viewpoint remains a camera facade, never an extra
  // player. Both its body flags and spawn records are cleared natively.
  game.unitsSim.parkViewpoint(game.playerUnit);
  game.playerUnit.undraw();
  const connections=new F4Connections(game.tailOwner);
  const roster = createMultiplayerRoster(game, armySize);
  const updateViews = networkViews(game, players);
  // A network player's weapons draw what no press seeds (a held trigger's
  // repeats, a charge's release, a throw, a melee swing) from the world's
  // network-weapons stream; a press's own fire cycle keeps its spread seed.
  const networkWeapons = game.randomStream?.('network-weapons') ?? null;
  // His input stops: he left, paused (a suspended local world), or nothing
  // came from him for half a second. Whatever he controls is stilled, a
  // hull's sticks and a flown recon droid's keys alike; leaving also hands
  // the droid back (removePlayer), but a pause or a gap in his input does not.
  function stopInput(p) {
    p.mailbox.clear();p.controls.clear();p.fire.clear();p.actions.clear();
    const unit=p.unit;if(!unit)return;
    Object.assign(unit.intent,blankSeatIntent(),{flight:{}});
    unit.networkShot=null;unit.networkShots=[];unit.networkViewTick=null;unit.networkInputLeadTicks=null;unit.networkPlayoutMaxTicks=null;
    if(unit.seat?.vehicle && unit.seat.index===0) { game.vehicles.f2.seats.resetSticks(unit); }
  }
  function spawn(p,className=p.className,postName=null) {
    return connections.command('spawn',p,className,postName);
  }
  function unspawn(p) {
    connections.command('unspawn',p);p.unit.actor.root.visible=false;
  }
  function applyNetworkControls(p, u, command, tick) {
    const c=p.controls, intent=u.intent;
    for(const [action,kind,label]of [['squadMoveOut',0,'moveout'],['squadHold',2,'hold'],['squadFollow',3,'follow'],['squadAtEase',1,'atease']]){
      if(c.take(action)&&game.ai?.playerCommand){game.ai.playerCommand(kind,p.character);game.simEvents.emit('presentation',{event:{kind:'squad',ownerId:u.id,ownerGeneration:u.generation,label}});}
    }
    // Older clients still send N (nextSpawn): dropped, so it never piles up.
    while(c.take('nextSpawn'));
    // Fire and seat keys as his seat reads them now (src/boarding.js).
    c.flightScheme=()=>seatScheme(u.seat?.vehicle,command.flightSettings?.bindings);
    if(typeof command.firstPerson==='boolean')u.combat.firstPerson=command.firstPerson;
    if(u.combat.remotes){if(typeof command.firstPerson==='boolean')u.combat.remotes.firstPerson=command.firstPerson;if(Number.isFinite(command.aspect)&&command.aspect>0&&u.combat.remotes.aimCamera)u.combat.remotes.aimCamera.aspect=command.aspect;}
    for(const axis of ['forward','right','turn'])intent[axis]=c.axis(axis);
    intent.pitch=0;
    for(const action of ['jump','crouch','prone','use','reload','nextPrimary','previousPrimary','nextSecondary','zoom'])intent[action]=c.take(action);
    u.networkShot = null;u.networkShots=[];
    u.networkViewTick = p.inputsStale ? null : command.viewTick;
    u.networkInputLeadTicks = p.inputsStale ? null : (p.inputLeadTicks ?? null);
    u.networkPlayoutMaxTicks = ONLINE_PLAYOUT_MAX_TICKS;
    if(p.reliableFire) {
      const fire=p.fire.take(tick,u.generation,{tick:p.mailbox.stateTick,buttons:[c.down.has('Mouse0'),c.down.has('Mouse2')]});
      intent.buttons[0]=fire.buttons[0];intent.buttons[1]=fire.buttons[1];
      // How late each press ran: its lockout counts from its own tick.
      for(const shot of fire.shots)shot.late=Math.max(0,tick-shot.tick);
      u.networkShots=fire.shots;
    } else { intent.buttons[0]=c.held('firePrimary')?1:0; intent.buttons[1]=c.held('fireSecondary')?1:0; }
    intent.brake=c.down.has('Space'); intent.up=c.down.has('Space')?1:0; intent.down=c.down.has('ControlLeft')?1:0;
    const scale=Math.min(command.mouseScale ?? game.settings.mouseScale,2);
    for(const [dx,dy] of command.look){const x=Math.max(-10000,Math.min(10000,dx)),y=Math.max(-10000,Math.min(10000,dy));if(u.combat.remotes?.active)u.combat.remotes.look(x,y,scale,command.invertMouse||false);else (u.soldier.lookRaw || u.soldier.look).call(u.soldier,x/u.combat.zoom,y/u.combat.zoom,scale,command.invertMouse || false);}
    if (u.seat) {
      if (u.seat.vehicle) {
        if(intent.nextPrimary)cycleNetworkSeat(game.vehicles,u,1);
        if(intent.previousPrimary)cycleNetworkSeat(game.vehicles,u,-1);
        intent.nextPrimary=intent.previousPrimary=false;
        // The keys of the seat he holds after any switch, as the hull takes them.
        const keys=networkVehicleInput(u.seat.vehicle,u.seat.index,c,command);
        intent.flight=keys; intent.forward=keys.forward||0; intent.right=keys.right||0; intent.turn=keys.turn||0; intent.brake=Boolean(keys.brake);
      } else {
        intent.turn += command.look.reduce((n,v)=>n+v[0],0)*scale*60 / 2048;
        intent.pitch = -command.look.reduce((n,v)=>n+v[1],0)*scale*60 / 2048*(command.invertMouse?-1:1);
      }
    } else if (intent.use && !u.combat.remotes?.active) {
      // What his E reaches, by the player's rules (src/boarding.js).
      const mount=mountInReach(game,u,true);
      if(mount?.vehicle) game.vehicles.board(u,mount.vehicle);
      else if(mount?.turret) game.turretSeats?.man(u,mount.turret);
      if(u.seat) intent.use=false;
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
        existing.disconnectedAt = null; existing.mailbox = inputMailbox(); existing.controls = commandControls(); existing.fire = fireCommands(); existing.reliableFire = reliableFire; existing.lastInputAt = performance.now(); existing.inputsStale=false;
        existing.actions = inputActions(); existing.separateActions = separateActions;
        if(existing.unit)existing.unit.f3.advanceLife(existing.unit);
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
      const p = { id,team,className,character,spectator,reliableFire,separateActions,teamSelect:teamSelect!==false,actions:inputActions(),fire:fireCommands(),lastInputAt:performance.now(),inputsStale:false,ordinal:players.size+1, mailbox:inputMailbox(),controls:commandControls(),unit:null,
        spawn:{phase:'unspawned',className,post:null,spawnedOnce:false,deadTime:0,pending:null,ended:false,auto:null} };
      if(!spectator) {
        for(const team of [1,2]) for(const entry of game.spawnPort.roster(team)) {
          await game.factory.prepare(entry.className);game.unitsSim.construction.prepare(entry.className,1);
        }
        game.unitsSim.construction.bindPlayer(character,null,{combatRandom:game.randomStream('gameplay'),weaponRandom:networkWeapons,
          reconKey:remoteWorld?.admitKey(id)??null,reconSeed:remoteWorld?.randomFor(id).seed??0});
        connections.command('admit',p,className);
      }
      players.set(id,p);
      if (!spectator && !spawn(p)) { players.delete(id); roster.release(character); throw new Error('no eligible spawn'); }
      if (!spectator && !autoSpawn) unspawn(p);

      return this.player(id);
    },
    player(id) { const p=players.get(id); return p ? { unitId:p.unit?.id ?? game.playerUnit.id, playerId:p.unit?.id ?? game.playerUnit.id,generation:p.unit?.generation ?? 0, team:p.team,className:p.className,spectator:p.spectator,teamSelect:p.teamSelect!==false,tick:game.worldClock.tick } : null; },
    removePlayer(id) {
      const p=players.get(id); if(!p)return;
      p.disconnectedAt = game.worldClock.tick + 1;
      stopInput(p);
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
      const p=players.get(id);if(p)stopInput(p);
    },
    fire(id,edge) {
      const p=players.get(id);if(!p || !p.reliableFire || p.spectator || p.disconnectedAt || !p.unit?.alive)return false;
      const accepted=p.fire.push(edge,game.worldClock.tick,p.unit.generation);
      if(accepted){p.lastInputAt=performance.now();p.inputsStale=false;}return accepted;
    },
    actions(id,message) {
      const p=players.get(id);if(!p || !p.separateActions || p.spectator || p.disconnectedAt)return false;
      const accepted=p.actions.push(message,game.worldClock.tick,p.unit.generation);
      if(message.human?.length)traceHuman({stage:'received',tick:game.worldClock.tick,commandTick:message.tick,generation:message.generation,currentGeneration:p.unit.generation,id:message.id,human:message.human,accepted});
      if(accepted){p.lastInputAt=performance.now();p.inputsStale=false;}return accepted;
    },
    input(id,command) {
      const p=players.get(id); if(!p || p.spectator || p.disconnectedAt)return false;
      if (!command || Math.abs(command.tick-game.worldClock.tick)>120) return false;
      // This client's input lead: how far ahead of the server tick it stamps
      // its commands, bounded. The max seen is its configured lead; a late
      // command must not shrink the rewind budget its shots are judged by.
      // It is a running max that never decays, so one lag spike widens this
      // client's rewind window for the rest of the connection; letting it
      // shrink would change simulation results, so it is left as it is.
      const lead=command.tick-game.worldClock.tick;
      if(Number.isFinite(lead))p.inputLeadTicks=Math.max(p.inputLeadTicks??0,Math.max(0,Math.min(MAX_INPUT_LEAD_TICKS,lead)));
      const accepted=p.mailbox.push(p.separateActions ? {...command,actions:[],human:undefined} : command);
      if(accepted){p.lastInputAt=performance.now();p.inputsStale=false;}return accepted;
    },
    // One tick. at: the host's wall time for it (ms, performance.now()'s),
    // the step's one clock input: half a second of it without input stops a
    // player (stopInput).
    step(at = performance.now()) {
      const tick=game.worldClock.tick+1;
      hitHistory.capture(game.worldClock.tick);
      for(const p of players.values()) {
        p.spawn.ended = game.battle.gameOver;
        if (p.disconnectedAt) {
          if (tick - p.disconnectedAt >= disconnectGraceTicks) {
            if (!p.spectator) {
              roster.release(p.character);
            }
            remoteWorld?.retire(p.id);
            players.delete(p.id);
          }
          continue;
        }
        if(p.spectator)continue;
        if(!p.inputsStale && at-p.lastInputAt>500) {
          p.inputsStale=true;stopInput(p);
        }
        let u=p.unit;const command=p.mailbox.take(tick,u.generation);
        if(p.separateActions) {
          const discrete=p.actions.take(tick,u.generation);
          command.actions=discrete.actions; command.human=discrete.human;
        }
        p.controls.accept(command);
        for(const choice of command.human || []) {
          if(choice.kind==='suicide' && u.alive)u.combat.damage(Infinity);
          if(choice.kind==='team' && !u.alive && p.teamSelect!==false && [1,2].includes(choice.team) && roster.switchTeam(p.character,choice.team)) { p.team=choice.team; connections.command('team',p); }
          const spawned=choice.kind==='spawn' && !p.unit.alive ? spawn(p,choice.className,choice.post) : null;
          traceHuman({stage:'decision',tick,choice,spawned,team:p.team,unit:{id:p.unit.id,generation:p.unit.generation,className:p.unit.className,alive:p.unit.alive,deadTime:p.unit.deadTime},phase:p.spawn.phase});
        }
        // A class change may replace the body. Continue this tick on the new
        // life instead of overwriting its phase from the removed corpse.
        u=p.unit;
        // Native removed-body clocks run before controls and the world.
        if(!u.alive)connections.command('state',p);
        u=p.unit;
        if(!u.alive)continue;
        applyNetworkControls(p, u, command, tick);
      }
      updateViews();
      game.step({epoch:1,sequence:tick,tick,generation:game.playerUnit.generation,down:[],actions:[],look:[]});
      return tick;
    },
  };
}
