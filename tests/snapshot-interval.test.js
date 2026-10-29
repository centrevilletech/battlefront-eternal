import test from 'node:test';
import assert from 'node:assert/strict';
import { SnapshotInterval, renderedViewTick } from '../src/runtime/snapshot-interval.js';
import { InputLead } from '../src/runtime/reconcile.js';
for (const spacing of [1,3]) test(`${60/spacing}Hz snapshots interpolate across actual sample ticks at 144Hz render`,()=>{
  const interval=new SnapshotInterval();let next=0;
  const positions=[];
  for(let time=6;time<120;time+=60/144) {
    while(interval.needsSample(time)){interval.apply(next);next+=spacing;}
    const position=interval.previous+(interval.current-interval.previous)*interval.alpha(time);
    assert.ok(Math.abs(position-time)<1e-9);
    positions.push(position);
  }
  for(let i=1;i<positions.length;i++)assert.ok(Math.abs(positions[i]-positions[i-1]-60/144)<1e-9);
});
test('sample interpolation holds during starvation and resumes with bracketing samples',()=>{
  const interval=new SnapshotInterval();interval.apply(3);interval.apply(6);
  assert.equal(interval.alpha(4.5),.5);assert.equal(interval.alpha(9),1);
  interval.apply(9);interval.apply(12);assert.equal(interval.alpha(9.5),1/6);
  interval.reset();assert.equal(interval.needsSample(9),true);
});
test('input lead provisions measured round trip before late acknowledgements',()=>{
  for(const rtt of [0,.05,.1,.2]) {
    const lead=new InputLead({maxLead:24});lead.arrive(600,10+rtt/2);
    const target=lead.target(10+rtt/2,600,rtt);
    assert.ok(target>=600+rtt*60+1);
    assert.ok(target<=624);
  }
});

test('20Hz arrival jitter still renders between bracketing snapshots at 60Hz', async()=>{
  const { PlayoutClock }=await import('../src/runtime/playout.js');
  const playout=new PlayoutClock({minDelay:6,maxDelay:18}), interval=new SnapshotInterval(),queue=[];
  const arrivals=Array.from({length:120},(_,i)=>({tick:(i+1)*3,at:(i+1)*.05+(i%7)*.003}));
  let index=0, samples=0;
  for(let frame=0;frame<350;frame++) {
    const now=frame/60;
    while(index<arrivals.length&&arrivals[index].at<=now){const a=arrivals[index++];queue.push(a.tick);playout.arrive(a.tick,now);}
    const clock=playout.advance(1/60);
    while(queue.length&&interval.needsSample(clock))interval.apply(queue.shift());
    if(interval.previous!==null&&interval.previous<=clock&&interval.current>=clock){
      const position=interval.previous+(interval.current-interval.previous)*interval.alpha(clock);
      assert.ok(Math.abs(position-clock)<1e-9);samples++;
    }
  }
  assert.ok(samples>330);
});


test('shot view stamps follow rendered time rather than the future bracketing sample',()=>{
  for(const rtt of [0,50,100,200]) {
    const latest=600, clock=latest-6-rtt/2000*60+.4;
    const interval=new SnapshotInterval();interval.apply(Math.floor(clock/3)*3);interval.apply(Math.floor(clock/3)*3+3);
    assert.equal(renderedViewTick(clock,latest),Math.floor(clock));
    assert.ok(renderedViewTick(clock,latest)<interval.current);
  }
  assert.equal(renderedViewTick(NaN,600),0);assert.equal(renderedViewTick(-2,600),0);assert.equal(renderedViewTick(700,600),600);
});
