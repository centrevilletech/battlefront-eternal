import assert from 'node:assert/strict';

// Read inside the HUD callback, before the compositor can discard the frame.
async function drawnFrame(page,stage) {
  if(process.env.BFE_TEST_CPU==='1')return;
  const result=await page.evaluate(()=>new Promise(resolve=>{
    const g=__bf.graphics,r=g.renderer,gl=r.getContext(),hud=__bf.hud,render=hud.render;
    hud.render=function(...args){
      const value=render.apply(this,args);hud.render=render;
      const w=gl.drawingBufferWidth,h=gl.drawingBufferHeight,bytes=new Uint8Array(w*h*4);
      r.setRenderTarget(null);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,bytes);
      let lit=0,min=255,max=0;
      for(let i=0;i<bytes.length;i+=16){const n=Math.max(bytes[i],bytes[i+1],bytes[i+2]);if(n>8)lit++;min=Math.min(min,n);max=Math.max(max,n);}
      resolve({w,h,lit,min,max,frame:g.frameCount,lost:gl.isContextLost()});return value;
    };
  }));
  assert.ok(!result.lost&&result.frame>2&&result.lit>100&&result.max-result.min>16,'a nonblank gameplay frame reaches the drawn framebuffer');
  console.log('drawn-frame',stage,JSON.stringify(result));
}

// Fresh models can compile on their first drawn frame. Let that work settle
// before a discrete input with the production half-second expiry window.
async function inputCadence(page) {
  const result=await page.evaluate(()=>new Promise(resolve=>{
    let previous=performance.now(),stable=0;const start=previous;
    function sample(now){const gap=now-previous;previous=now;stable=gap<250?stable+1:0;
      if(stable>=4||now-start>60000)resolve({stable:stable>=4,elapsed:now-start});else requestAnimationFrame(sample);}
    requestAnimationFrame(sample);
  }));
  assert.ok(result.stable,'drawn input frames settle within the reliable action window');
}

// Ordinary spawn, walking, use and seat-cycle commands. No teleport, direct
// board(), health edits or replacement physics are allowed in this gate.
export async function mountedGameGate([actor, observer]) {
  await actor.bringToFront();
  await actor.evaluate(()=>{__bf.controls.locked=true;if(__bf.menu.isOpen)__bf.menu.close(true);});
  await actor.evaluate(()=>{
    window.__spawnEdges=[];
    const sender=__bf.client.host.entityInput,add=sender.add.bind(sender),feedback=sender.feedback.bind(sender);
    sender.add=(command,edge)=>{const value=add(command,edge);if(['team','spawn','suicide','cancel'].includes(edge.kind))window.__spawnEdges.push({sent:value});return value;};
    sender.feedback=message=>{if(message.results?.length)window.__spawnEdges.push({feedback:message});return feedback(message);};
  });
  const choose=(nearby=false)=>{
    const options=[];
    for(const team of Object.keys(__bf.spawnPort.teams()).map(Number)) {
      const className=__bf.spawnPort.roster(team).find(({className})=>Object.values(__bf.data.sides).some(side=>{
        const def=side.units?.[className];return def&&def.base!=='walkerdroid'&&!Number(def.NoEnterVehicles);
      }))?.className;
      if(!className)continue;
      for(const v of __bf.vehicles.vehicles) {
        if(!v.alive||v.flying||v.seats.length<2||v.seats.every(s=>s.lease&&(s.lease.human||s.lease.unit?.networkHuman))||v.controllingTeam>0&&v.controllingTeam!==team)continue;
        for(const p of __bf.spawnPort.posts(team).filter(p=>p.eligible)) {
          const distance=Math.hypot(...p.position.map((n,i)=>n-v.position.toArray()[i]));
          options.push({team,className,post:p.name,vehicleId:v.aiTarget.id,vehicleGeneration:v.aiTarget.generation,distance,speed:Math.hypot(v.velocity.x,v.velocity.z)});
        }
      }
    }
    return options.sort((a,b)=>a.distance-b.distance).find(c=>!nearby||c.distance<25&&c.speed<2);
  };
  let choice;
  for(let attempt=0;attempt<60&&!choice;attempt++){choice=await actor.evaluate(choose);if(!choice)await actor.waitForTimeout(250);}
  if(!choice)console.log('mounted-unavailable',await actor.evaluate(()=>({posts:__bf.spawnPort.posts(__bf.playerTeam),vehicles:__bf.vehicles.vehicles.map(v=>({id:v.aiTarget.id,alive:v.alive,team:v.controllingTeam,flying:v.flying,seats:v.seats.map(s=>s.lease?.unit?.id??null)}))})));
  assert.ok(choice,'mission has an available ground pilot seat near an eligible spawn');
  console.log('mounted-spawn-choice',JSON.stringify(choice));
  // A new connection is unspawned, not a dead player waiting to respawn.
  // Establish a real first life before proving the death-screen transition.
  async function spawn() {
    await actor.waitForFunction(()=>['unspawned','awaiting'].includes(__bf.spawnPort.humanState().phase),null,{timeout:60000});
    await inputCadence(actor);
    await actor.evaluate(choice=>__bf.spawnPort.request(choice),choice);
    try { await actor.waitForFunction(c=>__bf.playerUnit.alive&&__bf.playerUnit.className===c.className&&__bf.playerTeam===c.team,choice,{timeout:60000}); }
    catch(error) {
      console.log('spawn-timeout',JSON.stringify(await actor.evaluate(async()=>({phase:__bf.spawnPort.humanState(),alive:__bf.playerUnit.alive,team:__bf.playerTeam,loading:window.__loading?.closed,pending:__bf.texturesPending(),tick:__bf.worldClock.tick,running:__bf.client.running,hidden:document.hidden,edges:window.__spawnEdges,inspection:await __bf.client.host.call('inspect')}))));
      throw error;
    }
    await actor.waitForFunction(()=>!__bf.spawnScreen?.isOpen&&__bf.controls.enabled&&!__bf.soldier.air&&__bf.soldier.lock<=0,null,{timeout:15000});
    await actor.bringToFront();
    await actor.evaluate(()=>{if(__bf.menu.isOpen)__bf.menu.close(true);__bf.controls.locked=true;});
  }
  if(!await actor.evaluate(()=>__bf.playerUnit.alive))await spawn();
  const first=await actor.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation}));
  await observer.waitForFunction(({id,generation})=>{const u=__bf.units.byId(id);return u?.alive&&u.generation===generation&&u.actor?.root.visible;},first,{timeout:15000});
  await drawnFrame(actor,'first-life');
  await drawnFrame(observer,'observed-first-life');
  await actor.evaluate(()=>__bf.spawnPort.suicide());
  await actor.waitForFunction(()=>!__bf.playerUnit.alive,null,{timeout:15000});
  await observer.waitForFunction(({id,generation})=>{const u=__bf.units.byId(id);return !u?.alive||u.generation!==generation;},first,{timeout:15000});
  await actor.waitForFunction(()=>__bf.spawnPort.humanState().phase==='awaiting'&&__bf.spawnScreen?.isOpen,null,{timeout:60000});
  // Wait for a nearby slow hull, rather than chasing a bot-driven tank
  // across the battle. Vehicle destruction and respawns remain authoritative.
  choice=null;
  for(let attempt=0;attempt<480&&!choice;attempt++){choice=await actor.evaluate(choose,true);if(!choice)await actor.waitForTimeout(250);}
  assert.ok(choice,'a live hull becomes available near an eligible spawn');
  console.log('mounted-respawn-choice',JSON.stringify(choice));
  await spawn();
  const life=await actor.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation}));
  assert.ok(life.id!==first.id||life.generation>first.generation,'respawn establishes a new life');
  console.log('mounted-respawn',JSON.stringify({first,life}));
  let navigation;
  try {
    await actor.keyboard.down('KeyW');
    assert.ok(await actor.evaluate(()=>__bf.controls.down.has('KeyW')),'movement key reaches enabled Controls after the spawn screen closes');
    for(let step=0;step<240;step++) {
      navigation=await actor.evaluate(({vehicleId,vehicleGeneration})=>{
        const v=__bf.vehicles.vehicles.find(v=>v.aiTarget.id===vehicleId&&v.aiTarget.generation===vehicleGeneration);
        if(!v?.alive||!__bf.playerUnit.alive)return {invalid:true,actorAlive:__bf.playerUnit.alive,hullAlive:v?.alive};
        const p=__bf.playerUnit.position,dx=v.position.x-p.x,dz=v.position.z-p.z;
        const yaw=Math.atan2(dx,dz),difference=Math.atan2(Math.sin(yaw-__bf.soldier.aimYaw),Math.cos(yaw-__bf.soldier.aimYaw));
        // Derive counts from the shared look method without mutating the body.
        // Soldier yaw decreases with mouse X and its rate is class-dependent.
        const preview=Object.create(__bf.soldier);preview.aimYaw=__bf.soldier.aimYaw;preview.aimPitch=__bf.soldier.aimPitch;
        preview.lookRaw(1,0,__bf.settings.mouseScale,false);
        const rate=preview.aimYaw-__bf.soldier.aimYaw;
        if(Math.abs(rate)>1e-9)__bf.controls.looks.push([Math.max(-10000,Math.min(10000,difference/rate)),0]);
        return {distance:Math.hypot(dx,dz),radius:v.radius,position:p.toArray(),seat:v.seats[0].lease?.unit?.id??null};
      },choice);
      if(navigation.invalid)console.log('mounted-interrupted',JSON.stringify(navigation));
      assert.ok(!navigation.invalid,'actor and selected hull stay alive during approach');
      if(navigation.distance<navigation.radius+2.5)break;
      await actor.waitForTimeout(200);
    }
  } finally {await actor.keyboard.up('KeyW');}
  console.log('mounted-approach',JSON.stringify(navigation));
  assert.ok(navigation.distance<navigation.radius+2.5,'ordinary movement reaches boarding distance');
  await actor.waitForFunction(()=>!__bf.soldier.air&&__bf.soldier.lock<=0,null,{timeout:15000});
  await inputCadence(actor);
  for(let attempt=0;attempt<10;attempt++) {
    await actor.keyboard.press('KeyE');
    try { await actor.waitForFunction(c=>__bf.playerUnit.seat?.vehicle?.aiTarget.id===c.vehicleId,choice,{timeout:1500});break; }
    catch(error) { if(attempt===9) { console.log('boarding-timeout',JSON.stringify(await actor.evaluate(()=>({alive:__bf.playerUnit.alive,seat:__bf.playerUnit.seat?.index,enabled:__bf.controls.enabled,locked:__bf.controls.locked,edges:window.__spawnEdges}))));throw error; } }
  }
  // Ordinary seat cycling may exchange a bot pilot with the human passenger.
  if(await actor.evaluate(()=>__bf.playerUnit.seat.index!==0))await actor.evaluate(()=>{__bf.pending.nextPrimary=true;});
  await actor.waitForFunction(()=>__bf.playerUnit.seat?.index===0&&__bf.client.entity.previous?.controlMode===4,null,{timeout:15000});
  const identity=await actor.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation}));
  await observer.waitForFunction(({id,vehicleId})=>__bf.units.byId(id)?.seat?.vehicle?.aiTarget.id===vehicleId,{...identity,...choice},{timeout:15000});
  await actor.evaluate(()=>{const host=__bf.client.host,receive=host.onEntities;window.__restoreMountedTransport=()=>host.onEntities=receive;host.onEntities=bytes=>setTimeout(()=>{try{receive(bytes);}catch(e){host.fail(e);}},180);});
  const start=await observer.evaluate(id=>__bf.vehicles.vehicles.find(v=>v.aiTarget.id===id).position.toArray(),choice.vehicleId);
  await actor.keyboard.down('KeyW');
  await actor.waitForFunction(()=>{const p=__bf.client.vehiclePrediction,v=__bf.driver.vehicle;return p&&p.predictionTick>__bf.worldClock.tick&&p.position.distanceTo(v.position)>.02;},null,{timeout:15000});
  await observer.waitForFunction(({id,start})=>{const v=__bf.vehicles.vehicles.find(v=>v.aiTarget.id===id);return v&&Math.hypot(...v.position.toArray().map((x,i)=>x-start[i]))>.5;},{id:choice.vehicleId,start},{timeout:15000});
  const pilot=await actor.evaluate(()=>({tick:__bf.client.vehiclePrediction.predictionTick,authority:__bf.worldClock.tick,control:__bf.client.entity.previous.controlMode}));
  await actor.keyboard.up('KeyW');await actor.mouse.click(320,240);
  await actor.evaluate(()=>window.__restoreMountedTransport());await actor.waitForTimeout(700);
  await actor.evaluate(()=>{__bf.pending.nextPrimary=true;});
  await actor.waitForFunction(()=>__bf.playerUnit.seat?.index>0&&__bf.client.entity.previous?.controlMode===2,null,{timeout:15000});
  const before=await actor.evaluate(()=>__bf.driver.vehicle.seats[__bf.playerUnit.seat.index].aimYaw);
  await actor.evaluate(()=>document.dispatchEvent(new MouseEvent('mousemove',{movementX:80,movementY:20,bubbles:true})));
  await actor.mouse.click(460,190);
  await actor.waitForFunction(before=>Math.abs(__bf.driver.vehicle.seats[__bf.playerUnit.seat.index].aimYaw-before)>.01,before,{timeout:15000});
  // Disconnect while occupying the hull, then prove the server releases it.
  const tick=await observer.evaluate(()=>__bf.worldClock.tick);
  await drawnFrame(actor,'passenger');
  await drawnFrame(observer,'observed-driving');
  await actor.evaluate(()=>__bf.client.host.dispose());
  await observer.waitForFunction(({id,vehicleId,tick})=>{const v=__bf.vehicles.vehicles.find(v=>v.aiTarget.id===vehicleId);return __bf.worldClock.tick>tick&&!__bf.units.byId(id)?.seat&&v?.seats.every(s=>s.lease?.unit?.id!==id);},{...identity,...choice,tick},{timeout:15000});
  console.log('Actual respawn, pilot prediction, driving, passenger aim and occupied disconnect passed',JSON.stringify({choice,pilot,identity}));
}
