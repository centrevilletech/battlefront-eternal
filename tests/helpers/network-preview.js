// Fixtures submit the same raw facts and coarse frame as a live page. These
// helpers contain no steering, smoothing or interpolation equations.
import {OwnPreview} from '../../src/runtime/prediction-view.js';
import {createPageNetwork} from '../../src/runtime/page-network.js';
import {NetworkPreview} from '../../src/runtime/network-preview.js';
import {NetPreviewFrame as F,NetPreviewOutput as O} from '../../src/runtime/network-layout.js';
export class PreviewFrame{
  constructor(){this.network=createPageNetwork();this.network.previewSetup();this.phase=new NetworkPreview(this.network);this.game={client:{pending:[],predicted:false,inputClock:{alpha:1}},soldier:{aimYaw:0,aimPitch:0,maxTurn:2,lock:0,posture:'stand',air:null,dive:0,pitchLimits:[-1,1]},combat:{dead:false,zoom:1},settings:{mouseScale:1,get:()=>false},controls:{looks:[]},playerUnit:{walker:false,presentation:{tickYaw:0}},ownPreview:new OwnPreview(),driver:null,gunner:null};}
  present(pending=[],looks=null,scale=1,invert=false,alpha=1){
    const g=this.game;g.client.pending=pending;g.controls.looks=looks||[];g.settings.mouseScale=scale;g.settings.get=()=>invert;g.previewPresentAlpha=alpha;g.client.inputClock.alpha=alpha;
    this.phase.prepare(g,true,alpha);if(looks===null)this.network.previewFrame[F.HAS_LOOSE]=0;
    this.network.present(0);this.phase.publish(g);return this.network.previewOutput;
  }
  dispose(){this.network.dispose();}
}
const owners=new WeakMap();
function frame(resource){let owner=owners.get(resource);if(!owner){owner=new PreviewFrame();owners.set(resource,owner);}return owner;}
export function previewGunner(gunner,pending,looks,scale,invert,alpha=1){const h=frame(gunner);h.game.gunner=gunner;h.present(pending,looks,scale,invert,alpha);}
export function previewDriver(driver,pending,looks,scale,invert){const h=frame(driver);h.game.driver=driver;h.present(pending,looks,scale,invert);}
export function previewSteer(turret,state,pending=[],looks=null,scale=1,invert=false){const h=frame(state);const t=Object.assign(Object.create(turret),state);h.game.gunner={active:true,turret:t,previewState:state};h.present(pending,looks,scale,invert);return state;}
// Camera-only fixtures may set preview deltas explicitly. Publish their
// shown angles through the native frame instead of duplicating its sums.
export function shownGunner(gunner){const h=frame(gunner);h.game.gunner=gunner;h.phase.prepare(h.game,true,1,true,false);const n=h.network;n.present(0);h.phase.prepare(h.game,true,1,false,true);n.previewOutput[O.TURRET_PREVIEW_YAW]=gunner.previewYaw;n.previewOutput[O.TURRET_PREVIEW_PITCH]=gunner.previewPitch;n.present(0);h.phase.publish(h.game);}

// Fixed-rate intent fixtures express their pointer counts at the standard
// command tick. The native servo remains the only integration/filter owner.
export function stepTurretPreview(state,dt,turn,pitch,zoom=1){return previewSteer({...state,turnRate:state.turnRate/zoom},state,[{look:[[-turn*2048*dt,-pitch*2048*dt]],mouseScale:1,invertMouse:false}],null);}
