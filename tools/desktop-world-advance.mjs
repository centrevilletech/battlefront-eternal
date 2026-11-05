// Check an already loaded desktop battle over its remote-debugging port:
// CDP_PORT=... node tools/desktop-world-advance.mjs --seconds 20 --minimum-tps 40
// Run after the loading screen has released the battle. Unlike world-hash
// and the lockstep browser checks, this requires both live clocks to advance.
import fs from 'node:fs';
import { worldProgress } from './lib/world-progress.mjs';
const argv = process.argv.slice(2), option = (name, value) => {
  const at = argv.indexOf(`--${name}`); return at < 0 ? value : argv[at + 1];
};
const seconds = Number(option('seconds', 20)), minimumTps = Number(option('minimum-tps', 1));
const port = Number(process.env.CDP_PORT || option('port', 9222));
const info = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
const ws = new WebSocket(info.webSocketDebuggerUrl), pending = new Map(), errors = [];
await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
let serial = 0, pageSession = null;
const attached = [];
ws.onmessage = ({ data }) => {
  const m = JSON.parse(data);
  if (m.method === 'Target.attachedToTarget') attached.push(m.params);
  if (m.id && pending.has(m.id)) { const request = pending.get(m.id); pending.delete(m.id); if (m.error) request.reject(Error(JSON.stringify(m.error))); else request.resolve(m.result); }
  if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text);
  if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') errors.push(m.params.args.map(a => a.value ?? a.description).join(' '));
};
function send(method, params = {}, sessionId) {
  return new Promise((resolve, reject) => {
    const id = ++serial, timer = setTimeout(() => { pending.delete(id); reject(Error(`${method} timed out`)); }, 15000);
    pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
    ws.send(JSON.stringify({ id, method, params, sessionId }));
  });
}
async function evaluate(sessionId, expression) {
  const r = await send('Runtime.evaluate', { expression, returnByValue: true, timeout: 10000 }, sessionId);
  if (r.exceptionDetails) throw Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
  return r.result.value;
}
try {
  const { targetInfos } = await send('Target.getTargets');
  const page = targetInfos.find(t => t.type === 'page' && t.url.startsWith('app:'));
  if (!page) throw Error('A loaded desktop battle is required');
  const { sessionId: p } = await send('Target.attachToTarget', { targetId: page.targetId, flatten: true });
  await send('Runtime.enable', {}, p);
  // Workers belong to the page's target. Page auto-attachment also finds
  // module workers whose browser-level target URL is empty in Electron.
  pageSession = p;
  await send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true }, p);
  await new Promise(resolve => setTimeout(resolve, 500));
  let w = null;
  for (const target of attached) {
    if (target.targetInfo.type !== 'worker') continue;
    await send('Runtime.enable', {}, target.sessionId);
    if (await evaluate(target.sessionId, 'Boolean(self.__server)')) { w = target.sessionId; break; }
  }
  if (!w) throw Error('A loaded world worker is required');
  const samples = [], started = performance.now();
  do {
    const page = await evaluate(p, '({tick:__bf.worldClock.tick,alive:__bf.playerUnit.alive,running:__bf.client.running,ready:__bf.client.ready,hidden:document.hidden,state:__bf.clockState})');
    const world = await evaluate(w, '({tick:__bf.worldClock.tick,alive:__server.actor.unit.alive,running:__server.running,stepping:__server.stepping})');
    samples.push({ at: performance.now(), pageTick: page.tick, worldTick: world.tick, running: world.running, stepping: world.stepping, page, world });
    if (performance.now() - started >= seconds * 1000) break;
    await new Promise(resolve => setTimeout(resolve, 1000));
  } while (true);
  const result = { ...worldProgress(samples, { minimumTps }), errors, samples };
  if (errors.length) result.failures.push('Page or worker reported errors');
  console.log(JSON.stringify(result, null, 2));
  const output = option('output', null); if (output) fs.writeFileSync(output, JSON.stringify(result, null, 2));
  if (result.failures.length) process.exitCode = 1;
} finally {
  if (pageSession) await send('Target.setAutoAttach', { autoAttach: false, waitForDebuggerOnStart: false, flatten: true }, pageSession).catch(() => {});
  ws.close();
}
