import test from 'node:test';
import assert from 'node:assert/strict';
import {Vector3,Quaternion} from 'three';
import {EntityWorldCapture} from '../src/runtime/network-entity-capture.js';
import {encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
const registry={classes:new Map([[1,'soldier'],[2,'flyer']]),weapons:new Map([[1,'rifle']])};
function fixture(){
 const unit={id:1,generation:1,className:'soldier',team:1,soldier:{position:new Vector3(1,2,3),velocity:new Vector3(),aimYaw:0,aimPitch:0,posture:'stand'},combat:{health:100,arsenal:{current:()=>({name:'rifle'})}}};
 const vehicle={name:'flyer',alive:true,flying:true,team:1,aiTarget:{id:2,generation:1},position:new Vector3(),velocity:new Vector3(),quaternion:new Quaternion(),health:1000,flightState:'Flying',power:1};
 const game={units:{all:()=>[unit]},vehicles:{vehicles:[vehicle]}};
 return {unit,vehicle,game,capture:new EntityWorldCapture(game,registry)};
}
test('capture records encode independently and seat lifecycle retains exact version',()=>{
 const {unit,vehicle,capture}=fixture();unit.seat={vehicle,index:0};
 const first=capture.capture(1),seat=first.find(r=>r.kind==='seat');
 const wire=encodeEntityPacket({epoch:1,packetId:1,tick:1,records:first});
 assert.equal(decodeEntityPacket(wire).records.length,4,'soldier, seat, hull and its crew pose');
 assert.equal(capture.capture(2).find(r=>r.kind==='seat').leaseVersion,seat.leaseVersion);
 unit.seat=null;
 const released=capture.capture(3).find(r=>r.kind==='seat');
 assert.equal(released.vehicleId,0);assert.ok(released.leaseVersion>seat.leaseVersion);
 vehicle.alive=false;
 assert.equal(capture.capture(4).find(r=>r.id===2).kind,'tombstone');
 vehicle.alive=true;vehicle.aiTarget.generation++;
 assert.equal(capture.capture(5).find(r=>r.id===2).generation,2);
});
test('a turret occupant stays a soldier whose seat names the turret target; a droideka uses its own pose record',()=>{
 const {unit,capture}=fixture();capture.capture(1);
 unit.seat={turret:{aiTarget:{id:7,generation:3}},index:0};
 const records=capture.capture(2),seat=records.find(r=>r.kind==='seat'&&r.id===1);
 assert.equal(records.find(r=>r.id===1&&r.kind==='soldier').kind,'soldier');
 assert.equal(seat.vehicleId,7);assert.equal(seat.vehicleGeneration,3);assert.equal(seat.seatIndex,0);
 assert.equal(capture.unsupported.length,0);
 unit.seat=null;
 assert.equal(capture.capture(3).find(r=>r.kind==='seat'&&r.id===1).vehicleId,0,'leaving the turret releases the lease');
 unit.walker=true;Object.assign(unit.soldier,{mode:0,reverse:false,dead:false,time:0,roll:0,lean:0,legs:0});
 const walker=capture.capture(4);
 assert.equal(walker.some(r=>r.id===1&&r.kind==='soldier'),false);
 assert.equal(capture.unsupported.length,0);
 const record=walker.find(r=>r.id===1&&r.kind==='droideka');assert.ok(record);
 assert.equal(decodeEntityPacket(encodeEntityPacket({epoch:1,packetId:4,tick:4,records:walker})).records.find(r=>r.id===1).kind,'droideka');
 unit.walker=false;unit.removed=true;
 assert.equal(capture.capture(5).find(r=>r.id===1).kind,'tombstone');
});
test('health overflow fails explicitly',()=>{
 const {unit,capture}=fixture();
 unit.combat.health=70000;
 assert.throws(()=>capture.capture(1),/health outside/);
});
