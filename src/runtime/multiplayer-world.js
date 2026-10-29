import {entitySpawnPosts} from './entity-spawn-posts.js';
import { blankSeatIntent } from '../seats.js';
import { HitHistory, MAX_INPUT_LEAD_TICKS, ONLINE_PLAYOUT_MAX_TICKS } from './hit-history.js';
import { networkVehicleInput } from './network-vehicle.js';
import { cycleNetworkSeat } from './network-seat.js';
import { mountInReach, seatScheme } from '../boarding.js';
import { Character } from '../battle.js';
import { choosePost, spawnPointAt, RESPAWN_DELAY } from '../human-spawn.js';
import { CommandControls } from './commands.js';
import { FireCommands } from './fire-commands.js';
import { InputMailbox } from './transport.js';
import { InputActions } from './input-actions.js';
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
  const disconnectGraceTicks = Math.round(DISCONNECT_GRACE_SECONDS * (game.worldClock.hz ?? 60));
  game.serverWorld = true;
  let humanTraceLeft = globalThis.process?.env?.BFE_TRACE_HUMAN === '1' ? 64 : 0;
  const traceHuman = value => { if (humanTraceLeft > 0) { humanTraceLeft--; console.log('network-human-trace', JSON.stringify(value)); } };
  const remoteWorld = game.createNetworkRemotes?.(maxPlayers);
  const hitHistory = game.ordnance.hitHistory = new HitHistory(game.units);
  // The headless game's original viewpoint is not an extra player. Its body,
  // spawned at load from the URL's side, leaves the world with it: every
  // client is sent its visibility, and would stand it at that spawn point.
  game.playerUnit.alive = false;
  game.playerUnit.removed = true;
  game.playerUnit.undraw();
  game.combat.dead = true;
  game.humanSpawn.auto = null;
  game.humanSpawn.pending = null;
  game.humanSpawn.spawnedOnce = true;
  const dormant = game.battle.human;
  if (dormant) dormant.unit = null;
  const roster = createMultiplayerRoster(game, armySize);
  const updateViews = networkViews(game, players);
  const relation = (a,b) => game.units.relation(a,b);
  // His input stops: he left, paused (a suspended local world), or nothing
  // came from him for half a second. Whatever he controls is stilled, a
  // hull's sticks and a flown recon droid's keys alike; leaving also hands
  // the droid back (removePlayer), but a pause or a gap in his input does not.
  function stopInput(p) {
    p.mailbox.clear();p.controls.clear();p.fire.clear();p.actions.clear();
    const unit=p.unit;if(!unit)return;
    Object.assign(unit.intent,blankSeatIntent(),{flight:{}});
    unit.networkShot=null;unit.networkShots=[];unit.networkViewTick=null;unit.networkInputLeadTicks=null;unit.networkPlayoutMaxTicks=null;
    if(unit.seat?.vehicle && unit.seat.index===0)unit.seat.vehicle.stickX=unit.seat.vehicle.stickY=0;
  }
  function spawn(p, className = p.className, postName = null) {
    if (game.battle.gameOver || !game.spawnPort.roster(p.team).some(x => x.className === className)) return false;
    if (p.unit && !p.unit.alive && p.unit.deadTime + 1e-6 < RESPAWN_DELAY) return false;
    const posts = entitySpawnPosts(game);
    const post = choosePost(posts, p.team, posts.find(x => x.name === postName), relation, () => 0.5);
    if (!post || !game.factory.prepared(className)) return false;
    // Keep connection IDs stable between lives; do not lend a player's body
    // to the AI pool while the connection still owns it.
    const previous = p.unit;
    const unit = previous?.className === className ? previous : game.factory.acquire(className);
    if (previous && previous !== unit) {
      previous.human = previous.networkHuman = previous.combat.player = false;
      previous.onRemoved = u => game.factory.release(u);
      previous.remove();
    }
    unit.onRemoved = () => {};
    unit.human = true;
    unit.networkHuman = true;
    unit.networkShot = null; unit.networkShots=[]; unit.networkViewTick = null; unit.networkInputLeadTicks = null; unit.networkPlayoutMaxTicks = null;
    unit.combat.player = true;
    unit.team = unit.combat.team = p.team;
    unit.character = p.character;
    p.character.unit = unit;
    p.character.className = className;
    p.character.spawn.ready = false;
    p.character.spawn.timer = Infinity;
    if (!previous?.alive && game.ai?.spawning?.teams?.[p.team]) game.ai.spawning.teams[p.team].living++;
    if (remoteWorld) {
      if(previous&&previous!==unit){previous.combat.remotes=null;previous.remoteStep=null;}
      unit.combat.remotes=remoteWorld.rebind(p.id,unit);
      unit.remoteStep=(dt,input,controls)=>remoteWorld.step(p.id,game.worldClock.tick+1,dt,input,controls);
    }
    p.unit = unit; p.className = className;
    if (previous && previous !== unit) unit.generation = Math.max(unit.generation, previous.generation);
    unit.spawn(...spawnPointAt(post,p.team,() => ((p.ordinal * 0.381966) % 1)));
    p.spawn.phase = 'alive'; p.spawn.spawnedOnce = true; p.spawn.className = className; p.spawn.post = post.name;
    game.simEvents.emit('spawn', { target: game.units.ref(unit), team:p.team, position:unit.position, className, post:post.name });
    return true;
  }
  // Off the field without a death: a joining player waits at the spawn screen
  // as the original's Character does. No ticket is spent, no corpse lies,
  // nothing counts, and his first choice may spawn at once. His body (and so
  // his own corrections) exist; nobody else is sent it while it is removed.
  function unspawn(p) {
    const unit = p.unit;
    unit.alive = false; unit.dead = unit.combat.dead = unit.soldier.dead = true; unit.removed = true; unit.targetable = false;
    unit.actor.root.visible = false;
    unit.deadTime = RESPAWN_DELAY; unit.combat.deathTime = 0;
    if (game.ai?.spawning?.teams?.[p.team]) game.ai.spawning.teams[p.team].living--;
    p.spawn.phase = 'unspawned'; p.spawn.spawnedOnce = false; p.spawn.post = null; p.spawn.deadTime = unit.deadTime;
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
        existing.disconnectedAt = null; existing.mailbox = new InputMailbox(); existing.controls = new CommandControls(); existing.fire = new FireCommands(); existing.reliableFire = reliableFire; existing.lastInputAt = performance.now(); existing.inputsStale=false;
        existing.actions = new InputActions(); existing.separateActions = separateActions;
        if (existing.unit) existing.unit.generation++;
        return this.player(id);
      }
      if (players.size >= maxPlayers) throw new Error('server full');
      team = team === 1 || team === 2 ? team : roster.counts().sort((a,b)=>a.humans-b.humans)[0].team;
      className ||= game.spawnPort.roster(team)[0]?.className;
      if (!className || !game.spawnPort.roster(team).some(x => x.className === className)) throw new Error('invalid player class');
      await game.factory.prepare(className);
      const character = spectator ? new Character(-1,team,true) : roster.reserve(team,id,name);
      if (spectator) character.name = String(name).slice(0,32);
      character.hero = false;
      const p = { id,team,className,character,spectator,reliableFire,separateActions,teamSelect:teamSelect!==false,actions:new InputActions(),fire:new FireCommands(),lastInputAt:performance.now(),inputsStale:false,ordinal:players.size+1, mailbox:new InputMailbox(),controls:new CommandControls(),unit:null,
        spawn:{phase:'unspawned',className,post:null,spawnedOnce:false,deadTime:0,pending:null,ended:false,auto:null} };
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
      // His body stays where it rode (as a nextSpawn exit leaves it).
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
    step() {
      const tick=game.worldClock.tick+1;
      hitHistory.capture(game.worldClock.tick);
      for(const p of players.values()) {
        p.spawn.ended = game.battle.gameOver;
        if (p.disconnectedAt) {
          if (tick - p.disconnectedAt >= disconnectGraceTicks) {
            if (!p.spectator) {
              p.character.unit = p.unit;
              roster.release(p.character);
            }
            remoteWorld?.retire(p.id);
            players.delete(p.id);
          }
          continue;
        }
        if(p.spectator)continue;
        if(!p.inputsStale && performance.now()-p.lastInputAt>500) {
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
          if(choice.kind==='team' && !u.alive && p.teamSelect!==false && [1,2].includes(choice.team) && roster.switchTeam(p.character,choice.team)) { p.team=choice.team; u.team=u.combat.team=p.team;  }
          const spawned=choice.kind==='spawn' && !p.unit.alive ? spawn(p,choice.className,choice.post) : null;
          traceHuman({stage:'decision',tick,choice,spawned,team:p.team,unit:{id:p.unit.id,generation:p.unit.generation,className:p.unit.className,alive:p.unit.alive,deadTime:p.unit.deadTime},phase:p.spawn.phase});
        }
        // A class change may replace the body. Continue this tick on the new
        // life instead of overwriting its phase from the removed corpse.
        u=p.unit;
        // A body a death region took at once (a fall into Bespin's clouds after
        // bailing out) is not ticked; its respawn delay still runs out.
        if(!u.alive && u.removed && u.deadTime<RESPAWN_DELAY) u.deadTime=Math.min(RESPAWN_DELAY,u.deadTime+1/60);
        if(!u.alive && p.autoRespawn && u.deadTime+1e-6>=RESPAWN_DELAY)spawn(p,p.className,p.spawn.post);
        u=p.unit;
        if(!u.alive) { p.spawn.phase=!p.spawn.spawnedOnce?'unspawned':u.deadTime+1e-6>=RESPAWN_DELAY?'awaiting':'dying'; p.spawn.deadTime=u.deadTime; continue; }
        const c=p.controls, intent=u.intent;
        for(const [action,kind,label]of [['squadMoveOut',0,'moveout'],['squadHold',2,'hold'],['squadFollow',3,'follow'],['squadAtEase',1,'atease']]){
          if(c.take(action)&&game.ai?.playerCommand){game.ai.playerCommand(kind,p.character);game.simEvents.emit('presentation',{event:{kind:'squad',ownerId:u.id,ownerGeneration:u.generation,label}});}
        }
        if(c.take('nextSpawn')){
          const eligible=game.spawnPort.posts(p.team).filter(post=>post.eligible);
          const next=eligible[(eligible.findIndex(post=>post.name===p.spawn.post)+1)%eligible.length];
          if(next){if(u.seat?.vehicle)game.vehicles.unboard(u,{force:true,placement:false});else if(u.seat?.turret)game.turretSeats.leave(u,{placement:false});spawn(p,p.className,next.name);u=p.unit;}
        }
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
      updateViews();
      game.step({epoch:1,sequence:tick,tick,generation:game.playerUnit.generation,down:[],actions:[],look:[]});
      return tick;
    },
  };
}
