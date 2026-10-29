import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, statSync, mkdirSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createPublicKey, sign, verify } from 'node:crypto';
import { createNetworkBridge, loadNetworkIdentity } from '../electron/network.js';

test('desktop identity persists, stays out of the renderer, and signs nothing for it', async () => {
  const directory = mkdtempSync(path.join(tmpdir(), 'bfe-identity-'));
  try {
    const bridge = createNetworkBridge(directory, () => {});
    const keys = await bridge('identity');
    assert.equal('privateKey' in keys, false);
    assert.deepEqual(await createNetworkBridge(directory, () => {})('identity'), keys);
    if (process.platform !== 'win32') assert.equal(statSync(path.join(directory, 'network-identity.json')).mode & 0o777, 0o600);
    // The network process signs its own handshakes with the same identity.
    const full = loadNetworkIdentity(directory);
    assert.equal(full.publicKey, keys.publicKey);
    const signature = sign(null, Buffer.from('handshake'), full.privateKey);
    assert.ok(verify(null, Buffer.from('handshake'), createPublicKey({ key: Buffer.from(keys.publicKey, 'base64'), type: 'spki', format: 'der' }), signature));
    const transcript = JSON.stringify(['BFE/2', 'a'.repeat(64), 'b'.repeat(64), 'sha-256 ' + Array(32).fill('AB').join(':'), 'sha-256 ' + Array(32).fill('CD').join(':'), 2, 'bfe-sim-20']);
    await assert.rejects(bridge('proof', transcript), /Unknown network action/, 'the renderer can no longer obtain identity signatures');
    await assert.rejects(bridge('servers'), /privacy notice/);
    assert.equal(await bridge('accept', 'old-version'), false);
    assert.equal(await bridge('accept', '2026-10-11'), true);
    assert.equal((await createNetworkBridge(directory, () => {})('notice')).accepted, true);
    await assert.rejects(bridge('directInfo', { address: 'attacker.example', port: 80 }), /Invalid server address/);
    const probe={to:'a'.repeat(64),type:'matchProbe',payload:JSON.stringify({type:'matchProbe',matchProbe:'b'.repeat(32),nonce:'c'.repeat(32)})};
    assert.equal(await bridge('signal',probe),false,'a valid readiness message is admitted but needs an open signaling socket');
    await assert.rejects(bridge('signal',{...probe,payload:JSON.stringify({type:'matchProbe',matchProbe:'bad',nonce:'c'.repeat(32)})}),/Invalid match readiness probe/);
    for (const payload of [
      {type:'matchProbe',matchProbe:['b'.repeat(32)],nonce:'c'.repeat(32)},
      {type:'matchProbe',matchProbe:'b'.repeat(32),nonce:['c'.repeat(32)]},
      {type:'matchProbe',matchProbe:'b'.repeat(32),nonce:'c'.repeat(32),extra:true},
      {type:'matchProbe',matchProbe:'b'.repeat(32)},
      null,
    ]) await assert.rejects(bridge('signal',{...probe,payload:JSON.stringify(payload)}),/Invalid match readiness probe/);
    await assert.rejects(bridge('signal',{...probe,type:'matchReady'}),/Invalid signal/,'clients cannot impersonate server readiness replies');
  } finally { rmSync(directory, { recursive: true, force: true }); }
});


test('online consent is accepted only after a successful save, and can be retried', async () => {
  const directory = mkdtempSync(path.join(tmpdir(), 'bfe-consent-'));
  try {
    const bridge = createNetworkBridge(directory, () => {});
    const consentFile = path.join(directory, 'online-consent.json');
    mkdirSync(consentFile);
    await assert.rejects(bridge('accept', '2026-10-11'));
    assert.equal((await bridge('notice')).accepted, false);
    await assert.rejects(bridge('servers'), /privacy notice/);
    assert.equal(readdirSync(directory).some(name => name.endsWith('.tmp')), false);
    rmSync(consentFile, { recursive: true });
    assert.equal(await bridge('accept', '2026-10-11'), true);
    assert.equal((await createNetworkBridge(directory, () => {})('notice')).accepted, true);
    assert.equal(await bridge('accept', 'stale-version'), false);
    assert.equal((await bridge('notice')).accepted, true);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('a read the master never answered is tried once more, signed afresh; then it says why', async (t) => {
  const directory = mkdtempSync(path.join(tmpdir(), 'bfe-unreachable-'));
  const real = globalThis.fetch;
  t.after(() => { globalThis.fetch = real; rmSync(directory, { recursive: true, force: true }); });
  const bridge = createNetworkBridge(directory, () => {});
  await bridge('accept', '2026-10-11');
  const sent = [];
  const down = () => Promise.reject(new TypeError('fetch failed', { cause: Object.assign(new Error('connect ETIMEDOUT'), { code: 'ETIMEDOUT' }) }));
  let answers = [down, async () => ({ ok: true, json: async () => ({ servers: [] }) })];
  globalThis.fetch = (url, init) => { sent.push([url, init.method, init.headers['x-bfe-nonce']]); return answers.shift()(); };
  assert.deepEqual(await bridge('servers'), { servers: [] }, 'the second try got through');
  assert.equal(sent.length, 2);
  assert.notEqual(sent[0][2], sent[1][2], 'each try signed with its own nonce');
  answers = [down, down];
  await assert.rejects(bridge('servers'), /^Error: Can't reach the online service \(ETIMEDOUT\)\. Check your connection and try again\.$/);
  sent.length = 0;
  answers = [down, down];
  await assert.rejects(bridge('credentials', 'a'.repeat(64)), /Can't reach the online service \(ETIMEDOUT\)/);
  assert.equal(sent.length, 1, 'a request that changes something is never sent twice');
});

test('slow links: each address gets long enough to answer over a congested link', async () => {
  const net = await import('node:net');
  const { CONNECT_ATTEMPT, SLOW_LINKS_FLAG } = await import('../electron/slow-links.js');
  assert.equal(net.getDefaultAutoSelectFamilyAttemptTimeout(), CONNECT_ATTEMPT);
  assert.ok(CONNECT_ATTEMPT >= 2000, 'well past the 800 ms round trips a congested link has shown');
  assert.equal(SLOW_LINKS_FLAG, `--network-family-autoselection-attempt-timeout=${CONNECT_ATTEMPT}`);
});
