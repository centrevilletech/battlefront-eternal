// Unit.updateSpheres' page mirrors. A registry operation observes the whole
// native fleet once; individual bodies only expose retained resource values.
import * as THREE from 'three';
import {UnitCollisionInput as I,UnitCollisionOutput as O,UnitShape as S,UnitShapeKind as K,UnitCollisionLimits as L} from './layout.js';
const TYPES=Object.keys(K).map(key=>key.toLowerCase()),POSTURES=['stand','crouch','prone'];
// The legacy half field has two types. A retained getter keeps numeric
// storage as a double even when a posture changes to a vector half-size.
function half(){return this.type==='box'||this.type==='capsule'?this.halfSize:this.halfNumber;}
export class UnitCollision {
 constructor(owner){this.owner=owner;this.buffer=null;this.refresh();}
 // Resource birth can relocate a vector without growing linear memory.
 // Ordinary frames compare only the memory object and make no getter call.
 refresh(force=false){const owner=this.owner,k=owner.kernel,b=owner.loaded.memory.buffer;
  if(this.buffer===b&&!force)return;
  this.buffer=b;this.input=new Float64Array(b,k.collision_input_ptr(),k.collision_input_len());
  this.output=new Float64Array(b,k.collision_output_ptr(),k.collision_output_len());
  this.shapes=new Float64Array(b,k.collision_shapes_ptr(),k.collision_shapes_len());
 }
 bind(row,unit){
  this.refresh(true);if(unit.f3)return;const previous=this.owner.byUnit.get(unit);if(previous&&previous!==row)previous.unit=null;
  this.owner.byUnit.set(unit,row);row.unit=unit;row.collisionBinding=(row.collisionBinding||0)+1;
  // Capacity is retained before the first observation, including ten limbs.
  row.collisionShapes=Array.from({length:L.SHAPES},()=>{const s={type:'sphere',part:'body',center:new THREE.Vector3(),radius:NaN,halfNumber:NaN,halfSize:new THREE.Vector3(),yaw:NaN};Object.defineProperty(s,'half',{get:half});return s;});
  row.collisionList=row.collisionShapes.slice();while(row.collisionList.length)row.collisionList.pop();
  row.collisionBounds={center:new THREE.Vector3(),radius:NaN};unit.bounds=row.collisionBounds;unit.hitSet=row.collisionList;
 }
 // These are correction facts and resource handles, with no shape math.
 // Current native mount/held samples are joined when the batch observes them.
 write(row,unit){this.refresh();const x=this.input,at=row.id*I.STRIDE,s=unit.soldier;if(at>=x.length)return;
  // A prepared class can be rebound without advancing its tick or life.
  x[at+I.BINDING]=row.collisionBinding;
  x[at+I.ENABLED]=unit.removed?0:1;x[at+I.GENERATION]=unit.generation;x[at+I.TICK]=unit.tickCount;
  x[at+I.POSITION]=s.position.x;x[at+I.POSITION+1]=s.position.y;x[at+I.POSITION+2]=s.position.z;
  x[at+I.YAW]=s.aimYaw;x[at+I.POSTURE]=POSTURES.indexOf(s.posture);x[at+I.AIR]=s.air?1:0;x[at+I.DIVE]=s.dive||0;x[at+I.DEAD]=unit.dead?1:0;
  x[at+I.WALKER]=unit.walker?1:0;x[at+I.MODE]=s.mode||0;x[at+I.SCALE]=unit.rootScale??1;x[at+I.BALL_RADIUS]=s.ballRadius??.65;
  const lease=unit.seat;if(lease&&!lease.posePick)this.owner.lease(unit,lease);
  const vehicle=lease?.vehicle,turret=lease?.turret,held=unit.held;
  x[at+I.SEAT]=lease?1:0;x[at+I.EXPOSED]=lease?.exposed?1:0;
  x[at+I.SOURCE_RIG]=vehicle?lease.sourceRig??-1:-1;x[at+I.SOURCE_SEAT]=lease?.index??0;
  x[at+I.SOURCE_TURRET]=turret?.presentationSource??-1;x[at+I.SOURCE_PROP]=held?.prop.presentationSource??-1;
  const pick=lease?.posePick;x[at+I.CLIP]=held?row.content.ids.get(row.source.clips.get(held.grab.clip))??-1:pick?.clip??-1;
  x[at+I.TIMED]=pick?.timed?1:0;x[at+I.FIXED_TIME]=pick?.time!=null?1:0;x[at+I.TIME]=pick?.time??0;
  x[at+I.NINE]=(turret?.rider||vehicle?.seats[lease.index]?.view.rider)?.ninePose?1:0;
 }
 // Queries can precede the body display. Observe current correction fields
 // and the preceding native source pose in one coarse registry phase.
 observe(riders=true,exclude=null,alive=false){this.refresh();this.input.fill(0);
  for(const row of this.owner.rows){const unit=row.unit;
   if(!unit||unit.f3||(alive?!unit.alive:!unit.alive&&!(unit.dead&&!unit.removed)))continue;
   if(unit.seat&&!riders||unit===exclude||Array.isArray(exclude)&&exclude.includes(unit))continue;
   this.write(row,unit);
  }
  this.owner.kernel.collide_units();this.refresh();this.publish();
 }
 // Only changed stamps publish. Existing vectors and the shape list survive
 // every posture, seat transition and pooled life.
 publish(){this.refresh();const x=this.output,shapes=this.shapes;
  for(const row of this.owner.rows){if(!row.collisionBounds||!x[row.id*O.STRIDE+O.CHANGED])continue;
   const at=row.id*O.STRIDE,n=x[at+O.COUNT],list=row.collisionList;
   row.collisionBounds.center.fromArray(x,at+O.CENTER);row.collisionBounds.radius=x[at+O.RADIUS];
   while(list.length>n)list.pop();while(list.length<n)list.push(row.collisionShapes[list.length]);
   for(let k=0;k<n;k++){const s=list[k],a=(row.id*L.SHAPES+k)*S.STRIDE;
    s.type=TYPES[shapes[a+S.KIND]];s.part=shapes[a+S.HEAD]?'head':'body';s.center.fromArray(shapes,a+S.CENTER);
    s.radius=shapes[a+S.RADIUS];s.halfNumber=shapes[a+S.HALF];s.halfSize.fromArray(shapes,a+S.HALF_SIZE);s.yaw=shapes[a+S.YAW];
   }
  }
 }
}
