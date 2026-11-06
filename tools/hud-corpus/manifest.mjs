// Four representative battlefields, both armies, in Classic and Enhanced.
// Shared renderer presets keep MSAA, resolution and scene settings immutable.
import { readFileSync } from 'node:fs';
import { expand } from '../renderer-corpus/manifest.mjs';
const renderer = expand(JSON.parse(readFileSync(new URL('../renderer-corpus/manifest.json', import.meta.url))));
export const states = ['first-person', 'health-ammo-messages', 'scope-lock-hit', 'vehicle-seat', 'expanded-map', 'death', 'spawn-layer', 'covered'];
export const cases = [];
for (const preset of ['classic', 'balanced']) for (const map of ['nab2', 'tat1', 'bes1', 'hot1']) {
  const base = renderer.cases.find(c => c.id === `${preset}-${map}`);
  const armies = map === 'hot1' ? [['imp', 'imp_inf_storm_troopersnow'], ['all', 'all_inf_soldiersnow']] : [['rep', 'rep_inf_clone_trooper'], ['cis', 'cis_inf_battledroid']];
  for (const [side, unit] of armies) {
    const query = new URLSearchParams(base.query); query.set('side', side); query.set('unit', unit);
    cases.push({ id: `${preset}-${map}-${side}`, settings: base.settings, query: String(query) });
  }
}
