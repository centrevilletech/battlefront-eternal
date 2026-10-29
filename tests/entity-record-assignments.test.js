import test from 'node:test';
import assert from 'node:assert/strict';
import { encodeEntityRecord, encodeEntityPacket, decodeEntityPacket, checkCorrectionRecordFits } from '../src/runtime/network-entity-codec.js';

test('mount, combat, item and turret extensions retain distinct wire meanings in one packet',()=>{
  const base={generation:1,tick:10};
  const records=[
    {...base,kind:'mount',id:1,slot:0,tilt:0,roll:0,walkPhase:0,walkBlend:0,seats:[]},
    {...base,kind:'combat',id:2,schema:1,payload:new Uint8Array([5,6])},
    {...base,kind:'item',id:3,active:false},
    {...base,kind:'turret',id:4,building:0,state:'built',team:1,health:100,yaw:0,pitch:0,gunYaw:0,gunPitch:0},
  ];
  assert.deepEqual(records.map(r=>encodeEntityRecord(r)[0]),[7,8,9,11], 'deployed combat kind8 cannot be reused by turret integration');
  const decoded=decodeEntityPacket(encodeEntityPacket({epoch:1,packetId:1,tick:10,records}));
  assert.deepEqual(decoded.records,records,'extensions survive a shared packet without interpreting another kind body');
});

test('correction fit check agrees with encoding at the size boundary',()=>{
  const record=length=>({kind:'correction',id:1,generation:1,tick:10,schema:1,payload:new Uint8Array(length)});
  for(const length of [0,1050,1051,1052,1100,2000]){
    let encoded=null; try{encoded=encodeEntityRecord(record(length));}catch{}
    if(encoded)assert.doesNotThrow(()=>checkCorrectionRecordFits(record(length).payload),`length ${length} encodes, so it must fit`);
    else assert.throws(()=>checkCorrectionRecordFits(record(length).payload),/cannot fit|invalid/,`length ${length} cannot encode, so it must not fit`);
  }
  assert.throws(()=>checkCorrectionRecordFits([1,2,3]),/invalid correction payload/);
});
