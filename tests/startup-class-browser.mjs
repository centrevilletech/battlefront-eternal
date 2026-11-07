// Run against a desktop page with CDP_PORT set. An invalid local class must
// close loading and show its error before starting a world worker.
import assert from 'node:assert/strict';
const port = Number(process.env.CDP_PORT);
assert.ok(port, 'CDP_PORT is required');
const info = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
const socket = new WebSocket(info.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let serial = 0;
const waiting = new Map();
socket.onmessage = ({ data }) => {
  const message = JSON.parse(data);
  if (message.id) { waiting.get(message.id)?.(message); waiting.delete(message.id); }
};
function send(method, params = {}, sessionId) {
  return new Promise((resolve, reject) => {
    const id = ++serial;
    const timer = setTimeout(() => { waiting.delete(id); reject(Error(`${method} timed out`)); }, 15000);
    waiting.set(id, message => { clearTimeout(timer); message.error ? reject(Error(JSON.stringify(message.error))) : resolve(message.result); });
    socket.send(JSON.stringify({ id, method, params, sessionId }));
  });
}
try {
  const { targetInfos } = await send('Target.getTargets');
  const target = targetInfos.find(t => t.type === 'page');
  const { sessionId } = await send('Target.attachToTarget', { targetId: target.targetId, flatten: true });
  const page = (method, params) => send(method, params, sessionId);
  await page('Page.navigate', { url: 'app://bfe/?map=end1&side=imp&era=gcw&unit=imp_inf_pilottie&ai=classic&seed=1' });
  let result;
  const deadline = Date.now() + 60000;
  do {
    await new Promise(resolve => setTimeout(resolve, 250));
    const { result: value } = await page('Runtime.evaluate', { expression: `({message:document.querySelector('#hud')?.textContent,closed:window.__loading?.closed,game:!!window.__bf})`, returnByValue: true });
    result = value.value;
  } while (!result?.message?.includes('is not available in this mission') && Date.now() < deadline);
  assert.match(result.message, /Failed to load game: imp_inf_pilottie is not available in this mission/);
  assert.equal(result.closed, true);
  assert.equal(result.game, false, 'no partial game left waiting for a worker');
  console.log(JSON.stringify(result));
} finally { socket.close(); }
