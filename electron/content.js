// Serves the game to the renderer over app://bfe/. The engine is the Vite
// build shipped inside the app; the extracted game data (maps, sides, common
// and audio, the folders tools/extract.py writes) lives outside it, in the
// player's own game data folder. Retail data is never packaged.
import { createReadStream, promises as fs } from "node:fs";
import path from "node:path";
import { Readable } from "node:stream";

export const SCHEME = "app";
export const HOST = "bfe";
export const ORIGIN = `${SCHEME}://${HOST}`;

// The top-level folders of the extractor's output (tools/extract.py).
export const CONTENT_ROOTS = ["maps", "sides", "common", "audio"];

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
  ".ico": "image/x-icon",
  ".ktx2": "image/ktx2",
  ".wav": "audio/wav",
  ".ogg": "audio/ogg",
  ".mp3": "audio/mpeg",
  ".opus": "audio/ogg",
  ".flac": "audio/flac",
  ".mp4": "video/mp4",
  ".webm": "video/webm",
  ".bik": "application/octet-stream",
  ".bin": "application/octet-stream",
  ".ttf": "font/ttf",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
};

// The inline ?noreload script in index.html needs 'unsafe-inline'. The KTX2
// transcoder (three's basis_transcoder, Emscripten) needs 'wasm-unsafe-eval'
// and 'unsafe-eval': embind builds its call wrappers with new Function, in
// the decoding workers, which inherit this policy. Without it every KTX2
// decode throws and no texture arrives. The renderer is sandboxed, loads
// nothing from the network, and evaluates no untrusted strings.
export const CSP = [
  "default-src 'self'",
  "script-src 'self' 'unsafe-inline' 'unsafe-eval' 'wasm-unsafe-eval'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "media-src 'self' data: blob:",
  "font-src 'self' data:",
  "worker-src 'self' blob:",
  "connect-src 'self' data: blob:",
  "object-src 'none'",
  "base-uri 'self'",
  "frame-ancestors 'none'",
].join("; ");

// Resolves a URL path below root, or null when it would escape it.
export function resolveBelow(root, urlPath) {
  let decoded;
  try {
    decoded = decodeURIComponent(urlPath);
  } catch {
    return null;
  }
  if (decoded.includes("\0")) return null;
  const file = path.resolve(root, "." + path.posix.normalize("/" + decoded));
  const base = path.resolve(root);
  return file === base || file.startsWith(base + path.sep) ? file : null;
}

// The desktop app's own pages (electron/ui), such as the first-run loader.
export const DESKTOP_PREFIX = "__desktop";

// Maps an app://bfe/ path to a file: content roots go to the game data
// folder, /__desktop/ to the app's pages, everything else to the engine build.
export function locate(urlPath, { appRoot, contentRoot, uiRoot }) {
  const first = urlPath.split("/")[1] || "";
  if (CONTENT_ROOTS.includes(first)) return contentRoot ? resolveBelow(contentRoot, urlPath) : null;
  if (first === DESKTOP_PREFIX) return uiRoot ? resolveBelow(uiRoot, urlPath.slice(DESKTOP_PREFIX.length + 1)) : null;
  return resolveBelow(appRoot, urlPath === "/" ? "/index.html" : urlPath);
}

// "bytes=a-b", "bytes=a-" or "bytes=-n" against a file of size bytes.
export function parseRange(header, size) {
  const match = /^bytes=(\d*)-(\d*)$/.exec(String(header || "").trim());
  if (!match || (match[1] === "" && match[2] === "")) return null;
  let start;
  let end;
  if (match[1] === "") {
    start = Math.max(0, size - Number(match[2]));
    end = size - 1;
  } else {
    start = Number(match[1]);
    end = match[2] === "" ? size - 1 : Math.min(Number(match[2]), size - 1);
  }
  if (start > end || start >= size) return "unsatisfiable";
  return { start, end };
}

// Cross-origin isolation: the page may then hand the world worker shared
// memory (src/runtime/worker-host.js). Every resource is app://bfe/, so
// require-corp blocks nothing; the dev server sets the same headers.
export const ISOLATION_HEADERS = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

function headers(file, extra = {}) {
  return {
    "Content-Type": TYPES[path.extname(file).toLowerCase()] || "application/octet-stream",
    "Content-Security-Policy": CSP,
    ...ISOLATION_HEADERS,
    "X-Content-Type-Options": "nosniff",
    "Cache-Control": "no-cache",
    "Accept-Ranges": "bytes",
    ...extra,
  };
}

const notFound = () => new Response("Not found", { status: 404, headers: { "Content-Type": "text/plain" } });

// The protocol.handle callback: (Request) => Response.
export function createHandler(roots) {
  return async (request) => {
    const url = new URL(request.url);
    if (url.host !== HOST) return notFound();
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("Method not allowed", { status: 405 });
    }
    const file = locate(url.pathname, roots());
    if (!file) return notFound();
    let stat;
    try {
      stat = await fs.stat(file);
    } catch {
      return notFound();
    }
    if (!stat.isFile()) return notFound();

    const range = request.headers.get("range") ? parseRange(request.headers.get("range"), stat.size) : null;
    if (range === "unsatisfiable") {
      return new Response(null, { status: 416, headers: headers(file, { "Content-Range": `bytes */${stat.size}` }) });
    }
    const head = request.method === "HEAD";
    if (range) {
      const length = range.end - range.start + 1;
      const body = head ? null : Readable.toWeb(createReadStream(file, { start: range.start, end: range.end }));
      return new Response(body, {
        status: 206,
        headers: headers(file, {
          "Content-Length": String(length),
          "Content-Range": `bytes ${range.start}-${range.end}/${stat.size}`,
        }),
      });
    }
    const body = head || stat.size === 0 ? null : Readable.toWeb(createReadStream(file));
    return new Response(body, { status: 200, headers: headers(file, { "Content-Length": String(stat.size) }) });
  };
}
