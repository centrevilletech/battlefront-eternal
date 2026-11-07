// Cold input transport for native local class admission. Keep the roster as
// authored: silently changing the requested body would hide an invalid URL.
import { bridgeFor } from '../sim/runtime.js';
import { F4StartupClassLayout, putF4StartupClass } from '../sim/gen/f4.js';
const INPUT = { words: F4StartupClassLayout.words, put: putF4StartupClass };

export function validateStartupClass(instance, policy, mission) {
  const bridge = bridgeFor(instance);
  const rosters = [0, 1, 2].map(team => ({ classes: (mission?.teams?.[team]?.classes || []).map(entry => entry[0]) }));
  bridge.callArgs(INPUT, { className: policy.className, team: policy.team, rosters });
  if (!bridge.x.f4_startup_class()) {
    throw Error(`${policy.className} is not available in this mission. Choose a class from its spawn screen.`);
  }
}
