// Electron's fs reads a path ending in .asar as an archive to look inside;
// the updater copies, hashes and deletes app.asar as the plain file it is
// (original-fs, in Electron; plain fs in Node).
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
let fs;
try { fs = require("original-fs"); } catch { fs = require("node:fs"); }
export default fs;
