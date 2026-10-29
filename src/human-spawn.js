// The player's way into the battle: the spawn screen's world side, as the
// original's spawn display and Character run it (docs/human-spawn.md).
// Headless: it decides when he may spawn, keeps his queued choices, and
// applies them; the game (src/game.js) moves bodies and teams, and the
// screen (src/spawn-screen) only reads and requests.
//
// Phases:
//   unspawned  the match has started and he has never had a unit: the screen
//              is open (the original's Character starts with no unit and a
//              spent timer)
//   alive      he has a unit
//   dying      dead, his 4 s respawn timer running: nothing shows
//   awaiting   dead and the timer done: the screen is open
// With the Classic AI the spawn manager owns the timer and the spawn
// (awaitingHuman, switchTeam, onHumanReady); otherwise this keeps its own
// 4 s timer and spawns him itself, at a random point of the post's spawn
// path.
//
// The screen's choices arrive as world commands (command.human: team,
// spawn, suicide, cancel; src/runtime/commands.js) and are applied in order
// at one point of the world tick, so a worker and a replay apply them alike.
// A spawn choice waits until he may spawn, and cancel withdraws it. A team
// change only applies while he has no unit, and costs nothing: the death
// already spent a ticket. When the URL names his unit
// (src/spawn-screen/policy.js) there is no screen: the game confirms that
// choice itself, and only the pause menu's RESPAWN (suicide) applies. Once
// the battle is over nobody spawns: team and spawn choices are ignored and
// the screen doesn't open again.

import { humanEntry } from "./runtime/commands.js";
import { activePosts, friendlySpawn, humanSpawnPost, ownerOf, postAI } from "./ai/classic/goals.js";

export const RESPAWN_DELAY = 4; // s from a death to the screen (combat.js)
export const CORPSE_LINGER = 4; // s a body lies where it fell (units.js)
// Time summed tick by tick falls just short of a whole second count (240
// steps of 1/60 make 3.99999...): timers count as done within this.
export const TIME_SLACK = 1e-6;
export const EVENTS = ["awaitingSpawn", "spawned", "teamChanged", "playerDied"];

// ------------------------------------------------------------ spawn posts

// Posts a team can spawn at: held by the team with a spawn path
// or class spawn points, or held by a friend with an ally path.
export function eligiblePosts(posts, team, relation) {
  return activePosts(posts).filter((post) => friendlySpawn(post, team, relation));
}

// The post the screen selects: an eligible previous choice is
// kept; an ineligible one moves to the eligible post nearest it; with none,
// the eligible post nearest an enemy-held post (the front line); failing
// that a random eligible post. random() is [0, 1).
export function choosePost(posts, team, previous, relation, random = Math.random) {
  const pick = humanSpawnPost(previous || null, { posts, team, relation });
  if (pick) return pick;
  const eligible = eligiblePosts(posts, team, relation);
  return eligible.length ? eligible[Math.min(Math.floor(random() * eligible.length), eligible.length - 1)] : null;
}

// Where at a post he appears: a random point of its spawn path (its ally
// path when an ally holds it), else a random class spawn point (own posts
// only), else the post itself. [x, y, z, yaw].
export function spawnPointAt(post, team, random) {
  const ai = postAI(post);
  const allied = ownerOf(post) !== team;
  const path = allied ? ai.allyPath : ai.spawnPath;
  const points = path?.present ? path.points : null;
  if (points?.length) return points[Math.min(Math.floor(random() * points.length), points.length - 1)];
  if (!allied && ai.classSpawnPoints > 0 && ai.classSpawnWorld?.length) {
    return ai.classSpawnWorld[Math.min(Math.floor(random() * ai.classSpawnPoints), ai.classSpawnWorld.length - 1)] || ai.classSpawnWorld[0];
  }
  return [post.position.x, post.position.y, post.position.z, ai.yaw || 0];
}

// ------------------------------------------------------------ team switch

// Every place the player's team is kept, moved together (docs/human-spawn.md,
// "Switching team"). h: {
//   setTeams(own, foe)  the game's own playerTeam and enemyTeam
//   character           his Character
//   ai                  the Classic AI's handle (its switchTeam moves him
//                       between the rosters and retargets the aim curves), or null
//   posts               relation colours (CommandPosts)
//   battle              own/foe: the HUD's bars, Victory or Defeat
//   live                the repair and lock-on default (LiveObjects), or null
//   combats             every body's PlayerCombat (rounds, mines, droids)
//   unit                his Unit
//   audio               music and voice-over (retarget)
// }
// Teams 1 and 2 only. Returns false, changing nothing, if the roster refuses.
export function switchHumanTeam(h, team) {
  if (team !== 1 && team !== 2) return false;
  const foe = 3 - team;
  if (h.ai?.switchTeam) {
    if (!h.ai.switchTeam(h.character, team)) return false;
  } else if (h.character) {
    h.character.team = team;
  }
  h.setTeams(team, foe);
  h.posts?.setPlayerTeam(team);
  h.battle?.setHumanTeam(team, foe);
  if (h.live) h.live.playerTeam = team;
  for (const combat of h.combats || []) combat.team = team;
  if (h.unit) h.unit.team = team;
  h.audio?.retarget?.({ team, foe });
  return true;
}

// State received from the authority is presentation, not a new spawn request.
// Do not run local AI roster capacity, post selection or queue reservation.
export function applyAuthoritativeHumanTeam(h, team) {
  const moved = switchHumanTeam({ ...h, ai: null }, team);
  if (moved) h.ai?.presentHumanTeam?.(team);
  return moved;
}

// ------------------------------------------------------------ the phases

export class HumanSpawn {
  // world, the game's side:
  //   team()                     his team now
  //   dead()                     whether he has no unit in the world
  //   character()                his Character
  //   classic                    the Classic AI's handle, or null (AI off, stub)
  //   roster(team)               [className] the screen offers, mission order
  //   canSpawn(className)        whether that body can be built now
  //   setTeam(team)              setHumanTeam (true if he moved)
  //   spawn(className, post)     without Classic: into the world now, at
  //                              that post name (null: the world's pick);
  //                              false if no post serves
  //   ready(className, post)     with Classic: hand the choice to the spawn
  //                              manager (true if it took it)
  //   kill()                     a normal death (the pause menu's RESPAWN)
  //   cancel()                   with Classic: take back a choice the spawn
  //                              manager holds while he waits for a post
  //   over()                     whether the battle is over
  // policy: { className, auto } from src/spawn-screen/policy.js; post: the
  // name of the post he starts at, if the URL named one.
  constructor({ world, policy, post = null }) {
    this.world = world;
    this.auto = policy.auto || null;
    this.className = policy.className;
    this.post = post;
    this.phase = "unspawned";
    this.canSwitchTeams = true; // false where the server assigns teams
    this.spawnedOnce = false;
    this.deadTime = 0;
    this.pending = null; // { className, post } waiting for him to be able to spawn
    this.ended = false; // the battle's end has been seen
    this.listeners = new Map(EVENTS.map((name) => [name, new Set()]));
    this.queue = []; // events of this tick, sent after it
  }

  get screen() {
    return !this.auto;
  }

  on(event, fn) {
    const set = this.listeners.get(event);
    if (!set || typeof fn !== "function") return () => {};
    set.add(fn);
    return () => set.delete(fn);
  }

  emit(event, detail) {
    this.queue.push([event, detail]);
  }

  // After each world tick: the tick's events, in order. A listener's error is
  // reported and doesn't stop the battle.
  flush() {
    if (!this.queue.length) return;
    const events = this.queue.splice(0);
    for (const [event, detail] of events) {
      for (const fn of [...this.listeners.get(event)]) {
        try {
          fn(detail);
        } catch (error) {
          console.error(`spawn port: a ${event} listener failed`, error);
        }
      }
    }
  }

  // The value-only state the screen and replays read.
  state() {
    const team = this.world.team();
    const roster = this.world.roster(team);
    const index = roster.indexOf(this.className);
    return {
      phase: this.phase,
      team,
      className: this.className,
      classIndex: index >= 0 ? index : 0,
      post: this.post,
      canSwitchTeams: this.canSwitchTeams,
      // The battle is over: the screen closes, as the original's does.
      over: Boolean(this.world.over?.()),
    };
  }

  // The authority's account of him (a server's spawn summary,
  // src/runtime/network-entity-summary.js): the same fields, and the same
  // events the local world raises on the way there, so the screen, the pause
  // menu and the HUD see one spawn flow whoever runs the world. Events fire
  // at once: online no world tick flushes them.
  applyAuthoritative({ phase, className = null, post = null, spawnedOnce = false, ended = false, deadTime = 0, pending = null, auto = null }) {
    const was = this.phase, team = this.world.team();
    if (className) this.className = className;
    if (post) this.post = post;
    this.phase = phase;
    this.spawnedOnce = Boolean(spawnedOnce);
    this.ended = Boolean(ended);
    this.deadTime = deadTime;
    this.pending = pending;
    this.auto = auto;
    if (phase === "alive") {
      if (was !== "alive") this.emit("spawned", { team, className: this.className, post: this.post });
    } else if (was === "alive") this.emit("playerDied", { team, className: this.className });
    if ((phase === "awaiting" || phase === "unspawned") && was !== "awaiting" && was !== "unspawned") this.emit("awaitingSpawn", this.state());
    this.flush();
  }

  // He died (combat.onDeath): the timer starts.
  died() {
    this.phase = "dying";
    this.deadTime = 0;
    this.emit("playerDied", { team: this.world.team(), className: this.className });
  }

  // He has a unit again, as className at the named post.
  spawned(className, post = null) {
    const was = this.phase;
    this.className = className;
    if (post) this.post = post;
    this.phase = "alive";
    this.spawnedOnce = true;
    this.pending = null;
    if (was !== "alive") this.emit("spawned", { team: this.world.team(), className, post: post || null });
  }

  // He changed teams (setHumanTeam): the screen's class is clamped into the
  // new roster, as the Character's class index is.
  teamChanged(team, previous) {
    const before = this.world.roster(previous).indexOf(this.className);
    const roster = this.world.roster(team);
    if (!roster.includes(this.className) && roster.length) this.className = roster[Math.min(Math.max(before, 0), roster.length - 1)];
    this.emit("teamChanged", { team, previous });
  }

  // One world tick's part, where the spawn manager's choices are made (after
  // the battle's update): the tick's commands, then the phase, then a waiting
  // choice if he may spawn now.
  update(dt, commands = null) {
    const world = this.world;
    if (world.dead()) this.deadTime += dt;
    if (commands) for (let i = 0; i < commands.length; i++) this.apply(humanEntry(commands[i]));
    // The battle has ended: a choice waiting here, or one the spawn manager
    // took and hasn't carried out, is dropped (once).
    if (!this.auto && !this.ended && world.over?.()) {
      this.ended = true;
      this.pending = null;
      world.cancel?.();
    }
    this.updatePhase();
    if (this.pending && (this.phase === "unspawned" || this.phase === "awaiting")) this.realize();
  }

  apply(entry) {
    if (!entry) return;
    const world = this.world;
    if (entry.kind === "suicide") {
      if (!world.dead()) world.kill();
      return;
    }
    // Without a screen the game confirms the URL's choice itself; once the
    // battle is over nobody spawns.
    if (this.auto || world.over?.()) return;
    if (entry.kind === "team") {
      if (world.dead() && entry.team !== world.team()) world.setTeam(entry.team);
    } else if (entry.kind === "spawn") {
      if (!world.dead()) return;
      this.pending = { className: entry.className, post: entry.post };
    } else if (entry.kind === "cancel") {
      this.pending = null;
      world.cancel?.();
    }
  }

  updatePhase() {
    const world = this.world;
    const was = this.phase;
    if (!world.dead()) this.phase = "alive";
    else if (!this.spawnedOnce) this.phase = "unspawned";
    else {
      const classic = world.classic;
      const character = world.character();
      const open = !world.over?.() && (classic && character?.spawn
        ? !character.unit && character.spawn.timer <= 0
        : this.deadTime >= RESPAWN_DELAY - TIME_SLACK);
      this.phase = open ? "awaiting" : "dying";
    }
    if (this.phase === "awaiting" && was !== "awaiting") this.emit("awaitingSpawn", this.state());
  }

  // The waiting choice, if it can be had: a class of his team's roster that
  // can be built; the post as asked (the world falls back as the original
  // does when it doesn't serve).
  realize() {
    const world = this.world;
    const { className, post } = this.pending;
    if (!world.roster(world.team()).includes(className) || !world.canSpawn(className)) {
      console.warn(`spawn port: ${className} can't be spawned on team ${world.team()}`);
      this.pending = null;
      return;
    }
    this.className = className;
    if (world.classic) {
      if (world.ready(className, post)) this.pending = null;
    } else {
      world.spawn(className, post);
    }
  }
}
