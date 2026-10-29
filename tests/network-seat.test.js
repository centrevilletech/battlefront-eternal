import test from 'node:test';
import assert from 'node:assert/strict';
import { cycleNetworkSeat } from '../src/runtime/network-seat.js';
import { IntentControls, blankIntent } from '../src/units.js';

function fixture() {
  const vehicle={alive:true,seats:Array.from({length:3},()=>({lease:null,aimYaw:1,pitch:.1,lookYaw:1,lookPitch:.1})),stickX:1,stickY:1,
    restAim(index){this.rested=index;},clampAim(index){this.clamped=index;}};
  const human={networkHuman:true,intent:blankIntent()},bot={networkHuman:false};
  const lease={vehicle,index:0,unit:human}, botLease={vehicle,index:1,unit:bot};
  human.seat=lease;bot.seat=botLease;
  vehicle.seats[0].lease=lease;vehicle.seats[1].lease=botLease;
  const system={move(occupant,index){if(vehicle.seats[occupant.index].lease===occupant)vehicle.seats[occupant.index].lease=null;vehicle.seats[index].lease=occupant;occupant.index=index;}};
  return {vehicle,human,bot,system};
}
test('network seat cycling swaps bots into the vacated seat and preserves other humans',()=>{
  const {vehicle,human,bot,system}=fixture();
  assert.equal(cycleNetworkSeat(system,human),true);
  assert.equal(human.seat.index,1);assert.equal(bot.seat.index,0);
  assert.equal(vehicle.seats[0].lease.unit,bot);assert.equal(vehicle.seats[1].lease.unit,human);
  assert.equal(vehicle.stickX,0);assert.equal(vehicle.stickY,0);
  vehicle.seats[2].lease={unit:{networkHuman:true}};
  assert.equal(cycleNetworkSeat(system,human),true);
  assert.equal(human.seat.index,0,'skip the occupied human seat');assert.equal(vehicle.rested,0);
  assert.equal(cycleNetworkSeat(system,human,-1),true);
  assert.equal(human.seat.index,1,'reverse cycling also skips humans');
  vehicle.seats[0].lease={unit:{networkHuman:true}};
  assert.equal(cycleNetworkSeat(system,human),false,'a full human crew cannot steal seats');
});
test('previous weapon cycling passes through network intent once',()=>{
  const unit={intent:blankIntent()}, controls=new IntentControls(unit);
  unit.intent.previousPrimary=true;
  assert.equal(controls.take('previousPrimary'),true);
  assert.equal(controls.take('previousPrimary'),false);
});
