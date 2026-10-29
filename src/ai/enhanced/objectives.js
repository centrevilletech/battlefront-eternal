// Enhanced objectives (docs/ai-enhanced.md): which command post a bot goes
// for, and where it spawns to get there.
//
// Classic draws each bot's post from every post its team cannot spawn at,
// weighted only by the map's authored values, so a bot is sent to the
// nearest one about one time in three and most never arrive. Enhanced keeps
// the same candidates and the authored values (a post the map weighs 0 is
// still never chosen) and multiplies in:
//
//   - nearness: (nearest candidate's distance / this one's)^2, measured from
//     the bot when it is alive, else from the nearest post its team spawns
//     at (the team's front);
//   - neutral posts x1.5 (no enemy hold to break first);
//   - a standing destructible enemy objective x2;
//   - crowding: past six teammates already sent to a post, its weight falls;
//   - defense: a friendly post whose capture meter is running against its
//     owner is a candidate too (x2), and for a few seconds after.
//
// A post under attack also pulls in up to four of its owner's bots within
// 80 m, so a capture in progress meets resistance before the post is lost
// (Classic only answers once the post has gone neutral). The capture meter
// is what the player sees on the HUD; no hidden enemy count is read.
//
// EnhancedSpawning then spawns each bot at the post its team can spawn at
// that is nearest its objective (Classic picks one at random).

import * as THREE from "three";
import { ClassicObjectives, activePosts, friendlySpawn, ownerOf, postAI, postWeight } from "../classic/goals.js";
import { ClassicSpawning } from "../classic/spawning.js";

export const ENHANCED_OBJECTIVES = Object.freeze({
  nearFloor: 20, // m: nearer than this counts as this
  nearPower: 2,
  neutralBias: 1.5,
  objectiveBias: 2,
  defenseBias: 2,
  crowd: 6, // teammates on one post before its weight starts to fall
  defenseRadius: 80,
  defenseLimit: 4, // defenders sent to one post at a time
  defenseGrace: 8, // s a post stays a defense candidate after its meter stops
  defenseRepeat: 10, // s before the same post recruits again
  poll: 1, // s between defense checks
});

const flat = (a, b) => Math.hypot(a.x - b.x, a.z - b.z);
const scratch = new THREE.Vector3();

export class EnhancedObjectives extends ClassicObjectives {
  // options: ClassicObjectives' options, plus
  //   characters(): the roster (crowding, defense)
  //   now(): simulation seconds
  //   tuning: overrides for ENHANCED_OBJECTIVES
  constructor({ characters = () => [], now = () => 0, tuning = {}, ...options }) {
    super(options);
    this.characters = characters;
    this.now = now;
    this.tuning = { ...ENHANCED_OBJECTIVES, ...tuning };
    this.threatAt = new Map(); // post -> last time its meter ran against its owner
    this.recruitAt = new Map(); // `${team}:${post.name}` -> last recruitment
    this.pollAt = 0;
  }

  // A post friendly to `team` that enemies are taking, or were a moment ago.
  threatened(post, team) {
    if (post.active === false) return false;
    const owner = ownerOf(post);
    if (!owner || !(this.relation(team, owner) > 0)) return false;
    if (post.neutralize > 0) return true;
    const at = this.threatAt.get(post);
    return at !== undefined && this.now() - at < this.tuning.defenseGrace;
  }

  standingEnemyObjective(post, team) {
    return Boolean(post.building?.alive) && this.relation(team, ownerOf(post)) < 0;
  }

  // Teammates (living, human or not) already sent to `post`.
  crowding(post, team, except) {
    let n = 0;
    for (const other of this.characters()) {
      if (other !== except && other.team === team && other.objective === post && other.unit?.alive) n++;
    }
    return n;
  }

  // Where the bot's trip starts, per candidate: the bot itself when alive and
  // on foot or driving, else the nearest post its team can spawn at.
  distances(character, list) {
    const body = character.unit?.alive ? character.unit.seat?.vehicle ?? character.unit : null;
    const team = character.team;
    const spawns = body ? null : activePosts(this.posts).filter((post) => friendlySpawn(post, team, this.relation));
    return list.map((post) => {
      if (body) return flat(body.position, post.position);
      let best = Infinity;
      for (const spawn of spawns) if (spawn !== post) best = Math.min(best, flat(spawn.position, post.position));
      return Number.isFinite(best) ? best : 0;
    });
  }

  select(character, excludePrevious = false, previous = null) {
    const t = this.tuning;
    const team = character.team;
    const faction = this.faction(team);
    const seat = this.crew ? this.crew(character) : null;
    const driver = Boolean(seat?.isDriver);
    const mask = driver && this.vehicleMask ? this.vehicleMask(seat.vehicle) : 0;
    const list = [];
    const weights = [];
    for (const post of activePosts(this.posts)) {
      const defend = this.threatened(post, team);
      if (friendlySpawn(post, team, this.relation) && !defend) continue;
      if (driver && postAI(post).bans & mask) continue;
      // The patrol's revalidation asks for "anything but this one"; a post
      // still under attack keeps its defenders.
      if (excludePrevious && post === previous && !defend) continue;
      let weight = postWeight(post, team, faction);
      if (defend) weight = (weight > 0 ? weight : 100) * t.defenseBias;
      if (!(weight > 0)) continue;
      if (!defend && !ownerOf(post)) weight *= t.neutralBias;
      if (this.standingEnemyObjective(post, team)) weight *= t.objectiveBias;
      const crowd = this.crowding(post, team, character);
      if (crowd > t.crowd) weight /= 1 + (crowd - t.crowd) / t.crowd;
      list.push(post);
      weights.push(weight);
    }
    // Nothing worth going for: Classic's fallback (any post, uniformly).
    if (!list.length) return super.select(character, excludePrevious, previous);
    const d = this.distances(character, list).map((x) => Math.max(x, t.nearFloor));
    const nearest = Math.min(...d);
    let total = 0;
    for (let i = 0; i < list.length; i++) {
      weights[i] *= (nearest / d[i]) ** t.nearPower;
      total += weights[i];
    }
    let draw = this.random.uniform() * total;
    for (let i = 0; i < list.length; i++) {
      draw -= weights[i];
      if (draw < 0) return list[i];
    }
    return list[list.length - 1];
  }

  // Each simulation step: posts whose meter runs against their owner call
  // the owner's (and its friends') nearby bots in.
  tick() {
    const now = this.now();
    if (now < this.pollAt) return;
    this.pollAt = now + this.tuning.poll;
    for (const post of activePosts(this.posts)) {
      const owner = ownerOf(post);
      if (!owner || !(post.neutralize > 0)) continue;
      this.threatAt.set(post, now);
      for (let team = 1; team < 8; team++) {
        if (this.relation(team, owner) > 0) this.defend(post, team, now);
      }
    }
  }

  // Send up to defenseLimit of the team's on-foot bots near the post to it,
  // nearest first, skipping any already there, fighting, mounted, taking
  // another post's capture region, or defending another post.
  defend(post, team, now) {
    const t = this.tuning;
    const key = `${team}:${post.name}`;
    if (now - (this.recruitAt.get(key) ?? -Infinity) < t.defenseRepeat) return 0;
    if (!this.units?.query) return 0;
    let already = 0;
    const found = [];
    for (const unit of this.units.query(post.position, t.defenseRadius, [], (u) => u.team === team && !u.human)) {
      const character = unit.character;
      if (!character || !unit.controller || unit.seat) continue;
      if (unit.kind !== 0 && unit.kind !== 3) continue;
      if (character.objective === post) {
        already++;
        continue;
      }
      if (unit.controller.state !== 3 && unit.controller.state !== 1) continue;
      if (postAI(post).bans & this.travelMask(unit)) continue;
      const taking = character.objective;
      if (taking && taking !== post && typeof taking.contains === "function" && taking.contains(unit.position, scratch)) continue;
      // Already defending another post under attack: it stays there.
      if (taking && taking !== post && this.threatened(taking, team)) continue;
      found.push([flat(unit.position, post.position), character]);
    }
    found.sort((a, b) => a[0] - b[0]);
    let sent = 0;
    for (const [, character] of found) {
      if (already + sent >= t.defenseLimit) break;
      this.publish(character, post);
      sent++;
    }
    if (sent) this.recruitAt.set(key, now);
    return sent;
  }
}

// ClassicSpawning with Enhanced objectives and spawn posts.
export class EnhancedSpawning extends ClassicSpawning {
  // options: ClassicSpawning's, plus now() (simulation seconds) and
  // objectiveTuning (ENHANCED_OBJECTIVES overrides).
  constructor({ start = true, objectives, now = () => 0, objectiveTuning = {}, ...options }) {
    super({ ...options, objectives: null, start: false });
    this.objectives =
      objectives ??
      new EnhancedObjectives({
        posts: () => this.posts,
        random: options.random,
        relation: (a, b) => this.relation(a, b),
        faction: (team) => this.faction(team),
        crew: options.crew ?? null,
        vehicleMask: options.vehicleMask ?? null,
        units: options.units ?? null,
        travelMask: options.travelMask ?? null,
        onPublish: options.onPublish ?? null,
        characters: () => this.characters,
        now,
        tuning: objectiveTuning,
      });
    if (start) this.begin();
  }

  // A bot spawns at the post its team can spawn at that is nearest its
  // objective; the human, and a bot with no objective, as in Classic.
  choosePost(character) {
    const goal = character.objective;
    if (character.human || !goal?.position) return super.choosePost(character);
    let best = null;
    let bestDistance = Infinity;
    for (const post of activePosts(this.posts)) {
      if (!friendlySpawn(post, character.team, (a, b) => this.relation(a, b))) continue;
      const d = flat(post.position, goal.position);
      if (d < bestDistance) {
        bestDistance = d;
        best = post;
      }
    }
    return best ?? super.choosePost(character);
  }
}
