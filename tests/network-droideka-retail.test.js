import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { Vector3 } from 'three';
import { buildActor } from '../src/actor.js';
import { loadClips } from '../src/anim.js';
import { Droideka } from '../src/droideka.js';
import { CollisionWorld } from '../src/collision.js';
import { captureDroidekaState as capture, restoreDroidekaState as restore } from '../src/runtime/network-droideka-state.js';
const base=resolve(process.env.BFE_ASSET_ROOT||'public','sides/cis/actors/cis_walk_droideka');
const available=existsSync(base+'.json');
function buffer(path){const b=readFileSync(path);return b.buffer.slice(b.byteOffset,b.byteOffset+b.byteLength);}
function fixture(){
  const model=JSON.parse(readFileSync(base+'.json'));
  const actor=buildActor(model,buffer(base+'.bin'),buffer(base+'_index.bin'),()=>null,{caster(){}});
  const clips=loadClips(JSON.parse(readFileSync(base+'_anims.json')),buffer(base+'_anims.bin'),model.bones.map(b=>b.name));
  const world=new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]),new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
  const d=new Droideka(world,{AnimationSpeed:2,BallRadius:.65},clips,actor);
  d.arm([{aimers:['hp_gun_1','hp_gun_2']},{base:'shield',MaxShield:100,AddShield:-5,AddShieldOff:2}]);d.spawn(0,0,0,0);return d;
}
test('owned retail Droideka root motion and independent collision muzzles remain exact after checkpoints',{skip:!available},()=>{
  const a=fixture(),b=fixture();let folded=false,rolled=false,unfolded=false,furthest=0;
  for(let i=0;i<540;i++){
    if(i%29===0)restore(b,capture(a));
    const input={forward:i<220?1:i<300?-1:0,right:i%71<9?.5:0,use:i===90||i===320};
    for(const d of [a,b]){d.look(Math.sin(i)*3,Math.cos(i)*2);d.tick(input,1/60);if(i===10)d.toggleShield();d.sampleMuzzles(1/60);}
    if(i%7===0){const x=a.nextGun(new Vector3()),y=b.nextGun(new Vector3());assert.ok(x&&y);assert.ok(x.distanceTo(y)<1e-9);}
    assert.deepEqual(capture(b),capture(a),`retail solver tick ${i}`);
    folded ||= a.mode===11;rolled ||= a.mode===12;unfolded ||= a.mode===13;furthest=Math.max(furthest,a.position.length());
  }
  assert.ok(folded&&rolled&&unfolded);assert.ok(furthest>5,'actual root motion and rolling moved the body');
});
