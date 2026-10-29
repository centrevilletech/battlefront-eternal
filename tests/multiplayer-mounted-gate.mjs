import assert from 'node:assert/strict';

// Ordinary spawn, walking, use and seat-cycle commands. No teleport, direct
// board(), health edits or replacement physics are allowed in this gate.
export async function mountedGameGate([actor, observer]) {
  await actor.bringToFront();
  await actor.evaluate(()=>{__bf.controls.locked=true;if(__bf.menu.isOpen)__bf.menu.close(true);});
  const choose=()=>{
    const options=[];
    for(const team of Object.keys(__bf.spawnPort.teams()).map(Number)) {
      const className=__bf.spawnPort.roster(team).find(({className})=>Object.values(__bf.data.sides).some(side=>{
        const def=side.units?.[className];return def&&def.base!=='walkerdroid'&&!Number(def.NoEnterVehicles);
      }))?.className;
      if(!className)continue;
      for(const v of __bf.vehicles.vehicles) {
        if(!v.alive||v.flying||v.seats.length<2||v.seats.every(s=>s.lease)||v.controllingTeam>0&&v.controllingTeam!==team)continue;
        for(const p of __bf.spawnPort.posts(team).filter(p=>p.eligible)) {
          const distance=Math.hypot(...p.position.map((n,i)=>n-v.position.toArray()[i]));
          options.push({team,className,post:p.name,vehicleId:v.aiTarget.id,vehicleGeneration:v.aiTarget.generation,distance});
        }
      }
    }
    return options.sort((a,b)=>a.distance-b.distance)[0];
  };
  let choice;
  for(let attempt=0;attempt<60&&!choice;attempt++){choice=await actor.evaluate(choose);if(!choice)await actor.waitForTimeout(250);}
  if(!choice)console.log('mounted-unavailable',await actor.evaluate(()=>({posts:__bf.spawnPort.posts(__bf.playerTeam),vehicles:__bf.vehicles.vehicles.map(v=>({id:v.aiTarget.id,alive:v.alive,team:v.controllingTeam,flying:v.flying,seats:v.seats.map(s=>s.lease?.unit?.id??null)}))})));
  assert.ok(choice,'mission has an available ground pilot seat near an eligible spawn');
  console.log('mounted-spawn-choice',JSON.stringify(choice));
  if(await actor.evaluate(()=>__bf.playerUnit.alive))await actor.evaluate(()=>__bf.spawnPort.suicide());
  await actor.waitForFunction(()=>__bf.spawnPort.humanState().phase==='awaiting',null,{timeout:60000});
  await actor.evaluate(choice=>__bf.spawnPort.request(choice),choice);
  await actor.waitForFunction(c=>__bf.playerUnit.alive&&__bf.playerUnit.className===c.className&&__bf.playerTeam===c.team,choice,{timeout:60000});
  await actor.evaluate(()=>{__bf.controls.locked=true;if(__bf.menu.isOpen)__bf.menu.close(true);});
  let navigation;
  try {
    await actor.keyboard.down('KeyW');
    for(let step=0;step<240;step++) {
      navigation=await actor.evaluate(({vehicleId,vehicleGeneration})=>{
        const v=__bf.vehicles.vehicles.find(v=>v.aiTarget.id===vehicleId&&v.aiTarget.generation===vehicleGeneration);
        if(!v?.alive||!__bf.playerUnit.alive)return {invalid:true};
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
      assert.ok(!navigation.invalid,'actor and selected hull stay alive during approach');
      if(navigation.distance<navigation.radius+2.5)break;
      await actor.waitForTimeout(200);
    }
  } finally {await actor.keyboard.up('KeyW');}
  console.log('mounted-approach',JSON.stringify(navigation));
  assert.ok(navigation.distance<navigation.radius+2.5,'ordinary movement reaches boarding distance');
  await actor.keyboard.press('KeyE');
  await actor.waitForFunction(c=>__bf.playerUnit.seat?.vehicle?.aiTarget.id===c.vehicleId,choice,{timeout:15000});
  // Ordinary seat cycling may exchange a bot pilot with the human passenger.
  if(await actor.evaluate(()=>__bf.playerUnit.seat.index!==0))await actor.evaluate(()=>{__bf.pending.nextPrimary=true;});
  await actor.waitForFunction(()=>__bf.playerUnit.seat?.index===0&&__bf.client.entity.previous?.controlMode===4,null,{timeout:15000});
  const identity=await actor.evaluate(()=>({id:__bf.playerUnit.id,generation:__bf.playerUnit.generation}));
  await observer.waitForFunction(({id,vehicleId})=>__bf.units.byId(id)?.seat?.vehicle?.aiTarget.id===vehicleId,{...identity,...choice},{timeout:15000});
  await actor.evaluate(()=>{const host=__bf.client.host,receive=host.onEntities;window.__restoreMountedTransport=()=>host.onEntities=receive;host.onEntities=bytes=>setTimeout(()=>{try{receive(bytes);}catch(e){host.fail(e);}},180);});
  await actor.keyboard.down('KeyW');
  await actor.waitForFunction(()=>{const p=__bf.client.vehiclePrediction,v=__bf.driver.vehicle;return p&&p.predictionTick>__bf.worldClock.tick&&p.position.distanceTo(v.position)>.02;},null,{timeout:15000});
  const pilot=await actor.evaluate(()=>({tick:__bf.client.vehiclePrediction.predictionTick,authority:__bf.worldClock.tick,control:__bf.client.entity.previous.controlMode}));
  await actor.keyboard.up('KeyW');await actor.mouse.click(320,240);
  await actor.evaluate(()=>window.__restoreMountedTransport());await actor.waitForTimeout(700);
  await actor.evaluate(()=>{__bf.pending.nextPrimary=true;});
  await actor.waitForFunction(()=>__bf.playerUnit.seat?.index>0&&__bf.client.entity.previous?.controlMode===2,null,{timeout:15000});
  const before=await actor.evaluate(()=>__bf.driver.vehicle.seats[__bf.playerUnit.seat.index].aimYaw);
  await actor.mouse.move(460,190,{steps:8});await actor.mouse.click(460,190);
  await actor.waitForFunction(before=>Math.abs(__bf.driver.vehicle.seats[__bf.playerUnit.seat.index].aimYaw-before)>.01,before,{timeout:15000});
  await actor.keyboard.press('KeyE');
  await actor.waitForFunction(()=>!__bf.playerUnit.seat&&__bf.client.entity.previous?.controlMode===0,null,{timeout:15000});
  await observer.waitForFunction(({id})=>!__bf.units.byId(id)?.seat,identity,{timeout:15000});
  console.log('Actual pilot prediction, passenger aim and exit passed',JSON.stringify({choice,pilot,identity}));
}
