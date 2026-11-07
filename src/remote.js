import * as THREE from "three";
import { Arsenal } from "./weapons.js";
import { captureOrdnanceOwner } from "./ordnance-owner.js";

// Replicated recon resources. F3 owns authority and ClientPrediction owns
// private flight, weapons, countdowns and computational camera state.
export const BODY_RADIUS = 0.271;

export class RemoteDroids {
  // model(name) supplies a cold mesh resource; onFired names native events.
  constructor({ scene, camera, cameraPhase, water = null, model, firePoints = {}, onFired, owner = "player" }) {
    this.onFired = onFired;
    this.owner = owner;
    this.scene = scene;
    this.camera = camera;
    this.cameraPhase = cameraPhase;
    this.water = water;
    this.model = model;
    this.firePoints = firePoints;
    this.droids = [];
    this.piloted = null; // the droid the player flies
    this.view = null;
    this.lookX = 0;
    this.lookY = 0;
    this.zoomStep = 0;
    this.hoverTime = 0;
    this.strikes = [];
  }

  get active() {
    return Boolean(this.piloted);
  }

  // The live droid a remote weapon launched, if any.
  of(weapon) {
    return this.droids.find((droid) => droid.weapon === weapon && !droid.dead) || null;
  }

  // Shared construction for authority and semantic presentation. It does not
  // advance physics, take control, fire, or register a gameplay launch.
  createDroid(def, weapon, position, velocity, yaw, team) {
    const make = def.GeometryName ? this.model(def.GeometryName) : null;
    const mesh = make ? make() : null;
    if (mesh) this.scene.add(mesh);
    const droid = {
      def,
      owner: captureOrdnanceOwner(this.owner),
      weapon,
      team,
      mesh,
      position: position.clone(),
      velocity: velocity.clone(),
      yaw,
      pitch: 0,
      health: def.MaxHealth ?? 40,
      dead: false,
      arsenal: new Arsenal((def.weapons || []).map((w) => ({ ...w, ammo: w.ammo }))),
      countdown: -1, // s into the destruct countdown, -1 before it starts
      salvo: null,
      poseMissing:true,
      eye: new THREE.Vector3(),
      aimDir: new THREE.Vector3(0, 0, 1),
      target: new THREE.Vector3(),
    };
    this.place(droid);
    return droid;
  }

  enter(droid) {
    if (!droid || droid.dead) return;
    this.piloted = droid;
    const d = droid.def;
    // Recon continuation keeps the phase's scalar camera record in its
    // checkpoint. Restore is a native lens command; no camera moves here.
    const make = () => ({ fov: (d.ThirdPersonFOV ?? 48) * (Math.PI / 180), reach: 1, fresh: true,
      applyFov: () => this.cameraPhase.projectRemote(this) });
    this.view = this.camera ? make() : null;
    this.simulationView = this.aimCamera ? make() : null;
    this.cameraPhase.remoteDefinition(droid);
    if(this.aimCamera)this.cameraPhase.bindCamera(this.aimCamera);
    this.lookX = 0;
    this.lookY = 0;
    this.zoomStep = 0;
  }

  exit() {
    if (this.piloted?.mesh) this.piloted.mesh.visible = !this.piloted.dead;
    this.piloted = null;
    this.view = null;
    this.simulationView = null;
    this.zoomStep = 0;
  }

  // Input events can accumulate already-normalized raw counts; no flight
  // or weapon controller runs through this resource facade.
  look(dx,dy,sensitivity,invert){this.lookX+=dx*sensitivity;this.lookY+=(invert?dy:-dy)*sensitivity;}

  get zoom() {
    const blaster = this.piloted?.arsenal.current(0);
    return this.zoomStep && blaster ? blaster.def.ZoomMin || 1 : 1;
  }

  place(droid) {
    if (!droid.mesh) return;
    droid.mesh.position.copy(droid.position);
    droid.mesh.rotation.set(0, droid.yaw, 0);
  }

  // Cosmetic hover only: leave the retail spring, collision and eye alone.
  present(dt, enhanced, firstPerson) {
    this.hoverTime += dt;
    for (const droid of this.droids) {
      if (!droid.mesh) continue;
      this.place(droid);
      droid.mesh.visible = !droid.dead && !(droid === this.piloted && (firstPerson || this.zoom > 1));
      if (!enhanced) continue;
      const t = this.hoverTime + droid.position.x * 0.13 + droid.position.z * 0.09;
      droid.mesh.position.y += Math.sin(t * 2.6) * 0.045;
      droid.mesh.rotation.x = Math.sin(t * 1.7) * 0.025;
      droid.mesh.rotation.z = Math.sin(t * 2.1) * 0.035;
    }
  }

  // The camera on the flown droid: its chase camera, or its eye in first
  // person (or zoomed): 0.3 m behind the eye point along the aim (0.5 m
  // zoomed), looking along the aim.
  updateCamera(dt, firstPerson) {
    if (!this.piloted || !this.simulationView) return;
    this.cameraPhase.remoteFrame(this, dt, firstPerson);
  }
}
