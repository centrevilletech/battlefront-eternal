import * as THREE from "three";
import { radarRelation } from "./hud-units.js";
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

const rate = (count) => 2 - 2 ** (1 - count);
const attackRate = (counts, owner) => {
  let sum = 0;
  for (const [team, n] of counts) if (team !== owner && n > 0) sum += rate(n);
  return sum;
};

class Post {
  constructor(desc, rules, index) {
    this.desc = desc;
    // Hoth's world places one post with no name; the network names posts,
    // so it takes its place in the list, the same on every machine.
    this.name = desc.name || `#${index}`;
    this.label = desc.displayName || desc.label;
    this.team = desc.team;
    this.rules = rules;
    this.matrix = new THREE.Matrix4().fromArray(desc.matrix);
    this.position = new THREE.Vector3().setFromMatrixPosition(this.matrix);
    this.anchor = new THREE.Vector3().fromArray(desc.hologram);
    this.capture = desc.capture ? buildRegion(desc.capture) : null;
    // On the maps: its class's MapTexture at MapScale (extracts without
    // them draw every post).
    this.mapped = !("mapTexture" in desc) || (Boolean(desc.mapTexture) && (desc.mapScale ?? 1) > 0);
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

  // Its building destroyed: out of the battle, its owner kept (the
  // original game leaves the team as it was).
  destroy() {
    this.active = false;
    this.counts.clear();
    this.neutralize = 0;
    this.progress = 0;
    this.toward = 0;
    this.contested = false;
    return { kind: "lost", from: this.team, destroyed: true };
  }

  // Whether a point is in its capture region (src/regions.js, the
  // original game's region test).
  contains(point, scratch) {
    return Boolean(this.capture) && inRegion(this.capture, point, scratch);
  }

  // Seconds until the current capture or neutralize lands, or Infinity.
  remaining() {
    const teams = [...this.counts.entries()].filter(([, n]) => n > 0);
    if (this.team) {
      const attack = attackRate(this.counts, this.team);
      return attack ? ((1 - this.neutralize) * this.rules.neutralizeTime) / attack : Infinity;
    }
    if (teams.length !== 1) return Infinity;
    const [team, n] = teams[0];
    if (team !== this.toward && this.progress > 0) return Infinity;
    return ((1 - this.progress) * this.rules.captureTime) / rate(n);
  }

  update(dt) {
    const teams = [...this.counts.entries()].filter(([, n]) => n > 0);
    this.contested = teams.length > 1;
    if (this.team) {
      const attack = attackRate(this.counts, this.team);
      if (!attack) {
        this.neutralize = 0;
        return null;
      }
      this.neutralize += (attack * dt) / this.rules.neutralizeTime;
      if (this.neutralize >= 1) {
        const lost = this.team;
        this.team = 0;
        this.neutralize = 0;
        this.progress = 0;
        this.toward = 0;
        return { kind: "neutralized", from: lost };
      }
      return null;
    }
    if (teams.length !== 1) return null;
    const [team, n] = teams[0];
    const step = (rate(n) * dt) / this.rules.captureTime;
    if (this.progress > 0 && team !== this.toward) {
      this.progress = Math.max(this.progress - step, 0);
      return null;
    }
    this.toward = team;
    this.progress += step;
    if (this.progress >= 1) {
      this.team = team;
      this.progress = 0;
      this.toward = 0;
      return { kind: "captured", by: team };
    }
    return null;
  }
}

// The hologram over a post: the owner's logo (spinning, flickering, colored
// by relation), a beam, a flare, and a light.
class Hologram {
  constructor(post, look, logos, textures, settings) {
    this.settings = settings;
    this.post = post;
    this.look = look;
    this.group = new THREE.Group();
    this.group.position.copy(post.anchor);
    this.logo = new THREE.Group();
    this.logo.position.y = look.height;
    this.logo.scale.setScalar(look.size);
    this.group.add(this.logo);
    this.logos = logos;
    this.logoMeshes = new Map();
    this.fade = 0; // 0..1, the logo's fade in
    this.flicker = 1;
    this.shownTeam = null;

    this.color = new THREE.Color(1, 1, 1);
    const beamTexture = gradientTexture();
    this.beam = new THREE.Mesh(
      new THREE.CylinderGeometry(0.35, 0.05, look.height, 16, 1, true).translate(0, look.height / 2, 0),
      new THREE.MeshBasicMaterial({
        map: beamTexture,
        color: this.color,
        transparent: true,
        opacity: 0.35 * look.beamIntensity,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
        side: THREE.DoubleSide,
        fog: true,
      }),
    );
    this.group.add(this.beam);
    this.flare = new THREE.Sprite(
      new THREE.SpriteMaterial({
        map: textures.flare,
        color: this.color,
        transparent: true,
        opacity: 0.6 * (look.flareIntensity ?? 1),
        blending: THREE.AdditiveBlending,
        depthWrite: false,
      }),
    );
    this.flare.scale.setScalar(1.6);
    this.flare.position.y = 0.15;
    this.group.add(this.flare);
    // Explicit emission halo keeps saturated team colors glowing even when
    // their translucent core falls below the scene bloom's white threshold.
    // Normal depth testing keeps the halo behind architecture.
    const haloMaterial = new THREE.SpriteMaterial({
      map: haloTexture(), color: this.color, transparent: true,
      blending: THREE.AdditiveBlending, depthWrite: false,
    });
    this.halo = new THREE.Sprite(haloMaterial);
    this.halo.scale.set(2.6, Math.max(look.height * 1.4, 2.6), 1);
    this.halo.position.y = look.height * 0.45;
    this.group.add(this.halo);
    // Lights the world per vertex like the game's point lights: N.L times
    // (1 - d^2 / r^2) of the colour.
    this.light = new THREE.PointLight(this.color, look.lightIntensity, look.lightRadius, 1);
    this.light.position.y = 1;
    this.group.add(this.light);
  }

  // This post's own copy of a side's logo (its own material: color and
  // opacity differ per post).
  logoFor(side) {
    if (!this.logoMeshes.has(side)) {
      const make = this.logos.get(side);
      const logo = make ? make() : null;
      this.logoMeshes.set(side, logo);
      if (logo) this.logo.add(logo);
    }
    return this.logoMeshes.get(side);
  }

  update(dt, time, relation, teamNames, camera) {
    const look = this.look;
    const owner = this.post.team;
    this.group.visible = this.post.active !== false;
    if (!this.group.visible) return;
    // Dark while a change is about to land; the new owner's logo fades in.
    const landing = this.post.remaining() <= this.post.rules.holoTurnOnTime;
    if (owner !== this.shownTeam) {
      this.shownTeam = owner;
      this.fade = 0;
    }
    const target = landing ? 0 : 1;
    this.fade += Math.sign(target - this.fade) * Math.min(Math.abs(target - this.fade), dt / look.fadeInTime);
    // A new flicker alpha now and then.
    if (Math.random() < look.flickerRate * 60 * dt) {
      this.flicker = look.flickerAlpha[0] + Math.random() * (look.flickerAlpha[1] - look.flickerAlpha[0]);
    }
    const color = look[relation] || look.neutral;
    this.color.setRGB(color[0] / 255, color[1] / 255, color[2] / 255);
    const distance = camera.position.distanceTo(this.group.position);
    const reach = THREE.MathUtils.clamp((look.visibleDistance - distance) / (look.visibleDistance * 0.1), 0, 1);

    this.logo.rotation.y = time * look.rotateRate * Math.PI * 2;
    const side = owner ? (teamNames[owner] || "").toLowerCase() : null;
    if (side) this.logoFor(side);
    for (const [name, logo] of this.logoMeshes) {
      if (!logo) continue;
      logo.visible = name === side;
      if (logo.visible) {
        logo.userData.material.color.copy(this.color);
        logo.userData.material.opacity = look.imageIntensity * this.flicker * this.fade * reach;
      }
    }
    this.beam.material.color.copy(this.color);
    this.flare.material.color.copy(this.color);
    const emission = this.fade * this.flicker * reach;
    this.halo.material.color.copy(this.color);
    this.halo.material.opacity = 0.48 * emission;
    this.halo.visible = reach > 0 && (this.settings?.get("bloom") ?? true);
    this.beam.material.opacity = 0.35 * look.beamIntensity * emission;
    this.flare.material.opacity = 0.6 * (look.flareIntensity ?? 1) * emission;
    this.beam.visible = reach > 0;
    this.flare.visible = reach > 0;
    this.light.color.copy(this.color);
    this.light.intensity = look.lightIntensity * (owner ? 0.4 + 0.6 * this.fade : 1);
  }
}

let sharedHalo;
function haloTexture() {
  if (sharedHalo) return sharedHalo;
  const size = 128;
  const data = new Uint8Array(size * size * 4);
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
    const radius = Math.hypot((x + 0.5) / size * 2 - 1, (y + 0.5) / size * 2 - 1);
    const alpha = Math.max(0, Math.exp(-radius * radius * 5) - Math.exp(-5));
    data.set([255, 255, 255, Math.round(alpha * 255)], (y * size + x) * 4);
  }
  sharedHalo = new THREE.DataTexture(data, size, size);
  sharedHalo.magFilter = THREE.LinearFilter;
  sharedHalo.minFilter = THREE.LinearFilter;
  sharedHalo.needsUpdate = true;
  return sharedHalo;
}

// A soft vertical gradient for the beam: bright at the base, gone at the top.
export function gradientTexture() {
  const data = new Uint8Array(4 * 64 * 4);
  for (let y = 0; y < 64; y++) for (let x = 0; x < 4; x++) {
    const at = (y * 4 + x) * 4;
    data.set([255, 255, 255, Math.round((y + .5) / 64 * 255)], at);
  }
  const texture = new THREE.DataTexture(data, 4, 64);
  texture.flipY = true;
  texture.magFilter = texture.minFilter = THREE.LinearFilter;
  texture.needsUpdate = true;
  return texture;
}

export class CommandPosts {
  // logos: side name -> a function making a new copy of that side's logo
  // (a group whose userData.material is its material).
  // teams: who is whose friend (src/registry.js: relation, locals,
  // captureTeam), settable later; without it every other team is an enemy.
  constructor(scene, map, { logos, textures, playerTeam, settings, teams = null, headless = false }) {
    const rules = map.commandPost || { neutralizeTime: 12, captureTime: 10, holoTurnOnTime: 2.2 };
    this.posts = (map.commandPosts || []).map((desc, index) => new Post(desc, rules, index));
    this.playerTeam = playerTeam;
    this.teams = teams;
    this.teamNames = Object.fromEntries(Object.entries(map.mission?.teams || {}).map(([team, t]) => [Number(team), t.name]));
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
    this.mobile?.refresh();
    return this.listed;
  }

  // The command vehicles' posts lost or changed hands since the last call
  // (MobilePosts.events), for the battle's authority.
  mobileEvents() {
    return this.mobile?.events() ?? [];
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

  // How a team looks to the viewer's team (src/hud-units.js radarRelation).
  look(viewer, team) {
    const teams = this.teams;
    return teams ? radarRelation(viewer, team, (a, b) => teams.relation(a, b), (t) => teams.locals(t)) : radarRelation(viewer, team);
  }

  // soldiers: [{ position, team }] for everyone who can capture; each counts
  // for his team's capture team.
  update(dt, soldiers, camera) {
    this.time += dt;
    for (const post of this.posts) {
      if (post.active === false) continue;
      if (post.building && !post.building.alive) {
        this.events.push({ post, ...post.destroy() });
        continue;
      }
      post.counts.clear();
      for (const soldier of soldiers) {
        const team = this.teams ? this.teams.captureTeam(soldier.team) : soldier.team;
        if (!team || !post.contains(soldier.position, this.scratch)) continue;
        post.counts.set(team, (post.counts.get(team) || 0) + 1);
      }
      const change = post.update(dt);
      if (change) this.events.push({ post, ...change });
    }
    if (camera) this.present(dt, camera);
  }

  present(dt, camera) {
    if (this.textures.logo) this.textures.logo.offset.y = (this.time * 0.25) % 1;
    for (const hologram of this.holograms) {
      hologram.update(dt, this.time, this.relation(hologram.post.team), this.teamNames, camera);
    }
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
