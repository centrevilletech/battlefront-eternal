import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import {actorAsset,actorNode} from './actor-resources.js';
import {EFFECT} from './rust-render/effect-layout.js';
import * as THREE from "three";
import { lighting } from "./materials.js";

import {EffectPhase} from './effects-gpu.js';
import {ShieldFrame as F} from './presentation/visual-effects-layout.js';
const BANKS=[['input',Float64Array],['impacts',Float64Array],['powers',Float32Array],['blasts',Float32Array]];
const HITS = 6;



// A narrow emission veil outside the field gives the rim an actual halo even
// without post bloom. A small sphere preserves depth at foreground edges.


export class ShieldBubble {
  constructor(scene, look, texture, settings = null) {
    this.look = look;
    this.settings = settings;
    this.phase=new EffectPhase('PageShield',BANKS);
    this.fade=0;this.time=0;this.birth=10;this.pop=0;this.flash=0;this.hitIndex=0;
    const map = look.Texture ? texture(look.Texture) : null;
    this.map=map;this.impacts=new Float64Array(HITS*4);this.powers=new Float32Array(HITS);this.blasts=new Float32Array(HITS);
    this.impacts.set(this.phase.impacts);
    // Sphere topology, defaults, side/blend policy and repeat sampler aliases
    // are native cold recipes. The page copies native hit and charge publications.
    const make=(row,width,height)=>{
      const asset=actorAsset({kind:'sphere',radius:1,widthSegments:width,heightSegments:height});
      const mesh=actorNode(asset,null),owner={map};
      const source={mesh,asset,owner,family:'actorEffect',visibility:'anchor',values:new Float64Array(row.LENGTH),nativeKind:row.ID,center:null,authored:look,repeat:true};
      source.prepareActor=camera=>this.prepareActor(source,row,camera);mesh.userData.effectModel=source;return mesh;
    };
    this.mesh=make(EFFECT.ACTOR_SHIELD,80,48);this.mesh.name='droideka-shield';sceneProperty(this.mesh,"visible",false);sceneProperty(this.mesh,"renderOrder",4);scene.add(this.mesh);
    this.aura=make(EFFECT.ACTOR_AURA,48,24);this.aura.name='droideka-shield-aura';sceneProperty(this.aura,"renderOrder",4);sceneProperty(this.aura,"visible",false);
    // Parenting makes external visibility/layer changes apply to both shells.
    this.mesh.add(this.aura);
    sceneTransform(this.aura.scale.setScalar(1.035));
  }

  facts(droideka,position=droideka.position) {
    this.phase.refresh();const x=this.phase.input,shield=droideka.shield;
    position.toArray(x,F.POSITION);x[F.YAW]=droideka.aimYaw;
    const offset=shield?.def.ShieldOffset;
    x[F.OFFSET]=offset?.[0]??0;x[F.OFFSET+1]=offset?.[1]??1;x[F.OFFSET+2]=offset?.[2]??0;
    x[F.RADIUS]=shield?.def.ShieldRadius??this.look.Radius??1.25;x[F.HEIGHT]=this.look.Height??x[F.RADIUS];
    x[F.MAX]=shield?.max??0;x[F.VALUE]=shield?.value??0;x[F.ON]=shield?.on?1:0;x[F.DEAD]=droideka.dead?1:0;x[F.ROLLED]=droideka.rolled?1:0;
    x[F.ENHANCED]=this.settings?.get('droidekaEffects')!=='classic'?1:0;x[F.FADE_IN]=this.look.FadeIn??0;x[F.FADE_OUT]=this.look.FadeOut??0;
  }
  publish() {
    const x=this.phase.input;this.fade=x[F.FADE];this.time=x[F.TIME];this.birth=x[F.BIRTH];this.pop=x[F.POP];this.flash=x[F.FLASH];this.hitIndex=x[F.HIT_INDEX];
    this.charge=x[F.CHARGE];this.enhanced=Boolean(x[F.ENHANCED]);
    this.impacts.set(this.phase.impacts);this.powers.set(this.phase.powers);this.blasts.set(this.phase.blasts);
    sceneTransform(this.mesh.position.fromArray(x,F.CENTER));sceneTransform(this.mesh.scale.fromArray(x,F.SCALE));
    sceneProperty(this.mesh,"visible",Boolean(x[F.VISIBLE]));sceneProperty(this.aura,"visible",Boolean(x[F.AURA]));
  }
  hit(source,amount,droideka,kind=null) {
    this.facts(droideka);const x=this.phase.input;x[F.COMMAND]=1;x[F.AMOUNT]=amount;x[F.HAS_SOURCE]=source?1:0;
    if(source){x[F.SOURCE]=source.x;x[F.SOURCE+1]=source.y;x[F.SOURCE+2]=source.z;}
    x[F.EXPLOSION]=kind==='explosion'?1:0;this.phase.frame();this.publish();
  }
  update(dt,droideka,options) {
    this.facts(droideka,options?.position??droideka.position);this.phase.input[F.DT]=dt;this.phase.input[F.COMMAND]=0;
    this.phase.frame();this.publish();
  }
  hide(){this.phase.refresh();this.phase.input[F.COMMAND]=2;this.phase.frame();this.publish();}
  // ShieldBubble.onBeforeRender's inside test is a view fact. Generated rows
  // publish it and hit history without material uniform-name dispatch.
  prepareActor(source,row,camera){const v=source.values;v.set(camera.projectionMatrix.elements,row.PROJECTION_MATRIX);
    this.phase.refresh();camera.position.toArray(this.phase.input,F.CAMERA);this.mesh.position.toArray(this.phase.input,F.CENTER);this.mesh.scale.toArray(this.phase.input,F.SCALE);
    this.phase.input[F.COMMAND]=3;this.phase.frame();
    v[row.SHIELD_TIME]=this.time;v[row.SHIELD_FADE]=this.fade;v[row.BIRTH]=this.birth;v[row.INSIDE]=this.phase.input[F.INSIDE];v[row.FLASH]=this.flash;
    if(row===EFFECT.ACTOR_SHIELD){v.set(camera.matrixWorldInverse.elements,row.VIEW_MATRIX);v[row.CHARGE]=this.charge??1;v[row.ENHANCED]=this.enhanced===false?0:1;
      lighting.sunDirection.value.toArray(v,row.SUN_DIRECTION);lighting.sunColor.value.toArray(v,row.SUN_COLOR);v.set(this.impacts,row.IMPACTS);v.set(this.powers,row.POWERS);v.set(this.blasts,row.BLASTS);}}
  dispose(){this.phase.dispose();this.mesh.removeFromParent();}
}
