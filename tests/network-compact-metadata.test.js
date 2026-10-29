import test from 'node:test';
import assert from 'node:assert/strict';
import { compactMetadata,expandMetadata,packetParts,personalizePacket } from '../src/runtime/shared-packet.js';
import { BinaryWriter,BinaryReader } from '../src/runtime/packet.js';
const metadata={ack:125,playerId:61,generation:3,alive:true,playerClass:'rep_inf_rifleman',playerTeam:1,seats:[['driver',-1],['gunner',-1]],
 personal:{spawn:{phase:'alive',className:'rep_inf_rifleman',post:'cp1',spawnedOnce:true,deadTime:0,pending:null,ended:false,auto:null},deadTime:0,
 combat:{deathTime:0,origin:[125.123046875,2.5,22.25],pivot:[125.123046875,1.5,22.25],aimDir:[.5,0,.5]},soldier:{settled:[125.123046875,0,22.25],stateTime:1.0625},weapons:[{wait:.03125,kick:.0625},{wait:0,kick:0}]},
 health:{stepMs:0,packetMs:0,bytes:0,tickMs:0,debt:0,dropped:0,late:2,queued:1},extension:'kept'};
test('compact metadata preserves prediction values and extensions with less than half the JSON',()=>{
 const compact=compactMetadata(metadata);
 assert.deepEqual(expandMetadata(JSON.parse(JSON.stringify(compact))),metadata);
 assert.ok(JSON.stringify(compact).length<JSON.stringify(metadata).length*.5);
 assert.equal(expandMetadata(metadata),metadata,'existing object metadata remains supported');
});
test('network personalization expands negotiated compact metadata before replica decoding',()=>{
 const writer=new BinaryWriter();writer.keyframe();writer.begin();writer.value('common');
 const parts=packetParts(writer.finish({tick:125,keyframe:true}));
 const packet=new BinaryReader().read(personalizePacket(parts,compactMetadata(metadata)));
 assert.equal(packet.playerId,61);assert.deepEqual(packet.personal,metadata.personal);assert.equal(packet.compact,undefined);
 assert.throws(()=>expandMetadata({compact:1,values:[]}),/invalid/);
});
