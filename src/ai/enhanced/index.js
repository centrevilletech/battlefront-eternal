// The Enhanced AI (docs/ai-enhanced.md): the Classic AI's machinery with the
// decisions players asked to change. It runs the same controllers,
// navigation, perception, weapons and vehicle code as Classic (src/ai/classic)
// and replaces, through the Classic entry point's own extension options:
//
//   difficulty.js  one skill for both teams; faster decisions on Normal and Hard
//   objectives.js  posts drawn by nearness and need, defense of posts under
//                  attack, spawning at the post nearest the objective
//   agents.js      objective agents that keep going for their post
//   aim.js         reaction, wandering error and the player's spread rules
//
// Classic itself is untouched: `?ai=classic` still runs the recovered 2004
// behavior. src/ai/index.js picks between them.

import { ENHANCED_AGENTS } from "./agents.js";
import { enhancedCombat } from "./aim.js";

export { EnhancedDifficulty, ENHANCED_SKILL, ENHANCED_SCHEDULE } from "./difficulty.js";
export { EnhancedObjectives, EnhancedSpawning, ENHANCED_OBJECTIVES } from "./objectives.js";
export { ENHANCED_AGENTS, ENHANCED_FOCUS } from "./agents.js";
export { AIM_MODEL, AIM_PROFILES, EnhancedAimer, enhancedCombat } from "./aim.js";

// createClassicAI's options for an Enhanced match.
export function enhancedOptions(world, { tier = 2, seed = 0 } = {}) {
  return { agents: ENHANCED_AGENTS, combat: enhancedCombat(world, { tier, seed }) };
}
