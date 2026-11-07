import fs from 'node:fs';
import init, { PageItems } from '../../src/presentation-pkg/bfe_presentation.js';

// The presentation module as the page loads it (src/presentation/effects.js
// loadPresentation), from the built files.
const wasm = await init({ module_or_path: fs.readFileSync(new URL('../../src/presentation-pkg/bfe_presentation_bg.wasm', import.meta.url)) });
export const loaded = { module: { PageItems }, memory: wasm.memory };
