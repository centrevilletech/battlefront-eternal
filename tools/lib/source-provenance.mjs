// Farm/source archives need not carry a repository's object database. Their
// caller can supply recorded checkout metadata; ordinary runs read Git.
// This affects reports only, never replay inputs or the simulation instance.
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
let recorded;
function metadata() {
  if (!process.env.BFE_SOURCE_METADATA) return null;
  if (!recorded) {
    recorded = JSON.parse(readFileSync(process.env.BFE_SOURCE_METADATA, 'utf8'));
    if (!/^[a-f0-9]{40}$/.test(recorded.revision) || typeof recorded.dirty !== 'boolean'
      || !Array.isArray(recorded.runtimeFiles)) throw Error('Invalid recorded source metadata');
  }
  return recorded;
}
export function sourceState() {
  const saved = metadata();
  return saved ? { revision: saved.revision, dirty: saved.dirty } : {
    revision: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(),
    dirty: Boolean(execFileSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).trim()),
  };
}
export function runtimeSourceFiles() {
  const saved = metadata();
  return saved ? saved.runtimeFiles : execFileSync('git', ['ls-files', '-co', '--exclude-standard', '-z', 'src', 'package-lock.json'], { encoding: 'utf8' }).split('\0').filter(Boolean);
}
