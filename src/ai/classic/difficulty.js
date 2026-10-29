// Classic difficulty, as the 2004 game has it.
//
// The menu tier is Easy 1, Medium 2 or Hard 3. Behind it the game keeps two
// integer skills, 1..20: one for the AI on the player's team and one for
// every other team's AI. A tier sets them asymmetrically (Easy 6/4, Medium
// 4/14, Hard 4/17, player/enemy), and four curves of each skill drive aiming:
// A and D damp and limit the aim direction's acceleration, B scales cannon
// spread build-up, S adds sniper yaw error. Each curve runs straight through
// its values at skill 1, 14 and 20, computed in float32 one operation at a
// time, in the order the 2004 game does it.
//
// Mission scripts then adjust the skills, in call order: the plain setters
// replace a skill, the Easy/Medium/Hard ones add to it only on that tier, and
// every call clamps to 1..20 and recomputes the curves. The Advanced Training
// planetary bonus re-runs the tier initialization with the player's team at
// +4, replacing whatever the mission did. An override flag (never set by the
// shipped game) makes initialization keep the current skills; the mission
// setters ignore it.
//
// Who counts as the player's team: the team of local player 1's Character,
// offline only. In a network game every AI uses the enemy curves, and so does
// everyone when two split-screen players are on different teams. Locals
// allied with the player still use the enemy curves.

const f32 = Math.fround;

export const EASY = 1;
export const MEDIUM = 2;
export const HARD = 3;

export const SKILL_MIN = 1;
export const SKILL_MAX = 20;

// [player, enemy] skill by tier. Any other tier value falls through to 4/14.
const BASE = { [EASY]: [6, 4], [MEDIUM]: [4, 14], [HARD]: [4, 17] };
const FALLBACK = [4, 14];

// Each curve's values at skill 1, 14 and 20.
export const ANCHORS = {
  A: [0.8, 1.9, 20], // aim acceleration limit (C1)
  D: [0.8, 1.9, 2.4], // aim damping (C2)
  B: [3, 1.6, 1], // cannon spread build-up scale (C3)
  S: [2, 1, 0], // extra sniper yaw error (C4)
};

// The grenade/sticky danger notifier's delay after first contact, chosen by
// the enemy skill for every recipient (allies included): below 14, 14..16,
// 17 and up.
export const DANGER_ROWS = [
  [1.1, 2.1],
  [0.6, 1.3],
  [0, 0.8],
];

export function clampSkill(value) {
  return Math.min(Math.max(value | 0, SKILL_MIN), SKILL_MAX);
}

// A curve at an integer skill: lo + t * (hi - lo) on the interval the skill
// falls in, each operation rounded to float32.
export function curve([a, b, c], skill) {
  const below = skill < 14;
  const lo = f32(below ? a : b);
  const hi = f32(below ? b : c);
  const t = f32(below ? (skill - 1) / 13 : (skill - 14) / 6);
  return f32(lo + f32(t * f32(hi - lo)));
}

export function skillCurves(skill) {
  return {
    A: curve(ANCHORS.A, skill),
    D: curve(ANCHORS.D, skill),
    B: curve(ANCHORS.B, skill),
    S: curve(ANCHORS.S, skill),
  };
}

// A mission script's number argument as its wrapper reads it: float32,
// truncated toward zero.
function luaInt(value) {
  return Math.trunc(f32(Number(value) || 0));
}

// The 2004 game's float range draw: lo itself (and no draw) when the ends
// are equal, else lo + U * (hi - lo) in float32.
export function drawRange(random, lo, hi) {
  lo = f32(lo);
  hi = f32(hi);
  if (lo === hi) return lo;
  const u = random.uniform();
  return f32(f32(u * f32(hi - lo)) + lo);
}

const SETTERS = {
  SetPlayerTeamDifficulty: (d, n) => d.setPlayer(n),
  SetEnemyTeamDifficulty: (d, n) => d.setEnemy(n),
  SetPlayerTeamDifficultyEasy: (d, n) => d.addPlayer(EASY, n),
  SetPlayerTeamDifficultyMedium: (d, n) => d.addPlayer(MEDIUM, n),
  SetPlayerTeamDifficultyHard: (d, n) => d.addPlayer(HARD, n),
  SetEnemyTeamDifficultyEasy: (d, n) => d.addEnemy(EASY, n),
  SetEnemyTeamDifficultyMedium: (d, n) => d.addEnemy(MEDIUM, n),
  SetEnemyTeamDifficultyHard: (d, n) => d.addEnemy(HARD, n),
};

export class ClassicDifficulty {
  // tier: the selected tier (the profile's offline; the server's in a
  //   network game). It picks the base skills and the scheduler numbers.
  // profileTier: the local profile's tier, which the tier-conditional
  //   mission setters test even in a network game. Defaults to tier.
  // playerTeam / secondPlayerTeam: the local players' teams (null: none).
  // network: a network game (everyone uses the enemy curves).
  // override: the skill-override guard; initialization then keeps `skills`.
  constructor({
    tier = MEDIUM,
    profileTier = tier,
    playerTeam = null,
    secondPlayerTeam = null,
    network = false,
    override = false,
    skills = { player: FALLBACK[0], enemy: FALLBACK[1] },
  } = {}) {
    this.tier = tier;
    this.profileTier = profileTier;
    this.playerTeam = playerTeam;
    this.secondPlayerTeam = secondPlayerTeam;
    this.network = network;
    this.override = override;
    this.playerSkill = clampSkill(skills.player);
    this.enemySkill = clampSkill(skills.enemy);
    this.player = null;
    this.enemy = null;
    this.init(0, 0);
  }

  // The tier initialization: base skills plus offsets, clamped, then the
  // curves. With the override set only the curves are recomputed.
  init(playerOffset = 0, enemyOffset = 0) {
    if (!this.override) {
      const [player, enemy] = BASE[this.tier] || FALLBACK;
      this.playerSkill = clampSkill(player + (playerOffset | 0));
      this.enemySkill = clampSkill(enemy + (enemyOffset | 0));
    }
    this.refresh();
    return this;
  }

  // Yavin's Advanced Training bonus: the tier's skills again, player +4.
  advancedTraining() {
    return this.init(4, 0);
  }

  refresh() {
    this.enemy = skillCurves(this.enemySkill);
    this.player = skillCurves(this.playerSkill);
  }

  setPlayer(value) {
    this.playerSkill = clampSkill(luaInt(value));
    this.refresh();
  }

  setEnemy(value) {
    this.enemySkill = clampSkill(luaInt(value));
    this.refresh();
  }

  addPlayer(tier, value) {
    if (this.profileTier !== tier) return;
    this.playerSkill = clampSkill(this.playerSkill + luaInt(value));
    this.refresh();
  }

  addEnemy(tier, value) {
    if (this.profileTier !== tier) return;
    this.enemySkill = clampSkill(this.enemySkill + luaInt(value));
    this.refresh();
  }

  // Mission calls in source order: [{name, args}] (ai-mission.json) or
  // [[name, args]]. Calls that are not difficulty setters are skipped.
  apply(calls) {
    for (const call of calls || []) {
      const name = Array.isArray(call) ? call[0] : call.name;
      const args = (Array.isArray(call) ? call[1] : call.args) || [];
      const setter = SETTERS[name];
      if (setter && args.length) setter(this, args[0]);
    }
    return this;
  }

  // The AI aiming filter's team test: the player's curves for the AI on the
  // local player's team, offline; the enemy curves for everyone else.
  isPlayerTeam(team) {
    if (this.network || this.playerTeam == null) return false;
    if (this.secondPlayerTeam != null && this.secondPlayerTeam !== this.playerTeam) return false;
    return team === this.playerTeam;
  }

  skill(team) {
    return this.isPlayerTeam(team) ? this.playerSkill : this.enemySkill;
  }

  // { A, D, B, S } float32 for a unit of this team.
  curves(team) {
    return this.isPlayerTeam(team) ? this.player : this.enemy;
  }

  // Hard runs the expensive per-unit work more often (update calls, not
  // seconds): the perception cycle, the deferred line-of-sight budget per
  // update, and the vehicle notification bucket multiplier.
  get hard() {
    return this.tier === HARD;
  }

  get cycle() {
    return this.hard ? 3 : 15;
  }

  get losBudget() {
    return this.hard ? 12 : 6;
  }

  get vehicleBucket() {
    return this.hard ? 10 : 2;
  }

  // The danger notifier's [lo, hi) row for the current enemy skill.
  dangerRow() {
    const k = this.enemySkill;
    return DANGER_ROWS[k < 17 ? (k < 14 ? 0 : 1) : 2];
  }

  // A fresh delay (seconds after first contact) for one notifier invocation.
  dangerDelay(random) {
    const [lo, hi] = this.dangerRow();
    return drawRange(random, lo, hi);
  }
}

// The Classic entry point binds these by name (world.difficulty is a
// ClassicDifficulty). Undefined when no difficulty is set.
export function curvesFor(difficulty, team) {
  return difficulty?.curves ? difficulty.curves(team) : undefined;
}

export function isHard(difficulty) {
  return difficulty instanceof ClassicDifficulty ? difficulty.hard : undefined;
}
