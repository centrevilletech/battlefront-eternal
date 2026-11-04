import assert from 'node:assert/strict';

// Ordinary spawn, walking, use and seat-cycle commands. No teleport, direct
// board(), health edits or replacement physics are allowed in this gate.
export async function mountedGameGate([actor, observer]) {
  await actor.bringToFront();
  await actor.evaluate(()=>{__bf.controls.locked=true;if(__bf.menu.isOpen)__bf.menu.close(true);});
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
    await actor.evaluate(choice=>__bf.spawnPort.request(choice),choice);
    await actor.waitForFunction(c=>__bf.playerUnit.alive&&__bf.playerUnit.className===c.className&&__bf.playerTeam===c.team,choice,{timeout:60000});
    await actor.waitForFunction(()=>!__bf.spawnScreen?.isOpen&&__bf.controls.enabled&&!__bf.soldier.air&&__bf.soldier.lock<=0,null,{timeout:15000});
    await actor.bringToFront();
    await actor.evaluate(()=>{if(__bf.menu.isOpen)__bf.menu.close(true);__bf.controls.locked=true;});
  }
  if(!await actor.evaluate(()=>__bf.playerUnit.alive))await spawn();
  const first=await actor.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation}));
  await observer.waitForFunction(({id,generation})=>{const u=__bf.units.byId(id);return u?.alive&&u.generation===generation;},first,{timeout:15000});
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
  await actor.keyboard.press('KeyE');
  await actor.waitForFunction(c=>__bf.playerUnit.seat?.vehicle?.aiTarget.id===c.vehicleId,choice,{timeout:15000});
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
  if(process.env.BFE_TEST_CPU==='1')await actor.evaluate(()=>document.dispatchEvent(new MouseEvent('mousemove',{movementX:80,movementY:20,bubbles:true})));
  else await actor.mouse.move(460,190,{steps:8});
  await actor.mouse.click(460,190);
  await actor.waitForFunction(before=>Math.abs(__bf.driver.vehicle.seats[__bf.playerUnit.seat.index].aimYaw-before)>.01,before,{timeout:15000});
  // Disconnect while occupying the hull, then prove the server releases it.
  const tick=await observer.evaluate(()=>__bf.worldClock.tick);
  await actor.evaluate(()=>__bf.client.host.dispose());
  await observer.waitForFunction(({id,vehicleId,tick})=>{const v=__bf.vehicles.vehicles.find(v=>v.aiTarget.id===vehicleId);return __bf.worldClock.tick>tick&&!__bf.units.byId(id)?.seat&&v?.seats.every(s=>s.lease?.unit?.id!==id);},{...identity,...choice,tick},{timeout:15000});
  console.log('Actual respawn, pilot prediction, driving, passenger aim and occupied disconnect passed',JSON.stringify({choice,pilot,identity}));
}
