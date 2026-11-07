// Cold page resources have no AiWorld. Their targeting and HUD views still
// consume the native content parser, including authored mask ordering.
import {bridgeFor} from './sim/runtime.js';
import {registerWeaponClass} from './weapon-content-sim.js';
import {loadAIWeaponData} from './ai-data.js';
import {publishWeaponPolicies} from './weapon-policy.js';
import {AiWeaponPolicySourceLayout,putAiWeaponPolicySource,AiWeaponPolicyContentLayout,
  AiWeaponPolicyContentMirror,syncAiWeaponPolicyContent} from './sim/gen/ai-phase.js';

const SOURCE={words:AiWeaponPolicySourceLayout.words,put:putAiWeaponPolicySource};
const POLICIES={words:AiWeaponPolicyContentLayout.words,Mirror:AiWeaponPolicyContentMirror,sync:syncAiWeaponPolicyContent};
export function readWeaponPolicies(bridge,pointer) {
  if(!pointer)throw Error('Weapon policy content is invalid');
  const policies=bridge.callResult(POLICIES,pointer);
  for(const row of policies.weapons)publishWeaponPolicies(row.weapon,row.levels);
}

export function bindWeaponPolicyContent(bridge,weapons,data) {
  for(const weapon of weapons)registerWeaponClass(bridge,weapon);
  bridge.callArgs(SOURCE,{weapons,data:data.filter(Boolean).map(json=>({json}))});
  readWeaponPolicies(bridge,bridge.x.ai_weapon_policies());
}

export async function loadPageWeaponPolicies(instance,{sides,vehicles,props}) {
  const bridge=bridgeFor(instance),definitions=new Set();
  const add=weapon=>{
    if(!weapon||definitions.has(weapon))return;
    definitions.add(weapon);
    for(const child of weapon.droid?.weapons??[])add(child);
  };
  for(const side of Object.values(sides))for(const unit of Object.values(side.units))for(const weapon of unit.weapons??[])add(weapon);
  for(const hull of Object.values(vehicles??{}))for(const seat of hull.seats??[])for(const weapon of seat.weapons??[])add(weapon.def);
  for(const prop of props??[])add(prop.weapon);
  const weapons=[...definitions];
  const bytes=await loadAIWeaponData(Object.keys(sides));
  bindWeaponPolicyContent(bridge,weapons,bytes);
  return bytes;
}
