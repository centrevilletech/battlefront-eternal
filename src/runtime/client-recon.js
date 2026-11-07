// Transport for ClientPrediction's private F3 recon session. The retained
// source checkpoint is republished before packet application; neither this
// host nor the displayed RemoteDroids resource advances a controller tick.
import { ClientReconClassMirror, ClientReconStateMirror, ClientWeaponsMirror,
  ClientArsenalTailMirror, putClientReconClass, putClientReconState,
  putClientWeapons, putClientArsenalTail, syncClientReconState,
  CLIENT_RECON_CLASS_WORDS, CLIENT_RECON_STATE_WORDS, CLIENT_RECON_STATE_OFFSETS, CLIENT_WEAPONS_WORDS,
  CLIENT_ARSENAL_TAIL_WORDS, CLIENT_ARSENAL_TAIL_OFFSETS } from '../sim/gen/client.js';
import { ReconSalvoMirror } from '../sim/gen/recon.js';
import { putWeapon, syncWeapon, publishWeapon, WeaponMirror, WEAPON_WORDS } from '../sim/gen/combat.js';
import { packPredictionWeaponDefinition } from './client-content.js';
const VECTORS=['position','velocity','eye','aim','target'], SOURCE=['position','velocity','eye','aimDir','target'];
const NO_VIEW=Object.freeze({});
export class ClientRecon {
  constructor(bridge) {
    this.bridge=bridge;this.state=new ClientReconStateMirror();this.salvo=new ReconSalvoMirror();
    this.head=new ClientWeaponsMirror();this.tail=new ClientArsenalTailMirror();
    // The authority checkpoint has one fixed state block and the largest
    // admitted arsenal tail. Prediction and restore reuse those same bytes;
    // no clone of the droid, camera, vectors or weapon graph belongs to a frame.
    this.saved=new Float64Array(CLIENT_RECON_STATE_WORDS+CLIENT_WEAPONS_WORDS+32*WEAPON_WORDS+CLIENT_ARSENAL_TAIL_WORDS);
    this.salvoView={def:null,at:this.salvo.at.clone(),x:this.salvo.x.clone(),y:this.salvo.y.clone(),z:this.salvo.z.clone(),left:0,wait:0};
    this.mirrors=Array.from({length:32},()=>new WeaponMirror());
    this.context=null;this.droid=null;this.strikes=[];this.active=false;this.savedWords=0;
  }
  // A new droid/life resolves authored classes and exact definition ids once.
  // No mutable frame serializes a property tree or creates an isolated mesh.
  configure(context,droid) {
    if(this.context===context&&this.droid===droid&&this.generation===droid.networkGeneration)return null;
    this.context=context;this.droid=droid;this.renderView=context.view;this.simView=context.simulationView;this.generation=droid.networkGeneration;
    const guns=droid.arsenal.weapons,row=new ClientReconClassMirror(),words=new Array(CLIENT_RECON_CLASS_WORDS).fill(0);
    row.count=Math.min(32,guns.length);row.idsAt=words.length;
    const definitions=[];this.strikes.length=0;
    for(let i=0;i<row.count;i++){
      words.push(this.bridge.defs.weapon.id(guns[i].levels[0]));definitions.push(guns[i].levels[0]);
      for(const level of guns[i].levels)if(level.strike)this.strikes.push(level);
    }
    // Authored muzzle position is immutable input to the recon core. The
    // native gun and strike definitions retain it beside their class handles.
    // Mesh transforms are resources and never become controller prerequisites.
    const muzzle=context.firePoints?.[droid.def.GeometryName];
    if(muzzle)row.muzzle.fromArray(muzzle);else row.muzzle.set(0,0,0);
    row.definitionAt=words.length;
    packPredictionWeaponDefinition(words,{droid:{...droid.def,weapons:definitions}});
    putClientReconClass(null,row,words,0,null);return words;
  }
  // captureRemoteReplay's numeric fields already have fixed native layouts.
  // Copy facts only; the kernel owns zoom, flight, aim, countdown and replay.
  capture(target,at,view=NO_VIEW) {
    const c=this.context,d=this.droid,s=this.state;
    for(let i=0;i<VECTORS.length;i++)s[VECTORS[i]].copy(d[SOURCE[i]]);
    // Named floating stores preserve V8's unboxed field representation.
    s.yaw=d.yaw;s.pitch=d.pitch;s.health=d.health;s.dead=d.dead;s.countdown=d.countdown;
    s.needsAim=Boolean(d.poseMissing);s.team=d.team;s.piloted=c.piloted===d;
    s.lookX=c.lookX;s.lookY=c.lookY;s.zoom=Boolean(c.zoomStep);s.firstPerson=c.firstPerson;
    s.hasCamera=Boolean(c.simulationView&&c.aimCamera);
    // PerspectiveCamera always owns a numeric aspect. A nullable numeric
    // expression would force its double through a tagged temporary each frame.
    if(c.aimCamera)s.aspect=c.aimCamera.aspect;else s.aspect=1;
    if(s.hasCamera){s.camera.position.copy(c.aimCamera.position);s.camera.rotation.copy(c.aimCamera.quaternion);
      s.camera.fov=c.simulationView.fov;s.camera.reach=c.simulationView.reach;s.camera.fresh=c.simulationView.fresh;}
    else{s.camera.position.set(0,0,0);s.camera.rotation.set(0,0,0,1);s.camera.fov=0;s.camera.reach=1;s.camera.fresh=true;}
    s.viewFirst=view.firstPerson;s.hasViewAspect=view.aspect!=null;s.viewAspect=view.aspect??0;
    s.hasWaterLevel=c.water?.level!=null;s.waterLevel=c.water?.level??0;
    // A live salvo borrows its registered definition and fixed vectors.
    // Optional generated mirrors are primed before sync, including a salvo
    // first created by prediction rather than by an earlier server packet.
    s.salvo=null;
    if(d.salvo){const salvo=this.salvo,source=d.salvo;
      salvo.strike=this.strikes.indexOf(source.def);
      if(salvo.strike<0)throw Error('recon salvo definition is not registered');
      salvo.at.copy(source.at);salvo.x.copy(source.x);salvo.y.copy(source.y);salvo.z.copy(source.z);
      salvo.left=source.left;salvo.wait=source.wait;s.salvo=salvo;
    }
    putClientReconState(null,s,target,at,null);
    // The RNG already retains a typed seed word. Transfer that source
    // directly, without constructing another scalar checkpoint observation.
    target[at+CLIENT_RECON_STATE_OFFSETS.seed]=c.random.stateWords[0];
    if(this.stateBuffer!==target.buffer){this.stateBuffer=target.buffer;this.stateSpan=new Float64Array(target.buffer,target.byteOffset+at*8,CLIENT_RECON_STATE_WORDS);}
    this.saved.set(this.stateSpan,0);
  }
  weapons(target,at) {
    const guns=this.droid.arsenal.weapons;this.head.count=Math.min(guns.length,32);
    putClientWeapons(null,this.head,target,at,null);at+=CLIENT_WEAPONS_WORDS;
    for(let i=0;i<this.head.count;i++,at+=WEAPON_WORDS)putWeapon(this.bridge,guns[i],target,at,null);
    this.tail.primary=this.droid.arsenal.selected[0];this.tail.secondary=this.droid.arsenal.selected[1];
    this.tail.zoomStep=0;this.tail.barrelsCount=0;putClientArsenalTail(null,this.tail,target,at,null);
    const words=CLIENT_WEAPONS_WORDS+this.head.count*WEAPON_WORDS+CLIENT_ARSENAL_TAIL_WORDS;
    if(this.weaponBuffer!==target.buffer||this.weaponWords!==words){this.weaponBuffer=target.buffer;this.weaponWords=words;this.weaponSpan=new Float64Array(target.buffer,target.byteOffset+(at+CLIENT_ARSENAL_TAIL_WORDS-words)*8,words);}
    this.saved.set(this.weaponSpan,CLIENT_RECON_STATE_WORDS);
    this.savedWords=words;return words;
  }
  // Native reconstruction completes the authority checkpoint even when its
  // wire record omitted derived eye and target vectors. Restore that complete
  // source block after prediction, rather than the resource's initial zeros.
  acceptAuthority(words,at){
    if(this.authorityBuffer!==words.buffer||this.authorityAt!==at){this.authorityBuffer=words.buffer;this.authorityAt=at;this.authoritySpan=new Float64Array(words.buffer,words.byteOffset+at*8,CLIENT_RECON_STATE_WORDS);}
    this.saved.set(this.authoritySpan,0);
  }
  // Only fields still read by page resources return. Numerical history and
  // controller state stay private; publication retains every vector/weapon.
  publish(words,at,weaponsAt) {
    const c=this.context,d=this.droid;this.state.salvo=this.salvo;const s=syncClientReconState(null,words,at,0,this.state);
    for(let i=0;i<VECTORS.length;i++)d[SOURCE[i]].copy(s[VECTORS[i]]);
    d.yaw=s.yaw;d.pitch=s.pitch;d.health=s.health;d.dead=s.dead;d.countdown=s.countdown;
    d.poseMissing=s.needsAim;
    c.lookX=s.lookX;c.lookY=s.lookY;c.zoomStep=s.zoom?1:0;c.firstPerson=s.firstPerson;c.random.stateWords[0]=s.seed;
    c.piloted=s.piloted&&!d.dead?d:null;c.view=c.piloted?this.renderView:null;c.simulationView=c.piloted?this.simView:null;
    if(s.hasCamera&&c.simulationView&&c.aimCamera){c.aimCamera.position.copy(s.camera.position);c.aimCamera.quaternion.copy(s.camera.rotation);
      c.aimCamera.aspect=s.aspect;c.simulationView.fov=s.camera.fov;c.simulationView.reach=s.camera.reach;c.simulationView.fresh=s.camera.fresh;}
    // Salvos are semantic resources. Native predictions can create one on a
    // destruct edge, so reserve its fixed vectors at this droid's admission.
    if(s.salvo){const salvo=this.salvoView;salvo.def=this.strikes[s.salvo.strike];salvo.at.copy(s.salvo.at);
      salvo.x.copy(s.salvo.x);salvo.y.copy(s.salvo.y);salvo.z.copy(s.salvo.z);salvo.left=s.salvo.left;salvo.wait=s.salvo.wait;d.salvo=salvo;
    }else d.salvo=null;
    // The context list is a resource index, not another strike scheduler.
    // Keep it consistent with the published salvo without replacing its array;
    // restore removes a speculative strike before the next packet is applied.
    const atStrike=c.strikes.indexOf(d);
    if(d.salvo&&atStrike<0)c.strikes.push(d);
    else if(!d.salvo&&atStrike>=0){for(let i=atStrike+1;i<c.strikes.length;i++)c.strikes[i-1]=c.strikes[i];c.strikes.length--;}

    const count=words[weaponsAt];let row=weaponsAt+CLIENT_WEAPONS_WORDS;
    for(let i=0;i<count;i++,row+=WEAPON_WORDS){const weapon=d.arsenal.weapons[i],mirror=syncWeapon(this.bridge,words,row,0,this.mirrors[i]);
      publishWeapon(weapon,mirror);weapon.level=mirror.level;weapon.def=weapon.levels[mirror.level];}
    d.arsenal.selected[0]=words[row+CLIENT_ARSENAL_TAIL_OFFSETS.primary];
    d.arsenal.selected[1]=words[row+CLIENT_ARSENAL_TAIL_OFFSETS.secondary];
  }
  // Restoring publishes the saved authority facts only. It advances neither
  // a native tick nor a resource camera, and needs no additional WASM call.
  restore(){if(this.active){this.publish(this.saved,0,CLIENT_RECON_STATE_WORDS);this.active=false;}}
}
