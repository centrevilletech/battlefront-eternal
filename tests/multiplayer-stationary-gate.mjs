import assert from 'node:assert/strict';

// This gate only selects a normal spawn and drives ordinary movement/use/look.
// Turret identity and exact own state are observed, never assigned by the test.
export async function stationaryTurretGate([actor, observer]) {
  await actor.bringToFront();
  await actor.evaluate(()=>{__bf.controls.locked=true;if(__bf.menu.isOpen)__bf.menu.close(true);});
  const choose=()=>{
    const options=[];
    for(const team of Object.keys(__bf.spawnPort.teams()).map(Number)) {
      const className=__bf.spawnPort.roster(team).find(({className})=>Object.values(__bf.data.sides).some(side=>{
        const d=side.units?.[className];return d&&d.base!=='walkerdroid'&&!Number(d.NoEnterVehicles)&&d.weapons?.some(w=>w.base==='repair');
      }))?.className;
      if(!className)continue;
      for(const t of __bf.live.turrets) {
        if(t.gunner||t.lease)continue;
        for(const p of __bf.spawnPort.posts(team).filter(p=>p.eligible&&Math.abs(p.position[1]-t.position.y)<2)) {
          options.push({team,className,post:p.name,turretId:t.aiTarget.id,turretGeneration:t.aiTarget.generation,distance:Math.hypot(...p.position.map((n,i)=>n-t.position.toArray()[i]))});
        }
      }
    }
    return options.sort((a,b)=>a.distance-b.distance)[0];
  };
  let choice;
  for(let n=0;n<60&&!choice;n++){choice=await actor.evaluate(choose);if(!choice)await actor.waitForTimeout(250);}
  if(!choice)console.log('stationary-unavailable',await actor.evaluate(()=>__bf.live.turrets.map(t=>({id:t.aiTarget.id,alive:t.alive,gunner:Boolean(t.gunner),position:t.position.toArray()}))));
  assert.ok(choice,'mission has a pilot class and unmanned stationary turret near an eligible spawn');
  console.log('stationary-choice',JSON.stringify(choice));
  if(await actor.evaluate(()=>__bf.playerUnit.alive))await actor.evaluate(()=>__bf.spawnPort.suicide());
  await actor.waitForFunction(()=>__bf.spawnPort.humanState().phase==='awaiting',null,{timeout:60000});
  await actor.evaluate(c=>__bf.spawnPort.request(c),choice);
  await actor.waitForFunction(c=>__bf.playerUnit.alive&&__bf.playerUnit.className===c.className&&__bf.playerTeam===c.team,choice,{timeout:60000});
  await actor.evaluate(()=>{__bf.controls.locked=true;if(__bf.menu.isOpen)__bf.menu.close(true);});
  let approach, previousDistance=Infinity, stuck=0, jumps=0;
  try {
    await actor.keyboard.down('KeyW');
    for(let n=0;n<240;n++) {
      approach=await actor.evaluate(c=>{
        const t=__bf.live.turrets.find(t=>t.aiTarget.id===c.turretId&&t.aiTarget.generation===c.turretGeneration);
        if(!t||!__bf.playerUnit.alive||t.gunner)return {invalid:true};
        const p=__bf.playerUnit.position,dx=t.position.x-p.x,dz=t.position.z-p.z;
        const yaw=Math.atan2(dx,dz),difference=Math.atan2(Math.sin(yaw-__bf.soldier.aimYaw),Math.cos(yaw-__bf.soldier.aimYaw));
        const preview=Object.create(__bf.soldier);preview.aimYaw=__bf.soldier.aimYaw;preview.aimPitch=__bf.soldier.aimPitch;preview.lookRaw(1,0,__bf.settings.mouseScale,false);
        const rate=preview.aimYaw-__bf.soldier.aimYaw;
        if(Math.abs(rate)>1e-9)__bf.controls.looks.push([Math.max(-10000,Math.min(10000,difference/rate)),0]);
        return {distance:Math.hypot(dx,dz),position:p.toArray(),target:t.position.toArray(),yaw:__bf.soldier.aimYaw};
      },choice);
      assert.ok(!approach.invalid,'actor and available turret remain valid during approach');
      if(approach.distance<4)break;
      if(Math.abs(previousDistance-approach.distance)<.05)stuck++;else stuck=0;
      previousDistance=approach.distance;
      if(stuck>=10&&jumps<4){await actor.keyboard.press('Space');jumps++;stuck=0;console.log('stationary-obstacle-jump',JSON.stringify({jumps,approach}));}
      await actor.waitForTimeout(200);
    }
  } finally {await actor.keyboard.up('KeyW');}
  console.log('stationary-approach',JSON.stringify(approach));
  assert.ok(approach.distance<4,'ordinary movement reaches stationary turret');
  // A jump retains ordinary airborne momentum after W is released. Wait for
  // the landing, then face the pad again before using the short-range cutter.
  await actor.waitForFunction(()=>!__bf.soldier.air&&__bf.soldier.velocity.length()<.1,null,{timeout:15000});
  const aimed=await actor.evaluate(c=>{
    const t=__bf.live.turrets.find(t=>t.aiTarget.id===c.turretId),p=__bf.playerUnit.position,s=__bf.soldier;
    const dx=t.position.x-p.x,dz=t.position.z-p.z,yaw=Math.atan2(dx,dz),pitch=Math.atan2(t.position.y+1.5-(p.y+1.7),Math.hypot(dx,dz));
    const preview=Object.create(s);preview.aimYaw=s.aimYaw;preview.aimPitch=s.aimPitch;preview.lookRaw(1,1,__bf.settings.mouseScale,false);
    const dyaw=Math.atan2(Math.sin(yaw-s.aimYaw),Math.cos(yaw-s.aimYaw));
    __bf.controls.looks.push([dyaw/(preview.aimYaw-s.aimYaw),(pitch-s.aimPitch)/(preview.aimPitch-s.aimPitch)]);
    return {yaw,pitch,distance:Math.hypot(dx,dz),position:p.toArray()};
  },choice);
  await actor.waitForFunction(a=>Math.abs(Math.atan2(Math.sin(__bf.soldier.aimYaw-a.yaw),Math.cos(__bf.soldier.aimYaw-a.yaw)))<.05,aimed,{timeout:10000});
  console.log('stationary-settled-aim',JSON.stringify(aimed));
  if(await actor.evaluate(c=>!__bf.live.turrets.find(t=>t.aiTarget.id===c.turretId).alive,choice)) {
    const repair=await actor.evaluate(()=>{const w=__bf.combat.arsenal.weapons.find(w=>w.repair);return w?{channel:w.channel,name:w.name,range:w.def.LockOnRange}:null;});
    assert.ok(repair,'normal pilot loadout contains a repair tool');
    for(let n=0;n<8&&!await actor.evaluate(c=>__bf.combat.arsenal.current(c)?.repair,repair.channel);n++) {
      await actor.evaluate(c=>{__bf.pending[c?'nextSecondary':'nextPrimary']=true;},repair.channel);await actor.waitForTimeout(150);
    }
    assert.ok(await actor.evaluate(c=>__bf.combat.arsenal.current(c)?.repair,repair.channel),'ordinary weapon cycling selects repair tool');
    await actor.mouse.down({button:repair.channel?'right':'left'});
    try {await actor.waitForFunction(c=>__bf.live.turrets.find(t=>t.aiTarget.id===c.turretId)?.alive,choice,{timeout:30000});}
    catch(error){console.log('stationary-repair-failure',await actor.evaluate(async c=>{const t=__bf.live.turrets.find(t=>t.aiTarget.id===c.turretId);return {turret:{state:t.state,health:t.health,position:t.position.toArray()},actor:__bf.playerUnit.position.toArray(),yaw:__bf.soldier.aimYaw,pitch:__bf.soldier.aimPitch,lock:__bf.combat.repairLock,weapon:__bf.combat.arsenal.weapons.map(w=>({name:w.name,base:w.def.base,channel:w.channel,range:w.def.LockOnRange})),inspection:await __bf.client.host.call('inspect')};},choice));throw error;}
    finally {await actor.mouse.up({button:repair.channel?'right':'left'});}
    console.log('stationary-built-by-repair',JSON.stringify({choice,repair}));
  }
  await actor.keyboard.press('KeyE');
  await actor.waitForFunction(c=>__bf.playerUnit.seat?.turret?.aiTarget.id===c.turretId&&__bf.client.entity.previous?.controlMode===3,choice,{timeout:15000});
  const identity=await actor.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation}));
  await observer.waitForFunction(({id,generation,turretId})=>{const u=__bf.units.byId(id);return u?.generation===generation&&u.seat?.turret?.aiTarget.id===turretId;},{...identity,...choice},{timeout:15000});
  const before=await observer.evaluate(id=>__bf.units.byId(id).seat.turret.yaw,identity.id);
  await actor.evaluate(()=>{const h=__bf.client.host,receive=h.onEntities;window.__restoreStationary=()=>h.onEntities=receive;h.onEntities=bytes=>setTimeout(()=>{try{receive(bytes);}catch(error){h.fail(error);}},180);});
  let prediction;
  try {
    await actor.evaluate(()=>document.dispatchEvent(new MouseEvent('mousemove',{movementX:80,movementY:10,bubbles:true})));
    await actor.waitForFunction(()=>__bf.gunner.active&&Math.abs(__bf.gunner.previewYaw)>.005,null,{timeout:15000,polling:20});
    prediction=await actor.evaluate(()=>({control:__bf.client.entity.previous.controlMode,authorityTick:__bf.worldClock.tick,pending:__bf.client.pending.length,authoritativeYaw:__bf.gunner.turret.yaw,previewYaw:__bf.gunner.previewYaw,previewPitch:__bf.gunner.previewPitch}));
  } finally {await actor.evaluate(()=>window.__restoreStationary());}
  assert.equal(prediction.control,3);assert.ok(prediction.pending>0);
  await observer.waitForFunction(({id,before})=>Math.abs(__bf.units.byId(id)?.seat?.turret?.yaw-before)>.005,{id:identity.id,before},{timeout:15000});
  if(process.env.BFE_TEST_MOUNTED_FIRE==='1')await mountedFireReadiness(actor,observer,identity);
  await actor.keyboard.press('KeyE');
  await actor.waitForFunction(()=>!__bf.playerUnit.seat&&__bf.client.entity.previous?.controlMode===0,null,{timeout:15000});
  await observer.waitForFunction(({id,generation})=>{const u=__bf.units.byId(id);return u?.generation===generation&&!u.seat;},identity,{timeout:15000});
  console.log('Stationary turret own checkpoint, local aim, observer and exit passed',JSON.stringify({choice,identity,prediction}));
}

// Instrument existing callbacks after they execute; never assign gameplay state.
async function mountedFireReadiness(actor,observer,identity) {
  await observer.evaluate(identity=>{
    const combat=__bf.client.entity.combat,launch=combat.onLaunch;
    window.__mountedRemote=[];
    combat.onLaunch=function(event,...args){
      launch?.call(this,event,...args);
      if(event.ownerId===identity.id&&event.ownerGeneration===identity.generation&&__mountedRemote.length<128)
        __mountedRemote.push({fireId:event.fireId,tick:event.launchTick});
    };
    window.__restoreMountedRemote=()=>combat.onLaunch=launch;
  },identity);
  await actor.evaluate(()=>{
    const host=__bf.client.host,receive=host.onEntities,prediction=__bf.client.mountedWeapons;
    if(!prediction)throw Error('mounted weapon prediction is not installed');
    const predict=prediction.predict,shots=__bf.predictedShots,fire=shots.fire;
    window.__mountedSamples=[];window.__mountedLocal=[];
    host.onEntities=bytes=>setTimeout(()=>{try{receive(bytes);}catch(error){host.fail(error);}},180);
    prediction.predict=function(...args){
      const result=predict.apply(this,args),entry=this.saved[0];
      if(entry&&__mountedSamples.length<1024)__mountedSamples.push({tick:result,authority:__bf.worldClock.tick,
        baseline:{wait:entry.state.wait,heat:entry.state.heat,clip:entry.state.clip},
        predicted:{wait:entry.weapon.wait,heat:entry.weapon.heat,clip:entry.weapon.clip}});
      return result;
    };
    shots.fire=function(edge,...args){
      const start=performance.now(),result=fire.call(this,edge,...args);
      if(edge.pressed&&__mountedLocal.length<64)__mountedLocal.push({fireId:edge.id,result,elapsed:performance.now()-start,
        authority:__bf.worldClock.tick,pending:__bf.client.pending.length});
      return result;
    };
    window.__restoreMountedFire=()=>{host.onEntities=receive;prediction.predict=predict;shots.fire=fire;};
  });
  let evidence;
  try {
    await actor.bringToFront();
    await actor.waitForTimeout(400);
    for(let n=0;n<6;n++){
      await actor.mouse.down();await actor.waitForTimeout(180);await actor.mouse.up();await actor.waitForTimeout(220);
    }
    await actor.waitForFunction(()=>__mountedLocal.some(s=>s.result),null,{timeout:5000});
    const local=await actor.evaluate(()=>__mountedLocal.filter(s=>s.result));
    await observer.waitForFunction(ids=>__mountedRemote.some(s=>ids.includes(s.fireId)),local.map(s=>s.fireId),{timeout:10000});
    evidence=await actor.evaluate(()=>({local:__mountedLocal,samples:__mountedSamples}));
    evidence.remote=await observer.evaluate(()=>__mountedRemote);
  } catch(error){
    console.log('mounted-readiness-failure',await actor.evaluate(()=>({local:__mountedLocal,samples:__mountedSamples.slice(-20),weapon:__bf.playerUnit.seat?.turret?.weapon?.name})));
    throw error;
  } finally {
    await actor.mouse.up();await actor.evaluate(()=>window.__restoreMountedFire?.());
    await observer.evaluate(()=>window.__restoreMountedRemote?.());
  }
  const advancing=evidence.samples.filter(s=>s.tick>s.authority&&s.baseline.wait>0&&s.predicted.wait<s.baseline.wait-1e-5);
  assert.ok(advancing.length,'shared mounted cooldown advances locally beyond the delayed authoritative checkpoint');
  assert.ok(evidence.samples.some(s=>s.predicted.heat!==s.baseline.heat||s.predicted.clip!==s.baseline.clip),'shared heat or ammunition advances locally during ordinary fire');
  assert.ok(evidence.local.some(s=>s.result&&s.pending>0),'ordinary press produces an immediate local cosmetic with unacknowledged input');
  console.log('Mounted fire readiness and matched remote presentation passed',JSON.stringify({local:evidence.local,remote:evidence.remote,advancing:advancing.slice(0,8)}));
}
