import test from 'node:test';
import assert from 'node:assert/strict';
import {Vector3} from 'three';
import {captureEntitySoldier} from '../src/runtime/network-entity-capture.js';
// Single-record codec fixtures remain useful to page decoders. Canonical
// roster/seat capture is exercised by native frozen-store and host cases.
function fixture(){
 const unit={id:1,generation:1,team:1,soldier:{position:new Vector3(),velocity:new Vector3(),aimYaw:0,aimPitch:0,posture:'stand'},combat:{health:100,arsenal:{current:()=>null}}};
 return {unit,capture:{capture:tick=>[captureEntitySoldier(unit,{tick,classId:1})]}};
}
test('health above 16 bits saturates, as Darth Vader\'s does',()=>{
 const {unit,capture}=fixture();
 unit.combat.health=1000000;
 assert.equal(capture.capture(1).find(r=>r.id===1).health,65535);
});
test('health that is not a number fails explicitly',()=>{
 const {unit,capture}=fixture();
 unit.combat.health=Number.NaN;
 assert.throws(()=>capture.capture(1),/not a number/);
});
