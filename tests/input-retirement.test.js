import test from 'node:test';import assert from 'node:assert/strict';
import {retireAuthoritativeInputs} from '../src/runtime/input-retirement.js';
import {replayTicks} from '../src/runtime/reconcile.js';
const command=(sequence,down)=>({sequence,tick:sequence+10,generation:1,down,actions:[],look:[]});
function fixture(){const late=[],resets=[];return {pending:[command(1,['KeyW']),command(2,['KeyW']),command(3,[])],held:['KeyS'],heldGeneration:1,replayView:{gamepad:{axes:[1]}},lead:{acknowledge:v=>late.push(v)},predictionWindow:{reset:()=>resets.push('foot')},vehicleWindow:{reset:()=>resets.push('vehicle')},late,resets};}
test('discarded future input cannot become a consumed acknowledgement or held movement',()=>{
 const c=fixture();retireAuthoritativeInputs(c,{ack:1,ackTick:11,discardedInput:2,heldInput:[],viewInput:{},generation:1,tick:11});
 assert.deepEqual(c.late,[0]);assert.equal(c.ackInput,1);assert.equal(c.discardedInput,2);assert.deepEqual(c.pending.map(x=>x.sequence),[3]);assert.deepEqual(c.held,[]);assert.deepEqual(c.replayView,{});assert.deepEqual(c.resets,['foot','vehicle']);
 const keys=[];replayTicks(c.pending,11,1,c.held,(_,input)=>keys.push(input.down),c.replayView);assert.ok(keys.every(down=>down.length===0));
});
test('receipt-only feedback is not a retirement operation and later consumed input still measures lateness',()=>{
 const c=fixture();retireAuthoritativeInputs(c,{ack:0,discardedInput:2,heldInput:[],generation:1,tick:10});assert.deepEqual(c.late,[]);
 retireAuthoritativeInputs(c,{ack:3,ackTick:14,discardedInput:2,heldInput:[],generation:1,tick:14});assert.deepEqual(c.late,[1]);assert.equal(c.pending.length,0);assert.equal(c.resets.length,2);
});
test('legacy packets without authoritative held context derive keys only from consumed commands',()=>{
 const c=fixture();retireAuthoritativeInputs(c,{ack:1,discardedInput:2,generation:1,tick:11});assert.deepEqual(c.held,['KeyW']);assert.deepEqual(c.late,[0]);
});
