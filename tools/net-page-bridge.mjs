// Test and tool harness: gives a plain Chromium page (Playwright) the
// desktop network bridge. Requests go to electron/network.js in Node, and
// each battle's MessagePort is backed by the real network service
// (electron/net-service.js, native UDP) over a loopback WebSocket, so a page
// without Electron's preload still joins exactly as the desktop app does.
// Binary payloads cross as base64; this costs time, so measure latency with
// actual Electron or tools/net-probe.mjs, not through this bridge.
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { WebSocketServer } from 'ws';
import { createNetService } from '../electron/net-service.js';
import { loadNetworkIdentity } from '../electron/network.js';

const encode = value => JSON.stringify(value, (key, item) => item instanceof ArrayBuffer ? { $b: Buffer.from(item).toString('base64') } : ArrayBuffer.isView(item) ? { $b: Buffer.from(item.buffer, item.byteOffset, item.byteLength).toString('base64') } : item);
const decode = text => JSON.parse(text, (key, item) => item && typeof item === 'object' && typeof item.$b === 'string' && Object.keys(item).length === 1 ? (bytes => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength))(Buffer.from(item.$b, 'base64')) : item);

// One loopback WebSocket server per harness; `directory` holds the
// identity (the same file createNetworkBridge uses for that client). Only
// pages this harness attached, holding its random token, may connect, and
// only to the directories they were attached with: any local process could
// otherwise open sessions as any identity on disk. Never ship or expose it.
export async function startPageNetwork() {
  const services = new Map(), allowed = new Set(), token = randomBytes(16).toString('hex');
  const server = new WebSocketServer({ host: '127.0.0.1', port: 0, maxPayload: 4 << 20 });
  await new Promise(done => server.once('listening', done));
  server.on('connection', (socket, request) => {
    const params = new URL(request.url, 'http://bridge').searchParams, directory = resolve(params.get('identity') ?? '');
    if (params.get('token') !== token || !allowed.has(directory)) { socket.close(1008, 'not attached'); return; }
    let service = services.get(directory);
    // Loopback stands in for public addresses in these tests.
    if (!service) { service = createNetService({ identity: loadNetworkIdentity(directory), candidateFilter: list => list }); services.set(directory, service); }
    let handler = null, closeHandler = null;
    service.serve({
      post: message => { if (socket.readyState === socket.OPEN) socket.send(encode(message)); },
      onMessage: fn => { handler = fn; }, onClose: fn => { closeHandler = fn; }, close: () => socket.close(),
    });
    socket.on('message', raw => { try { handler?.(decode(raw.toString())); } catch {} });
    socket.on('close', () => closeHandler?.());
  });
  return {
    port: server.address().port,
    // Adds bfeDesktop.network.openSession to a page whose bridge requests
    // already reach `directory`'s createNetworkBridge.
    async attach(page, directory) {
      directory = resolve(directory); allowed.add(directory);
      await page.addInitScript(({ port, directory, token }) => {
        const base64 = buffer => { const bytes = new Uint8Array(buffer); let text = ''; for (let i = 0; i < bytes.length; i += 8192) text += String.fromCharCode(...bytes.subarray(i, i + 8192)); return btoa(text); };
        const encode = value => JSON.stringify(value, (key, item) => item instanceof ArrayBuffer ? { $b: base64(item) } : item);
        const decode = text => JSON.parse(text, (key, item) => item && typeof item === 'object' && typeof item.$b === 'string' && Object.keys(item).length === 1 ? Uint8Array.from(atob(item.$b), c => c.charCodeAt(0)).buffer : item);
        window.bfeTestOpenSession = id => {
          const socket = new WebSocket(`ws://127.0.0.1:${port}/?token=${token}&identity=${encodeURIComponent(directory)}`), { port1, port2 } = new MessageChannel(), queue = [];
          port1.onmessage = event => { const text = encode(event.data); if (socket.readyState === 1) socket.send(text); else queue.push(text); };
          socket.onopen = () => { for (const text of queue.splice(0)) socket.send(text); };
          socket.onmessage = event => port1.postMessage(decode(event.data));
          socket.onclose = () => port1.close();
          window.postMessage({ type: 'bfe:net-port', id }, '*', [port2]);
        };
      }, { port: this.port, directory, token });
    },
    close() { for (const service of services.values()) service.close(); server.close(); },
  };
}
