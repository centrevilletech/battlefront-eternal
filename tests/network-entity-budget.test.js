import {test} from 'node:test';
import assert from 'node:assert/strict';
import {EntityBudget,readStateBudget,DEFAULT_STATE_BYTES_PER_SECOND} from '../src/runtime/network-entity-budget.js';
test('bounded offered payload refreshes a whole field each publication without unbounded recovery bursts',()=>{
 const budget=new EntityBudget({now:0});let own=0,remote=0;
 for(let now=0;now<10000;now+=50){assert(budget.take(1100,now));own++;while(budget.take(1100,now))remote++;}
 assert.equal(own,200);assert(remote>=700,'about four datagrams per 20 Hz publication');assert(budget.offeredBytes<=DEFAULT_STATE_BYTES_PER_SECOND*10+6667);
 assert.equal(budget.available(60000),6667);for(let n=0;n<6;n++)assert(budget.take(1100,60000));assert(!budget.take(1100,60000));assert(budget.take(67,60000));assert(!budget.take(1,60000));
 assert.equal(budget.available(59000),0,'clock reversal cannot refill twice');assert.equal(budget.available(60000),0);
 assert.throws(()=>budget.take(1101,60000));
});
test('a downstream drop refunds its offered bytes within the burst bound',()=>{
 const budget=new EntityBudget({now:0});
 assert(budget.take(1100,0));assert.equal(budget.available(0),6667-1100);
 budget.refund(1100,0);assert.equal(budget.available(0),6667);assert.equal(budget.offeredBytes,0);
 budget.refund(1100,0);assert.equal(budget.available(0),6667,'a refund cannot exceed the burst');
 assert.throws(()=>budget.refund(1101,0));
});
test('operator state budget is validated and derives a bounded burst',()=>{
 assert.deepEqual(readStateBudget({}),{bytesPerSecond:100000,burstBytes:6667});
 assert.deepEqual(readStateBudget({stateBytesPerSecond:120000}),{bytesPerSecond:120000,burstBytes:8000});
 assert.deepEqual(readStateBudget({stateBytesPerSecond:26000}),{bytesPerSecond:26000,burstBytes:2200});
 assert.deepEqual(readStateBudget({stateBytesPerSecond:500000}),{bytesPerSecond:500000,burstBytes:11000});
 for(const value of [1000,'120000',1e7,12.5,-1])assert.throws(()=>readStateBudget({stateBytesPerSecond:value}),/stateBytesPerSecond/);
 const budget=new EntityBudget({...readStateBudget({stateBytesPerSecond:26000}),now:0});
 assert(budget.take(1100,0));assert(budget.take(1100,0));assert(!budget.take(1100,0));
});
