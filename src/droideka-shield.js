import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import {actorAsset,actorNode} from './actor-resources.js';
import {EFFECT} from './rust-render/effect-layout.js';
import * as THREE from "three";
import { lighting } from "./materials.js";

const HITS = 6;



// A narrow emission veil outside the field gives the rim an actual halo even
// without post bloom. A small sphere preserves depth at foreground edges.


export class ShieldBubble {
  constructor(scene, look, texture, settings = null) {
    this.look = look;
    this.settings = settings;
    this.fade = 0;
    this.time = 0;
    this.birth = 10;
    this.on = false;
    this.pop = 0;
    this.fromPop = 0;
    this.fromFade = 0;
    this.flash = 0;
    this.hitIndex = 0;
    this.offset = new THREE.Vector3();
    this.source = new THREE.Vector3();
    this.cameraLocal = new THREE.Vector3();
    const map = look.Texture ? texture(look.Texture) : null;
    this.map=map;this.impacts=new Float64Array(HITS*4);this.powers=new Float32Array(HITS);this.blasts=new Float32Array(HITS);
    for(let i=0;i<HITS;i++){this.impacts[i*4+2]=1;this.impacts[i*4+3]=-1000;}
    // Sphere topology, defaults, side/blend policy and repeat sampler aliases
    // are native cold recipes. Only hit history and live charge stay here.
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

  place(droideka, position = droideka.position) {
    const [x, y, z] = droideka.shield?.def.ShieldOffset || [0, 1, 0];
    const yaw = droideka.aimYaw;
    this.offset.set(Math.cos(yaw) * x + Math.sin(yaw) * z, y, -Math.sin(yaw) * x + Math.cos(yaw) * z);
    sceneTransform(this.mesh.position.copy(position)).add(this.offset);
  }

  // Called only when gameplay actually absorbs damage; source is optional.
  hit(source, amount, droideka, kind = null) {
    if (!(amount > 0)) return;
    this.place(droideka);
    const power = THREE.MathUtils.clamp(Math.sqrt(amount / Math.max(droideka.shield?.max || 1, 1)) * 3, 0.25, 1.5);
    if (!source || ![source.x, source.y, source.z].every(Number.isFinite)) {
      // Damage without provenance lights the field, without inventing a hit side.
      this.flash = Math.max(this.flash, power);
      return;
    }
    this.source.copy(source).sub(this.mesh.position);
    const radius = droideka.shield?.def.ShieldRadius ?? this.look.Radius ?? 1.25;
    this.source.x /= radius; this.source.y /= this.look.Height ?? radius; this.source.z /= radius;
    if (this.source.lengthSq() < 1e-8) this.source.set(0, 1, 0);
    this.source.normalize();
    const i = this.hitIndex++ % HITS;
    const at=i*4;this.impacts[at]=this.source.x;this.impacts[at+1]=this.source.y;this.impacts[at+2]=this.source.z;this.impacts[at+3]=this.time;
    this.powers[i] = power;
    this.blasts[i] = kind === "explosion" ? 1 : 0;
  }

  update(dt, droideka, { position = droideka.position } = {}) {
    dt = Math.max(dt, 0);
    this.time += dt;
    const shield = droideka.shield;
    const on = Boolean(shield?.on && !droideka.dead && !droideka.rolled);
    const enhanced = this.settings?.get("droidekaEffects") !== "classic";
    if (on !== this.on) {
      this.fromPop = this.pop;
      this.fromFade = this.fade;
      if (on) this.birth = 0;
    }
    this.on = on;
    this.birth += dt;
    const duration = enhanced ? (on ? 0.24 : 0.26) : (on ? this.look.FadeIn : this.look.FadeOut);
    this.fade = THREE.MathUtils.clamp(this.fade + (on ? dt : -dt) / Math.max(duration || 0.15, 1e-3), 0, 1);
    sceneProperty(this.mesh,"visible",this.fade > 0);
    sceneProperty(this.aura,"visible",enhanced && this.mesh.visible);
    this.place(droideka, position);
    const f = this.fade;
    // Native classic curve grows through 1.06 and settles to 1, shrinking off.
    const classic = on ? (f < 0.94 ? f * 1.06 / 0.94 : 1 + (1 - f)) : f;
    const progress = on ? (f - this.fromFade) / Math.max(1 - this.fromFade, 1e-4) : f / Math.max(this.fromFade, 1e-4);
    const expansion = 1 + 2.2 * (progress - 1) ** 3 + 1.2 * (progress - 1) ** 2;
    const pop = enhanced ? (on ? this.fromPop + (1 - this.fromPop) * expansion : this.fromPop * (0.7 + 0.3 * progress)) : classic;
    this.pop = f > 0 ? pop : 0;
    const radius = shield?.def.ShieldRadius ?? this.look.Radius ?? 1.25;
    sceneTransform(this.mesh.scale.set(radius * Math.max(pop, 0.001), (this.look.Height ?? radius) * Math.max(pop, 0.001), radius * Math.max(pop, 0.001)));
    this.flash *= Math.exp(-10 * dt);
    this.enhanced=enhanced;this.charge=shield?.max>0?THREE.MathUtils.clamp(shield.value/shield.max,0,1):0;
  }

  // Gone with its body (off the field): nothing left drawn, nothing to fade.
  hide() {
    this.on = false;
    this.fade = this.pop = 0;
    sceneProperty(this.mesh,"visible",false);
  }

  // ShieldBubble.onBeforeRender's inside test is a view fact. Generated rows
  // publish it and hit history without material uniform-name dispatch.
  prepareActor(source,row,camera){const v=source.values;v.set(camera.projectionMatrix.elements,row.PROJECTION_MATRIX);
    this.cameraLocal.copy(camera.position).sub(this.mesh.position).divide(this.mesh.scale);
    v[row.SHIELD_TIME]=this.time;v[row.SHIELD_FADE]=this.fade;v[row.BIRTH]=this.birth;v[row.INSIDE]=this.cameraLocal.lengthSq()<1?1:0;v[row.FLASH]=this.flash;
    if(row===EFFECT.ACTOR_SHIELD){v.set(camera.matrixWorldInverse.elements,row.VIEW_MATRIX);v[row.CHARGE]=this.charge??1;v[row.ENHANCED]=this.enhanced===false?0:1;
      lighting.sunDirection.value.toArray(v,row.SUN_DIRECTION);lighting.sunColor.value.toArray(v,row.SUN_COLOR);v.set(this.impacts,row.IMPACTS);v.set(this.powers,row.POWERS);v.set(this.blasts,row.BLASTS);}}
  dispose(){this.mesh.removeFromParent();}
}
