import {publishPointLight} from "./rust-render/point-inputs.js";
import {sceneComponent,sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import { ownedBasicEffect } from './rust-render/effect-material.js';
import * as THREE from "three";
import { radarRelation } from "./targeting-facts.js";
import { MobilePosts } from "./mobile-posts.js";
import { buildRegion, inRegion } from "./regions.js";

// Command posts, following the 2004 game.
//
// Soldiers inside a post's capture region count for their team's capture
// team (src/registry.js captureTeam: an army for itself, a local team for the
// one army it is friendly with, for nobody when it is friendly with both, for
// itself when with neither). N soldiers of a team capture at 1 + 1/2 + ...
// (N terms) times the base rate. An owned post that other teams stand in is
// neutralized over NeutralizeTime at the sum of their rates (defenders don't
// slow it); when they leave, that progress is lost at once. A neutral post
// fills toward one team over CaptureTime; another team must first drain it;
// two teams present freezes it; an empty post keeps its progress. A post
// without a capture region (Tatooine's Tusken campfires, Hoth's probe droid
// drop) never changes hands. The hologram shows the owner's logo, coloured as
// the viewer's team sees the owner, and goes dark while a change is about to
// land; a post whose class has no HoloOdf has none, and one without a
// MapTexture (or a MapScale of 0) isn't drawn on the maps. A command vehicle
// (the AT-AT, the AT-TE) is a post too while it lives (src/mobile-posts.js):
// all() and owned() count it after the placed posts; it has no capture
// region or hologram, and the maps draw it as the vehicle it is.
//
// A destructible post (a class with MaxHealth: Hoth's shield generator,
// Geonosis' Techno Union ships and forward command center, Endor's bunker
// control panel) is also a building (src/props.js ObjectivePost). It has
// no capture region; when the building is destroyed the post goes inactive
// for the rest of the battle: no spawns, no bleed
// value, no AI objective, off the maps, its hologram dark, and it is
// announced lost (docs/objectives.md).

import {EffectPhase} from './effects-gpu.js';
import {PostFrame as F,PostState as S,PostTeam as T,VisualLimits as L} from './presentation/visual-effects-layout.js';
const BANKS=[['input',Float64Array],['counts',Float64Array],['art',Uint8Array],['beam_positions',Float32Array],['beam_normals',Float32Array],['beam_uv',Float32Array],['beam_indices',Uint16Array]];

class Post {
  constructor(desc, rules, index,phase) {
    this.desc = desc;this.index=index;
    // Hoth's world places one post with no name; the network names posts,
    // so it takes its place in the list, the same on every machine.
    this.name = desc.name || `#${index}`;
    this.label = desc.displayName || desc.label;
    this.team = desc.team;
    this.rules = rules;
    this.matrix = new THREE.Matrix4().fromArray(desc.matrix);
    this.position = new THREE.Vector3().fromArray(this.matrix.elements,12);
    this.anchor = new THREE.Vector3().fromArray(desc.hologram);
    this.capture = desc.capture ? buildRegion(desc.capture) : null;
    // On the maps: its class's MapTexture at MapScale (extracts without
    // them draw every post).
    this.mapped=index<L.POSTS&&Boolean(phase.input[index*F.STRIDE+F.MAPPED]);
    // Neutralize progress (0..1) against the owner, or capture progress (0..1)
    // toward `toward` while neutral.
    this.neutralize = 0;
    this.progress = 0;
    this.toward = 0;
    this.counts = new Map();
    this.contested = false;
    // A destructible one's building (CommandPosts.attachBuildings), and
    // whether the post still stands.
    this.building = null;
    this.active = true;
  }

  // Whether a point is in its capture region (src/regions.js, the
  // original game's region test).
  contains(point, scratch) {
    return Boolean(this.capture) && inRegion(this.capture, point, scratch);
  }

  // Seconds until the current capture or neutralize lands, or Infinity.
  remaining() {
    this.owner.writeCounts();this.owner.phase.kernel.remaining(this.owner.posts.length);
    return this.index<L.POSTS?this.owner.phase.input[this.index*F.STRIDE+F.REMAINING]:Infinity;
  }

}

// The hologram over a post: the owner's logo (spinning, flickering, colored
// by relation), a beam, a flare, and a light.
class Hologram {
  constructor(post, look, logos, textures, settings) {
    this.settings = settings;
    this.post = post;
    this.look = look;
    this.group = new THREE.Group();sceneProperty(this.group,"visible",post.index<L.POSTS);
    sceneTransform(this.group.position.copy(post.anchor));
    this.logo = new THREE.Group();
    sceneComponent(this.logo.position,"y","=",look.height);
    sceneTransform(this.logo.scale.setScalar(look.size));
    this.group.add(this.logo);
    this.logos = logos;
    this.logoMeshes = new Map();this.logoRecords=[];
    this.fade = 0; // 0..1, the logo's fade in
    this.flicker = 1;
    this.shownTeam = null;

    this.color = new THREE.Color(1, 1, 1);
    const beamTexture = gradientTexture();
    const phase=post.owner.phase,at=post.index*F.STRIDE;phase.refresh();
    const beamGeometry=new THREE.BufferGeometry();beamGeometry.setAttribute('position',new THREE.BufferAttribute(phase.beam_positions.slice(),3));beamGeometry.setAttribute('normal',new THREE.BufferAttribute(phase.beam_normals.slice(),3));beamGeometry.setAttribute('uv',new THREE.BufferAttribute(phase.beam_uv.slice(),2));beamGeometry.setIndex(new THREE.BufferAttribute(phase.beam_indices.slice(),1));beamGeometry.addGroup(0,phase.beam_indices.length,0);
    this.beam = new THREE.Mesh(
      beamGeometry,
      ownedBasicEffect('basic', { visible: true,
        map: beamTexture,
        color: this.color,
        transparent: true,
        opacity:phase.input[at+F.BEAM],
        blending: THREE.AdditiveBlending,
        depthWrite: false,
        side: THREE.DoubleSide,
        fog: true,
      }),
    );
    this.group.add(this.beam);
    this.flare = new THREE.Sprite(
      ownedBasicEffect('sprite', { visible: true,
        map: textures.flare,
        color: this.color,
        transparent: true,
        opacity:phase.input[at+F.FLARE],
        blending: THREE.AdditiveBlending,
        depthWrite: false,
      }),
    );
    sceneTransform(this.flare.scale.setScalar(1.6));
    sceneComponent(this.flare.position,"y","=",0.15);
    this.group.add(this.flare);
    // Explicit emission halo keeps saturated team colors glowing even when
    // their translucent core falls below the scene bloom's white threshold.
    // Normal depth testing keeps the halo behind architecture.
    const haloMaterial = ownedBasicEffect('sprite', { visible: true,
      map: haloTexture(), color: this.color, transparent: true,
      blending: THREE.AdditiveBlending, depthWrite: false,
    });
    this.halo = new THREE.Sprite(haloMaterial);
    sceneTransform(this.halo.scale.fromArray(phase.input,at+F.HALO_SCALE));
    sceneComponent(this.halo.position,"y","=",phase.input[at+F.HALO_HEIGHT]);
    this.group.add(this.halo);
    // Lights the world per vertex like the game's point lights: N.L times
    // (1 - d^2 / r^2) of the colour.
    this.light = new THREE.PointLight(this.color, look.lightIntensity, look.lightRadius, 1);
    sceneComponent(this.light.position,"y","=",1);
    this.group.add(this.light);
  }

  // This post's own copy of a side's logo (its own material: color and
  // opacity differ per post).
  logoFor(side) {
    if (!this.logoMeshes.has(side)) {
      const make = this.logos.get(side);
      const logo = make ? make() : null;
      this.logoMeshes.set(side, logo);this.logoRecords.push({name:side,logo});
      if (logo) this.logo.add(logo);
    }
    return this.logoMeshes.get(side);
  }

  write(phase,dt,time,relation,camera) {
    const x=phase.input,at=this.post.index*F.STRIDE;if(this.post.index>=L.POSTS)return;
    x[at+F.DT]=dt;x[at+F.TIME]=time;x[at+F.OWNER]=this.post.team??0;x[at+F.ACTIVE]=this.post.active===false?0:1;
    x[at+F.TURN_ON_TIME]=this.post.rules.holoTurnOnTime;camera.position.toArray(x,at+F.CAMERA);this.group.position.toArray(x,at+F.POSITION);
    const color=this.look[relation]||this.look.neutral;
    for(let k=0;k<3;k++)x[at+F.COLOR+k]=color[k];
  }
  publish(phase,teamNames) {
    const x=phase.input,at=this.post.index*F.STRIDE;
    sceneProperty(this.group,"visible",this.post.index<L.POSTS&&Boolean(x[at+F.ACTIVE]));if(!this.group.visible)return;
    const owner=this.post.team;this.fade=x[at+F.FADE];this.flicker=x[at+F.FLICKER];this.shownTeam=owner;
    this.color.fromArray(x,at+F.COLOR);sceneComponent(this.logo.rotation,"y","=",x[at+F.ROTATION]);
    const side=owner?(teamNames[owner]||''):null;if(side)this.logoFor(side);
    for(let k=0;k<this.logoRecords.length;k++){const entry=this.logoRecords[k],logo=entry.logo;if(!logo)continue;sceneProperty(logo,"visible",entry.name===side);if(logo.visible){logo.userData.material.color.copy(this.color);logo.userData.material.opacity=x[at+F.IMAGE];}}
    this.beam.material.color.copy(this.color);this.flare.material.color.copy(this.color);this.halo.material.color.copy(this.color);
    this.halo.material.opacity=x[at+F.HALO];sceneProperty(this.halo,"visible",x[at+F.REACH]>0&&(this.settings?.get('bloom')??true));
    this.beam.material.opacity=x[at+F.BEAM];this.flare.material.opacity=x[at+F.FLARE];
    sceneProperty(this.beam,"visible",x[at+F.REACH]>0);sceneProperty(this.flare,"visible",x[at+F.REACH]>0);this.light.color.copy(this.color);this.light.intensity=x[at+F.LIGHT];publishPointLight(this.light);
  }
}

let sharedHalo,artPhase;
// Texture shells only retain native pixels. The resource loader consumes
// these authored sampler flags; no canvas or browser texel math is needed.
function art(){if(!artPhase)artPhase=new EffectPhase('PagePostEffects',BANKS,'{}');artPhase.refresh();return artPhase.art;}
function haloTexture(){
  if(sharedHalo)return sharedHalo;
  sharedHalo=new THREE.DataTexture(art().slice(0,128*128*4),128,128);
  sharedHalo.magFilter=sharedHalo.minFilter=THREE.LinearFilter;sceneProperty(sharedHalo,"needsUpdate",true);return sharedHalo;
}
export function gradientTexture(){
  const texture=new THREE.DataTexture(art().slice(128*128*4),4,64);texture.flipY=true;
  texture.magFilter=texture.minFilter=THREE.LinearFilter;sceneProperty(texture,"needsUpdate",true);return texture;
}

export class CommandPosts {
  // logos: side name -> a function making a new copy of that side's logo
  // (a group whose userData.material is its material).
  // teams: who is whose friend (src/registry.js: relation, locals,
  // captureTeam), settable later; without it every other team is an enemy.
  constructor(scene, map, { logos, textures, playerTeam, settings, teams = null, headless = false }) {
    const rules = map.commandPost || { neutralizeTime: 12, captureTime: 10, holoTurnOnTime: 2.2 };
    this.phase=new EffectPhase('PagePostEffects',BANKS,JSON.stringify({look:map.hologram||{},posts:map.commandPosts||[]}));
    this.posts=(map.commandPosts||[]).map((desc,index)=>new Post(desc,rules,index,this.phase));
    for(const post of this.posts)post.owner=this;
    this.countTarget=0;this.countEntries=0;
    // Map.forEach supplies scalar facts without allocating entry pairs.
    this.writeCount=(population,team)=>{if(this.countEntries===256)return;const at=this.countTarget+S.TEAMS+this.countEntries++*T.STRIDE;this.phase.counts[at+T.TEAM]=team;this.phase.counts[at+T.POPULATION]=population;};
    this.playerTeam = playerTeam;
    this.teams = teams;this.teamRelation=(a,b)=>this.teams.relation(a,b);this.teamLocals=t=>this.teams.locals(t);
    this.teamNames = Object.fromEntries(Object.entries(map.mission?.teams || {}).map(([team, t]) => [Number(team), (t.name||'').toLowerCase()]));
    this.holograms = !headless && map.hologram
      ? this.posts.filter((post) => !("holoOdf" in post.desc) || post.desc.holoOdf).map((post) => new Hologram(post, map.hologram, logos, textures, settings))
      : [];
    for (const hologram of this.holograms) scene.add(hologram.group);
    this.time = 0;
    this.textures = textures;
    this.scratch = new THREE.Vector3();
    this.events = [];
    // The command vehicles' posts (src/mobile-posts.js), after the placed
    // ones, as the original game appends theirs to its list.
    this.mobile = null;
    this.listed = this.posts;
  }

  // The destructible posts' buildings (src/props.js LiveObjects.objectives),
  // each its post's by name; the building takes the post's team, for the
  // friendly fire factor and the AI.
  attachBuildings(live) {
    for (const building of live?.objectives || []) {
      const post = this.posts.find((each) => each.desc.name && each.desc.name === building.desc.post);
      if (!post) continue;
      post.building = building;
      building.post = post;
      building.team = post.team;
      // The post object is the building: the AI arrives within its
      // collision radius (src/ai/classic/patrol.js), not at its middle,
      // which is inside the generator.
      post.radius = building.sphere(this.scratch);
    }
  }

  // The vehicles (src/vehicles.js Vehicles) whose command classes are posts
  // while they live, with those classes' post data (ai-mission.json
  // commandClasses).
  attachVehicles(system, classes = null) {
    this.mobile = system ? new MobilePosts(system, classes || {}) : null;
    this.listed = this.mobile?.posts.length ? [...this.posts, ...this.mobile.posts] : this.posts;
  }

  // Every post the battle counts and the AI walks: the placed ones, then
  // the command vehicles' as they are now (inactive while none lives).
  all() {
    return this.listed;
  }

  // A remote page derives mobile announcements from its replicated hulls.
  presentMobileEvents() {
    return this.mobile?.presentEvents() ?? [];
  }

  // The player changed teams: every post and hologram is recoloured from his
  // new side the next time it is drawn.
  setPlayerTeam(team) {
    this.playerTeam = team;
  }

  // How a team looks to the player: friendly, enemy, neutral or locals.
  relation(team) {
    return this.look(this.playerTeam, team);
  }

  // How a team looks to the viewer's team (src/targeting-facts.js radarRelation).
  look(viewer, team) {
    const teams = this.teams;
    return teams ? radarRelation(viewer, team, this.teamRelation, this.teamLocals) : radarRelation(viewer, team);
  }

  writeCounts(){
    this.phase.refresh();const x=this.phase.counts;
    for(let i=0;i<this.posts.length&&i<L.POSTS;i++){
      const post=this.posts[i],at=i*S.STRIDE;x[at+S.OWNER]=post.team??0;x[at+S.NEUTRALIZE]=post.neutralize;x[at+S.PROGRESS]=post.progress;x[at+S.TOWARD]=post.toward;
      x[at+S.NEUTRALIZE_TIME]=post.rules.neutralizeTime;x[at+S.CAPTURE_TIME]=post.rules.captureTime;
      this.countTarget=at;this.countEntries=0;post.counts.forEach(this.writeCount);x[at+S.COUNT]=this.countEntries;
    }
  }
  present(dt,camera){
    this.writeCounts();const phase=this.phase;phase.input[F.TIME]=this.time;
    for(const h of this.holograms)h.write(phase,dt,this.time,this.relation(h.post.team),camera);
    phase.kernel.frame(this.posts.length,Math.random);phase.refresh();
    if(this.textures.logo)this.textures.logo.offset.y=phase.input[F.LOGO_OFFSET];
    for(const h of this.holograms)h.publish(phase,this.teamNames);
  }

  // The post whose capture region holds this point, if any.
  at(point) {
    return this.posts.find((post) => post.active !== false && post.contains(point, this.scratch)) || null;
  }

  // The posts a team holds, its living command vehicles' included.
  owned(team) {
    return this.all().filter((post) => post.team === team && post.active !== false);
  }
}
