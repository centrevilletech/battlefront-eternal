// Release versions: package.json's 0.1.0-preview.262 and the Debian form
// 0.1.0~preview.262 name the same build. A release without a preview number
// sorts after every preview of the same base version.
const PATTERN = /^(\d+)\.(\d+)\.(\d+)(?:[-~]preview\.(\d+))?$/;

export function parseVersion(value) {
  const match = PATTERN.exec(String(value ?? ""));
  return match ? [Number(match[1]), Number(match[2]), Number(match[3]), match[4] === undefined ? Infinity : Number(match[4])] : null;
}

export function compareVersions(a, b) {
  const x = parseVersion(a), y = parseVersion(b);
  if (!x || !y) throw Error(`Not a release version: ${x ? b : a}`);
  for (let i = 0; i < x.length; i++) if (x[i] !== y[i]) return x[i] < y[i] ? -1 : 1;
  return 0;
}

export const isNewer = (a, b) => compareVersions(a, b) > 0;
