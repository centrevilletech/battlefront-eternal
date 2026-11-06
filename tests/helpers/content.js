// Optional game-file fixtures share the same root as the game and test runner.
// Resolve relative overrides from the caller's working directory; public is
// anchored to the repository so a test's location never changes its fallback.
import { resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export function contentURL(relative = '') {
  const root = process.env.BFE_CONTENT_DIR;
  const base = root ? pathToFileURL(resolve(root) + sep) : new URL('../../public/', import.meta.url);
  return new URL(relative, base);
}

export function contentPath(relative = '') {
  return fileURLToPath(contentURL(relative));
}
