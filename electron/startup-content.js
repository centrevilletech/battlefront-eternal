// Content selection is shared by boot and the loader's Play action. Imported
// checkpoints belong to another machine, so freshness cannot depend on its path.
import { preparedContentCurrent, preparedFor, readPrepareState } from './prepare.js';

export async function startupContent({ chosen, managed, hasGameData, findInstall, findContent, installedAddOns, readState = readPrepareState }) {
  if (chosen && hasGameData(chosen)) {
    if (preparedContentCurrent(await readState(chosen))) return { mode: 'menu', content: chosen };
    const install = await findInstall();
    return { mode: install ? 'prepare' : 'outdated', content: chosen, install, selected: true };
  }
  const install = await findInstall();
  if (install) {
    const state = await readState(managed);
    return { mode: hasGameData(managed) && preparedFor(state, install.dir, await installedAddOns(install.dir)) ? 'menu' : 'prepare', content: managed, install };
  }
  const content = hasGameData(managed) ? managed : (await findContent())?.dir;
  if (!content) return { mode: 'missing' };
  return { mode: preparedContentCurrent(await readState(content)) ? 'menu' : 'outdated', content };
}
