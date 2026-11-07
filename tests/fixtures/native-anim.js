import './native-render-descriptors.js';
import { setAnimKernel } from '../../src/anim.js';
import { loadAnimKernel } from '../../src/anim-kernel.js';

// Synthetic actors need the same prepared sampler as loaded battles.
setAnimKernel(await loadAnimKernel());
