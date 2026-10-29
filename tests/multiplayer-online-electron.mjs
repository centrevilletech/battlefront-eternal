// Exercise real shell actions, privacy acceptance, public discovery and join.
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
const { _electron: electron } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const directory=await mkdtemp(join(tmpdir(),'bfe-online-ui-'));let app;
try {
  app=await electron.launch({executablePath:resolve('node_modules/electron/dist/electron'),args:[resolve('.'),'--no-sandbox','--use-angle=swiftshader','--enable-unsafe-swiftshader',`--user-data-dir=${directory}`,`--dev-url=${process.env.BFE_TEST_URL || 'http://127.0.0.1:18650'}`,'--query=?screen=main&shadows=off'],timeout:60000});
  const page=await app.firstWindow();await page.setViewportSize({width:640,height:480});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error'&&/frame failed|world worker failed/.test(m.text())){errors.push(m.text());console.error(m.text());}});
  await page.waitForFunction(()=>window.__shell?.screens,null,{timeout:120000});
  await page.evaluate(()=>{
    const activate=id=>{const s=__shell.stack.top,r=s.widget('rows');r.activate(r.rows.findIndex(row=>row.id===id),s);};
    activate('mp');activate('online');
  });
  await page.waitForFunction(()=>__shell.stack.top.widget('rows')?.rows.some(r=>r.id==='notice'));
  await page.evaluate(()=>{const s=__shell.stack.top,r=s.widget('rows');r.activate(r.rows.findIndex(row=>row.id==='notice'),s);const popup=__shell.stack.top;popup.widget('answer').activate(0,popup);});
  await page.waitForFunction(()=>__shell.stack.top.widget('rows')?.rows.some(r=>/^[a-f0-9]{64}$/.test(r.id)),null,{timeout:30000});
  const serverId=await page.evaluate(preferred=>{const s=__shell.stack.top,r=s.widget('rows'),i=r.rows.findIndex(row=>preferred?row.id===preferred:/^[a-f0-9]{64}$/.test(row.id));if(i<0)throw Error('Requested public server missing');const id=r.rows[i].id;r.activate(i,s);return id;},process.env.BFE_TEST_SERVER_ID||null);
  await page.waitForURL(url=>url.searchParams.get('network')==='1'&&url.searchParams.get('server')===serverId,{timeout:30000});
  assert.equal(new URL(page.url()).searchParams.has('address'),false,'ONLINE uses authenticated public discovery');
  await page.waitForFunction(()=>window.__bf?.client?.ready,null,{timeout:180000});
  await page.evaluate(()=>__bf.begin());
  await page.waitForFunction(()=>__bf.worldClock.tick>0&&__bf.playerUnit.id>1,null,{timeout:90000,polling:100});
  const state=await page.evaluate(()=>({id:__bf.playerUnit.id,tick:__bf.worldClock.tick,status:__bf.client.host.statusText}));
  console.log('ONLINE joined',JSON.stringify(state));
  try { await page.waitForFunction(tick=>__bf.worldClock.tick>tick+60,state.tick,{timeout:60000,polling:100}); }
  catch(error){console.error('ONLINE diagnostics',JSON.stringify(await page.evaluate(()=>({tick:__bf.worldClock.tick,alive:__bf.playerUnit.alive,pending:__bf.client.pending.length,closed:__bf.client.host.closed,status:__bf.client.host.statusText,welcome:__bf.client.host.welcome}))));throw error;}
  assert.deepEqual(errors,[]);console.log('ONLINE shell to live match passed',JSON.stringify(state));
}finally{await app?.close();await rm(directory,{recursive:true,force:true});}
