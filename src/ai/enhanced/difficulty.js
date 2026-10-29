// Enhanced difficulty (docs/ai-enhanced.md): one skill for every team.
//
// Classic gives the AI on the player's team one skill and every other team
// another (Easy 6/4, Medium 4/14, Hard 4/17), and mission scripts move them
// further apart. Enhanced follows a fairness rule instead: allies and
// opponents use the same tier, so the bots beside the player are as capable
// as the ones he fights. The mission's difficulty setters and the Advanced
// Training bonus are Classic asymmetry and are ignored here.
//
// The skill still feeds the Classic pieces Enhanced shares (the attack
// pattern table and the grenade warning delay). Aim no longer reads the
// skill curves: src/ai/enhanced/aim.js has its own profiles.

import { ClassicDifficulty, EASY, HARD, MEDIUM, clampSkill } from "../classic/difficulty.js";

// Every team's skill, by tier.
export const ENHANCED_SKILL = Object.freeze({ [EASY]: 8, [MEDIUM]: 14, [HARD]: 17 });

// How often each bot decides (in updates) and the line-of-sight tests per
// update, by tier. Classic runs Easy and Medium at 15/6 and Hard at 3/12;
// Enhanced steps between them so Normal bots react sooner than Classic
// Medium without Hard's whole cost on big battles.
export const ENHANCED_SCHEDULE = Object.freeze({
  [EASY]: { cycle: 15, losBudget: 6 },
  [MEDIUM]: { cycle: 10, losBudget: 9 },
  [HARD]: { cycle: 6, losBudget: 12 },
});

export class EnhancedDifficulty extends ClassicDifficulty {
  constructor({ tier = MEDIUM, playerTeam = null, network = false } = {}) {
    super({ tier, playerTeam, network });
  }

  init() {
    const skill = clampSkill(ENHANCED_SKILL[this.tier] ?? ENHANCED_SKILL[MEDIUM]);
    this.playerSkill = skill;
    this.enemySkill = skill;
    this.refresh();
    return this;
  }

  // The mission's skill calls make the teams unequal; Enhanced keeps them equal.
  apply() {
    return this;
  }

  advancedTraining() {
    return this;
  }

  get enhanced() {
    return true;
  }

  get cycle() {
    return (ENHANCED_SCHEDULE[this.tier] ?? ENHANCED_SCHEDULE[MEDIUM]).cycle;
  }

  get losBudget() {
    return (ENHANCED_SCHEDULE[this.tier] ?? ENHANCED_SCHEDULE[MEDIUM]).losBudget;
  }
}

export { EASY, MEDIUM, HARD };
