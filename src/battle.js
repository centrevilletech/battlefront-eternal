// Construction and replication facades. Native battle rules own every
// tick and score mutation; the page reads these stable scoreboard objects.

// The battle's rules: who fights for each team, their reinforcements, the
// bleed of reinforcements from holding fewer posts, and how a team loses.
//
// Each team has a fixed set of Characters, as the mission's SetUnitCount
// gives it. In ordinary offline play that count includes the local human:
// Theed's 22 a side is the player and 21 bots on the player's team, and 22
// bots on the other. A Character outlives its bodies: each spawn gives it a
// unit of the class it picked, and each death costs its team a
// reinforcement.
//
// Each team's update, as the 2004 game runs it (teams 1 to 7 in order, from
// the spawn manager's update). The armies (teams 1 and 2):
// - Bleed: the team scores each post's ValueBleed for the posts it holds,
//   its living command vehicles among them (src/mobile-posts.js: Hoth's
//   AT-ATs are worth 10 each), plus 1 per neutral post. The first of the
//   mission's thresholds (lowest first) the score doesn't pass sets the
//   rate; the rate x seconds gather, and each update that finds 4 or more
//   spends one reinforcement (at most one per update, however far behind).
// - No posts: a team whose score is 0 (no post of any value held, none
//   neutral) counts down from 20 s; a neutral post pauses the count, holding
//   a post resets it. At 0 the team's reinforcements, and its starting count,
//   drop to nothing. Hoth's probe droid drop, worth nothing, saves nobody.
// A team out of reinforcements has lost. Teams 3 to 7, the locals, neither
// bleed nor count down: the mission gives them no reinforcements and they
// start with 2147483647, so they keep coming all battle. A local team
// friendly with exactly one army loses its reinforcements once that army's
// are down to its living soldiers (Endor's Ewoks stop coming when the
// Rebels are about to lose); one friendly with both or neither never does.
//
// The end, as the 2004 game's mission loop has it: 2 s after a team has
// lost comes the game-over flag, when the scores stop counting and the
// end display shows; 7 s after that the end-of-match screens take over.
//
// The scores (docs/scoreboard.md): each Character counts every kill
// credited to it, the team kills among them, its deaths and its command
// post points. The scoreboard shows KILLS as kills - 2 x team kills. The
// rest of what the end-of-match screens report (DETAILS, AWARDS) is kept in
// each Character's record (src/battle-stats.js).

import { botNames, humanName } from "./bot-names.js";
import { BattleStats } from "./battle-stats.js";

export const GAME_OVER_DELAY = 2; // s from a team's last reinforcement to the game-over flag
export const END_SEQUENCE = 7; // s from the game-over flag to the end-of-match screens

export class Character {
  constructor(index, team, human = false) {
    this.index = index; // within the team
    this.team = team;
    this.human = human;
    this.name = human ? humanName(0) : null; // a bot's comes with its roster
    this.className = null; // the class of its current or next body
    this.unit = null; // the living (or dying) body, or null
    this.controller = null; // a bot's Classic controller
    this.objective = null; // the command post it is sent to
    this.kills = 0; // every kill credited, friend or foe
    this.teamKills = 0; // the friends among them, himself included
    this.deaths = 0;
    this.captures = 0; // command post points (16 bits, as the original keeps them)
    // Seconds before being outside the battlefield kills him, -1 while he is
    // inside (src/battlefield.js).
    this.outOfBounds = -1;
  }
}

// What the scoreboard shows of a Character: KILLS, DEATHS and CP POINTS.
export function shownStats(character) {
  return {
    kills: (character?.kills ?? 0) - 2 * (character?.teamKills ?? 0),
    deaths: character?.deaths ?? 0,
    points: character?.captures ?? 0,
  };
}

export class Battle {
  // teams: the mission's team table (map.json mission.teams).
  // relation(a, b): team a's relation to team b (src/registry.js); living(t):
  // team t's living soldiers, heroes excepted (the spawn manager's count).
  constructor({ teams, posts, own, foe, onOver = () => {}, relation = null, living = null }) {
    this.posts = posts;
    this.own = own;
    this.foe = foe;
    this.teams = teams;
    this.names = Object.fromEntries(Object.entries(teams).map(([t, v]) => [Number(t), v.label || v.name]));
    this.reinforcements = Object.fromEntries(Object.entries(teams).map(([t, v]) => [Number(t), v.reinforcements ?? (Number(t) >= 3 ? 2147483647 : 0)]));
    this.start = { ...this.reinforcements };
    this.teamIds = Object.keys(teams).map(Number).sort((a, b) => a - b);
    this.noPost = Object.fromEntries(this.teamIds.map((t) => [t, 20]));
    this.bleed = {};
    this.victoryClock = null;
    this.countdown = null;
    this.losing = false;
    this.over = false; // a team is out of reinforcements
    this.loser = null;
    this.overTime = 0; // s since
    this.gameOver = false; // the scores stop counting
    this.finished = false; // the end-of-match screens take over
    this.onOver = onOver;
    this.relation = relation || ((a, b) => (a === b ? 1 : -1));
    const counted = (team) => this.characters.reduce((n, c) => n + (c.team === team && !c.hero && c.unit?.alive ? 1 : 0), 0);
    this.living = (team) => living?.(team) ?? counted(team);
    this.stats = new BattleStats();
    // Whose records tally each tick: every bot; the player while he has a
    // body (the world narrows it to his spawn until his spawn screen).
    this.statsActive = (character) => !character.human || Boolean(character.unit);
    this.bleedRules = Object.fromEntries(
      Object.entries(teams).map(([team, t]) => [Number(team), [...(t.bleed || [])].sort((a, b) => a[0] - b[0])]),
    );
    this.bleedClock = {};
    this.characters = [];
  }

  // Each team's Characters: SetUnitCount of them, the human among his own.
  // Only the two armies: the Classic spawn manager fields the locals.
  populate(humanTeam) {
    this.characters = [];
    for (const team of [this.own, this.foe]) {
      const count = Math.max(this.teams[team]?.units ?? 0, team === humanTeam ? 1 : 0);
      for (let i = 0; i < count; i++) this.characters.push(new Character(i, team, team === humanTeam && i === 0));
    }
    return this.characters;
  }

  // The local player's Character: offline the one human of the roster; in
  // a network game, where every player is one, the one the server names
  // (applyWorldSummary sets it).
  get human() {
    return this.local || this.characters.find((c) => c.human) || null;
  }

  set human(character) {
    this.local = character || null;
  }

  // The player changed teams (between lives): his side of the HUD's bars and
  // of Victory or Defeat follows him.
  setHumanTeam(own, foe) {
    this.own = own;
    this.foe = foe;
  }

  team(team) {
    return this.characters.filter((c) => c.team === team);
  }

  // Names for the bots that have none (a roster the Classic AI didn't
  // build), team by team in roster order, each team from its own generator
  // (src/bot-names.js). lists: ai-mission.json `names`; faction(team): the
  // team's side type.
  nameBots(lists, faction) {
    for (const team of [...new Set(this.characters.map((c) => c.team))]) {
      const next = botNames(lists, faction(team));
      for (const character of this.team(team)) if (!character.human && !character.name) character.name = next();
    }
  }

}
