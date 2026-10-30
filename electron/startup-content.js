// Content selection is shared by boot and the loader's Play action. Imported
// checkpoints belong to another machine, so freshness cannot depend on its path.
//
// Preparing from a copy the player hasn't confirmed asks first ('confirm':
// the loader shows the copy, its check and the choices). A copy confirmed
// before (confirmed: the player's gameDir) or already prepared into this
// folder prepares again without asking (an update's new generations, a run
// cut short), with the choices saved then (wanted).
import { preparedContentCurrent, preparedFor, preparedFrom, readPrepareState } from './prepare.js';

export async function startupContent({ chosen, managed, hasGameData, findInstall, findContent, installedAddOns, readState = readPrepareState, confirmed = () => true, wanted = {} }) {
  if (chosen && hasGameData(chosen)) {
    const state = await readState(chosen);
    if (preparedContentCurrent(state)) return { mode: 'menu', content: chosen };
    const install = await findInstall();
    const known = install && (confirmed(install) || preparedFrom(state, install.dir));
    return { mode: install ? (known ? 'prepare' : 'confirm') : 'outdated', content: chosen, install, selected: true };
  }
  const install = await findInstall();
  if (install) {
    const state = await readState(managed);
    if (hasGameData(managed) && preparedFor(state, install.dir, await installedAddOns(install.dir), wanted)) return { mode: 'menu', content: managed };
    const known = confirmed(install) || preparedFrom(state, install.dir);
    return { mode: known ? 'prepare' : 'confirm', content: managed, install };
  }
  const content = hasGameData(managed) ? managed : (await findContent())?.dir;
  if (!content) return { mode: 'missing' };
  return { mode: preparedContentCurrent(await readState(content)) ? 'menu' : 'outdated', content };
}
