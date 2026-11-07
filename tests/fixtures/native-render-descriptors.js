// Pure cold descriptor exports need no drawing context or content files.
import {readFileSync} from 'node:fs';
import {initSync} from '../../src/rust-pkg/bfe_render.js';
initSync({module:readFileSync(new URL('../../src/rust-pkg/bfe_render_bg.wasm',import.meta.url))});
