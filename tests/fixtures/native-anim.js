import './native-render-descriptors.js';
import { setAnimKernel } from '../../src/anim.js';
import { loadAnimKernel } from '../../src/sim/wasm/anim-kernel.js';

// Synthetic actors need the same prepared sampler as loaded battles.
setAnimKernel(await loadAnimKernel());
