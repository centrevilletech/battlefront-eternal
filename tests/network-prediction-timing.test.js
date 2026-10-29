import test from 'node:test';
import assert from 'node:assert/strict';
import {measurePrediction} from '../tools/prediction-timing.mjs';

for(const fps of [60,144])test(`real soldier responds before ACK at ${fps} FPS with 100ms RTT and 20Hz snapshots`,()=>{
 const result=measurePrediction({fps});
 assert.ok(result.responseMs.every(ms=>ms!==null&&ms<=34),JSON.stringify(result));
 assert.ok(result.firstVisibleMs+75<result.firstAckMs,'movement precedes delayed authoritative acknowledgement');
 assert.ok(result.correctionMeters.p95<.025,'quantized reconciliation remains below 2.5cm');
 assert.ok(result.residualMeters<.01,'stationary prediction converges after release');
});
test('actual movement under jitter and lost replaceable state stays responsive and converges',()=>{
 const result=measurePrediction({fps:144,jitterMs:30,lossEvery:7,snapshotLossEvery:11});
 assert.ok(result.responseMs.every(ms=>ms!==null&&ms<=34),JSON.stringify(result));
 assert.ok(result.firstVisibleMs<result.firstConsumedMs,'prediction moves before server consumes new input');
 assert.ok(result.correctionMeters.p95<.2,JSON.stringify(result));
 assert.ok(result.correctionMeters.max<.4,'lost movement transitions cannot cause a large teleport');
 assert.ok(result.residualMeters<.01,'lost updates do not leave residual drift');
});
