// electron-builder afterPack and afterSign hook: records the packaged app's
// own files (path, size, SHA-256) in resources/bfe-tree.json. The updater
// reuses every unchanged file of an installed build by it instead of
// reading them all again (electron/update/install.js). Signing rewrites
// binaries, so the index is written again after it; build/update-payload.mjs
// refuses a tree whose files no longer match it.
const { writeFile } = require("node:fs/promises");
const path = require("node:path");

exports.default = async function treeIndex(context) {
  // A signed macOS bundle cannot gain a file afterwards.
  if (context.electronPlatformName === "darwin") return;
  const { TREE_INDEX, walkTree } = await import("../electron/update/tree.js");
  const { files } = await walkTree(context.appOutDir, { exclude: (name) => name === TREE_INDEX });
  const index = { schema: 1, files: files.map(({ path: name, size, sha256 }) => ({ path: name, size, sha256 })) };
  await writeFile(path.join(context.appOutDir, ...TREE_INDEX.split("/")), JSON.stringify(index) + "\n");
};
