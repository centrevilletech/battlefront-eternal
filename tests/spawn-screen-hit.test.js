import test from 'node:test';
import assert from 'node:assert/strict';
import {ClassicView} from '../src/spawn-screen/classic/view.js';
import {InterfaceSurface} from '../src/interface/surface.js';
import {SpawnModel} from '../src/spawn-screen/model.js';
import {SpawnInput} from '../src/spawn-screen/input.js';
import {FakeSpawnPort,geonosisData} from './spawn-screen-fake-port.js';

test('the entire drawn spawn button accepts clicks at wide, standard and docked viewport sizes',()=>{
  for(const [width,height] of [[1024,768],[1280,720],[900,900],[650,720]]){
    const hud=Object.create(InterfaceSurface.prototype),quads=[],labels=[];
    Object.assign(hud,{renderer:{domElement:{clientWidth:width,clientHeight:height}},strings:{},
      fonts:{lineHeight:()=>18,draw:(batch,m,x,y,text)=>labels.push({m,x,y,text})},
      texture:name=>name,batch:{quad:(m,x0,y0,x1,y1)=>quads.push({m,x0,y0,x1,y1})},
      spawnMapPlace:()=>({x:0,y:0,size:1}),postIconSize:15,mapPoint:()=>[0,0]});
    const model=new SpawnModel({port:new FakeSpawnPort(geonosisData()),random:()=>0});model.open();
    const view=Object.create(ClassicView.prototype);Object.assign(view,{hud,model});
    const requests=[],input=new SpawnInput({model,hit:(x,y)=>view.hit(x,y),spawn:()=>requests.push(model.commit()),height:()=>hud.height});
    view.drawSpawn(hud,view.layout);
    const click=(point)=>{
      // DOM coordinates round-trip through the same responsive mapping.
      const local=hud.toInterface(point.x*width/1024,point.y*height/hud.height);
      assert.deepEqual(view.hit(local.x,local.y),{kind:'spawn'},`${width}x${height}: ${JSON.stringify(local)}`);
      input.move(local.x,local.y);input.button(0,true);
      assert.ok(requests.at(-1)?.className);model.uncommit();
    };
    // Actual rendered nine-slice centres cover both edges, corners and middle.
    for(const q of quads)click(hud.toPlane(q.m,(q.x0+q.x1)/2,(q.y0+q.y1)/2));
    // The flat SPAWN label sits over the tipped background.
    const label=labels[0];assert.equal(label.text,'SPAWN');
    for(const x of [-30,0,30])click(hud.toPlane(label.m,label.x+x,label.y+9));
    const box=view.layout.spawn,m=view.frame(hud,box.x,box.y,box.tilt,'x');
    const outside=hud.toPlane(m,box.w/2+10,0);
    assert.equal(view.hit(outside.x,outside.y),null,'neighboring background remains inert');
    assert.equal(requests.length,quads.length+3);
  }
});
