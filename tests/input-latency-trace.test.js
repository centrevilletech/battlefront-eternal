import test from 'node:test';
import assert from 'node:assert/strict';
import {InputLatencyTrace} from '../src/runtime/input-latency-trace.js';
test('trace separates sampling, prediction, drawn submission, ACK and authority confirmation',()=>{
  let now=100;const trace=new InputLatencyTrace({now:()=>now,timeOrigin:1700000000000});
  trace.noteInput({timeStamp:1700000000090});now=105;
  const command={epoch:1,generation:2,sequence:3,tick:10},metadata={playerId:1,generation:2,channel:0,fireId:4};
  trace.command(command);now=107;trace.predicted({...command,id:4},command,metadata);
  now=110;trace.submit(false);assert.equal(trace.report().metrics.inputToSubmitMs,undefined);
  now=115;trace.submit(true);trace.submit(true);
  now=155;trace.ack({epoch:1,generation:2,ack:3,ackTick:12});trace.ack({epoch:1,generation:2,ack:3,ackTick:12});
  now=170;trace.confirmed(metadata,{tick:11});trace.confirmed(metadata,{tick:11});
  const m=trace.report().metrics;
  for(const [name,value]of Object.entries({eventDispatchMs:10,inputToSampleMs:15,sampleToPredictionMs:2,inputToSubmitMs:25,sampleToAckMs:50,executionLateTicks:2,predictionToConfirmationMs:63,shotExecutionLateTicks:1})){
    assert.equal(m[name].max,value,name);assert.equal(m[name].total,1,name);
  }
});
test('bounded trace isolates lives and dual held channels and ignores replayed samples',()=>{
  let now=0;const trace=new InputLatencyTrace({now:()=>now,capacity:2});
  for(let i=0;i<4;i++){
    const command={epoch:1,generation:2,sequence:i,tick:i};trace.command(command);trace.command(command);
    trace.predicted(command,command,{playerId:1,generation:2,channel:i%2,fireId:0});now++;
  }
  trace.confirmed({playerId:1,generation:2,channel:1,fireId:0},{tick:3});
  assert.equal(trace.shots[0].confirmed,false);assert.equal(trace.shots[1].confirmed,true);
  assert.equal(trace.commands.size,2);assert.equal(trace.shots.length,2);
  assert.equal(trace.report().metrics.sampleToPredictionMs.total,4);assert.equal(trace.report().metrics.sampleToPredictionMs.retained,2);
  trace.command({epoch:1,generation:3,sequence:4,tick:4});
  trace.ack({epoch:1,generation:2,ack:4,ackTick:7});assert.equal(trace.report().metrics.sampleToAckMs,undefined);
  assert.equal(trace.shots.length,0);
});
