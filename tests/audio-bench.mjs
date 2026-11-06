// Sample an already loaded battle: its audio render threads, page main thread
// and drawn frames. Run base and candidate builds in an interleaved job with
// the same mission, seed, quality and device layout. The renderer's offline
// fixtures are fidelity gates and do not measure real-time playback cost.
// AUDIO_CDP_URL is required. BENCH_WARM=6, BENCH_SECONDS=20 and BENCH_REPS=1
// control the sample window; AUDIO_BENCH_OUT optionally saves the JSON result.
import fs from "node:fs";
const endpoint = process.env.AUDIO_CDP_URL;
if (!endpoint) throw Error("AUDIO_CDP_URL must name the loaded battle's debugging endpoint");
const seconds = Number(process.env.BENCH_SECONDS || 20), warm = Number(process.env.BENCH_WARM ?? 6), reps = Number(process.env.BENCH_REPS || 1);
const pause = (s)=>new Promise((resolve)=>setTimeout(resolve,s*1000));
function counters(pids) {
  const audio = new Map(), page = new Map(), names = {};
  for (const pid of pids) {
    page.set(pid,Number(fs.readFileSync(`/proc/${pid}/task/${pid}/schedstat`,"utf8").split(" ")[0]));
    for (const tid of fs.readdirSync(`/proc/${pid}/task`)) {
      try {
        const path = `/proc/${pid}/task/${tid}/`, name = fs.readFileSync(path+"comm","utf8").trim();
        if (/audio/i.test(name)) { const key = `${pid}/${tid}`; audio.set(key,Number(fs.readFileSync(path+"schedstat","utf8").split(" ")[0])); names[key] = name; }
      } catch { /* Threads can retire between directory enumeration and read. */ }
    }
  }
  return {audio,page,names};
}
function delta(before,after,wall) { let ns = 0; for (const [id,value] of after) if (before.has(id)) ns += value-before.get(id); return ns/wall*100; }
// Node's built-in CDP transport keeps the benchmark independent of browser
// launch packages. It attaches to the existing app and disconnects at the end.
const version = await (await fetch(endpoint+"/json/version")).json(), ws = new WebSocket(version.webSocketDebuggerUrl), pending = new Map(), errors = [];
await new Promise((resolve,reject)=> { ws.onopen = resolve; ws.onerror = reject; });
let serial = 0, session;
ws.onmessage = ({data})=> {
  const m = JSON.parse(data);
  if (m.id && pending.has(m.id)) { const request = pending.get(m.id); pending.delete(m.id); if (m.error) request.reject(Error(JSON.stringify(m.error))); else request.resolve(m.result); }
  if (m.method === "Runtime.exceptionThrown") errors.push(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text);
};
function send(method,params={},sessionId) {
  return new Promise((resolve,reject)=> { const id = ++serial, timer = setTimeout(()=> { pending.delete(id); reject(Error(method+" timed out")); },15000); pending.set(id,{resolve:(r)=> { clearTimeout(timer); resolve(r); },reject:(e)=> { clearTimeout(timer); reject(e); }}); ws.send(JSON.stringify({id,method,params,sessionId})); });
}
async function evaluate(expression) {
  const r = await send("Runtime.evaluate",{expression,returnByValue:true,awaitPromise:true},session);
  if (r.exceptionDetails) throw Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
  return r.result.value;
}
try {
  const {targetInfos} = await send("Target.getTargets"), page = targetInfos.find((t)=>t.type === "page");
  if (!page) throw Error("The connected app has no battle page");
  session = (await send("Target.attachToTarget",{targetId:page.targetId,flatten:true})).sessionId;
  await send("Runtime.enable",{},session); const results = [];
  const probe = ()=>evaluate(`(()=> {
    const b = window.__bf, a = b?.audio, r = a?.engine.renderer, ctx = a?.engine.ctx;
    if (!a?.ready || !a.started || a.error || r?.error) throw Error(a?.error?.message || r?.error?.message || "Battle audio is not ready");
    return {time:performance.now(),frame:b.profile?.frame ?? null,tick:b.worldClock.tick,rate:ctx.sampleRate,state:ctx.state,quality:r.enhanced?"enhanced":"original",output:r.mode,real:a.engine.stats.real,virtual:a.engine.stats.virtual};
  })()`);
  await evaluate("window.__bf.audio.engine.ctx.resume().then(()=>true)");
  const setup = await probe(); if (setup.state !== "running") throw Error("The device clock is suspended");
  // Confirm nonzero Rust output outside the measured CPU window, then
  // detach the optional meter so its work is not reported as mixer cost.
  await evaluate(`(()=> { window.__audioBenchPeak = -Infinity; const r = window.__bf.audio.engine.renderer; r.onMeter = (m)=> { window.__audioBenchPeak = Math.max(window.__audioBenchPeak,m.truePeak); }; r.meter(true); })()`);
  await pause(Math.max(warm,1.5));
  const peak = await evaluate(`(()=> { const r = window.__bf.audio.engine.renderer; r.meter(false); r.onMeter = null; return window.__audioBenchPeak; })()`);
  if (!(peak > -96)) throw Error("No audible PCM reached the Rust output meter");
  for (let rep=0;rep<reps;rep++) {
    const pids = (await send("SystemInfo.getProcessInfo")).processInfo.filter((p)=>p.type==="renderer").map((p)=>p.id);
    if (pids.length !== 1) throw Error("Use an isolated app with one page renderer for a page CPU measurement");
    const info = await probe(), before = counters(pids), start = process.hrtime.bigint();
    if (!before.audio.size) throw Error("No audio render thread was found; refusing to report zero cost");
    await pause(seconds); const wall = Number(process.hrtime.bigint()-start), after = counters(pids), final = await probe();
    if (errors.length) throw Error(errors.join("; "));
    if (!(final.tick > info.tick) || final.state !== "running") throw Error("Battle or device clock stopped during the sample");
    const result = {peakDbTP:peak,rep:rep+1,seconds:wall/1e9,audioCorePercent:delta(before.audio,after.audio,wall),pageCorePercent:delta(before.page,after.page,wall),ticksPerSecond:(final.tick-info.tick)/(wall/1e9),audioThreads:after.names,info,final};
    results.push(result); console.log(JSON.stringify(result));
  }
  if (process.env.AUDIO_BENCH_OUT) fs.writeFileSync(process.env.AUDIO_BENCH_OUT,JSON.stringify({setup,results},null,2)+"\n");
} finally { ws.close(); }
