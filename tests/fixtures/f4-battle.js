// Synthetic native ownership uses the ordinary scoreboard facades. No models
// or game content are needed to construct their generated rows.
import { Battle, Character } from '../../src/battle.js';
export function battleFixture(posts) {
  const battle = new Battle({posts,teams:{1:{reinforcements:100},2:{reinforcements:100}},own:1,foe:2});
  battle.characters.push(new Character(0,1,false),new Character(1,2,false));
  return battle;
}
