// The whole-build gate's cases. Foot cases reuse the renderer corpus (17 maps
// at Classic, Balanced and High, first person, HUD drawn); vehicle cases seat
// the player in a hull on the same maps, in the chase view and the cockpit;
// shell cases are the front end's own screens, drawn without a battle.
import fs from 'node:fs';
import { expand } from '../renderer-corpus/manifest.mjs';

const corpus = expand(JSON.parse(fs.readFileSync(new URL('../renderer-corpus/manifest.json', import.meta.url))));
const PRESETS = ['classic', 'balanced', 'high'];
const SHELL_SCREENS = ['main', 'sp', 'ia', 'options', 'general', 'video', 'sound', 'controls'];

export const gateManifest = { captures: corpus.captures, gap: corpus.gap, historyFrames: corpus.historyFrames, settleFrames: corpus.settleFrames || 2 };

export function gateCases() {
  const cases = [];
  const foot = corpus.cases.filter(c => PRESETS.includes(c.preset) && c.id === `${c.preset}-${c.query.match(/^map=(\w+)/)[1]}`);
  for (const c of foot) cases.push({ id: `foot-${c.id}`, kind: 'foot', map: c.query.match(/^map=(\w+)/)[1], preset: c.preset, query: c.query, settings: c.settings });
  // The chase view drops view=1st; the cockpit keeps it. The cockpit is checked at Balanced only.
  for (const c of foot) {
    const map = c.query.match(/^map=(\w+)/)[1];
    cases.push({ id: `vehicle-${c.preset}-${map}`, kind: 'vehicle', map, preset: c.preset, query: c.query.replace('&view=1st', ''), settings: c.settings });
    if (c.preset === 'balanced') cases.push({ id: `cockpit-${c.preset}-${map}`, kind: 'vehicle', map, preset: c.preset, query: c.query, settings: c.settings });
  }
  const shellSettings = corpus.presets.balanced;
  for (const screen of SHELL_SCREENS) cases.push({ id: `shell-${screen}`, kind: 'shell', screen, preset: 'balanced', query: '', settings: shellSettings });
  return cases;
}

export function gateCase(id) {
  const found = gateCases().find(c => c.id === id);
  if (!found) throw Error(`Unknown gate case ${id}`);
  return found;
}
