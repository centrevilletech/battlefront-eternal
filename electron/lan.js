import dgram from 'node:dgram';
import { randomBytes } from 'node:crypto';
import { networkInterfaces } from 'node:os';
import { isIP } from 'node:net';

const MAGIC = 'BFE-LAN/1', GROUP = 'ff02::bf:1';
export const LAN_PORT = 18452;
function parse(bytes) {
  if (bytes.length > 2048) return null;
  try { const value = JSON.parse(bytes); return value.magic === MAGIC && /^[a-f0-9]{32}$/.test(value.nonce) ? value : null; } catch { return null; }
}
function ipv6Interfaces() {
  return Object.entries(networkInterfaces()).filter(([, entries]) => entries?.some(entry => entry.family === 'IPv6' && !entry.internal)).map(([name]) => name);
}
export function lanTargets() {
  const targets = new Set(['255.255.255.255']);
  for (const entries of Object.values(networkInterfaces())) for (const entry of entries || []) {
    if (entry.family !== 'IPv4' || entry.internal) continue;
    const address = entry.address.split('.').map(Number), mask = entry.netmask.split('.').map(Number);
    targets.add(address.map((n, i) => n | (255 ^ mask[i])).join('.'));
  }
  for (const name of ipv6Interfaces()) targets.add(`${GROUP}%${name}`);
  return [...targets];
}
async function bind(socket, port, address) {
  await new Promise((resolve, reject) => { socket.once('error', reject); socket.bind({ port, address, ipv6Only: true }, () => { socket.off('error', reject); resolve(); }); });
  socket.on('error', () => {});
}
const close = socket => new Promise(resolve => { try { socket.close(resolve); } catch { resolve(); } });
export async function startLanDiscovery({ port = LAN_PORT, host = '0.0.0.0', getInfo }) {
  const sockets = [], recent = new Map();
  const families = isIP(host) === 6 ? [['udp6', host]] : [['udp4', host], ['udp6', host === '127.0.0.1' ? '::1' : '::']];
  for (const [family, address] of families) {
    const socket = dgram.createSocket({ type: family, reuseAddr: true });
    try { await bind(socket, port, address); } catch (error) { await close(socket); if (!sockets.length) throw error; continue; }
    port = socket.address().port; sockets.push(socket);
    if (family === 'udp6') for (const name of ipv6Interfaces()) { try { socket.addMembership(GROUP, `::%${name}`); } catch {} }
    socket.on('message', (bytes, remote) => {
      const message = parse(bytes), now = Date.now();
      if (!message || message.type !== 'discover') return;
      if (now - (recent.get(remote.address) || 0) < 250) return;
      if (recent.size >= 1024) for (const [key, time] of recent) if (now - time > 1000) recent.delete(key);
      if (recent.size >= 1024 && !recent.has(remote.address)) return;
      recent.set(remote.address, now);
      const info = getInfo(); if (!info) return;
      const data = Buffer.from(JSON.stringify({ magic: MAGIC, type: 'server', nonce: message.nonce, info }));
      if (data.length <= 2048) socket.send(data, remote.port, remote.address, () => {});
    });
  }
  return { port, close: () => Promise.all(sockets.map(close)) };
}
export async function discoverLan({ port = LAN_PORT, timeout = 750, targets = lanTargets() } = {}) {
  if (!Number.isInteger(port) || port < 1 || port > 65535 || !Number.isFinite(timeout) || timeout < 1 || timeout > 5000 || targets.length > 64 || !targets.every(value => isIP(value))) throw Error('Invalid discovery options');
  const sockets = [], nonce = randomBytes(16).toString('hex'), found = new Map();
  const request = Buffer.from(JSON.stringify({ magic: MAGIC, type: 'discover', nonce }));
  for (const [family, address, version] of [['udp4', '0.0.0.0', 4], ['udp6', '::', 6]]) {
    const destinations = targets.filter(target => isIP(target) === version); if (!destinations.length) continue;
    const socket = dgram.createSocket(family);
    try { await bind(socket, 0, address); } catch (error) { await close(socket); if (!sockets.length && version === 6) throw error; continue; }
    sockets.push(socket); if (version === 4) socket.setBroadcast(true); else socket.setMulticastTTL(1);
    socket.on('message', (bytes, remote) => {
      const message = parse(bytes), info = message?.info;
      if (message?.type !== 'server' || message.nonce !== nonce || !info || !/^[a-f0-9]{64}$/.test(info.id) || typeof info.name !== 'string' || info.name.length > 80 || !Number.isInteger(info.port) || info.port < 1 || info.port > 65535 || found.size >= 256) return;
      // Keep both families available, preferring IPv4 when the host answers both.
      if (!found.has(info.id) || remote.family === 'IPv4') found.set(info.id, { ...info, address: remote.address });
    });
    for (const destination of destinations) socket.send(request, port, destination, () => {});
  }
  await new Promise(resolve => setTimeout(resolve, timeout));
  await Promise.all(sockets.map(close));
  return [...found.values()];
}
