// Actual native window clients and Chromium pages share an authenticated
// server. Kernel shaping covers all UDP in this disposable namespace.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir,mkdtemp,rm} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
import {createServer} from 'node:http';
import {execFileSync,spawn} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {enterProbeNamespace} from '../net-probe-shaping.mjs';
import {run,requireRun} from './process.mjs';
import {until,sleep} from './common.mjs';
import {startPageNetwork} from '../net-page-bridge.mjs';
import {createNetworkBridge} from '../../electron/network.js';
import {PERFORMANCE_PRESETS} from '../../src/settings.js';
const config=JSON.parse(await readFile(process.argv[2],'utf8'));
await enterProbeNamespace([...process.argv.slice(2),'--shape-up-kbps=1000']);
if(config.display)process.env.DISPLAY=config.display;
if(!process.env.DISPLAY)throw Error('Rendered peers require a configured X11 display');
const raw=join(config.raw,config.job.id),state=await mkdtemp(join(tmpdir(),'host-peers-'));
const server=resolve(config.engine,'../server'),gamePort=19451,healthPort=19452;
const tc=args=>execFileSync('tc',args,{encoding:'utf8'});
const report={passed:false,map:config.job.map,native:[],browser:[],errors:[],blackout:false};
let service,browser,network,http,log='',shaped=false,pages=[];
try{
  await writeFile(join(state,'config.json'),JSON.stringify({name:'Client qualification',engine:config.engine,data:config.data,maxPlayers:8,autoSpawn:true,autoAssignTeams:true,gamePort,bind:'127.0.0.1',discovery:false,onlineNoticeAccepted:false,rotation:[`swbf1/${config.job.map}:${config.job.side}:conquest`]}));
  service=spawn(process.execPath,[join(server,'src/main.js'),'--serve'],{env:{...process.env,BFE_STATE_DIR:state,BFE_ENGINE_DIR:config.engine,BFE_DATA_DIR:config.data,PORT:String(healthPort)},stdio:['ignore','pipe','pipe']});
  for(const stream of [service.stdout,service.stderr])stream.on('data',chunk=>{log=(log+chunk).slice(-8000);});
  let info;await until(async()=>{if(service.exitCode!==null)throw Error(log);try{const r=await fetch(`http://127.0.0.1:${gamePort}/info`);if(r.ok){info=await r.json();return true;}}catch{}},'rendered peer service',180000);
  tc(['qdisc','replace','dev','lo','root','handle','1:','prio']);
  tc(['qdisc','add','dev','lo','parent','1:3','handle','30:','netem','delay','50ms','15ms','loss','5%','limit','10000']);
  tc(['filter','add','dev','lo','protocol','ip','parent','1:','prio','1','u32','match','ip','protocol','17','0xff','flowid','1:3']);shaped=true;
  // Serve the minified desktop files and external content without importing
  // gameplay into the supervisor. Paths are constrained to either root.
  http=createServer(async(req,res)=>{
    const path=decodeURIComponent(new URL(req.url,'http://local').pathname),relative=path==='/'?'index.html':path.slice(1);
    if(relative.split('/').includes('..')){res.writeHead(403).end();return;}
    for(const base of [config.dist,config.data])try{
      const bytes=await readFile(join(base,relative));const ext=relative.split('.').at(-1);
      res.setHeader('content-type',({html:'text/html',js:'text/javascript',css:'text/css',json:'application/json',wasm:'application/wasm'})[ext]??'application/octet-stream');res.end(bytes);return;
    }catch{}res.writeHead(404).end();
  });await new Promise(resolve=>http.listen(0,'127.0.0.1',resolve));
  const {chromium}=await import(config.playwright??process.env.PLAYWRIGHT_MODULE??'playwright');
  browser=await chromium.launch({headless:true,executablePath:config.chromium,args:['--disable-backgrounding-occluded-windows','--disable-renderer-backgrounding','--no-sandbox','--use-angle=gl','--autoplay-policy=no-user-gesture-required']});network=await startPageNetwork();
  pages=[];
  for(let i=0;i<2;i++){
    const page=await browser.newPage({viewport:{width:640,height:480}});pages.push(page);
    const directory=join(state,`browser-${i}`),bridge=createNetworkBridge(directory,()=>{});
    await page.exposeFunction('bfeTestRequest',bridge);await network.attach(page,directory);
    await page.addInitScript(settings=>{
      // The deliberate unload between epochs has an opaque origin. Seed
      // settings only in the served client document, where storage is valid.
      if(location.protocol==='about:')return;
      localStorage.setItem('battlefront-eternal.settings',JSON.stringify(settings));
      window.bfeDesktop={network:{request:(...args)=>window.bfeTestRequest(...args),openSession:id=>window.bfeTestOpenSession(id),onSignal:()=>()=>{}}};
    },{...PERFORMANCE_PRESETS.minimum.values,renderScale:0.25,frameLimit:'30'});
    page.on('pageerror',error=>report.errors.push(error.message));
  }
  const url=`http://127.0.0.1:${http.address().port}/?map=${config.job.map}&side=${config.job.side}&ai=classic&seed=1&network=1&worker=1&server=${info.id}&address=127.0.0.1&port=${gamePort}&shadows=off`;
  const epoch=async index=>{
    await Promise.all(pages.map(async page=>{await page.goto(url);await page.waitForFunction(()=>window.__bf?.client?.ready,null,{timeout:180000});await page.evaluate(async()=>{await __bf.ready;__bf.begin();__bf.controls.locked=true;if(__bf.menu.isOpen)__bf.menu.close(true);});
      await page.waitForFunction(()=>__bf.client.running&&__bf.worldClock.tick>0,null,{timeout:30000});
      await page.evaluate(()=>{if(!__bf.playerUnit.alive){const state=__bf.spawnPort.humanState();__bf.spawnPort.request({team:state.team,className:state.className});}});
      await page.waitForFunction(()=>__bf.playerUnit.alive,null,{timeout:60000});}));
    await Promise.all([0,1].map(peer=>writeFile(join(raw,`native-${index}-${peer}.log`),'')));
    const native=Promise.all([0,1].map(async peer=>{
      const r=requireRun(await run('python3',['tools/native-client-window.py',config.nativeClient,'--join',`127.0.0.1:${gamePort}`,config.data,'--identity',join(state,`native-${peer}.json`),'--name',`Qualification ${peer}`,'--no-audio','--seconds','30'],{log:join(raw,`native-${index}-${peer}.log`),timeout:240000}));
      const row=r.tail.split('\n').flatMap(line=>{try{return [JSON.parse(line)];}catch{return [];}}).find(row=>row.welcomed!==undefined);
      assert.ok(row?.welcomed&&row.packets>0&&row.corrections>0&&row.draws>0,'native executable did not receive and draw a world');return row;
    }));
    let nativeError;native.catch(error=>{nativeError=error;});
    const before=await Promise.all(pages.map(page=>page.evaluate(()=>__bf.worldClock.tick)));
    await until(async()=>{if(nativeError)throw nativeError;const logs=await Promise.all([0,1].map(peer=>readFile(join(raw,`native-${index}-${peer}.log`),'utf8').catch(()=>'')));return logs.every(text=>text.includes('native online scene ready'));},'native clients drawing',180000);
    if(index===1){tc(['qdisc','change','dev','lo','parent','1:3','handle','30:','netem','delay','50ms','15ms','loss','100%','limit','10000']);await sleep(3000);tc(['qdisc','change','dev','lo','parent','1:3','handle','30:','netem','delay','50ms','15ms','loss','5%','limit','10000']);report.blackout=true;}
    await sleep(30000);
    const rows=await Promise.all(pages.map(page=>page.evaluate(()=>({tick:__bf.worldClock.tick,id:__bf.playerUnit.id,generation:__bf.playerUnit.generation,alive:__bf.playerUnit.alive,ready:__bf.client.ready}))));
    for(let i=0;i<rows.length;i++)assert.ok(rows[i].ready&&rows[i].tick>before[i],'browser world did not advance after impairment');
    report.browser.push(rows);report.native.push(await native);
  };
  await epoch(0);
  // Navigation creates fresh receivers while the profile directories retain
  // their Ed25519 identities. Native processes reconnect with the same keys.
  await Promise.all(pages.map(page=>page.goto('about:blank')));await sleep(1000);await epoch(1);
  assert.deepEqual(report.native[0].map(r=>r.identity),report.native[1].map(r=>r.identity));
  assert.deepEqual(report.errors,[]);report.kernel=tc(['-s','qdisc','show','dev','lo']);report.passed=true;
}catch(error){
  report.errors.push(String(error.stack??error).slice(0,6000));
  report.browserState=await Promise.all(pages.map(page=>page.evaluate(()=>window.__bf?{tick:__bf.worldClock.tick,running:__bf.client.running,ready:__bf.client.ready,phase:__bf.spawnPort.humanState(),menu:__bf.menu.isOpen,visibility:document.visibilityState,queued:__bf.controls.human?.length,status:__bf.client.host.statusText}:{missing:true}).catch(error=>({error:error.message}))));
}
finally{
  await browser?.close();network?.close();await new Promise(resolve=>http?http.close(resolve):resolve());
  if(service&&service.exitCode===null){service.kill('SIGTERM');await Promise.race([new Promise(resolve=>service.once('exit',resolve)),sleep(10000)]);if(service.exitCode===null)service.kill('SIGKILL');}
  if(shaped)tc(['qdisc','del','dev','lo','root']);await rm(state,{recursive:true,force:true});
  await writeFile(join(raw,'peers.json'),JSON.stringify(report));
}
assert.equal(report.passed,true,JSON.stringify(report.errors));
