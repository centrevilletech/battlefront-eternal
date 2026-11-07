import { F4Roster } from '../sim/wasm/f4-roster.js';
import { MAX_TEAM_SIZE } from '../battle-limits.js';
// Network connections reserve army slots, replacing bots without spending a
// reinforcement. Character array positions remain stable for replication.
export function createMultiplayerRoster(game,armySize=MAX_TEAM_SIZE) {
  return new F4Roster(game.tailOwner,game,armySize);
}
