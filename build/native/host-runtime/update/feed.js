// The release feed: one signed file per channel naming the newest build and,
// per platform, the hash of its file manifest. Only keys shipped with the
// app (keys.js) can sign it; HTTPS carries it but is not what is trusted.
import { createHash, createPublicKey, verify } from "node:crypto";
import { zstdDecompressSync } from "node:zlib";
import { safeRelative, validateTree } from "./tree.js";
import { parseVersion } from "./version.js";

export const FEED = "https://get.battlefronteternal.com/downloads/";
const MAX_ENVELOPE = 1 << 20, MAX_TREE = 32 << 20, MAX_TREE_JSON = 256 << 20;

// The body, refusing more than `limit` bytes however the server misbehaves.
export async function readBody(response, limit, onChunk) {
  const reader = response.body?.getReader();
  if (!reader) return Buffer.alloc(0);
  const chunks = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > limit) {
      await reader.cancel().catch(() => {});
      throw Error("Update server sent more than expected");
    }
    chunks.push(Buffer.from(value.buffer, value.byteOffset, value.byteLength));
    onChunk?.(value.byteLength);
  }
  return Buffer.concat(chunks, total);
}

// A signing key's id: the start of the SHA-256 of its SubjectPublicKeyInfo.
export const keyId = (spki) => createHash("sha256").update(spki).digest("hex").slice(0, 16);

export function openEnvelope(envelope, keys) {
  const key = typeof envelope?.key === "string" && Object.hasOwn(keys, envelope.key) ? keys[envelope.key] : null;
  if (!key || typeof envelope.payload !== "string" || typeof envelope.signature !== "string") throw Error("The update feed is not signed by a trusted key");
  const payload = Buffer.from(envelope.payload, "base64"), signature = Buffer.from(envelope.signature, "base64");
  const publicKey = createPublicKey({ key: Buffer.from(key, "base64"), format: "der", type: "spki" });
  if (signature.length !== 64 || !verify(null, payload, publicKey, signature)) throw Error("The update feed signature is invalid");
  const release = JSON.parse(payload.toString("utf8"));
  if (release?.schema !== 1 || typeof release.channel !== "string" || !parseVersion(release.version) || typeof release.release_id !== "string"
    || !/^[A-Za-z0-9._-]{1,64}$/.test(release.release_id) || !release.platforms || typeof release.platforms !== "object") throw Error("The update feed is malformed");
  for (const [platform, entry] of Object.entries(release.platforms)) {
    if (!/^[a-z0-9]+-[a-z0-9]+$/.test(platform) || !/^[a-f0-9]{64}$/.test(entry?.sha256) || !safeRelative(entry.manifest)) throw Error("The update feed is malformed");
  }
  return release;
}

// The channel's newest release, verified; null when the channel has none.
export async function fetchRelease({ channel, keys, fetch, base = FEED, signal }) {
  const response = await fetch(new URL(`updates/${encodeURIComponent(channel)}.json`, base).href, { signal, cache: "no-store" });
  if (response.status === 404) return null;
  if (!response.ok) throw Error(`The update feed answered ${response.status}`);
  const release = openEnvelope(JSON.parse((await readBody(response, MAX_ENVELOPE)).toString("utf8")), keys);
  if (release.channel !== channel) throw Error("The update feed is for another channel");
  return release;
}

// The file manifest of `release` for `platform`, matched against the hash
// the signed release names; null when the release has no build for it.
export async function fetchTree({ release, platform, fetch, base = FEED, signal }) {
  const entry = release.platforms[platform];
  if (!entry) return null;
  const response = await fetch(new URL(entry.manifest, base).href, { signal, cache: "no-store" });
  if (!response.ok) throw Error(`The update manifest answered ${response.status}`);
  const bytes = await readBody(response, MAX_TREE);
  if (createHash("sha256").update(bytes).digest("hex") !== entry.sha256) throw Error("The update manifest does not match the signed release");
  return validateTree(JSON.parse(zstdDecompressSync(bytes, { maxOutputLength: MAX_TREE_JSON }).toString("utf8")), { platform, version: release.version });
}
